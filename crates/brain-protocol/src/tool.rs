use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Environment, EnvironmentName, Outcome, SessionId, ToolId, TurnError};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAdmissionStatus {
    Admitted,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ToolAdmission {
    pub id: ToolId,
    pub status: ToolAdmissionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<TurnError>,
}

/// What the model may be told about a Tool.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub name: String,
    pub description: String,
    #[schemars(schema_with = "crate::schema::json_object")]
    pub input_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "crate::schema::json_object")]
    pub output_schema: Option<serde_json::Value>,
}

/// One Tool as a session declares it: what the model is told, the Environment of the
/// session that runs it, what it needs there, and the implementation that Environment
/// interprets. Brain reads the name and the Environment and carries the rest.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub name: String,
    pub description: String,
    #[schemars(schema_with = "crate::schema::json_object")]
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "crate::schema::json_object")]
    pub output_schema: Option<serde_json::Value>,
    pub environment: EnvironmentName,
    /// What the Tool needs from its Environment, as URIs: `pkg:` for software,
    /// `https:` or `wss:` for a network destination, `file:` for a filesystem location.
    /// Brain hands them to the Environment and reads none of them.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::needs")]
    pub needs: Vec<String>,
    /// Opaque to Brain; interpreted by the Environment. Absent when the Environment
    /// holds the implementation itself, as the host env does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<serde_json::Value>,
}

impl Tool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            output_schema: self.output_schema.clone(),
        }
    }
}

/// One call as the Agentloop makes it. `call_id` is the loop's own correlation, echoed
/// in the result; on every Environment wire the call is named by its sequence.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ToolInvocation {
    pub call_id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolDispatch {
    /// The sequence of the `tool_call_started` record. With `session_id`, the name of
    /// this call everywhere: on the Environment wire and in the finished record.
    pub sequence: u64,
    pub session_id: SessionId,
    pub tool: Tool,
    /// The Environment the Tool names, as the session declared it.
    pub environment: Environment,
    pub invocation: ToolInvocation,
    /// Caller-owned: Brain kills the call when this expires, because the remote cannot
    /// be trusted to.
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolCancellation {
    /// The sequence of the `tool_cancel_started` record.
    pub sequence: u64,
    /// The sequence of the `tool_call_started` record being cancelled.
    pub target_sequence: u64,
    pub session_id: SessionId,
    pub environment: Environment,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
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
                    "code": crate::codes::failure::TIMEOUT,
                    "message": "the Tool call did not finish before its deadline",
                }),
                is_error: true,
            },
            Outcome::Cancelled => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": crate::codes::failure::CANCELLED,
                    "message": "the Tool call was cancelled",
                }),
                is_error: true,
            },
            Outcome::Unknown { message } => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": crate::codes::failure::UNKNOWN,
                    "message": message,
                }),
                is_error: true,
            },
        }
    }
}
