//! What the server knows about a session that the session's own records do not.
//!
//! Two things are decided once, when a session is created, and never appear in the
//! conversation that follows: the `journal_id` the kernel minted, and the provider
//! credential the caller supplied. The journal is the record of what happened; this is the
//! record of what the session *is*. Restoring a session after a restart needs both.
//!
//! Written on the same terms as the journal: appended, **never fsynced**, and best effort.
//! A create returns as soon as the bytes are handed to the kernel's page cache, which is
//! what took four milliseconds out of it — the previous version was a SQLite table opened
//! `synchronous = FULL`, so every session create waited for a disk. A crash can lose the
//! tail, and a session whose metadata went with it comes back readable but cannot take
//! another turn. That is the trade this design accepts everywhere else, made here too.
//!
//! Credentials are encrypted at rest; `journal_id` is not, because it names a session and
//! reveals nothing that the session's own records do not.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use brain::KernelError;
use brain_protocol::ModelSelection;
use rand::RngCore;
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
    /// The journal id the kernel minted for a session.
    Session {
        session_id: String,
        journal_id: String,
    },
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
    journals: RwLock<HashMap<String, String>>,
    credentials: RwLock<HashMap<String, ModelCredential>>,
    /// Held only across a small buffered append. The old store held a process-global lock
    /// across an fsync and an AES-GCM round trip, which serialised every session create in
    /// the process behind a disk.
    log: Mutex<File>,
    key: Zeroizing<[u8; KEY_BYTES]>,
}

impl ServerMetadata {
    pub fn open(directory: &Path) -> Result<Self, KernelError> {
        fs::create_dir_all(directory).map_err(storage_error)?;
        let key = load_or_create_key(&directory.join("master.key"))?;
        let path = directory.join("metadata.log");
        let (journals, credentials) = replay(&path, &key)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(storage_error)?;
        Ok(Self {
            journals: RwLock::new(journals),
            credentials: RwLock::new(credentials),
            log: Mutex::new(log),
            key: Zeroizing::new(key),
        })
    }

    /// Remembers which journal a session was given.
    pub fn put_journal(&self, session_id: &str, journal_id: &str) -> Result<(), KernelError> {
        self.journals
            .write()
            .map_err(poisoned)?
            .insert(session_id.to_owned(), journal_id.to_owned());
        self.append(&Entry::Session {
            session_id: session_id.to_owned(),
            journal_id: journal_id.to_owned(),
        })
    }

    /// Every session this process knows a journal id for, for restoring rows at startup.
    pub fn journals(&self) -> Result<HashMap<String, String>, KernelError> {
        Ok(self.journals.read().map_err(poisoned)?.clone())
    }

    /// Seals a credential to an identity. The same credential again is the idempotent
    /// retry of a create the caller did not hear the answer to; a different one under the
    /// same identity is a different request wearing that identity's name.
    pub fn put_binding(
        &self,
        binding_id: &str,
        selection: &ModelSelection,
    ) -> Result<(), KernelError> {
        {
            let mut credentials = self.credentials.write().map_err(poisoned)?;
            if let Some(existing) = credentials.get(binding_id) {
                if existing.provider == selection.provider
                    && existing.api_key.as_str() == selection.api_key
                {
                    return Ok(());
                }
                return Err(KernelError::InvalidState(
                    "model binding identity is already sealed to different credentials".into(),
                ));
            }
            credentials.insert(
                binding_id.to_owned(),
                ModelCredential {
                    provider: selection.provider.clone(),
                    api_key: Zeroizing::new(selection.api_key.clone()),
                },
            );
        }
        let (nonce, ciphertext) = self.seal(binding_id, selection)?;
        self.append(&Entry::Binding {
            binding_id: binding_id.to_owned(),
            provider: selection.provider.clone(),
            nonce,
            ciphertext,
        })
    }

    pub fn binding(&self, binding_id: &str) -> Result<Option<ModelCredential>, KernelError> {
        Ok(self
            .credentials
            .read()
            .map_err(poisoned)?
            .get(binding_id)
            .cloned())
    }

