//! Durable session-object storage port. This is distinct from a Environment's live sandbox files.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::Result;

/// Internal immutable Tool bundle custody. Bundle objects are scoped to the root lifetime but
/// excluded from user storage listing and quota. A fresh short-lived fetch is minted whenever a
/// Environment preparation cache must be rebuilt after process loss.
#[async_trait]
pub trait BundleStoragePort: Send + Sync {
    async fn store_bundle(
        &self,
        root_id: &str,
        bundle_digest: &str,
        bytes: &[u8],
    ) -> Result<brain_protocol::environment::ObjectReference>;

    async fn prepare_bundle_fetch(
        &self,
        root_id: &str,
        bundle_digest: &str,
    ) -> Result<brain_protocol::environment::BundleFetch>;

    /// Remove only the root-hidden bundle namespace after a definitively rejected root create.
    /// This must never touch user storage; normal root deletion uses the exhaustive session purge.
    async fn purge_root_bundles(&self, root_id: &str) -> Result<()>;
}

/// Hosted defaults. They are copied into each immutable session configuration at create time;
/// changing process configuration therefore cannot widen an existing session.
pub const DEFAULT_MAX_STORAGE_OBJECT_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_MAX_SESSION_STORAGE_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_STORAGE_TRANSFER_TTL_MS: u64 = 15 * 60 * 1_000;

