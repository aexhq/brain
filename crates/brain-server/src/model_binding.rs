use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use async_trait::async_trait;
use brain::{KernelError, ModelExecutor, model::ModelTransport, model::RemoteModelClient};
use brain_protocol::{
    Identity, ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelSelection,
    ModelStreamEvent, OperationId,
};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Clone)]
pub struct ModelCredential {
    pub provider: String,
    pub api_key: Zeroizing<String>,
}

pub trait ModelBindingStore: Send + Sync + 'static {
    fn put(&self, binding_id: &str, selection: &ModelSelection) -> Result<(), KernelError>;
    fn get(&self, binding_id: &str) -> Result<Option<ModelCredential>, KernelError>;
    fn delete(&self, binding_id: &str) -> Result<(), KernelError>;
}

pub struct LocalModelBindingStore {
    connection: Mutex<Connection>,
    key: Zeroizing<[u8; KEY_BYTES]>,
    /// Credentials already read from the database, so that a model call is not a
    /// `synchronous = FULL` query plus an AES-GCM decrypt behind a process-global mutex
    /// on a Tokio worker thread. Every decision of every turn takes this path.
    ///
    /// Safe to cache because a binding is sealed at creation: `put` on an existing
    /// identity either matches the stored credential or fails, so an entry cannot go
    /// stale. Misses are not cached, so a later `put` is still visible, and `delete`
    /// evicts. The plaintext lives no longer than the master key already does — that key
    /// is resident for the life of the process, so anything that can read this map could
    /// already decrypt the table.
    ///
    /// Bounded by the number of rows: an entry only appears after a successful read, and
    /// a row only appears through `put`.
    cached: RwLock<HashMap<String, ModelCredential>>,
}

impl LocalModelBindingStore {
    pub fn open(directory: &Path) -> Result<Self, KernelError> {
        fs::create_dir_all(directory).map_err(storage_error)?;
        let key = load_or_create_key(&directory.join("master.key"))?;
        let connection = Connection::open(directory.join("bindings.sqlite3")).map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(db_error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS model_bindings (
                    binding_id TEXT PRIMARY KEY,
                    provider TEXT NOT NULL,
                    nonce BLOB NOT NULL,
                    ciphertext BLOB NOT NULL
                );",
            )
            .map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            key: Zeroizing::new(key),
            cached: RwLock::new(HashMap::new()),
        })
    }

    fn cache_read(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, ModelCredential>>, KernelError> {
        self.cached
            .read()
            .map_err(|_| KernelError::Journal("model binding cache is poisoned".into()))
    }

    fn cache_write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, ModelCredential>>, KernelError>
    {
        self.cached
            .write()
            .map_err(|_| KernelError::Journal("model binding cache is poisoned".into()))
    }

    fn decrypt(
        &self,
        binding_id: &str,
        provider: String,
        nonce: Vec<u8>,
        ciphertext: Vec<u8>,
    ) -> Result<ModelCredential, KernelError> {
        if nonce.len() != NONCE_BYTES {
            return Err(KernelError::Journal(
                "model binding nonce is corrupt".into(),
            ));
        }
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|_| KernelError::Journal("model binding key is invalid".into()))?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: binding_id.as_bytes(),
                },
            )
            .map_err(|_| KernelError::Journal("model binding cannot be decrypted".into()))?;
        let api_key = String::from_utf8(plaintext)
            .map_err(|_| KernelError::Journal("model binding plaintext is corrupt".into()))?;
        Ok(ModelCredential {
            provider,
            api_key: Zeroizing::new(api_key),
        })
    }
}

