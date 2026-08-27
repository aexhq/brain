use serde::{Deserialize, Serialize};

use crate::{AttachmentId, EnvironmentId, OperationId, SessionId, ToolInvocation, ToolResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePolicy { Session, Shared, External }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentBinding {
    pub environment_id: EnvironmentId,
    pub configuration_digest: String,
    pub adapter_binding: String,
    pub directory_generation: u64,
    pub lifecycle_policy: LifecyclePolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentOperation<T> {
    pub operation_id: OperationId,
    pub request_digest: String,
    pub environment_id: EnvironmentId,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<AttachmentId>,
    pub request: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentRequest {
    Setup { configuration: serde_json::Value },
    Attach { grants: serde_json::Value },
    Call { name: String, input: serde_json::Value },
    Execute { tool: ToolInvocation },
    Cancel { target_operation_id: OperationId },
    Detach,
    Teardown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentReceipt {
    Accepted,
    Progress { data: serde_json::Value },
    Result { output: serde_json::Value },
    ToolResult { result: ToolResult },
    Failure { code: String, message: String, retryable: bool },
    Conflict { expected_digest: String, actual_digest: String },
    Ambiguous { message: String },
}
