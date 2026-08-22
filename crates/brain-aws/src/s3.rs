//! S3 implementation of durable session storage.
//!
//! Large transfers use short-lived, narrowly scoped presigned requests. Uploads land under a
//! per-session staging key and are verified before the adapter copies them into the visible
//! storage namespace. Brain therefore never proxies large bytes and an incomplete upload never
//! becomes a session object.

use async_trait::async_trait;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{ChecksumMode, Delete, MetadataDirective, ObjectIdentifier};
use base64::Engine as _;
use brain::storage::{
    BundleStoragePort, SessionStoragePort, StorageObject, StoragePage, StoragePurgePage,
    StorageTransferTicket, StorageUploadRequest, StorageWriteRequest, is_internal_storage_key,
    validate_storage_adapter_key, validate_storage_key,
};
use brain::{BrainError, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

const DEFAULT_TRANSFER_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct S3SessionStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
    transfer_ttl: Duration,
}

impl S3SessionStorage {
    pub fn new(client: aws_sdk_s3::Client, bucket: impl Into<String>) -> Self {
        Self {
            client,
            bucket: bucket.into(),
            prefix: "sessions".into(),
            transfer_ttl: DEFAULT_TRANSFER_TTL,
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Result<Self> {
        let prefix = prefix.into().trim_matches('/').to_owned();
        if prefix.is_empty() || prefix.contains("..") {
            return Err(BrainError::Invalid("S3 session prefix is invalid".into()));
        }
        self.prefix = prefix;
        Ok(self)
    }

    pub fn with_transfer_ttl(mut self, ttl: Duration) -> Result<Self> {
        if ttl.is_zero() || ttl > Duration::from_secs(60 * 60) {
            return Err(BrainError::Invalid(
                "storage transfer TTL must be between one second and one hour".into(),
            ));
        }
        self.transfer_ttl = ttl;
        Ok(self)
    }

    fn session_prefix(&self, session_id: &str) -> String {
        format!("{}/{session_id}/", self.prefix)
    }

    fn object_prefix(&self, session_id: &str) -> String {
        format!("{}storage/", self.session_prefix(session_id))
    }

    fn object_key(&self, session_id: &str, key: &str) -> Result<String> {
        validate_storage_adapter_key(key)?;
        Ok(format!("{}{key}", self.object_prefix(session_id)))
    }

    fn staging_key(&self, session_id: &str, transfer_id: &str) -> Result<String> {
        validate_transfer_id(transfer_id)?;
        Ok(format!(
            "{}transfers/{transfer_id}",
            self.session_prefix(session_id)
        ))
    }

    fn bundle_key(&self, root_id: &str, bundle_digest: &str) -> Result<String> {
        validate_bundle_digest(bundle_digest)?;
        Ok(format!(
            "{}bundles/{bundle_digest}",
            self.session_prefix(root_id)
        ))
    }

    fn relative_key<'a>(&self, session_id: &str, key: &'a str) -> Result<&'a str> {
        key.strip_prefix(&self.object_prefix(session_id))
            .ok_or_else(|| {
                BrainError::Journal("S3 returned an object outside the session prefix".into())
            })
    }

