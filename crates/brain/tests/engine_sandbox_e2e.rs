//! The closed `brain.sandbox` capability reserves Brain-owned inventory before Hand materialization.

use async_trait::async_trait;
use brain::adapter::DisabledToolExecutor;
use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::hand::{
    HandResult, SandboxControlPort, SandboxFileContent, SandboxFileList, SandboxFileListRequest,
    SandboxSearchRequest,
};
use brain::journal::{Journal, Record, SandboxListQuery};
use brain::message::{Message, StopReason, Usage};
use brain::provider::{ModelRequest, Provider, ProviderEvent};
use brain::session::{Brain, BrainConfig, BrainServices};
use brain::storage::{
    SessionStoragePort, StorageObject, StoragePage, StoragePurgePage, StorageTransferTicket,
    StorageUploadRequest, StorageWriteRequest,
};
use brain::{BrainError, Result};
use brain_protocol::hand::{
    CreateSandboxRequest, FileEntry, SandboxCopyRequest, SandboxCopyResult,
    SandboxExecutionRequest, SandboxFileRequest, SandboxFileWriteRequest, SandboxStatus,
    SandboxTarget, SubmitReceipt, WriteStdinReceipt, WriteStdinRequest,
};
use brain_protocol::session::{CreateSessionRequest, MessageRequestContent};
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct SandboxControl {
    creates: AtomicUsize,
    status: Mutex<Option<SandboxStatus>>,
}

#[async_trait]
impl SandboxControlPort for SandboxControl {
    async fn create(&self, request: CreateSandboxRequest) -> HandResult<SandboxStatus> {
        self.creates.fetch_add(1, Ordering::Relaxed);
        let status: SandboxStatus = serde_json::from_value(json!({
            "state": "running",
            "target": request.target,
            "generation": request.generation_intent,
            "target_ref": "tgt_sandbox_e2e",
            "changed_at_ms": brain::wall_ms(),
            "expires_at_ms": brain::wall_ms() + 60_000,
        }))
        .unwrap();
        *self.status.lock().unwrap() = Some(status.clone());
        Ok(status)
    }

    async fn inspect(&self, _target: SandboxTarget) -> HandResult<SandboxStatus> {
        Ok(self.status.lock().unwrap().clone().expect("created"))
    }

    async fn execute(&self, _request: SandboxExecutionRequest) -> HandResult<SubmitReceipt> {
        panic!("unused")
    }

    async fn write_stdin(&self, _request: WriteStdinRequest) -> HandResult<WriteStdinReceipt> {
        panic!("unused")
    }

    async fn terminate(&self, _target: SandboxTarget) -> HandResult<SandboxStatus> {
        panic!("unused")
    }
}

struct UnusedFiles;

#[async_trait]
impl brain::hand::SandboxFilesPort for UnusedFiles {
    async fn status(&self, _target: SandboxTarget) -> HandResult<SandboxStatus> {
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

struct UnusedStorage;

#[async_trait]
impl SessionStoragePort for UnusedStorage {
    async fn list(
        &self,
        _session_id: &str,
        _prefix: Option<&str>,
        _cursor: Option<&str>,
        _limit: u32,
    ) -> Result<StoragePage> {
        panic!("unused")
    }
    async fn stat(&self, _session_id: &str, key: &str) -> Result<StorageObject> {
        Err(BrainError::FileNotFound(key.into()))
    }
    async fn read(&self, _session_id: &str, key: &str, _max_bytes: u64) -> Result<Vec<u8>> {
        Err(BrainError::FileNotFound(key.into()))
    }
    async fn write(&self, _request: StorageWriteRequest) -> Result<StorageObject> {
        panic!("unused")
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
    async fn complete_upload(&self, _session_id: &str, id: &str) -> Result<StorageObject> {
        Err(BrainError::FileNotFound(id.into()))
    }
    async fn abort_upload(&self, _session_id: &str, _id: &str) -> Result<()> {
        Ok(())
    }
    async fn delete(&self, _session_id: &str, _key: &str) -> Result<()> {
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

#[derive(Debug, Default)]
struct SandboxProvider;

#[async_trait]
impl Provider for SandboxProvider {
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
                    text: "sandbox ready".into(),
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
                    id: "create-1".into(),
                    name: "sandbox".into(),
                },
                ProviderEvent::ToolInputDelta {
                    index: 0,
                    partial_json: "{\"action\":\"create\"}".into(),
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
            "brain-sandbox-engine-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
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
                "name":"sandbox",
                "description":"isolated sandbox",
                "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "input_schema": {
                    "type":"object",
                    "properties":{"action":{"const":"create"}},
                    "required":["action"]
                },
                "output_schema": {}
            },
            "executor":{"kind":"engine","capability":"brain.sandbox"}
        }]}
    }))
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sandbox_create_reserves_inventory_before_typed_hand_materialization() {
    let _tmp = TempDir::new();
    let journal = Journal::new_memory("sandbox-engine-e2e");
    let control = Arc::new(SandboxControl::default());
    let provider = Arc::new(SandboxProvider);
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
            session_storage: Some(Arc::new(UnusedStorage)),
            sandbox_files: Some(Arc::new(UnusedFiles)),
            sandbox_control: Some(control.clone()),
            ..BrainServices::default()
        },
        Some(Arc::new(move |_| factory.clone() as Arc<dyn Provider>)),
    );
    let session = brain
        .create_session(create_request(), Some("sandbox-engine"))
        .await
        .unwrap();
    let session_id = session.id.to_string();
    brain
        .message(
            &session_id,
            MessageRequestContent::String("isolate this".parse().unwrap()),
        )
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while brain.get(&session_id).await.unwrap().current_turn.is_some() {
        assert!(Instant::now() < deadline, "turn timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(control.creates.load(Ordering::Relaxed), 1);
    let page = journal
        .list_sandbox_page(&SandboxListQuery {
            root_id: &session_id,
            limit: 10,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(page.sandboxes.len(), 1);
    assert_eq!(
        page.sandboxes[0].status.state,
        brain_protocol::hand::SandboxState::Running
    );
    assert_eq!(page.sandboxes[0].owner_session_id, session_id);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    let call = records
        .iter()
        .position(|entry| matches!(entry.record, Record::ToolCall { .. }))
        .unwrap();
    let result = records
        .iter()
        .position(|entry| matches!(entry.record, Record::ToolResult { .. }))
        .unwrap();
    assert!(call < result);
    assert!(matches!(
        records[result].record,
        Record::ToolResult { ref name, is_error: false, .. } if name == "sandbox"
    ));
}
