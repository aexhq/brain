//! Brain-owned, transport-neutral execution ports.
//!
//! A production Hand implements these traits. Brain commits an operation intent before calling
//! [`HandPort::submit`], commits a terminal observation before
//! [`HandPort::acknowledge_terminal`], and never substitutes one Hand for another.

use crate::journal::HeadDoc;
use crate::{BrainError, Result};
use async_trait::async_trait;
use brain_protocol::hand::{
    AcknowledgeTerminalRequest, Acknowledgement, CancelRequest, CancellationReceipt,
    CreateSandboxRequest, FileEntry, HandError, ObserveRequest, OperationObservation,
    PrepareSessionRequest, PreparedSession, ResolvedBinding, SandboxCopyRequest, SandboxCopyResult,
    SandboxExecutionRequest, SandboxFileRequest, SandboxFileWriteRequest, SandboxFileWriteResult,
    SandboxStatus, SandboxTarget, SealedBinding, SecretDeliveryRequest, SubmitReceipt,
    SubmitRequest, WriteStdinReceipt, WriteStdinRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type HandResult<T> = std::result::Result<T, HandError>;

/// The mandatory operation receipt protocol implemented by every Hand.
#[async_trait]
pub trait HandPort: Send + Sync {
    async fn resolve_binding(&self, binding: SealedBinding) -> HandResult<ResolvedBinding>;
    async fn submit(&self, request: SubmitRequest) -> HandResult<SubmitReceipt>;
    async fn observe(&self, request: ObserveRequest) -> HandResult<OperationObservation>;
    async fn cancel(&self, request: CancelRequest) -> HandResult<CancellationReceipt>;
    async fn acknowledge_terminal(
        &self,
        request: AcknowledgeTerminalRequest,
    ) -> HandResult<Acknowledgement>;
}

/// Optional lifecycle capability. Preparation never materializes the default sandbox.
#[async_trait]
pub trait SessionPreparationPort: Send + Sync {
    async fn prepare(&self, request: PrepareSessionRequest) -> HandResult<PreparedSession>;
    /// Idempotently materialize the shared default target. Brain supplies the durable logical
    /// target/binding and sealed root policy; this is distinct from additional-sandbox creation.
    async fn materialize_default(&self, request: CreateSandboxRequest)
    -> HandResult<SandboxStatus>;
    async fn dematerialize_default(&self, target: SandboxTarget) -> HandResult<SandboxStatus>;
    async fn purge_tree(&self, root_id: &str) -> HandResult<()>;
}

/// Plaintext returned only across the one-purpose secret redemption port. It deliberately has no
/// Serialize or Debug implementation so ordinary tracing/receipt machinery cannot encode it.
pub struct SecretMaterial(HashMap<String, String>);

impl SecretMaterial {
    pub fn new(values: HashMap<String, String>) -> Self {
        Self(values)
    }

    pub fn into_env(self) -> HashMap<String, String> {
        self.0
    }
}

#[async_trait]
pub trait SecretDeliveryPort: Send + Sync {
    async fn redeem(&self, request: SecretDeliveryRequest) -> HandResult<SecretMaterial>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxFileListRequest {
    pub target: SandboxTarget,
    pub expected_generation: String,
    pub path: String,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxFileList {
    pub entries: Vec<FileEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxFileContent {
    pub entry: FileEntry,
    pub content_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSearchRequest {
    pub target: SandboxTarget,
    pub expected_generation: String,
    pub path: String,
    pub expression: String,
    pub cursor: Option<String>,
    pub limit: u32,
}

/// Optional live-files capability. Every operation fences on the expected generation.
#[async_trait]
pub trait SandboxFilesPort: Send + Sync {
    async fn status(&self, target: SandboxTarget) -> HandResult<SandboxStatus>;
    async fn list(&self, request: SandboxFileListRequest) -> HandResult<SandboxFileList>;
    async fn stat(&self, request: SandboxFileRequest) -> HandResult<FileEntry>;
    async fn read(&self, request: SandboxFileRequest) -> HandResult<SandboxFileContent>;
    async fn write(&self, request: SandboxFileWriteRequest) -> HandResult<SandboxFileWriteResult>;
    async fn find(&self, request: SandboxSearchRequest) -> HandResult<SandboxFileList>;
    async fn grep(&self, request: SandboxSearchRequest) -> HandResult<SandboxFileList>;
    async fn transfer(&self, request: SandboxCopyRequest) -> HandResult<SandboxCopyResult>;
}

/// Optional effect capability for the official additional-sandbox Tool. Logical inventory and
/// pagination are Brain-owned durable state; Hand is addressed only with an exact sealed target.
#[async_trait]
pub trait SandboxControlPort: Send + Sync {
    async fn create(&self, request: CreateSandboxRequest) -> HandResult<SandboxStatus>;
    async fn inspect(&self, target: SandboxTarget) -> HandResult<SandboxStatus>;
    async fn execute(&self, request: SandboxExecutionRequest) -> HandResult<SubmitReceipt>;
    async fn write_stdin(&self, request: WriteStdinRequest) -> HandResult<WriteStdinReceipt>;
    async fn terminate(&self, target: SandboxTarget) -> HandResult<SandboxStatus>;
}

pub(crate) fn managed_hand_resources() -> Result<brain_protocol::hand::ResourceCeiling> {
    serde_json::from_value(serde_json::json!({
        "timeout_ms": 600_000,
        "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
    }))
    .map_err(BrainError::from)
}

pub(crate) fn sealed_sandbox_network(
    doc: &HeadDoc,
) -> Result<brain_protocol::hand::NetworkCeiling> {
    let network = match doc
        .prefix
        .network
        .get("outbound")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("none")
    {
        "none" => serde_json::json!({"kind":"none"}),
        "public" => serde_json::json!({"kind":"public"}),
        "allowlist" => serde_json::json!({
            "kind":"allowlist",
            "destinations": doc.prefix.network.get("destinations").cloned().unwrap_or_else(|| serde_json::json!([])),
        }),
        other => {
            return Err(BrainError::Invalid(format!(
                "sealed network policy has unknown outbound mode {other}"
            )));
        }
    };
    serde_json::from_value(network).map_err(BrainError::from)
}

pub(crate) fn map_hand_port_error(error: brain_protocol::hand::HandError) -> BrainError {
    use brain_protocol::hand::HandErrorCode;
    match error.code {
        HandErrorCode::SandboxNotMaterialized => BrainError::SandboxNotMaterialized,
        HandErrorCode::SandboxGone => BrainError::SandboxGone,
        HandErrorCode::GenerationConflict => BrainError::SandboxGenerationConflict,
        HandErrorCode::ResourceExhausted => BrainError::SandboxResourceExhausted,
        _ => BrainError::Hand(error.message.to_string()),
    }
}
