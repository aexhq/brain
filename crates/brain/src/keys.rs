//! BYOK key custody.
//!
//! The contract promise on `ModelConfig.api_key` is "Encrypted per session, never returned,
//! never logged". [`KeyCustody`] is the seam: the plaintext exists in memory only while a
//! session is resident; at rest the journal HEAD carries whatever opaque blob the custody
//! adapter produced, bound to its session id (a blob copied onto another session's row must
//! not decrypt). `brain-aws` carries the KMS implementation; [`PlainCustody`] is the local /
//! test one.
//!
//! Custody never pools per-tenant state. Keeping these calls stateless prevents accounting or key
//! material from crossing tenant boundaries.

use crate::config::ProviderKey;
use crate::{BrainError, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// AWS KMS direct Encrypt accepts at most 4096 plaintext bytes. Core validates this neutral
/// custody contract before invoking any adapter so oversized credentials are a stable 4xx, not
/// a late cloud-specific failure.
pub const MAX_CUSTODY_PLAINTEXT_BYTES: usize = brain_protocol::MAX_SESSION_SECRET_DOCUMENT_BYTES;

pub fn validate_custody_plaintext(label: &str, plaintext: &str) -> Result<()> {
    let bytes = plaintext.len();
    if bytes == 0 || bytes > MAX_CUSTODY_PLAINTEXT_BYTES {
        return Err(BrainError::Invalid(format!(
            "{label} must contain 1 to {MAX_CUSTODY_PLAINTEXT_BYTES} UTF-8 bytes"
        )));
    }
    Ok(())
}

#[async_trait::async_trait]
pub trait KeyCustody: Send + Sync {
    /// Encrypts a provider key for one session. Returns an opaque blob for the journal HEAD.
    async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>>;
    /// Decrypts the blob written by [`Self::encrypt`] for the same session.
    async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey>;
}

/// Local/test custody: an obfuscated but NOT cryptographically protected blob, bound to the
/// session id so binding bugs still surface. Used by local mode, where the journal never
/// rests on disk anyway; production custody is a real KMS (see `brain-aws`).
pub struct PlainCustody;

impl PlainCustody {
    fn mask(session_id: &str, data: &[u8]) -> Vec<u8> {
        let pad = Sha256::digest(session_id.as_bytes());
        data.iter()
            .enumerate()
            .map(|(i, b)| b ^ pad[i % pad.len()])
            .collect()
    }
}

#[async_trait::async_trait]
impl KeyCustody for PlainCustody {
    async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>> {
        Ok(Self::mask(session_id, key.expose().as_bytes()))
    }
    async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey> {
        let buf = Zeroizing::new(Self::mask(session_id, blob));
        let s = std::str::from_utf8(&buf)
            .map_err(|_| BrainError::Custody("wrong session for this key blob".into()))?;
        Ok(ProviderKey::new(s))
    }
}

/// Base64 helpers for carrying the ciphertext in a JSON HEAD attribute.
pub fn blob_to_b64(blob: &[u8]) -> String {
    B64.encode(blob)
}
pub fn blob_from_b64(s: &str) -> Result<Vec<u8>> {
    B64.decode(s)
        .map_err(|e| BrainError::Custody(format!("key blob is not base64: {e}")))
}

/// KMS grants `Decrypt` on ciphertext regardless of which key produced it (the ciphertext
/// names its key), so the custody key id only needs to be valid for encrypt.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plain_custody_round_trips_and_binds_to_the_session() {
        let c = PlainCustody;
        let key = ProviderKey::new("sk-test-abc123");
        let blob = c.encrypt("ses_a", &key).await.unwrap();
        assert_ne!(blob, b"sk-test-abc123", "blob must not be the plaintext");
        let back = c.decrypt("ses_a", &blob).await.unwrap();
        assert_eq!(back.expose(), "sk-test-abc123");
        // A different session either fails or yields garbage -- never the key.
        if let Ok(k) = c.decrypt("ses_b", &blob).await {
            assert_ne!(k.expose(), "sk-test-abc123")
        }
    }

    #[test]
    fn b64_round_trip() {
        let blob = vec![0u8, 1, 2, 255];
        assert_eq!(blob_from_b64(&blob_to_b64(&blob)).unwrap(), blob);
    }
}