    async fn head(&self, session_id: &str, key: &str) -> Result<StorageObject> {
        let object_key = self.object_key(session_id, key)?;
        let output = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&object_key)
            .checksum_mode(ChecksumMode::Enabled)
            .send()
            .await
            .map_err(|error| s3_error("head session object", error))?;
        let metadata = output.metadata();
        // Session objects are written exclusively by this composition with length and sha256
        // metadata; a HEAD without them is an unexpected object, not a 0-byte/empty-digest one.
        let updated_at_ms = output
            .last_modified()
            .and_then(|time| time.to_millis().ok())
            .ok_or_else(|| {
                BrainError::Journal(format!("session object {object_key} has no last-modified"))
            })? as u64;
        let bytes = output
            .content_length()
            .filter(|length| *length >= 0)
            .ok_or_else(|| {
                BrainError::Journal(format!("session object {object_key} has no content length"))
            })? as u64;
        Ok(StorageObject {
            key: key.into(),
            bytes,
            sha256: metadata
                .and_then(|values| values.get("sha256"))
                .cloned()
                .ok_or_else(|| {
                    BrainError::Journal(format!(
                        "session object {object_key} has no sha256 metadata"
                    ))
                })?,
            content_type: output.content_type().map(str::to_owned),
            publication_id: metadata
                .and_then(|values| values.get("publication-id"))
                .cloned(),
            created_at_ms: metadata
                .and_then(|values| values.get("created-ms"))
                .and_then(|value| value.parse().ok())
                .unwrap_or(updated_at_ms),
            updated_at_ms,
        })
    }

    /// Delete every version and delete marker for one exact object key. The exact equality check
    /// is load-bearing because S3's prefix query can also return adjacent longer keys. Restarting
    /// after each deleted page avoids relying on a marker whose referenced version was removed.
    async fn delete_exact_versions(
        &self,
        object_key: &str,
        keep_version: Option<&str>,
    ) -> Result<()> {
        let mut cursor = PurgeCursor::default();
        loop {
            let output = self
                .client
                .list_object_versions()
                .bucket(&self.bucket)
                .prefix(object_key)
                .set_key_marker(cursor.key_marker.clone())
                .set_version_id_marker(cursor.version_id_marker.clone())
                .max_keys(1_000)
                .send()
                .await
                .map_err(|error| s3_error("list exact object versions", error))?;
            let mut identifiers = Vec::new();
            for version in output.versions() {
                if let (Some(key), Some(version_id)) = (version.key(), version.version_id())
                    && key == object_key
                    && keep_version != Some(version_id)
                {
                    identifiers.push(object_identifier(key, version_id)?);
                }
            }
            for marker in output.delete_markers() {
                if let (Some(key), Some(version_id)) = (marker.key(), marker.version_id())
                    && key == object_key
                {
                    identifiers.push(object_identifier(key, version_id)?);
                }
            }
            if !identifiers.is_empty() {
                self.delete_identifiers(identifiers, "delete exact session object versions")
                    .await?;
                cursor = PurgeCursor::default();
                continue;
            }
            if output.is_truncated().unwrap_or(false) {
                cursor = PurgeCursor {
                    key_marker: output.next_key_marker().map(str::to_owned),
                    version_id_marker: output.next_version_id_marker().map(str::to_owned),
                };
                continue;
            }
            return Ok(());
        }
    }

    async fn delete_identifiers(
        &self,
        identifiers: Vec<ObjectIdentifier>,
        context: &str,
    ) -> Result<()> {
        let delete = Delete::builder()
            .set_objects(Some(identifiers))
            .quiet(true)
            .build()
            .map_err(|error| BrainError::Journal(format!("build version deletion: {error}")))?;
        let deleted = self
            .client
            .delete_objects()
            .bucket(&self.bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|error| s3_error(context, error))?;
        if let Some(error) = deleted.errors().first() {
            return Err(BrainError::Journal(format!(
                "{context} {}: {}",
                error.key().unwrap_or("<unknown>"),
                error.message().unwrap_or("S3 deletion failed")
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl BundleStoragePort for S3SessionStorage {
    async fn store_bundle(
        &self,
        root_id: &str,
        bundle_digest: &str,
        bytes: &[u8],
    ) -> Result<brain_protocol::hand::ObjectReference> {
        validate_bundle_digest(bundle_digest)?;
        if bytes.is_empty() || bytes.len() > brain_protocol::MAX_TOOL_BUNDLE_BYTES {
            return Err(BrainError::FileTooLarge {
                limit: brain_protocol::MAX_TOOL_BUNDLE_BYTES,
            });
        }
        if hex::encode(Sha256::digest(bytes)) != bundle_digest {
            return Err(BrainError::Invalid(
                "Tool bundle bytes do not match their immutable digest".into(),
            ));
        }
        let key = self.bundle_key(root_id, bundle_digest)?;
        let written = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(bytes.to_vec()))
            .content_type("application/javascript+esm")
            .metadata("sha256", bundle_digest)
            .send()
            .await
            .map_err(|error| s3_error("store immutable Tool bundle", error))?;
        self.delete_exact_versions(&key, Some(current_version_id(written.version_id())))
            .await?;
        bundle_object_reference(bundle_digest, bytes.len() as u64)
    }

    async fn prepare_bundle_fetch(
        &self,
        root_id: &str,
        bundle_digest: &str,
    ) -> Result<brain_protocol::hand::BundleFetch> {
        let key = self.bundle_key(root_id, bundle_digest)?;
        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|error| s3_error("head immutable Tool bundle", error))?;
        let bytes = head.content_length().unwrap_or(0).max(0) as u64;
        if head
            .metadata()
            .and_then(|metadata| metadata.get("sha256"))
            .map(String::as_str)
            != Some(bundle_digest)
            || bytes == 0
            || bytes > brain_protocol::MAX_TOOL_BUNDLE_BYTES as u64
        {
            return Err(BrainError::Journal(
                "immutable Tool bundle metadata conflicts with its digest or size seal".into(),
            ));
        }
        let request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(
                PresigningConfig::expires_in(self.transfer_ttl).map_err(|error| {
                    BrainError::Journal(format!("presign bundle fetch: {error}"))
                })?,
            )
            .await
            .map_err(|error| BrainError::Journal(format!("presign bundle fetch: {error}")))?;
        serde_json::from_value(serde_json::json!({
            "bundle_digest": bundle_digest,
            "url": request.uri().to_string(),
            "headers": request.headers().map(|(name, value)| (name.to_string(), value.to_string())).collect::<HashMap<_, _>>(),
            "expires_at_ms": brain::wall_ms().saturating_add(self.transfer_ttl.as_millis() as u64),
            "max_bytes": bytes,
        }))
        .map_err(BrainError::from)
    }

    async fn purge_root_bundles(&self, root_id: &str) -> Result<()> {
        let prefix = format!("{}bundles/", self.session_prefix(root_id));
        loop {
            let output = self
                .client
                .list_object_versions()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .max_keys(1_000)
                .send()
                .await
                .map_err(|error| s3_error("list rejected root Tool bundles", error))?;
            let mut identifiers =
                Vec::with_capacity(output.versions().len() + output.delete_markers().len());
            for version in output.versions() {
                if let (Some(key), Some(version_id)) = (version.key(), version.version_id()) {
                    identifiers.push(object_identifier(key, version_id)?);
                }
            }
            for marker in output.delete_markers() {
                if let (Some(key), Some(version_id)) = (marker.key(), marker.version_id()) {
                    identifiers.push(object_identifier(key, version_id)?);
                }
            }
            if identifiers.is_empty() {
                return Ok(());
            }
            self.delete_identifiers(identifiers, "purge rejected root Tool bundles")
                .await?;
        }
    }
}

fn validate_bundle_digest(bundle_digest: &str) -> Result<()> {
    if bundle_digest.len() != 64
        || !bundle_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(BrainError::Invalid(
            "Tool bundle digest must be lowercase SHA-256".into(),
        ));
    }
    Ok(())
}

fn bundle_object_reference(
    bundle_digest: &str,
    bytes: u64,
) -> Result<brain_protocol::hand::ObjectReference> {
    serde_json::from_value(serde_json::json!({
        "object_id": format!("bundle_{bundle_digest}"),
        "bytes": bytes,
        "sha256": bundle_digest,
        "media_type": "application/javascript+esm",
    }))
    .map_err(BrainError::from)
}

#[async_trait]
impl SessionStoragePort for S3SessionStorage {
    async fn list(
        &self,
        session_id: &str,
        prefix: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<StoragePage> {
        if let Some(prefix) = prefix
            && !prefix.is_empty()
        {
            validate_storage_key(prefix.trim_end_matches('/'))?;
        }
        let storage_prefix = format!("{}{}", self.object_prefix(session_id), prefix.unwrap_or(""));
        let output = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(storage_prefix)
            .set_continuation_token(cursor.map(str::to_owned))
            .max_keys(limit.clamp(1, 1_000) as i32)
            .send()
            .await
            .map_err(|error| s3_error("list session objects", error))?;
        let mut objects = Vec::with_capacity(output.contents().len());
        for item in output.contents() {
            let Some(key) = item.key() else { continue };
            let relative = self.relative_key(session_id, key)?;
            if is_internal_storage_key(relative) {
                continue;
            }
            // One bounded HEAD per listed item is intentionally avoided. Checksums and content
            // type are populated by `stat`; list remains a single indexed S3 request.
            let updated_at_ms = item
                .last_modified()
                .and_then(|time| time.to_millis().ok())
                .unwrap_or(0) as u64;
            objects.push(StorageObject {
                key: relative.into(),
                bytes: item.size().unwrap_or(0).max(0) as u64,
                sha256: String::new(),
                content_type: None,
                publication_id: None,
                created_at_ms: updated_at_ms,
                updated_at_ms,
            });
        }
        Ok(StoragePage {
            objects,
            next_cursor: output.next_continuation_token().map(str::to_owned),
        })
    }

    async fn stat(&self, session_id: &str, key: &str) -> Result<StorageObject> {
        self.head(session_id, key).await
    }

    async fn read(&self, session_id: &str, key: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let key = self.object_key(session_id, key)?;
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| s3_error("read session object", error))?;
        if output.content_length().unwrap_or(0).max(0) as u64 > max_bytes {
            return Err(BrainError::FileTooLarge {
                limit: max_bytes.min(usize::MAX as u64) as usize,
            });
        }
        let bytes = output
            .body
            .collect()
            .await
            .map(|bytes| bytes.into_bytes().to_vec())
            .map_err(|error| BrainError::Journal(format!("read session object body: {error}")))?;
        if bytes.len() as u64 > max_bytes {
            return Err(BrainError::FileTooLarge {
                limit: max_bytes.min(usize::MAX as u64) as usize,
            });
        }
        Ok(bytes)
    }

    async fn write(&self, request: StorageWriteRequest) -> Result<StorageObject> {
        validate_transfer_id(&request.publication_id)?;
        let key = self.object_key(&request.session_id, &request.key)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&request.content_base64)
            .map_err(|_| BrainError::Invalid("content_base64 is not valid base64".into()))?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let now = brain::wall_ms();
        let mut put = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(bytes))
            .metadata("sha256", &sha256)
            .metadata("publication-id", &request.publication_id)
            .metadata("created-ms", now.to_string());
        if let Some(content_type) = &request.content_type {
            put = put.content_type(content_type);
        }
        if !request.overwrite {
            put = put.if_none_match("*");
        }
        let written = put
            .send()
            .await
            .map_err(|error| s3_error("write session object", error))?;
        // A suspended-version bucket can retain numbered historical versions even though the new
        // current object is the `null` version. Overwrite means replacement, not retention: keep
        // only the just-written version and remove every older version and marker.
        if request.overwrite {
            // S3 omits x-amz-version-id for unversioned and, in some SDK responses, suspended
            // buckets. In both cases the current object's version identity is `null`; passing
            // `None` here would erase the object we just acknowledged.
            self.delete_exact_versions(&key, Some(current_version_id(written.version_id())))
                .await?;
        }
        self.head(&request.session_id, &request.key).await
    }

    async fn prepare_download(&self, session_id: &str, key: &str) -> Result<StorageTransferTicket> {
        let object = self.head(session_id, key).await?;
        let request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(self.object_key(session_id, key)?)
            .presigned(
                PresigningConfig::expires_in(self.transfer_ttl)
                    .map_err(|error| BrainError::Journal(format!("presign download: {error}")))?,
            )
            .await
            .map_err(|error| BrainError::Journal(format!("presign download: {error}")))?;
        Ok(StorageTransferTicket {
            transfer_id: brain::mint_id("xfer", 24),
            object_id: brain::storage::stored_object_id(&object.sha256),
            method: request.method().into(),
            url: request.uri().to_string(),
            headers: request
                .headers()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            expires_at_ms: brain::wall_ms() + self.transfer_ttl.as_millis() as u64,
            max_bytes: object.bytes,
        })
    }

    async fn prepare_upload(&self, request: StorageUploadRequest) -> Result<StorageTransferTicket> {
        validate_storage_adapter_key(&request.key)?;
        if let Some(sha256) = &request.sha256 {
            validate_sha256(sha256)?;
        }
        validate_transfer_id(&request.transfer_id)?;
        if !request.overwrite {
            match self.head(&request.session_id, &request.key).await {
                Ok(_) => {
                    return Err(BrainError::Invalid(format!(
                        "session storage object {} already exists",
                        request.key
                    )));
                }
                Err(BrainError::FileNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let now = brain::wall_ms();
        let remaining_ms = request.expires_at_ms.saturating_sub(now);
        if remaining_ms == 0 || remaining_ms > self.transfer_ttl.as_millis() as u64 {
            return Err(BrainError::Invalid(
                "storage upload expiry is outside the configured transfer TTL".into(),
            ));
        }
        let staging_key = self.staging_key(&request.session_id, &request.transfer_id)?;
        let mut put = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(staging_key)
            .content_length(request.bytes as i64)
            .metadata("destination-key", &request.key)
            .metadata("expected-bytes", request.bytes.to_string())
            .metadata("publication-id", &request.transfer_id)
            .metadata("created-ms", now.to_string())
            .metadata("overwrite", request.overwrite.to_string());
        if let Some(sha256) = &request.sha256 {
            let checksum =
                base64::engine::general_purpose::STANDARD.encode(hex::decode(sha256).map_err(
                    |_| BrainError::Invalid("sha256 must be lowercase hexadecimal".into()),
                )?);
            put = put.checksum_sha256(checksum).metadata("sha256", sha256);
        }
        if let Some(content_type) = &request.content_type {
            put = put.content_type(content_type);
        }
        let presigned = put
            .presigned(
                PresigningConfig::expires_in(Duration::from_millis(remaining_ms))
                    .map_err(|error| BrainError::Journal(format!("presign upload: {error}")))?,
            )
            .await
            .map_err(|error| BrainError::Journal(format!("presign upload: {error}")))?;
        let mut headers: HashMap<_, _> = presigned
            .headers()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        headers.insert("content-length".into(), request.bytes.to_string());
        Ok(StorageTransferTicket {
            object_id: brain::storage::pending_object_id(&request.transfer_id),
            transfer_id: request.transfer_id,
            method: presigned.method().into(),
            url: presigned.uri().to_string(),
            headers,
            expires_at_ms: request.expires_at_ms,
            max_bytes: request.bytes,
        })
    }

    async fn complete_upload(&self, session_id: &str, transfer_id: &str) -> Result<StorageObject> {
        let staging_key = self.staging_key(session_id, transfer_id)?;
        let staged = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&staging_key)
            .checksum_mode(ChecksumMode::Enabled)
            .send()
            .await
            .map_err(|error| s3_error("inspect staged upload", error))?;
        let metadata = staged.metadata().ok_or_else(|| {
            BrainError::Journal("staged upload is missing sealed metadata".into())
        })?;
        let key = metadata
            .get("destination-key")
            .ok_or_else(|| BrainError::Journal("staged upload is missing destination".into()))?;
        validate_storage_adapter_key(key)?;
        let expected_bytes: u64 = metadata
            .get("expected-bytes")
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| BrainError::Journal("staged upload is missing expected bytes".into()))?;
        if staged.content_length().unwrap_or(-1) != expected_bytes as i64 {
            return Err(BrainError::Invalid(
                "staged upload byte length does not match".into(),
            ));
        }
        let expected_sha = metadata.get("sha256");
        let publication_id = metadata.get("publication-id").ok_or_else(|| {
            BrainError::Journal("staged upload is missing publication provenance".into())
        })?;
        if publication_id != transfer_id {
            return Err(BrainError::Journal(
                "staged upload publication provenance does not match its transfer".into(),
            ));
        }
        let actual_sha = staged
            .checksum_sha256()
            .and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())
            .map(hex::encode)
            .ok_or_else(|| BrainError::Invalid("staged upload has no verified SHA-256".into()))?;
        if expected_sha.is_some_and(|expected| actual_sha != *expected) {
            return Err(BrainError::Invalid(
                "staged upload SHA-256 does not match".into(),
            ));
        }
        let overwrite = metadata
            .get("overwrite")
            .is_some_and(|value| value == "true");
        if !overwrite {
            match self.head(session_id, key).await {
                Ok(existing)
                    if existing.bytes == expected_bytes
                        && existing.sha256 == actual_sha
                        && existing.publication_id.as_deref() == Some(transfer_id) =>
                {
                    // Lost completion responses are safe to retry: the published object proves
                    // this sealed staging upload already completed.
                    return Ok(existing);
                }
                Ok(_) => {
                    return Err(BrainError::Invalid(format!(
                        "session storage object {key} already exists"
                    )));
                }
                Err(BrainError::FileNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let destination = self.object_key(session_id, key)?;
        let mut copy = self
            .client
            .copy_object()
            .bucket(&self.bucket)
            .key(&destination)
            .copy_source(format!("{}/{}", self.bucket, staging_key))
            .metadata_directive(MetadataDirective::Replace)
            .metadata("sha256", &actual_sha)
            .metadata("publication-id", transfer_id)
            .metadata(
                "created-ms",
                metadata
                    .get("created-ms")
                    .cloned()
                    .unwrap_or_else(|| brain::wall_ms().to_string()),
            );
        if let Some(content_type) = staged.content_type() {
            copy = copy.content_type(content_type);
        }
        let copied = copy
            .send()
            .await
            .map_err(|error| s3_error("publish staged upload", error))?;
        if overwrite {
            self.delete_exact_versions(&destination, Some(current_version_id(copied.version_id())))
                .await?;
        }
        // Keep the sealed staging object until explicit session deletion. A retry after a lost
        // completion response can therefore verify and reproduce the same publication instead of
        // becoming an ambiguous NotFound. Session purge includes the transfers namespace.
        self.head(session_id, key).await
    }

    async fn abort_upload(&self, session_id: &str, transfer_id: &str) -> Result<()> {
        let staging_key = self.staging_key(session_id, transfer_id)?;
        self.delete_exact_versions(&staging_key, None).await
    }

    async fn delete(&self, session_id: &str, key: &str) -> Result<()> {
        self.delete_exact_versions(&self.object_key(session_id, key)?, None)
            .await
    }

    async fn purge_session_page(
        &self,
        session_id: &str,
        cursor: Option<&str>,
    ) -> Result<StoragePurgePage> {
        // Always restart from the beginning. S3 continuation markers can name versions that the
        // preceding request just deleted, so persisting them cannot make a lost-response retry
        // demonstrably safe. A one-bit cursor means "run another bounded first page"; an empty
        // page is the only proof that all versions and delete markers are gone.
        if let Some(cursor) = cursor
            && cursor != "again"
        {
            return Err(BrainError::Invalid(
                "storage purge cursor is malformed".into(),
            ));
        }
        let output = self
            .client
            .list_object_versions()
            .bucket(&self.bucket)
            .prefix(self.session_prefix(session_id))
            .max_keys(1_000)
            .send()
            .await
            .map_err(|error| s3_error("list session object versions", error))?;
        let mut identifiers =
            Vec::with_capacity(output.versions().len() + output.delete_markers().len());
        let mut deleted_versions = 0;
        let mut deleted_markers = 0;
        for version in output.versions() {
            if let (Some(key), Some(version_id)) = (version.key(), version.version_id()) {
                identifiers.push(object_identifier(key, version_id)?);
                deleted_versions += 1;
            }
        }
        for marker in output.delete_markers() {
            if let (Some(key), Some(version_id)) = (marker.key(), marker.version_id()) {
                identifiers.push(object_identifier(key, version_id)?);
                deleted_markers += 1;
            }
        }
        let had_entries = !identifiers.is_empty();
        if had_entries {
            self.delete_identifiers(identifiers, "purge session object versions")
                .await?;
        }
        let next_cursor = purge_next_cursor(had_entries);
        Ok(StoragePurgePage {
            deleted_versions,
            deleted_markers,
            next_cursor,
        })
    }
}

fn object_identifier(key: &str, version_id: &str) -> Result<ObjectIdentifier> {
    ObjectIdentifier::builder()
        .key(key)
        .version_id(version_id)
        .build()
        .map_err(|error| BrainError::Journal(format!("build version identifier: {error}")))
}

fn current_version_id(response_version_id: Option<&str>) -> &str {
    response_version_id.unwrap_or("null")
}

fn purge_next_cursor(had_entries: bool) -> Option<String> {
    had_entries.then(|| "again".to_owned())
}

#[derive(Default)]
struct PurgeCursor {
    key_marker: Option<String>,
    version_id_marker: Option<String>,
}

fn validate_transfer_id(value: &str) -> Result<()> {
    if value.len() < 8
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(BrainError::Invalid("transfer id is malformed".into()));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(BrainError::Invalid(
            "sha256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

fn s3_error<E: ProvideErrorMetadata>(
    context: &str,
    error: aws_sdk_s3::error::SdkError<E>,
) -> BrainError {
    let code = error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code);
    match code {
        Some("NoSuchKey") | Some("NotFound") => BrainError::FileNotFound(context.into()),
        _ => BrainError::Journal(format!("{context}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_cursor_is_a_restart_token() {
        assert_eq!(purge_next_cursor(true).as_deref(), Some("again"));
        assert_eq!(purge_next_cursor(false), None);
        // A lost response repeats the same restart token; it never refers to a deleted version.
        assert_eq!(purge_next_cursor(true), purge_next_cursor(true));
    }

    #[test]
    fn missing_version_header_preserves_the_current_null_version() {
        assert_eq!(current_version_id(None), "null");
        assert_eq!(current_version_id(Some("version-7")), "version-7");
    }

    #[test]
    fn checksum_and_transfer_ids_are_closed_tokens() {
        validate_sha256(&"a".repeat(64)).unwrap();
        assert!(validate_sha256(&"A".repeat(64)).is_err());
        validate_transfer_id("xfer_12345678").unwrap();
        assert!(validate_transfer_id("../escape").is_err());
    }
}
