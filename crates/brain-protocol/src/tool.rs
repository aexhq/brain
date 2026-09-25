use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Environment, EnvironmentName, Event, Outcome, SessionId, ToolId, TurnError};

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

/// A canonical Tool definition with one implementation per authorized Environment.
/// Brain validates dispatched inputs and successful outputs against this definition.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<crate::EnvironmentGrant>,
    #[schemars(schema_with = "crate::schema::identifier")]
    pub name: String,
    pub description: String,
    #[schemars(schema_with = "crate::schema::json_object")]
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "crate::schema::json_object")]
    pub output_schema: Option<serde_json::Value>,
    /// One implementation per authorized Environment, fixed at create.
    #[schemars(length(min = 1))]
    pub placements: std::collections::BTreeMap<EnvironmentName, ToolPlacement>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPlacement {
    pub implementation: serde_json::Value,
}

/// Definitions and logical choices made available to an Agentloop, without executable descriptors.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ActivationTool {
    #[serde(flatten)]
    pub definition: ToolDefinition,
    pub environments: Vec<EnvironmentName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment_refs: Vec<crate::EnvironmentRef>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_sequence: Option<u64>,
    pub environment: EnvironmentName,
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
    pub placement: ToolPlacement,
    /// The Environment the Tool names, as the session declared it.
    pub environment: Environment,
    pub invocation: ToolInvocation,
    /// Caller-owned: Brain kills the call when this expires, because the remote cannot
    /// be trusted to.
    pub deadline_ms: Option<u64>,
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
    /// Optional model-facing text; the structured output and failure status are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// A Tool observation with an optional presentation for the Agentloop.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ToolOutput {
    #[serde(flatten)]
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl From<Outcome> for ToolOutput {
    fn from(outcome: Outcome) -> Self {
        Self {
            outcome,
            content: None,
        }
    }
}

/// Observations available when the synchronous phase returns. Execution can remain open.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ToolReturn {
    pub call_id: String,
    /// The original tool_call_started sequence.
    pub sequence: u64,
    pub events: Vec<Event>,
    pub finished: bool,
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
                content: None,
            },
            Outcome::Error { error } => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": error.code,
                    "message": error.message,
                    "retryable": error.retryable,
                    "details": error.details,
                }),
                is_error: true,
                content: None,
            },
            Outcome::Timeout => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": crate::codes::failure::TIMEOUT,
                    "message": "the Tool call did not finish before its deadline",
                }),
                is_error: true,
                content: None,
            },
            Outcome::Cancelled => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": crate::codes::failure::CANCELLED,
                    "message": "the Tool call was cancelled",
                }),
                is_error: true,
                content: None,
            },
            Outcome::Unknown { message } => ToolResult {
                call_id,
                output: serde_json::json!({
                    "code": crate::codes::failure::UNKNOWN,
                    "message": message,
                }),
                is_error: true,
                content: None,
            },
        }
    }
}
