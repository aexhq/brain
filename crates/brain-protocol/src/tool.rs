use serde::{Deserialize, Serialize};

use crate::{AttachmentId, EnvironmentId, OperationId, SessionId};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolBinding {
    pub name: String,
    pub environment_id: EnvironmentId,
    pub attachment_id: AttachmentId,
    pub remote_tool_id: String,
    pub grant: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedToolBinding {
    pub name: String,
    pub environment_id: EnvironmentId,
    pub remote_tool_id: String,
    pub grant: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolInvocation {
    pub call_id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolDispatch {
    pub operation_id: OperationId,
    pub request_digest: String,
    pub session_id: SessionId,
    pub binding: ToolBinding,
    pub invocation: ToolInvocation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolResult {
    pub call_id: String,
    pub output: serde_json::Value,
    pub is_error: bool,
}
