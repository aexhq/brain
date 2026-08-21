//! The closed `brain.storage` capability executes in Brain over the typed storage port.

use async_trait::async_trait;
use brain::adapter::DisabledToolExecutor;
use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::hand::{
    HandResult, SandboxFileContent, SandboxFileList, SandboxFileListRequest, SandboxSearchRequest,
};
use brain::journal::{Journal, Record};
use brain::message::{Message, StopReason, Usage};
use brain::provider::{ModelRequest, Provider, ProviderEvent};
use brain::session::{Brain, BrainConfig, BrainServices};
use brain::storage::{
    SessionStoragePort, StorageObject, StoragePage, StoragePurgePage, StorageTransferTicket,
    StorageUploadRequest, StorageWriteRequest,
};
use brain::{BrainError, Result};
use brain_protocol::hand::{
    FileEntry, SandboxCopyRequest, SandboxCopyResult, SandboxFileRequest, SandboxFileWriteRequest,
    SandboxStatus,
};
use brain_protocol::session::{CreateSessionRequest, MessageRequestContent};
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct MemoryStorage {
    objects: Mutex<MemoryObjects>,
}

type MemoryObjects = HashMap<(String, String), (StorageObject, Vec<u8>)>;

#[async_trait]
impl SessionStoragePort for MemoryStorage {
    async fn list(
        &self,
        session_id: &str,
        prefix: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<StoragePage> {
        let mut objects = self
            .objects
            .lock()
            .expect("storage")
            .iter()
            .filter(|((session, key), _)| {
                session == session_id
                    && prefix.is_none_or(|prefix| key.starts_with(prefix))
                    && cursor.is_none_or(|cursor| key.as_str() > cursor)
            })
            .map(|(_, (object, _))| object.clone())
            .collect::<Vec<_>>();
        objects.sort_by(|left, right| left.key.cmp(&right.key));
        objects.truncate(limit as usize);
        Ok(StoragePage {
            objects,
            next_cursor: None,
        })
    }

    async fn stat(&self, session_id: &str, key: &str) -> Result<StorageObject> {
        self.objects
            .lock()
            .expect("storage")
            .get(&(session_id.to_owned(), key.to_owned()))
            .map(|(object, _)| object.clone())
            .ok_or_else(|| BrainError::FileNotFound(key.into()))
    }

    async fn read(&self, session_id: &str, key: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let bytes = self
            .objects
            .lock()
            .expect("storage")
            .get(&(session_id.to_owned(), key.to_owned()))
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| BrainError::FileNotFound(key.into()))?;
        if bytes.len() as u64 > max_bytes {
            return Err(BrainError::FileTooLarge {
                limit: max_bytes as usize,
            });
        }
        Ok(bytes)
    }

    async fn write(&self, request: StorageWriteRequest) -> Result<StorageObject> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(request.content_base64)
            .map_err(|_| BrainError::Invalid("base64".into()))?;
        let now = brain::wall_ms();
        let object = StorageObject {
            key: request.key.clone(),
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            content_type: request.content_type,
            publication_id: Some(request.publication_id),
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.objects
            .lock()
            .expect("storage")
            .insert((request.session_id, request.key), (object.clone(), bytes));
        Ok(object)
    }

    async fn prepare_download(
        &self,
        _session_id: &str,
        key: &str,
    ) -> Result<StorageTransferTicket> {
        Err(BrainError::FileNotFound(key.into()))
    }

    async fn prepare_upload(&self, request: StorageUploadRequest) -> Result<StorageTransferTicket> {
        Err(BrainError::FileNotFound(request.transfer_id))
    }

    async fn complete_upload(&self, _session_id: &str, transfer_id: &str) -> Result<StorageObject> {
        Err(BrainError::FileNotFound(transfer_id.into()))
    }

