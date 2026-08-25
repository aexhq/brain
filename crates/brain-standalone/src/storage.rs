//! Durable filesystem-backed session storage for the explicit local composition.
//!
//! Object names are never used as host paths. Each validated logical key maps to a SHA-256 file
//! name and the exact key remains in a small metadata document, avoiding platform-specific path
//! syntax and traversal ambiguity. Large presigned transfer capabilities are a hosted-adapter
//! feature; local mode supports the bounded inline API through the same neutral port.

use async_trait::async_trait;
use base64::Engine as _;
use brain::storage::{
    BundleStoragePort, SessionStoragePort, StorageObject, StoragePage, StoragePurgePage,
    StorageTransferTicket, StorageUploadRequest, StorageWriteRequest, is_internal_storage_key,
    validate_storage_adapter_key,
};
use brain::{BrainError, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct LocalSessionStorage {
    root: PathBuf,
}

impl LocalSessionStorage {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|error| BrainError::Journal(format!("create local storage: {error}")))?;
        Ok(Self { root })
    }

    fn session_dir(&self, session_id: &str) -> Result<PathBuf> {
        validate_session_id(session_id)?;
        Ok(self
            .root
            .join(hex::encode(Sha256::digest(session_id.as_bytes()))))
    }

    fn object_paths(&self, session_id: &str, key: &str) -> Result<(PathBuf, PathBuf)> {
        validate_storage_adapter_key(key)?;
        let stem = hex::encode(Sha256::digest(key.as_bytes()));
        let session = self.session_dir(session_id)?;
        Ok((
            session.join("objects").join(format!("{stem}.bin")),
            session.join("metadata").join(format!("{stem}.json")),
        ))
    }

    fn bundle_path(&self, root_id: &str, bundle_digest: &str) -> Result<PathBuf> {
        validate_bundle_digest(bundle_digest)?;
        Ok(self
            .session_dir(root_id)?
            .join("bundles")
            .join(format!("{bundle_digest}.mjs")))
    }

    async fn load_metadata(&self, session_id: &str, key: &str) -> Result<StorageObject> {
        let (object_path, metadata_path) = self.object_paths(session_id, key)?;
        let bytes = tokio::fs::read(&metadata_path)
            .await
            .map_err(|error| file_error(key, error))?;
        let object: StorageObject = serde_json::from_slice(&bytes).map_err(|error| {
            BrainError::Journal(format!("decode local storage metadata: {error}"))
        })?;
        if object.key != key {
            return Err(BrainError::Journal(
                "local storage metadata key digest collision".into(),
            ));
        }
        let actual = tokio::fs::metadata(&object_path)
            .await
            .map_err(|error| file_error(key, error))?;
        if actual.len() != object.bytes {
            return Err(BrainError::Journal(
                "local storage bytes disagree with metadata".into(),
            ));
        }
        Ok(object)
    }
}

#[async_trait]
impl BundleStoragePort for LocalSessionStorage {
    async fn store_bundle(
        &self,
        root_id: &str,
        bundle_digest: &str,
        bytes: &[u8],
    ) -> Result<brain_protocol::environment::ObjectReference> {
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
        let path = self.bundle_path(root_id, bundle_digest)?;
        if let Ok(existing) = tokio::fs::read(&path).await {
            if existing == bytes {
                return bundle_object_reference(bundle_digest, bytes.len() as u64);
            }
            return Err(BrainError::Journal(
                "immutable local Tool bundle digest collision".into(),
            ));
        }
        tokio::fs::create_dir_all(path.parent().expect("bundle parent"))
            .await
            .map_err(|error| {
                BrainError::Journal(format!("create local bundle directory: {error}"))
            })?;
        atomic_write(&path, bytes).await?;
        bundle_object_reference(bundle_digest, bytes.len() as u64)
    }

    async fn prepare_bundle_fetch(
        &self,
        root_id: &str,
        bundle_digest: &str,
    ) -> Result<brain_protocol::environment::BundleFetch> {
        let path = self.bundle_path(root_id, bundle_digest)?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| file_error(bundle_digest, error))?;
        if hex::encode(Sha256::digest(&bytes)) != bundle_digest {
            return Err(BrainError::Journal(
                "durable local Tool bundle no longer matches its digest".into(),
            ));
        }
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(|error| BrainError::Journal(format!("canonicalize local bundle: {error}")))?;
        let url = url::Url::from_file_path(path)
            .map_err(|_| BrainError::Journal("local Tool bundle path is not a file URL".into()))?;
        serde_json::from_value(serde_json::json!({
            "bundle_digest": bundle_digest,
            "url": url.as_str(),
            "headers": {},
            "expires_at_ms": brain::wall_ms().saturating_add(15 * 60 * 1_000),
            "max_bytes": bytes.len(),
        }))
        .map_err(BrainError::from)
    }

    async fn purge_root_bundles(&self, root_id: &str) -> Result<()> {
        let path = self.session_dir(root_id)?.join("bundles");
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(BrainError::Journal(format!(
                "purge local Tool bundles: {error}"
            ))),
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
) -> Result<brain_protocol::environment::ObjectReference> {
    serde_json::from_value(serde_json::json!({
        "object_id": format!("bundle_{bundle_digest}"),
        "bytes": bytes,
        "sha256": bundle_digest,
        "media_type": "application/javascript+esm",
    }))
    .map_err(BrainError::from)
}

