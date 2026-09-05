//! What the server knows about a session that the session's own records do not.
//!
//! One thing is decided once, when a session is created, and never appears in the
//! conversation that follows: the provider credential the caller supplied. The journal is
//! the record of what happened; this is the record of what the session calls its model
//! with. Restoring a session after a restart needs both.
//!
//! Credentials and their key are durable before session admission succeeds.

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
use brain_protocol::ModelSelection;
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

/// One line of the metadata log. Folded in order; the last word about a key wins.
#[derive(Deserialize, Serialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum Entry {
    /// A provider credential, sealed to a binding identity.
    Binding {
        binding_id: String,
        provider: String,
        nonce: String,
        ciphertext: String,
    },
    /// The binding is gone. Written rather than rewriting the log, because the log only
    /// ever grows forwards.
    BindingForgotten { binding_id: String },
}

pub struct ServerMetadata {
    credentials: RwLock<HashMap<String, ModelCredential>>,
    log: Mutex<File>,
    key: Zeroizing<[u8; KEY_BYTES]>,
}

impl ServerMetadata {
    pub fn open(directory: &Path) -> Result<Self, brain::Error> {
        fs::create_dir_all(directory).map_err(storage_error)?;
        let key = load_or_create_key(&directory.join("master.key"))?;
        let path = directory.join("metadata.log");
        let (log, records) = crate::persistence::open_log(&path)?;
        let credentials = replay(records, &key)?;
        Ok(Self {
            credentials: RwLock::new(credentials),
            log: Mutex::new(log),
            key: Zeroizing::new(key),
        })
    }

    /// Seals a credential to an identity. The same credential again is the idempotent
    /// retry of a create the caller did not hear the answer to; a different one under the
    /// same identity is a different request wearing that identity's name.
    pub fn put_binding(
        &self,
        binding_id: &str,
        selection: &ModelSelection,
    ) -> Result<(), brain::Error> {
        let mut credentials = self.credentials.write().map_err(poisoned)?;
        if let Some(existing) = credentials.get(binding_id) {
            if existing.provider == selection.provider
                && existing.api_key.as_str() == selection.api_key
            {
                return Ok(());
            }
            return Err(brain::Error::InvalidState(
                "model binding identity is already sealed to different credentials".into(),
            ));
        }
        let (nonce, ciphertext) = self.seal(binding_id, selection)?;
        self.append(&Entry::Binding {
            binding_id: binding_id.to_owned(),
            provider: selection.provider.clone(),
            nonce,
            ciphertext,
        })?;
        credentials.insert(
            binding_id.to_owned(),
            ModelCredential {
                provider: selection.provider.clone(),
                api_key: Zeroizing::new(selection.api_key.clone()),
            },
        );
        Ok(())
    }

    pub fn binding(&self, binding_id: &str) -> Result<Option<ModelCredential>, brain::Error> {
        Ok(self
            .credentials
            .read()
            .map_err(poisoned)?
            .get(binding_id)
            .cloned())
    }

    pub fn forget_binding(&self, binding_id: &str) -> Result<(), brain::Error> {
        let mut credentials = self.credentials.write().map_err(poisoned)?;
        self.append(&Entry::BindingForgotten {
            binding_id: binding_id.to_owned(),
        })?;
        credentials.remove(binding_id);
        Ok(())
    }

    fn append(&self, entry: &Entry) -> Result<(), brain::Error> {
        let mut log = self
            .log
            .lock()
            .map_err(|_| brain::Error::Executor("server metadata log is poisoned".into()))?;
        crate::persistence::append(&mut log, entry)
    }

    fn seal(
        &self,
        binding_id: &str,
        selection: &ModelSelection,
    ) -> Result<(String, String), brain::Error> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|error| brain::Error::Executor(error.to_string()))?;
        let nonce: [u8; NONCE_BYTES] = rand::rng().random();
        // The binding id is authenticated but not encrypted: a credential lifted from one
        // identity must not decrypt under another.
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: selection.api_key.as_bytes(),
                    aad: binding_id.as_bytes(),
                },
            )
            .map_err(|error| brain::Error::Executor(error.to_string()))?;
        Ok((hex::encode(nonce), hex::encode(sealed)))
    }
}

fn replay(
    records: Vec<Entry>,
    key: &[u8; KEY_BYTES],
) -> Result<HashMap<String, ModelCredential>, brain::Error> {
    let mut credentials = HashMap::new();
    for entry in records {
        match entry {
            Entry::Binding {
                binding_id,
                provider,
                nonce,
                ciphertext,
            } => {
                let api_key = unseal(key, &binding_id, &nonce, &ciphertext).ok_or_else(|| {
                    brain::Error::Journal("model credential cannot be decrypted".into())
                })?;
                credentials.insert(
                    binding_id,
                    ModelCredential {
                        provider,
                        api_key: Zeroizing::new(api_key),
                    },
                );
            }
            Entry::BindingForgotten { binding_id } => {
                credentials.remove(&binding_id);
            }
        }
    }
    Ok(credentials)
}

fn unseal(
    key: &[u8; KEY_BYTES],
    binding_id: &str,
    nonce: &str,
    ciphertext: &str,
) -> Option<String> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce: [u8; NONCE_BYTES] = hex::decode(nonce).ok()?.try_into().ok()?;
    let ciphertext = hex::decode(ciphertext).ok()?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: binding_id.as_bytes(),
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
    fn damaged_nonce_lengths_are_errors_not_panics() {
        for nonce in ["", "00", &"00".repeat(13)] {
            assert!(super::unseal(&[0; super::KEY_BYTES], "binding", nonce, "00").is_none());
        }
    }
}
