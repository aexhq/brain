//! Local encrypted custody for a trusted single-node operator.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use async_trait::async_trait;
use brain::config::ProviderKey;
use brain::keys::KeyCustody;
use brain::{BrainError, Result};
use rand::RngCore;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const MAGIC: &[u8; 4] = b"BRK1";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

/// AES-256-GCM custody backed by one restrictive local master-key file.
pub struct LocalKeyCustody {
    path: PathBuf,
    key: Zeroizing<[u8; KEY_BYTES]>,
}

impl LocalKeyCustody {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let parent = path
            .parent()
            .ok_or_else(|| BrainError::Custody("master-key path has no parent".into()))?;
        std::fs::create_dir_all(parent)
            .map_err(|error| BrainError::Custody(format!("create key directory: {error}")))?;
        secure_directory(parent)?;
        let key = match create_key(&path) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_key(&path)?,
            Err(error) => {
                return Err(BrainError::Custody(format!(
                    "create local master key: {error}"
                )));
            }
        };
        validate_permissions(&path)?;
        Ok(Self {
            path,
            key: Zeroizing::new(key),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        Aes256Gcm::new_from_slice(self.key.as_ref())
            .map_err(|_| BrainError::Custody("invalid local master key".into()))
    }
}

#[async_trait]
impl KeyCustody for LocalKeyCustody {
    async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher()?
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: key.expose().as_bytes(),
                    aad: aad(session_id).as_bytes(),
                },
            )
            .map_err(|_| BrainError::Custody("encrypt session secret".into()))?;
        let mut blob = Vec::with_capacity(MAGIC.len() + nonce.len() + ciphertext.len());
        blob.extend_from_slice(MAGIC);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey> {
        if blob.len() <= MAGIC.len() + NONCE_BYTES || &blob[..MAGIC.len()] != MAGIC {
            return Err(BrainError::Custody(
                "unsupported or truncated local custody blob".into(),
            ));
        }
        let nonce = &blob[MAGIC.len()..MAGIC.len() + NONCE_BYTES];
        let plaintext = Zeroizing::new(
            self.cipher()?
                .decrypt(
                    Nonce::from_slice(nonce),
                    Payload {
                        msg: &blob[MAGIC.len() + NONCE_BYTES..],
                        aad: aad(session_id).as_bytes(),
                    },
                )
                .map_err(|_| {
                    BrainError::Custody(
                        "session secret authentication failed (wrong key or session)".into(),
                    )
                })?,
        );
        let value = std::str::from_utf8(&plaintext)
            .map_err(|_| BrainError::Custody("decrypted session secret is not UTF-8".into()))?;
        Ok(ProviderKey::new(value))
    }
}

fn aad(session_id: &str) -> String {
    format!("brain.local-custody.v1\0{session_id}")
}

fn create_key(path: &Path) -> std::io::Result<[u8; KEY_BYTES]> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let mut key = [0u8; KEY_BYTES];
    rand::rng().fill_bytes(&mut key);
    file.write_all(&key)?;
    file.sync_all()?;
    Ok(key)
}

fn read_key(path: &Path) -> Result<[u8; KEY_BYTES]> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| BrainError::Custody(format!("inspect local master key: {error}")))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(BrainError::Custody(
            "local master key must be a regular, non-symlink file".into(),
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|error| BrainError::Custody(format!("open local master key: {error}")))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| BrainError::Custody(format!("read local master key: {error}")))?;
    if bytes.len() != KEY_BYTES {
        return Err(BrainError::Custody(format!(
            "local master key must be exactly {KEY_BYTES} bytes"
        )));
    }
    let mut key = [0u8; KEY_BYTES];
    key.copy_from_slice(&bytes);
    bytes.fill(0);
    Ok(key)
}

fn secure_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| BrainError::Custody(format!("secure key directory: {error}")))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn validate_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|error| BrainError::Custody(format!("inspect key permissions: {error}")))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(BrainError::Custody(format!(
                "local master key permissions are {:o}; expected 0600",
                mode & 0o777
            )));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(brain::mint_id("brain-key-test", 12));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn persists_a_key_and_authenticates_the_session_binding() {
        let dir = TestDir::new();
        let path = dir.0.join("master.key");
        let custody = LocalKeyCustody::open(&path).unwrap();
        let secret = ProviderKey::new("sk-do-not-store-in-plaintext");
        let blob = custody.encrypt("ses_one", &secret).await.unwrap();
        assert!(
            !blob
                .windows(secret.expose().len())
                .any(|part| part == secret.expose().as_bytes())
        );
        assert_eq!(
            custody.decrypt("ses_one", &blob).await.unwrap().expose(),
            secret.expose()
        );
        assert!(custody.decrypt("ses_two", &blob).await.is_err());
        drop(custody);
        let reopened = LocalKeyCustody::open(&path).unwrap();
        assert_eq!(
            reopened.decrypt("ses_one", &blob).await.unwrap().expose(),
            secret.expose()
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_group_readable_master_key() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDir::new();
        let path = dir.0.join("master.key");
        let custody = LocalKeyCustody::open(&path).unwrap();
        drop(custody);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(LocalKeyCustody::open(&path).is_err());
    }
}