#[async_trait]
impl SessionStoragePort for LocalSessionStorage {
    async fn list(
        &self,
        session_id: &str,
        prefix: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<StoragePage> {
        let metadata_dir = self.session_dir(session_id)?.join("metadata");
        let mut objects = Vec::new();
        match tokio::fs::read_dir(&metadata_dir).await {
            Ok(mut entries) => {
                while let Some(entry) = entries
                    .next_entry()
                    .await
                    .map_err(|error| BrainError::Journal(format!("list local storage: {error}")))?
                {
                    let bytes = tokio::fs::read(entry.path()).await.map_err(|error| {
                        BrainError::Journal(format!("read local storage metadata: {error}"))
                    })?;
                    let object: StorageObject =
                        serde_json::from_slice(&bytes).map_err(|error| {
                            BrainError::Journal(format!("decode local storage metadata: {error}"))
                        })?;
                    if !is_internal_storage_key(&object.key)
                        && prefix.is_none_or(|prefix| object.key.starts_with(prefix))
                        && cursor.is_none_or(|cursor| object.key.as_str() > cursor)
                    {
                        objects.push(object);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BrainError::Journal(format!(
                    "list local storage metadata: {error}"
                )));
            }
        }
        objects.sort_by(|left, right| left.key.cmp(&right.key));
        let limit = limit.clamp(1, 1_000) as usize;
        let next_cursor = (objects.len() > limit).then(|| objects[limit - 1].key.clone());
        objects.truncate(limit);
        Ok(StoragePage {
            objects,
            next_cursor,
        })
    }

    async fn stat(&self, session_id: &str, key: &str) -> Result<StorageObject> {
        self.load_metadata(session_id, key).await
    }

    async fn read(&self, session_id: &str, key: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let object = self.load_metadata(session_id, key).await?;
        if object.bytes > max_bytes {
            return Err(BrainError::FileTooLarge {
                limit: max_bytes.min(usize::MAX as u64) as usize,
            });
        }
        let (path, _) = self.object_paths(session_id, key)?;
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|error| file_error(key, error))?;
        if bytes.len() as u64 > max_bytes {
            return Err(BrainError::FileTooLarge {
                limit: max_bytes.min(usize::MAX as u64) as usize,
            });
        }
        Ok(bytes)
    }

    async fn write(&self, request: StorageWriteRequest) -> Result<StorageObject> {
        let (object_path, metadata_path) = self.object_paths(&request.session_id, &request.key)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&request.content_base64)
            .map_err(|_| BrainError::Invalid("content_base64 is not valid base64".into()))?;
        if !request.overwrite && tokio::fs::try_exists(&object_path).await.unwrap_or(false) {
            return Err(BrainError::Invalid(format!(
                "session storage object {} already exists",
                request.key
            )));
        }
        let now = brain::wall_ms();
        let created_at_ms = self
            .load_metadata(&request.session_id, &request.key)
            .await
            .ok()
            .map_or(now, |object| object.created_at_ms);
        let object = StorageObject {
            key: request.key,
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            content_type: request.content_type,
            publication_id: Some(request.publication_id),
            created_at_ms,
            updated_at_ms: now,
        };
        let parent = object_path
            .parent()
            .ok_or_else(|| BrainError::Journal("local storage object has no parent".into()))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| BrainError::Journal(format!("create local objects: {error}")))?;
        tokio::fs::create_dir_all(metadata_path.parent().expect("metadata parent"))
            .await
            .map_err(|error| BrainError::Journal(format!("create local metadata: {error}")))?;
        atomic_write(&object_path, &bytes).await?;
        atomic_write(&metadata_path, &serde_json::to_vec(&object)?).await?;
        Ok(object)
    }

    async fn prepare_download(
        &self,
        _session_id: &str,
        _key: &str,
    ) -> Result<StorageTransferTicket> {
        Err(BrainError::Invalid(
            "large transfer tickets are unavailable in local mode; use the 1 MiB inline API".into(),
        ))
    }

    async fn prepare_upload(
        &self,
        _request: StorageUploadRequest,
    ) -> Result<StorageTransferTicket> {
        Err(BrainError::Invalid(
            "large transfer tickets are unavailable in local mode; use the 1 MiB inline API".into(),
        ))
    }

    async fn complete_upload(&self, _session_id: &str, transfer_id: &str) -> Result<StorageObject> {
        Err(BrainError::FileNotFound(format!(
            "local storage upload {transfer_id}"
        )))
    }

    async fn abort_upload(&self, _session_id: &str, _transfer_id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete(&self, session_id: &str, key: &str) -> Result<()> {
        let (object, metadata) = self.object_paths(session_id, key)?;
        remove_if_exists(&object).await?;
        remove_if_exists(&metadata).await
    }

    async fn purge_session_page(
        &self,
        session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<StoragePurgePage> {
        let dir = self.session_dir(session_id)?;
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BrainError::Journal(format!(
                    "purge local session storage: {error}"
                )));
            }
        }
        Ok(StoragePurgePage {
            deleted_versions: 0,
            deleted_markers: 0,
            next_cursor: None,
        })
    }
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", brain::mint_id("w", 12)));
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|error| BrainError::Journal(format!("write local storage: {error}")))?;
    if let Err(error) = tokio::fs::rename(&temporary, path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(BrainError::Journal(format!(
            "publish local storage: {error}"
        )));
    }
    Ok(())
}

