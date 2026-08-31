use serde::{Deserialize, Serialize};

use crate::{
    AttachmentId, Capability, EnvironmentBinding, EnvironmentId, Identity, OperationId, Outcome,
    SessionId,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

/// Where a tool's implementation executes: a provisioned artifact the environment
/// hosts, or a callback into the author's own running application.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolHosting {
    #[default]
    Provisioned,
    Callback,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    Esm,
    Component,
}

/// The deliverable artifact behind a provisioned tool, named by content identity so
/// re-provisioning is idempotent. Absent for tools baked into the environment itself
/// and for callback tools, whose code never leaves the author's process.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPayload {
    pub kind: PayloadKind,
    pub identity: Identity,
}

/// The `contracts/tool/v1` manifest: the only thing Brain and environments read about
/// a tool. Binding *values* are never here — the manifest declares names, the
/// environment injects values at runtime.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    pub requires: Vec<Capability>,
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ToolPayload>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolBinding {
    pub name: String,
    pub environment: EnvironmentBinding,
    pub attachment_id: AttachmentId,
    pub requires: Vec<Capability>,
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ToolPayload>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedToolBinding {
    pub name: String,
    pub environment_id: EnvironmentId,
    pub requires: Vec<Capability>,
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ToolPayload>,
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
    pub request_identity: Identity,
    pub session_id: SessionId,
    pub binding: ToolBinding,
    pub invocation: ToolInvocation,
    /// Caller-owned: Brain kills the call when this expires, because the remote cannot
    /// be trusted to.
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCancellation {
    pub operation_id: OperationId,
    pub request_identity: Identity,
    pub target_operation_id: OperationId,
    pub session_id: SessionId,
    pub binding: ToolBinding,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolResult {
    pub call_id: String,
    pub output: serde_json::Value,
    pub is_error: bool,
}

impl ToolResult {
    /// How an invoke [`Outcome`] lands in the loop's view of a tool call: anything but
    /// `ok` is a failed result whose output carries a code the loop can read.
    pub fn from_outcome(call_id: String, outcome: Outcome) -> Self {
        match outcome {
            Outcome::Ok { value } => ToolResult {
                call_id,
                output: value,
                is_error: false,
            },
            Outcome::Error { error } => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": error.code,
                    "message": error.message,
                    "details": error.details,
                }),
                is_error: true,
            },
            Outcome::Timeout => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": "timeout",
                    "message": "the Tool call did not finish before its deadline",
                }),
                is_error: true,
            },
            Outcome::Cancelled => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": "cancelled",
                    "message": "the Tool call was cancelled",
                }),
                is_error: true,
            },
        }
    }
}
