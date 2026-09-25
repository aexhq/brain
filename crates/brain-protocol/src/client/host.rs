use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HostId, ModelRequest, SessionId, ToolOutput};

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterHostRequest {}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct HostRegistration {
    pub host_id: HostId,
    pub token: String,
}

/// What a host is asked to do for a session placed in it. A call is named by the
/// command's `(session_id, sequence)`; the Tool's own call id never leaves Brain.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostOperation {
    InvokeTool {
        #[schemars(schema_with = "crate::schema::identifier")]
        name: String,
        input: serde_json::Value,
    },
    CancelTool {
        target_sequence: u64,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct HostCommand {
    pub environment: crate::EnvironmentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<crate::EnvironmentName>,
    pub session_id: SessionId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_at_ms: Option<u64>,
    pub operation: HostOperation,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostResult {
    pub session_id: SessionId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub update: ToolExecutionUpdate,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolExecutionUpdate {
    Result {
        outcome: ToolOutput,
    },
    Returned {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<ToolOutput>,
    },
    Finish {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<ToolOutput>,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostEvent {
    pub session_id: SessionId,
    /// The command this Event belongs to.
    #[schemars(range(min = 1))]
    pub sequence: u64,
    #[schemars(schema_with = "crate::schema::identifier")]
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct HostEventAck {
    /// The sequence Brain assigned to the committed Event.
    #[schemars(range(min = 1))]
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostModelRequest {
    pub session_id: SessionId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub request: ModelRequest,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostServiceRequest {
    pub session_id: SessionId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub call: crate::ExecutionCall,
}