async fn remove_if_exists(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BrainError::Journal(format!(
            "delete local storage: {error}"
        ))),
    }
}

fn file_error(key: &str, error: std::io::Error) -> BrainError {
    if error.kind() == std::io::ErrorKind::NotFound {
        BrainError::FileNotFound(key.into())
    } else {
        BrainError::Journal(format!("local storage {key}: {error}"))
    }
}

fn validate_session_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(BrainError::Invalid("session id is malformed".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_objects_survive_reopen_and_keep_keys_out_of_host_paths() {
        let root = std::env::temp_dir().join(format!(
            "brain-local-storage-{}-{}",
            std::process::id(),
            brain::wall_ms()
        ));
        let first = LocalSessionStorage::open(&root).unwrap();
        let object = first
            .write(StorageWriteRequest {
                session_id: "ses_localstorage00000000".into(),
                publication_id: "xfer_localstorage00000000".into(),
                key: "reports/final.txt".into(),
                content_base64: base64::engine::general_purpose::STANDARD.encode(b"durable"),
                content_type: Some("text/plain".into()),
                overwrite: false,
            })
            .await
            .unwrap();
        assert_eq!(object.bytes, 7);
        let reopened = LocalSessionStorage::open(&root).unwrap();
        assert_eq!(
            reopened
                .read("ses_localstorage00000000", "reports/final.txt", 7)
                .await
                .unwrap(),
            b"durable"
        );
        assert!(!root.join("reports").exists());
        reopened
            .purge_session_page("ses_localstorage00000000", None)
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn managed_bundles_are_root_durable_hidden_and_refetched_by_digest() {
        let root = std::env::temp_dir().join(format!(
            "brain-local-bundles-{}-{}",
            std::process::id(),
            brain::wall_ms()
        ));
        let bytes = b"export default async function fixture() { return 7; }";
        let digest = hex::encode(Sha256::digest(bytes));
        let first = LocalSessionStorage::open(&root).unwrap();
        let object = first
            .store_bundle("ses_bundle_root", &digest, bytes)
            .await
            .unwrap();
        assert_eq!(object.object_id.as_str(), format!("bundle_{digest}"));
        assert_eq!(object.sha256.as_str(), digest);
        assert_eq!(object.bytes, bytes.len() as u64);
        assert_eq!(
            object.media_type.as_deref().map(String::as_str),
            Some("application/javascript+esm")
        );
        assert!(
            first
                .list("ses_bundle_root", None, None, 100)
                .await
                .unwrap()
                .objects
                .is_empty(),
            "internal bundles must never appear in user storage"
        );

        let reopened = LocalSessionStorage::open(&root).unwrap();
        let fetch = reopened
            .prepare_bundle_fetch("ses_bundle_root", &digest)
            .await
            .unwrap();
        assert_eq!(fetch.bundle_digest.as_str(), digest);
        assert_eq!(fetch.max_bytes.get(), bytes.len() as u64);
        let path = url::Url::parse(fetch.url.as_str())
            .unwrap()
            .to_file_path()
            .unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), bytes);

        reopened
            .purge_session_page("ses_bundle_root", None)
            .await
            .unwrap();
        assert!(
            reopened
                .prepare_bundle_fetch("ses_bundle_root", &digest)
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sandbox_transfer_staging_is_hidden_from_local_public_pages() {
        let root = std::env::temp_dir().join(format!(
            "brain-local-hidden-transfer-{}-{}",
            std::process::id(),
            brain::wall_ms()
        ));
        let storage = LocalSessionStorage::open(&root).unwrap();
        let session_id = "ses_hidden_transfer";
        for (key, body) in [
            ("visible.txt", b"visible".as_slice()),
            (
                ".brain/sandbox-transfers/sbxfer_hidden",
                b"hidden".as_slice(),
            ),
        ] {
            storage
                .write(StorageWriteRequest {
                    session_id: session_id.into(),
                    publication_id: format!("pub_{}", hex::encode(Sha256::digest(key.as_bytes()))),
                    key: key.into(),
                    content_base64: base64::engine::general_purpose::STANDARD.encode(body),
                    content_type: None,
                    overwrite: false,
                })
                .await
                .unwrap();
        }
        let page = storage.list(session_id, None, None, 100).await.unwrap();
        assert_eq!(
            page.objects
                .iter()
                .map(|object| object.key.as_str())
                .collect::<Vec<_>>(),
            vec!["visible.txt"]
        );
        assert_eq!(page.next_cursor, None);
        storage.purge_session_page(session_id, None).await.unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
