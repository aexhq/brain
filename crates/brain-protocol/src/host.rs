use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HostId, Outcome, SessionId, ToolInvocation};

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterHostRequest {}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct HostRegistration {
    pub host_id: HostId,
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostOperation {
    InvokeTool { invocation: ToolInvocation },
    CancelTool { target_sequence: u64 },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct HostCommand {
    pub session_id: SessionId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub deadline_at_ms: u64,
    pub operation: HostOperation,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostResult {
    pub session_id: SessionId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostEvent {
    pub session_id: SessionId,
    /// The resident command this Event belongs to.
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
