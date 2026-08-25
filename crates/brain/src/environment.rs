//! Brain-owned, transport-neutral execution ports.
//!
//! A production Environment implements these traits. Brain commits an operation intent before calling
//! [`EnvironmentPort::submit`], commits a terminal observation before
//! [`EnvironmentPort::acknowledge_terminal`], and never substitutes one Environment for another.

use crate::journal::HeadDoc;
use crate::{BrainError, Result};
use async_trait::async_trait;
use brain_protocol::environment::{
    AcknowledgeTerminalRequest, Acknowledgement, CancelRequest, CancellationReceipt,
    CreateSandboxRequest, EnvironmentError, FileEntry, ObserveRequest, OperationObservation,
    PrepareSessionRequest, PreparedSession, ResolvedBinding, SandboxCopyRequest, SandboxCopyResult,
    SandboxExecutionRequest, SandboxFileRequest, SandboxFileWriteRequest, SandboxFileWriteResult,
    SandboxStatus, SandboxTarget, SealedBinding, SecretDeliveryRequest, SubmitReceipt,
    SubmitRequest, WriteStdinReceipt, WriteStdinRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub type EnvironmentResult<T> = std::result::Result<T, EnvironmentError>;

pub const COMPONENT_ENVIRONMENT_WORLD: &str = "aex:environment/environment@1.0.0";

#[derive(Debug, Clone)]
pub struct ComponentEnvironmentInvocation {
    pub tenant_id: String,
    pub session_id: String,
    pub environment_id: String,
    pub operation_id: String,
    pub descriptor_json: String,
    pub bundle: Option<Vec<u8>>,
    pub input_json: String,
    pub deadline_at_ms: u64,
}

#[async_trait]
pub trait ComponentEnvironmentRegistry: Send + Sync {
    fn admit(&self, component_digest: &str, world: &str, component: &[u8]) -> Result<()>;

    async fn invoke(
        &self,
        declaration: &brain_protocol::session::ComponentEnvironmentConfig,
        request: ComponentEnvironmentInvocation,
    ) -> Result<String>;
}

/// The mandatory operation receipt protocol implemented by every Environment.
#[async_trait]
pub trait EnvironmentPort: Send + Sync {
    async fn resolve_binding(&self, binding: SealedBinding) -> EnvironmentResult<ResolvedBinding>;
    async fn submit(&self, request: SubmitRequest) -> EnvironmentResult<SubmitReceipt>;
    async fn observe(&self, request: ObserveRequest) -> EnvironmentResult<OperationObservation>;
    async fn cancel(&self, request: CancelRequest) -> EnvironmentResult<CancellationReceipt>;
    async fn acknowledge_terminal(
        &self,
        request: AcknowledgeTerminalRequest,
    ) -> EnvironmentResult<Acknowledgement>;
}

#[derive(Clone)]
pub struct EnvironmentAdapter {
    pub execution: Arc<dyn EnvironmentPort>,
    pub preparation: Arc<dyn SessionPreparationPort>,
    pub files: Option<Arc<dyn SandboxFilesPort>>,
}

#[derive(Clone, Default)]
pub struct EnvironmentRegistry {
    adapters: HashMap<String, EnvironmentAdapter>,
}

impl EnvironmentRegistry {
    pub fn new(adapters: impl IntoIterator<Item = (String, EnvironmentAdapter)>) -> Result<Self> {
        let mut by_extension = HashMap::new();
        for (extension, adapter) in adapters {
            if extension.trim().is_empty() {
                return Err(BrainError::Invalid(
                    "environment extension identity is empty".into(),
                ));
            }
            if by_extension.insert(extension.clone(), adapter).is_some() {
                return Err(BrainError::Invalid(format!(
                    "environment extension {extension} is registered more than once"
                )));
            }
        }
        Ok(Self {
            adapters: by_extension,
        })
    }

    pub fn resolve(&self, extension: &str) -> Result<&EnvironmentAdapter> {
        self.adapters.get(extension).ok_or_else(|| {
            BrainError::EnvironmentUnavailable(format!(
                "environment extension {extension} is not registered by this composition"
            ))
        })
    }
}

#[derive(Clone)]
pub struct ManagedBinding {
    pub environment_name: String,
    pub resolved: ResolvedBinding,
    pub environment: Arc<dyn EnvironmentPort>,
}

/// Environment lifecycle and artifact preparation capability.
#[async_trait]
pub trait SessionPreparationPort: Send + Sync {
    async fn prepare(&self, request: PrepareSessionRequest) -> EnvironmentResult<PreparedSession>;
    async fn materialize(&self, request: CreateSandboxRequest) -> EnvironmentResult<SandboxStatus>;
    async fn dematerialize(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus>;
    async fn purge_tree(&self, root_id: &str) -> EnvironmentResult<()>;
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
    async fn redeem(&self, request: SecretDeliveryRequest) -> EnvironmentResult<SecretMaterial>;
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
    async fn status(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus>;
    async fn list(&self, request: SandboxFileListRequest) -> EnvironmentResult<SandboxFileList>;
    async fn stat(&self, request: SandboxFileRequest) -> EnvironmentResult<FileEntry>;
    async fn read(&self, request: SandboxFileRequest) -> EnvironmentResult<SandboxFileContent>;
    async fn write(
        &self,
        request: SandboxFileWriteRequest,
    ) -> EnvironmentResult<SandboxFileWriteResult>;
    async fn find(&self, request: SandboxSearchRequest) -> EnvironmentResult<SandboxFileList>;
    async fn grep(&self, request: SandboxSearchRequest) -> EnvironmentResult<SandboxFileList>;
    async fn transfer(&self, request: SandboxCopyRequest) -> EnvironmentResult<SandboxCopyResult>;
}

/// Optional provider-facing computer control surface. Brain does not register, schedule, or
/// inventory these targets; an environment extension may use it behind its own Tool implementation.
#[async_trait]
pub trait SandboxControlPort: Send + Sync {
    async fn create(&self, request: CreateSandboxRequest) -> EnvironmentResult<SandboxStatus>;
    async fn inspect(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus>;
    async fn execute(&self, request: SandboxExecutionRequest) -> EnvironmentResult<SubmitReceipt>;
    async fn write_stdin(&self, request: WriteStdinRequest)
    -> EnvironmentResult<WriteStdinReceipt>;
    async fn terminate(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus>;
}

pub(crate) fn managed_environment_resources() -> Result<brain_protocol::environment::ResourceCeiling>
{
    serde_json::from_value(serde_json::json!({
        "timeout_ms": 600_000,
        "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
    }))
    .map_err(BrainError::from)
}

pub(crate) fn sealed_sandbox_network(
    doc: &HeadDoc,
) -> Result<brain_protocol::environment::NetworkCeiling> {
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

pub(crate) fn map_environment_port_error(
    error: brain_protocol::environment::EnvironmentError,
) -> BrainError {
    use brain_protocol::environment::EnvironmentErrorCode;
    match error.code {
        EnvironmentErrorCode::SandboxNotMaterialized => BrainError::SandboxNotMaterialized,
        EnvironmentErrorCode::SandboxGone => BrainError::SandboxGone,
        EnvironmentErrorCode::GenerationConflict => BrainError::SandboxGenerationConflict,
        EnvironmentErrorCode::ResourceExhausted => BrainError::SandboxResourceExhausted,
        EnvironmentErrorCode::FileNotFound => BrainError::FileNotFound(error.message.to_string()),
        _ => BrainError::Environment(error.message.to_string()),
    }
}
