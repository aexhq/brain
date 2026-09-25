//! What the server knows about a session that the session's own records do not.
//!
//! Two things are decided when a session is created and never appear in the conversation
//! that follows: the provider credential the caller supplied for its model, and the
//! credential of each Environment it reaches over HTTP. The journal is the record of what
//! happened; this is the record of what the session calls those with. Restoring a session
//! after a restart needs both.
//!
//! Credentials and their key are durable before session admission succeeds, sealed under
//! the session id and forgotten together when the session is deleted.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use brain_protocol::{EnvironmentName, EnvironmentRef, ModelSelection, SessionId};
use rand::Rng;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Clone)]
pub struct ModelCredential {
    pub provider: String,
    pub api_key: Zeroizing<String>,
}

/// One line of the metadata log. Folded in order; the last word about a session wins.
#[derive(Deserialize, Serialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum Entry {
    Observer {
        session_id: SessionId,
        environment: EnvironmentRef,
        nonce: String,
        ciphertext: String,
    },
    /// The provider credential a session calls its model with.
    Model {
        session_id: SessionId,
        provider: String,
        nonce: String,
        ciphertext: String,
    },
    /// The credential a session reaches one of its Environments with.
    Environment {
        session_id: SessionId,
        environment: EnvironmentName,
        nonce: String,
        ciphertext: String,
    },
    /// Every credential of the session is gone. Written rather than rewriting the log,
    /// because the log only ever grows forwards.
    Forgotten { session_id: SessionId },
}

pub struct ServerMetadata {
    observers: RwLock<Observers>,
    models: RwLock<HashMap<SessionId, ModelCredential>>,
    environments: RwLock<HashMap<(SessionId, EnvironmentName), Zeroizing<String>>>,
    log: Mutex<File>,
    key: Zeroizing<[u8; KEY_BYTES]>,
}

impl ServerMetadata {
    pub fn open(directory: &Path) -> Result<Self, brain::Error> {
        fs::create_dir_all(directory).map_err(storage_error)?;
        let key = load_or_create_key(&directory.join("master.key"))?;
        let path = directory.join("metadata.log");
        let (log, records) = crate::persistence::open_log(&path)?;
        let (models, environments, observers) = replay(records, &key)?;
        Ok(Self {
            observers: RwLock::new(observers),
            models: RwLock::new(models),
            environments: RwLock::new(environments),
            log: Mutex::new(log),
            key: Zeroizing::new(key),
        })
    }

    /// Seals a session's model credential. The same credential again is the idempotent
    /// retry of a create the caller did not hear the answer to; a different one under the
    /// same session is a different request wearing that session's name.
    pub fn put_model(
        &self,
        session_id: &SessionId,
        selection: &ModelSelection,
    ) -> Result<(), brain::Error> {
        let mut models = self.models.write().map_err(poisoned)?;
        if let Some(existing) = models.get(session_id) {
            if existing.provider == selection.provider
                && existing.api_key.as_str() == selection.api_key
            {
                return Ok(());
            }
            return Err(brain::Error::InvalidState(
                "the session's model credential is already sealed to different credentials".into(),
            ));
        }
        let (nonce, ciphertext) = self.seal(&model_aad(session_id), &selection.api_key)?;
        self.append(&Entry::Model {
            session_id: session_id.clone(),
            provider: selection.provider.clone(),
            nonce,
            ciphertext,
        })?;
        models.insert(
            session_id.clone(),
            ModelCredential {
                provider: selection.provider.clone(),
                api_key: Zeroizing::new(selection.api_key.clone()),
            },
        );
        Ok(())
    }

    pub fn model(&self, session_id: &SessionId) -> Result<Option<ModelCredential>, brain::Error> {
        Ok(self
            .models
            .read()
            .map_err(poisoned)?
            .get(session_id)
            .cloned())
    }

    /// Seals the credential a session reaches one of its Environments with, under the
    /// same rule as the model credential.
    pub fn put_environment(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
        credential: &str,
    ) -> Result<(), brain::Error> {
        let mut environments = self.environments.write().map_err(poisoned)?;
        let key = (session_id.clone(), environment.clone());
        if let Some(existing) = environments.get(&key) {
            if existing.as_str() == credential {
                return Ok(());
            }
            return Err(brain::Error::InvalidState(format!(
                "the credential of Environment `{environment}` is already sealed to a different value"
            )));
        }
        let (nonce, ciphertext) =
            self.seal(&environment_aad(session_id, environment), credential)?;
        self.append(&Entry::Environment {
            session_id: session_id.clone(),
            environment: environment.clone(),
            nonce,
            ciphertext,
        })?;
        environments.insert(key, Zeroizing::new(credential.to_owned()));
        Ok(())
    }