    pub fn forget_binding(&self, binding_id: &str) -> Result<(), KernelError> {
        self.credentials
            .write()
            .map_err(poisoned)?
            .remove(binding_id);
        self.append(&Entry::BindingForgotten {
            binding_id: binding_id.to_owned(),
        })
    }

    /// Appends one line. No fsync, on purpose: this is best effort, and waiting for a disk
    /// here is the cost the whole design exists to avoid.
    fn append(&self, entry: &Entry) -> Result<(), KernelError> {
        let mut line = serde_json::to_vec(entry).map_err(|error| storage_json(&error))?;
        line.push(b'\n');
        let mut log = self
            .log
            .lock()
            .map_err(|_| KernelError::Executor("server metadata log is poisoned".into()))?;
        log.write_all(&line).map_err(storage_error)?;
        Ok(())
    }

    fn seal(
        &self,
        binding_id: &str,
        selection: &ModelSelection,
    ) -> Result<(String, String), KernelError> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|error| KernelError::Executor(error.to_string()))?;
        let mut nonce = [0_u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce);
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
            .map_err(|error| KernelError::Executor(error.to_string()))?;
        Ok((encode(&nonce), encode(&sealed)))
    }
}

/// What one read of the log recovered: journal ids by session, credentials by binding.
type Recovered = (HashMap<String, String>, HashMap<String, ModelCredential>);

/// Reads back what reached the disk.
///
/// A line that will not parse ends the read. The last write of a crashed process is the one
/// most likely to be half a line, and everything before it is whole — so the log is taken up
/// to the tear and no further, rather than refusing to start over a partial record.
fn replay(path: &Path, key: &[u8; KEY_BYTES]) -> Result<Recovered, KernelError> {
    let mut journals = HashMap::new();
    let mut credentials = HashMap::new();
    let Ok(file) = File::open(path) else {
        return Ok((journals, credentials));
    };
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let Ok(entry) = serde_json::from_str::<Entry>(&line) else {
            break;
        };
        match entry {
            Entry::Session {
                session_id,
                journal_id,
            } => {
                journals.insert(session_id, journal_id);
            }
            Entry::Binding {
                binding_id,
                provider,
                nonce,
                ciphertext,
            } => {
                // A credential that will not decrypt is dropped rather than fatal: the key
                // may have been replaced, and one unreadable binding must not stop a
                // process starting.
                if let Some(api_key) = unseal(key, &binding_id, &nonce, &ciphertext) {
                    credentials.insert(
                        binding_id,
                        ModelCredential {
                            provider,
                            api_key: Zeroizing::new(api_key),
                        },
                    );
                }
            }
            Entry::BindingForgotten { binding_id } => {
                credentials.remove(&binding_id);
            }
        }
    }
    Ok((journals, credentials))
}

fn unseal(
    key: &[u8; KEY_BYTES],
    binding_id: &str,
    nonce: &str,
    ciphertext: &str,
) -> Option<String> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = decode(nonce)?;
    let ciphertext = decode(ciphertext)?;
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

fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

fn load_or_create_key(path: &Path) -> Result<[u8; KEY_BYTES], KernelError> {
    match fs::read(path) {
        Ok(bytes) => return key_from_bytes(bytes),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(storage_error(error));
        }
        Err(_) => {}
    }
    let mut key = [0_u8; KEY_BYTES];
    rand::rng().fill_bytes(&mut key);
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

fn key_from_bytes(bytes: Vec<u8>) -> Result<[u8; KEY_BYTES], KernelError> {
    <[u8; KEY_BYTES]>::try_from(bytes.as_slice())
        .map_err(|_| KernelError::Executor("the master key is not 32 bytes".into()))
}

fn storage_error(error: std::io::Error) -> KernelError {
    KernelError::Executor(format!("server metadata store: {error}"))
}

fn storage_json(error: &serde_json::Error) -> KernelError {
    KernelError::Executor(format!("server metadata store: {error}"))
}

fn poisoned<T>(_: T) -> KernelError {
    KernelError::Executor("server metadata store is poisoned".into())
}

pub fn metadata_directory(data_dir: &Path) -> PathBuf {
    data_dir.join("server-metadata")
}