impl ModelBindingStore for LocalModelBindingStore {
    fn put(&self, binding_id: &str, selection: &ModelSelection) -> Result<(), KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("model binding mutex poisoned".into()))?;
        let existing = connection
            .query_row(
                "SELECT provider, nonce, ciphertext FROM model_bindings WHERE binding_id=?1",
                [binding_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(db_error)?;
        if let Some((provider, nonce, ciphertext)) = existing {
            let existing = self.decrypt(binding_id, provider, nonce, ciphertext)?;
            if existing.provider == selection.provider
                && existing.api_key.as_str() == selection.api_key
            {
                return Ok(());
            }
            return Err(KernelError::InvalidState(
                "model binding identity is already sealed to different credentials".into(),
            ));
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce);
        let cipher = Aes256Gcm::new_from_slice(self.key.as_slice())
            .map_err(|_| KernelError::Journal("model binding key is invalid".into()))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: selection.api_key.as_bytes(),
                    aad: binding_id.as_bytes(),
                },
            )
            .map_err(|_| KernelError::Journal("model binding encryption failed".into()))?;
        connection
            .execute(
                "INSERT INTO model_bindings(binding_id,provider,nonce,ciphertext) VALUES(?1,?2,?3,?4)",
                params![binding_id, selection.provider, nonce.as_slice(), ciphertext],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn get(&self, binding_id: &str) -> Result<Option<ModelCredential>, KernelError> {
        if let Some(credential) = self.cache_read()?.get(binding_id) {
            return Ok(Some(credential.clone()));
        }
        let row = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| KernelError::Journal("model binding mutex poisoned".into()))?;
            connection
                .query_row(
                    "SELECT provider, nonce, ciphertext FROM model_bindings WHERE binding_id=?1",
                    [binding_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(db_error)?
        };
        let Some((provider, nonce, ciphertext)) = row else {
            return Ok(None);
        };
        let credential = self.decrypt(binding_id, provider, nonce, ciphertext)?;
        self.cache_write()?
            .insert(binding_id.to_owned(), credential.clone());
        Ok(Some(credential))
    }

    fn delete(&self, binding_id: &str) -> Result<(), KernelError> {
        self.cache_write()?.remove(binding_id);
        self.connection
            .lock()
            .map_err(|_| KernelError::Journal("model binding mutex poisoned".into()))?
            .execute(
                "DELETE FROM model_bindings WHERE binding_id=?1",
                [binding_id],
            )
            .map_err(db_error)?;
        Ok(())
    }
}

pub struct ServerModelExecutor {
    bindings: Arc<dyn ModelBindingStore>,
    /// One connection pool for the process. The credential is the only part of a model
    /// call that varies by session, and a credential is a header, not a client.
    transport: Arc<ModelTransport>,
}

impl ServerModelExecutor {
    pub fn new(
        bindings: Arc<dyn ModelBindingStore>,
        base_url: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, KernelError> {
        Ok(Self {
            bindings,
            transport: Arc::new(ModelTransport::new(&base_url.into(), timeout)?),
        })
    }
}

#[async_trait]
impl ModelExecutor for ServerModelExecutor {
    async fn execute(
        &self,
        operation_id: &OperationId,
        request_identity: &Identity,
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        let credential = self
            .bindings
            .get(&binding.binding_id)?
            .ok_or_else(|| KernelError::Executor("model binding is unavailable".into()))?;
        if credential.provider != "vercel-ai-gateway" {
            return Err(KernelError::Executor(
                "model provider is unsupported".into(),
            ));
        }
        let client =
            RemoteModelClient::bound(self.transport.clone(), credential.api_key.to_string())?;
        client
            .execute(
                operation_id,
                request_identity,
                binding,
                presentation,
                request,
                on_event,
            )
            .await
    }
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
            file.sync_all().map_err(storage_error)?;
            Ok(key)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            key_from_bytes(fs::read(path).map_err(storage_error)?)
        }
        Err(error) => Err(storage_error(error)),
    }
}

fn key_from_bytes(bytes: Vec<u8>) -> Result<[u8; KEY_BYTES], KernelError> {
    bytes.try_into().map_err(|_| {
        KernelError::Journal("model binding master key must contain exactly 32 bytes".into())
    })
}

fn db_error(error: rusqlite::Error) -> KernelError {
    KernelError::Journal(error.to_string())
}