    pub fn environment(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
    ) -> Result<Option<Zeroizing<String>>, brain::Error> {
        Ok(self
            .environments
            .read()
            .map_err(poisoned)?
            .get(&(session_id.clone(), environment.clone()))
            .cloned())
    }

    /// Forgets every credential of a session.
    pub fn forget(&self, session_id: &SessionId) -> Result<(), brain::Error> {
        let mut models = self.models.write().map_err(poisoned)?;
        let mut environments = self.environments.write().map_err(poisoned)?;
        self.append(&Entry::Forgotten {
            session_id: session_id.clone(),
        })?;
        models.remove(session_id);
        environments.retain(|(session, _), _| session != session_id);
        self.observers
            .write()
            .map_err(poisoned)?
            .retain(|(session, _, _), _| session != session_id);
        Ok(())
    }

    pub fn observer(
        &self,
        session: &SessionId,
        environment: &EnvironmentRef,
    ) -> Result<Option<Zeroizing<String>>, brain::Error> {
        Ok(self
            .observers
            .read()
            .map_err(poisoned)?
            .get(&(
                session.clone(),
                environment.name.clone(),
                environment.sequence,
            ))
            .cloned())
    }

    pub fn register_observer(
        &self,
        session: &SessionId,
        environment: &EnvironmentRef,
    ) -> Result<Zeroizing<String>, brain::Error> {
        let mut observers = self.observers.write().map_err(poisoned)?;
        let key = (
            session.clone(),
            environment.name.clone(),
            environment.sequence,
        );
        if let Some(token) = observers.get(&key) {
            return Ok(token.clone());
        }
        let token = Zeroizing::new(brain::random_id("benv"));
        let (nonce, ciphertext) = self.seal(&observer_aad(session, environment), &token)?;
        self.append(&Entry::Observer {
            session_id: session.clone(),
            environment: environment.clone(),
            nonce,
            ciphertext,
        })?;
        observers.insert(key, token.clone());
        Ok(token)
    }

    fn append(&self, entry: &Entry) -> Result<(), brain::Error> {
        let mut log = self
            .log
            .lock()
            .map_err(|_| brain::Error::Executor("server metadata log is poisoned".into()))?;
        crate::persistence::append(&mut log, entry)
    }

    fn seal(&self, aad: &str, secret: &str) -> Result<(String, String), brain::Error> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|error| brain::Error::Executor(error.to_string()))?;
        let nonce: [u8; NONCE_BYTES] = rand::rng().random();
        // What the credential belongs to is authenticated but not encrypted: a credential
        // lifted from one session must not decrypt under another.
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: secret.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|error| brain::Error::Executor(error.to_string()))?;
        Ok((hex::encode(nonce), hex::encode(sealed)))
    }
}

fn model_aad(session_id: &SessionId) -> String {
    format!("model:{session_id}")
}

fn environment_aad(session_id: &SessionId, environment: &EnvironmentName) -> String {
    format!("environment:{session_id}:{environment}")
}

type Observers = HashMap<(SessionId, EnvironmentName, u64), Zeroizing<String>>;
type Credentials = (
    HashMap<SessionId, ModelCredential>,
    HashMap<(SessionId, EnvironmentName), Zeroizing<String>>,
    Observers,
);

fn observer_aad(session: &SessionId, environment: &EnvironmentRef) -> String {
    format!(
        "observer:{session}:{}:{}",
        environment.name, environment.sequence
    )
}