/// Root-hidden session-storage namespace used only for short-lived sandbox transfer staging.
/// Public storage keys and prefixes can never enter this namespace; exhaustive session deletion
/// still purges it because adapters scope deletion at the session prefix.
pub const INTERNAL_SANDBOX_TRANSFER_PREFIX: &str = ".brain/sandbox-transfers/";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageObject {
    pub key: String,
    pub bytes: u64,
    pub sha256: String,
    pub content_type: Option<String>,
    /// Adapter-authored durable provenance for the exact Brain storage intent that published
    /// this version. It is deliberately omitted from public object projections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_id: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePage {
    pub objects: Vec<StorageObject>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageWriteRequest {
    pub session_id: String,
    /// Brain-minted durable intent id, persisted before bytes are written. Adapters must store it
    /// with the published object and return it from `stat`/`write` so crash recovery never adopts
    /// an older byte-identical object.
    pub publication_id: String,
    pub key: String,
    pub content_base64: String,
    pub content_type: Option<String>,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageUploadRequest {
    pub session_id: String,
    /// Brain-minted durable id. It is reserved in the journal before a presigned request exists.
    pub transfer_id: String,
    pub key: String,
    pub bytes: u64,
    /// Lowercase hexadecimal SHA-256 of the final bytes.
    /// Expected digest for caller-uploaded bytes. Sandbox exports leave this absent and Environment
    /// supplies the actual digest from the single streamed transfer.
    pub sha256: Option<String>,
    pub content_type: Option<String>,
    pub overwrite: bool,
    /// Exact expiry already sealed in the journal reservation.
    pub expires_at_ms: u64,
}

/// Caller-facing upload intent. Brain adds the durable transfer identity and expiry only after
/// applying the session quota under its journal fence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageUploadIntent {
    pub key: String,
    pub bytes: u64,
    pub sha256: Option<String>,
    pub content_type: Option<String>,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageTransferTicket {
    pub transfer_id: String,
    /// Opaque immutable source identity (download) or pending destination identity (upload).
    /// It is distinct from the capability/reservation id and is safe to expose to a Environment.
    pub object_id: String,
    pub method: String,
    pub url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub expires_at_ms: u64,
    pub max_bytes: u64,
}

pub fn stored_object_id(sha256: &str) -> String {
    format!("obj_{sha256}")
}

pub fn pending_object_id(transfer_id: &str) -> String {
    format!("obj_{transfer_id}")
}

/// One bounded step of an exhaustive session purge. `next_cursor=None` is the only completion
/// signal. On a versioned backend each object version and each delete marker counts as a deleted
/// entry; hiding current keys behind delete markers is not completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoragePurgePage {
    pub deleted_versions: u64,
    pub deleted_markers: u64,
    pub next_cursor: Option<String>,
}

/// Adapter for durable per-session objects. Keys are scoped and validated by Brain first.
#[async_trait]
pub trait SessionStoragePort: Send + Sync {
    async fn list(
        &self,
        session_id: &str,
        prefix: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<StoragePage>;
    async fn stat(&self, session_id: &str, key: &str) -> Result<StorageObject>;
    async fn read(&self, session_id: &str, key: &str, max_bytes: u64) -> Result<Vec<u8>>;
    async fn write(&self, request: StorageWriteRequest) -> Result<StorageObject>;
    async fn prepare_download(&self, session_id: &str, key: &str) -> Result<StorageTransferTicket>;
    async fn prepare_upload(&self, request: StorageUploadRequest) -> Result<StorageTransferTicket>;
    async fn complete_upload(&self, session_id: &str, transfer_id: &str) -> Result<StorageObject>;
    /// Remove every staging version for an unpublished or durably published transfer. This is
    /// idempotent and is called before Brain releases the corresponding byte reservation.
    async fn abort_upload(&self, session_id: &str, transfer_id: &str) -> Result<()>;
    async fn delete(&self, session_id: &str, key: &str) -> Result<()>;
    /// Delete every current object, historical version, and delete marker for this session.
    /// Implementations return a durable opaque cursor for a retryable next page. They must make
    /// repeating the same cursor idempotent and must not report completion while any version or
    /// marker remains.
    async fn purge_session_page(
        &self,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StoragePurgePage>;
}

fn validate_storage_key_grammar(key: &str) -> Result<()> {
    if key.is_empty()
        || key.starts_with('/')
        || key.ends_with('/')
        || key
            .split('/')
            .any(|segment| segment.is_empty() || segment == ".." || segment == ".")
    {
        return Err(crate::BrainError::Invalid(format!(
            "invalid session storage key {key:?}"
        )));
    }
    if key.len() > 1024 {
        return Err(crate::BrainError::Invalid(
            "session storage key exceeds 1024 bytes".into(),
        ));
    }
    Ok(())
}

/// Validate the public relative slash-separated key grammar. The leading `.brain` component is
/// reserved for Brain-owned quota-metered objects and cannot be named through public storage APIs.
pub fn validate_storage_key(key: &str) -> Result<()> {
    validate_storage_key_grammar(key)?;
    if is_internal_storage_key(key) {
        return Err(crate::BrainError::Invalid(
            "session storage key uses a reserved namespace".into(),
        ));
    }
    Ok(())
}

/// Validate a key minted by Brain for its own hidden session-storage namespace.
pub(crate) fn validate_internal_storage_key(key: &str) -> Result<()> {
    validate_storage_key_grammar(key)?;
    if !is_internal_storage_key(key) {
        return Err(crate::BrainError::Invalid(
            "internal session storage key is outside the reserved namespace".into(),
        ));
    }
    Ok(())
}

/// Adapter-side grammar validation. Adapters receive both public keys (already admitted by the
/// public API) and Brain-minted hidden keys, so they must preserve the reserved namespace while
/// still rejecting malformed names.
#[doc(hidden)]
pub fn validate_storage_adapter_key(key: &str) -> Result<()> {
    validate_storage_key_grammar(key)
}

pub fn is_internal_storage_key(key: &str) -> bool {
    key.starts_with(INTERNAL_SANDBOX_TRANSFER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_keys_are_relative_and_unambiguous() {
        for valid in ["report.pdf", "outputs/report.pdf", "a/b/c"] {
            validate_storage_key(valid).unwrap();
        }
        for invalid in ["", "/a", "a/", "a//b", "a/../b", "a/./b"] {
            assert!(validate_storage_key(invalid).is_err(), "{invalid}");
        }
        assert!(validate_storage_key(".brain/sandbox-transfers/xfer_123").is_err());
        validate_internal_storage_key(".brain/sandbox-transfers/xfer_123").unwrap();
        assert!(validate_internal_storage_key("ordinary/object").is_err());
    }
}