fn storage_error(error: std::io::Error) -> KernelError {
    KernelError::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_survive_restart_without_plaintext_on_disk() {
        let directory = temporary_directory();
        let selection = ModelSelection {
            provider: "vercel-ai-gateway".into(),
            name: "openai/gpt-5-mini".into(),
            api_key: "provider-secret-that-must-not-leak".into(),
        };
        {
            let store = LocalModelBindingStore::open(&directory).unwrap();
            store.put("model_one", &selection).unwrap();
        }
        let store = LocalModelBindingStore::open(&directory).unwrap();
        let credential = store.get("model_one").unwrap().unwrap();
        assert_eq!(credential.provider, "vercel-ai-gateway");
        assert_eq!(credential.api_key.as_str(), selection.api_key);
        let database = fs::read(directory.join("bindings.sqlite3")).unwrap();
        assert!(
            !database
                .windows(selection.api_key.len())
                .any(|window| window == selection.api_key.as_bytes())
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    /// Deletes the row behind the store's back, so a `get` that still answers can only
    /// have answered from memory. Every model decision takes this path, and it used to be
    /// a `synchronous = FULL` query plus an AES-GCM decrypt under a process-global mutex.
    #[test]
    fn a_credential_is_read_from_the_database_once() {
        let directory = temporary_directory();
        let store = LocalModelBindingStore::open(&directory).unwrap();
        let selection = ModelSelection {
            provider: "vercel-ai-gateway".into(),
            name: "openai/gpt-5-mini".into(),
            api_key: "cached-secret".into(),
        };
        store.put("model_one", &selection).unwrap();
        assert_eq!(
            store.get("model_one").unwrap().unwrap().api_key.as_str(),
            "cached-secret"
        );

        Connection::open(directory.join("bindings.sqlite3"))
            .unwrap()
            .execute("DELETE FROM model_bindings", [])
            .unwrap();

        let credential = store
            .get("model_one")
            .unwrap()
            .expect("a cached credential must not need the row it came from");
        assert_eq!(credential.api_key.as_str(), "cached-secret");

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deleting_a_binding_forgets_its_credential() {
        let directory = temporary_directory();
        let store = LocalModelBindingStore::open(&directory).unwrap();
        let selection = ModelSelection {
            provider: "vercel-ai-gateway".into(),
            name: "openai/gpt-5-mini".into(),
            api_key: "revoked-secret".into(),
        };
        store.put("model_one", &selection).unwrap();
        store.get("model_one").unwrap().unwrap();

        store.delete("model_one").unwrap();

        assert!(store.get("model_one").unwrap().is_none());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    /// A miss is not cached, so sealing a binding after something asked for it still
    /// makes the credential visible.
    #[test]
    fn a_miss_does_not_hide_a_later_binding() {
        let directory = temporary_directory();
        let store = LocalModelBindingStore::open(&directory).unwrap();
        assert!(store.get("model_one").unwrap().is_none());

        store
            .put(
                "model_one",
                &ModelSelection {
                    provider: "vercel-ai-gateway".into(),
                    name: "openai/gpt-5-mini".into(),
                    api_key: "later-secret".into(),
                },
            )
            .unwrap();

        assert_eq!(
            store.get("model_one").unwrap().unwrap().api_key.as_str(),
            "later-secret"
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn binding_identity_rejects_different_credentials() {
        let directory = temporary_directory();
        let store = LocalModelBindingStore::open(&directory).unwrap();
        let first = ModelSelection {
            provider: "vercel-ai-gateway".into(),
            name: "openai/gpt-5-mini".into(),
            api_key: "first".into(),
        };
        let second = ModelSelection {
            api_key: "second".into(),
            ..first.clone()
        };
        store.put("model_one", &first).unwrap();
        assert!(matches!(
            store.put("model_one", &second),
            Err(KernelError::InvalidState(_))
        ));
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    fn temporary_directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "brain-model-binding-test-{}",
            rand::random::<u64>()
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