fn replay(records: Vec<Entry>, key: &[u8; KEY_BYTES]) -> Result<Credentials, brain::Error> {
    let mut models = HashMap::new();
    let mut environments = HashMap::new();
    let mut observers = HashMap::new();
    for entry in records {
        match entry {
            Entry::Observer {
                session_id,
                environment,
                nonce,
                ciphertext,
            } => {
                let token = unseal(
                    key,
                    &observer_aad(&session_id, &environment),
                    &nonce,
                    &ciphertext,
                )
                .ok_or_else(|| {
                    brain::Error::Journal(
                        "Environment observer credential cannot be decrypted".into(),
                    )
                })?;
                observers.insert(
                    (session_id, environment.name, environment.sequence),
                    Zeroizing::new(token),
                );
            }
            Entry::Model {
                session_id,
                provider,
                nonce,
                ciphertext,
            } => {
                let api_key = unseal(key, &model_aad(&session_id), &nonce, &ciphertext)
                    .ok_or_else(|| {
                        brain::Error::Journal("model credential cannot be decrypted".into())
                    })?;
                models.insert(
                    session_id,
                    ModelCredential {
                        provider,
                        api_key: Zeroizing::new(api_key),
                    },
                );
            }
            Entry::Environment {
                session_id,
                environment,
                nonce,
                ciphertext,
            } => {
                let credential = unseal(
                    key,
                    &environment_aad(&session_id, &environment),
                    &nonce,
                    &ciphertext,
                )
                .ok_or_else(|| {
                    brain::Error::Journal("Environment credential cannot be decrypted".into())
                })?;
                environments.insert((session_id, environment), Zeroizing::new(credential));
            }
            Entry::Forgotten { session_id } => {
                models.remove(&session_id);
                environments.retain(|(session, _), _| session != &session_id);
                observers.retain(|(session, _, _), _| session != &session_id);
            }
        }
    }
    Ok((models, environments, observers))
}

fn unseal(key: &[u8; KEY_BYTES], aad: &str, nonce: &str, ciphertext: &str) -> Option<String> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce: [u8; NONCE_BYTES] = hex::decode(nonce).ok()?.try_into().ok()?;
    let ciphertext = hex::decode(ciphertext).ok()?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .ok()?;
    String::from_utf8(plain).ok()
}

fn load_or_create_key(path: &Path) -> Result<[u8; KEY_BYTES], brain::Error> {
    match fs::read(path) {
        Ok(bytes) => return key_from_bytes(bytes),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(storage_error(error));
        }
        Err(_) => {}
    }
    let key: [u8; KEY_BYTES] = rand::rng().random();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key).map_err(storage_error)?;
            file.sync_all().map_err(storage_error)?;
            crate::persistence::sync_directory(path.parent().expect("master key has a directory"))?;
            Ok(key)
        }
        // Two processes opened the same directory at once. The one that lost reads what the
        // winner wrote rather than overwriting a key that credentials are already sealed to.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            key_from_bytes(fs::read(path).map_err(storage_error)?)
        }
        Err(error) => Err(storage_error(error)),
    }
}

fn key_from_bytes(bytes: Vec<u8>) -> Result<[u8; KEY_BYTES], brain::Error> {
    <[u8; KEY_BYTES]>::try_from(bytes.as_slice())
        .map_err(|_| brain::Error::Executor("the master key is not 32 bytes".into()))
}

fn storage_error(error: std::io::Error) -> brain::Error {
    brain::Error::Executor(format!("server metadata store: {error}"))
}

fn poisoned<T>(_: T) -> brain::Error {
    brain::Error::Executor("server metadata store is poisoned".into())
}

pub fn metadata_directory(data_dir: &Path) -> PathBuf {
    data_dir.join("server-metadata")
}

#[cfg(test)]
mod tests {
    #[test]
    fn reporter_credentials_survive_reopen_and_are_scoped_to_the_incarnation() {
        use super::*;
        let root = tempfile::tempdir().unwrap();
        let session = SessionId::new("ses_reporter");
        let binding = EnvironmentRef {
            name: EnvironmentName::new("workspace"),
            sequence: 1,
        };
        let metadata = ServerMetadata::open(root.path()).unwrap();
        let token = metadata.register_observer(&session, &binding).unwrap();
        assert!(metadata.register_observer(&session, &binding).unwrap() == token);
        assert!(
            metadata
                .observer(
                    &session,
                    &EnvironmentRef {
                        sequence: 2,
                        ..binding.clone()
                    }
                )
                .unwrap()
                .is_none()
        );
        assert!(
            metadata
                .observer(&SessionId::new("ses_other"), &binding)
                .unwrap()
                .is_none()
        );
        drop(metadata);
        let metadata = ServerMetadata::open(root.path()).unwrap();
        assert!(metadata.observer(&session, &binding).unwrap().unwrap() == token);
        metadata.forget(&session).unwrap();
        drop(metadata);
        let metadata = ServerMetadata::open(root.path()).unwrap();
        assert!(metadata.observer(&session, &binding).unwrap().is_none());
    }

    #[test]
    fn damaged_nonce_lengths_are_errors_not_panics() {
        use rand::Rng;
        let key = rand::random::<[u8; super::KEY_BYTES]>();
        for length in [0, 1, 13] {
            let mut nonce = vec![0; length];
            rand::rng().fill(&mut nonce[..]);
            assert!(super::unseal(&key, "model:ses_x", &hex::encode(nonce), "00").is_none());
        }
    }
}
