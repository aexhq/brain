//! KMS key custody: the production implementation of [`brain::keys::KeyCustody`].
//!
//! Encrypt/Decrypt with a per-plane key; the encryption context binds the ciphertext to its
//! session, so a blob copied onto another session's row fails inside KMS, not in our code.
//! A provider key is well under the 4 KiB direct-encrypt bound, so no envelope scheme is
//! needed; the ciphertext itself names its key.

use aws_sdk_kms::primitives::Blob;
use brain::config::ProviderKey;
use brain::keys::KeyCustody;
use brain::{BrainError, Result};
use zeroize::Zeroizing;

/// Where the session id goes in the KMS encryption context.
const CONTEXT_KEY: &str = "brain:session";

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
