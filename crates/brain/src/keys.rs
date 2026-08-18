//! BYOK key custody.
//!
//! The contract promise on `ModelConfig.api_key` is "Encrypted per session, never returned,
//! never logged". Here that means: the plaintext exists in memory only while a session is
//! resident; at rest it is a KMS ciphertext in the journal HEAD, bound to its session by
//! encryption context so a ciphertext copied onto another session's row will not decrypt.
//!
//! Custody never pools per-tenant state (ARCHITECTURE-v1 §2.9: a shared token accountant is
//! how P1 leaked accounting across tenants) -- there is nothing here but stateless calls.

use crate::config::ProviderKey;
use crate::{BrainError, Result};
use aws_sdk_kms::primitives::Blob;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Where the session id goes in the KMS encryption context. Decrypt with a different session
/// id fails inside KMS, not in our code.
const CONTEXT_KEY: &str = "aex:session";

#[async_trait::async_trait]
pub trait KeyCustody: Send + Sync {
    /// Encrypts a provider key for one session. Returns an opaque blob for the journal HEAD.
    async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>>;
    /// Decrypts the blob written by [`Self::encrypt`] for the same session.
    async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey>;
}

/// Production custody: AWS KMS `Encrypt`/`Decrypt` with a per-plane key.
///
/// A provider key is well under the 4 KiB KMS direct-encrypt bound, so no envelope scheme is
/// needed; the ciphertext itself carries the key id.
pub struct KmsCustody {
    kms: aws_sdk_kms::Client,
    key_id: String,
}

impl KmsCustody {
    pub fn new(kms: aws_sdk_kms::Client, key_id: impl Into<String>) -> Self {
        Self {
            kms,
            key_id: key_id.into(),
        }
    }
}

#[async_trait::async_trait]
impl KeyCustody for KmsCustody {
    async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>> {
        let out = self
            .kms
            .encrypt()
            .key_id(&self.key_id)
            .plaintext(Blob::new(key.expose().as_bytes()))
            .encryption_context(CONTEXT_KEY, session_id)
            .send()
            .await
            .map_err(|e| BrainError::Custody(format!("kms encrypt: {e}")))?;
        out.ciphertext_blob()
            .map(|b| b.as_ref().to_vec())
            .ok_or_else(|| BrainError::Custody("kms encrypt returned no ciphertext".into()))
    }

    async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey> {
        let out = self
            .kms
            .decrypt()
            .ciphertext_blob(Blob::new(blob))
            .encryption_context(CONTEXT_KEY, session_id)
            .send()
            .await
            .map_err(|e| BrainError::Custody(format!("kms decrypt: {e}")))?;
        let pt = out
            .plaintext()
            .ok_or_else(|| BrainError::Custody("kms decrypt returned no plaintext".into()))?;
        // Zeroized intermediate: the only durable copy of the plaintext is inside ProviderKey,
        // whose Debug is redacting and which is dropped with the session.
        let buf = Zeroizing::new(pt.as_ref().to_vec());
        let s = std::str::from_utf8(&buf)
            .map_err(|_| BrainError::Custody("decrypted key is not utf-8".into()))?;
        Ok(ProviderKey::new(s))
    }
}

/// Test custody: an obfuscated but NOT cryptographically protected blob, bound to the session
/// id so binding bugs still surface in tests. Never constructed by the server binary.
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
