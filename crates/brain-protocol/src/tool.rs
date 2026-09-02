use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    AttachmentId, EnvironmentBinding, EnvironmentId, Identity, OperationId, Outcome, Runtime,
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

/// Where a tool's implementation executes: a provisioned program the environment
/// launches, or an application process answering off the serve feed (`client`) — the
/// session's creator or anyone holding the session's share key.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolHosting {
    #[default]
    Provisioned,
    Client,
}

/// The request template of an `http` program: the environment fronts the endpoint,
/// the tool's input travels as the JSON body, and the response body is the output.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpProgramRequest {
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

/// The program behind a provisioned tool, named by content identity so
/// re-provisioning is idempotent. An `esm` bundle travels out of band under its
/// identity; a `shell` script and an `http` request template travel inline.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Program {
    Esm {
        identity: Identity,
    },
    Shell {
        identity: Identity,
        script: String,
    },
    Http {
        identity: Identity,
        request: HttpProgramRequest,
    },
}

impl Program {
    /// The runtime an environment must offer to launch this program.
    pub fn runtime(&self) -> Runtime {
        match self {
            Program::Esm { .. } => Runtime::Esm,
            Program::Shell { .. } => Runtime::Shell,
            Program::Http { .. } => Runtime::Http,
        }
    }

    pub fn identity(&self) -> &Identity {
        match self {
            Program::Esm { identity }
            | Program::Shell { identity, .. }
            | Program::Http { identity, .. } => identity,
        }
    }
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
    /// Resource names the program operates on; checked against the environment's
    /// declared resources at session create.
    pub needs: Vec<String>,
    pub binding_names: Vec<String>,
    pub program: Program,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolBinding {
    pub name: String,
    /// Absent for client-hosted tools: no environment is on their serving path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<AttachmentId>,
    #[serde(default)]
    pub needs: Vec<String>,
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    /// Absent for client-hosted tools and for tools the environment executes natively
    /// without a provisioned program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedToolBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
    #[serde(default)]
    pub needs: Vec<String>,
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
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