    async fn abort_upload(&self, _session_id: &str, _transfer_id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete(&self, session_id: &str, key: &str) -> Result<()> {
        self.objects
            .lock()
            .expect("storage")
            .remove(&(session_id.to_owned(), key.to_owned()));
        Ok(())
    }

    async fn purge_session_page(
        &self,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<StoragePurgePage> {
        Ok(StoragePurgePage {
            deleted_versions: 0,
            deleted_markers: 0,
            next_cursor: None,
        })
    }
}

struct UnusedSandboxFiles;

#[async_trait]
impl brain::hand::SandboxFilesPort for UnusedSandboxFiles {
    async fn status(
        &self,
        _target: brain_protocol::hand::SandboxTarget,
    ) -> HandResult<SandboxStatus> {
        panic!("unused")
    }
    async fn list(&self, _request: SandboxFileListRequest) -> HandResult<SandboxFileList> {
        panic!("unused")
    }
    async fn stat(&self, _request: SandboxFileRequest) -> HandResult<FileEntry> {
        panic!("unused")
    }
    async fn read(&self, _request: SandboxFileRequest) -> HandResult<SandboxFileContent> {
        panic!("unused")
    }
    async fn write(
        &self,
        _request: SandboxFileWriteRequest,
    ) -> HandResult<brain_protocol::hand::SandboxFileWriteResult> {
        panic!("unused")
    }
    async fn find(&self, _request: SandboxSearchRequest) -> HandResult<SandboxFileList> {
        panic!("unused")
    }
    async fn grep(&self, _request: SandboxSearchRequest) -> HandResult<SandboxFileList> {
        panic!("unused")
    }
    async fn transfer(&self, _request: SandboxCopyRequest) -> HandResult<SandboxCopyResult> {
        panic!("unused")
    }
}

#[derive(Debug, Default)]
struct StorageProvider;

#[async_trait]
impl Provider for StorageProvider {
    fn dialect(&self) -> Dialect {
        Dialect::AnthropicMessages
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        brain::provider::anthropic::Anthropic.build_request(prefix, history, key, base_url)
    }

    async fn stream(
        &self,
        request: ModelRequest,
        _outbound: &brain::outbound::Outbound,
    ) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        let body: Value = serde_json::from_slice(&request.body)?;
        let has_result = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .any(|block| block["type"] == "tool_result");
        let events = if has_result {
            vec![
                ProviderEvent::TextDelta {
                    index: 0,
                    text: "stored".into(),
                },
                ProviderEvent::MessageDone {
                    stop_reason: StopReason::EndTurn,
                    usage: usage(),
                },
            ]
        } else {
            vec![
                ProviderEvent::ToolUseStart {
                    index: 0,
                    id: "save-1".into(),
                    name: "storage".into(),
                },
                ProviderEvent::ToolInputDelta {
                    index: 0,
                    partial_json: serde_json::to_string(&json!({
                        "action": "save",
                        "key": "notes/result.txt",
                        "source": {"kind": "inline_text", "text": "durable result"}
                    }))?,
                },
                ProviderEvent::BlockDone { index: 0 },
                ProviderEvent::MessageDone {
                    stop_reason: StopReason::ToolUse,
                    usage: usage(),
                },
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok),
        )))
    }
}

fn usage() -> Usage {
    Usage {
        input_tokens: Some(1),
        output_tokens: Some(1),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reasoning_tokens: None,
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "brain-storage-engine-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn create_request() -> CreateSessionRequest {
    serde_json::from_value(json!({
        "model": {"provider":"anthropic", "name":"scripted", "api_key":"sk-fake"},
        "tools": {"items": [{
            "definition": {
                "name":"storage",
                "description":"durable storage",
                "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "input_schema": {
                    "type":"object",
                    "properties": {
                        "action":{"const":"save"},
                        "key":{"type":"string"},
                        "source":{
                            "type":"object",
                            "properties":{"kind":{"const":"inline_text"},"text":{"type":"string"}},
                            "required":["kind","text"]
                        }
                    },
                    "required":["action","key","source"]
                },
                "output_schema": {}
            },
            "executor":{"kind":"engine","capability":"brain.storage"}
        }]}
    })).expect("create request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_storage_commits_reservation_and_result_without_external_executor() {
    let _tmp = TempDir::new();
    let journal = Journal::new_memory("storage-engine-e2e");
    let storage = Arc::new(MemoryStorage::default());
    let provider = Arc::new(StorageProvider);
    let factory = provider.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices {
            session_storage: Some(storage.clone()),
            sandbox_files: Some(Arc::new(UnusedSandboxFiles)),
            ..BrainServices::default()
        },
        Some(Arc::new(move |_| factory.clone() as Arc<dyn Provider>)),
    );
    let session = brain
        .create_session(create_request(), Some("storage-engine"))
        .await
        .unwrap();
    let session_id = session.id.to_string();
    brain
        .message(
            &session_id,
            MessageRequestContent::String("save it".parse().unwrap()),
        )
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while brain.get(&session_id).await.unwrap().current_turn.is_some() {
        assert!(Instant::now() < deadline, "turn timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(
        storage
            .read(&session_id, "notes/result.txt", 1024)
            .await
            .unwrap(),
        b"durable result"
    );
    let records = journal.read_records(&session_id, 0).await.unwrap();
    let kinds = records
        .iter()
        .map(|entry| entry.record.kind_name())
        .collect::<Vec<_>>();
    let call = kinds.iter().position(|kind| *kind == "tool_call").unwrap();
    let reserved = kinds
        .iter()
        .position(|kind| *kind == "storage_upload_reserved")
        .unwrap();
    let completed = kinds
        .iter()
        .position(|kind| *kind == "storage_upload_completed")
        .unwrap();
    let result = kinds
        .iter()
        .position(|kind| *kind == "tool_result")
        .unwrap();
    assert!(call < reserved && reserved < completed && completed < result);
    assert!(records.iter().any(|entry| matches!(
        entry.record,
        Record::ToolResult { ref name, is_error: false, .. } if name == "storage"
    )));
}
