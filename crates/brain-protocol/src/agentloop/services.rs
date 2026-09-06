//! Brain's turn services as an Environment on another machine reaches them.
//!
//! A turn that runs outside this process gets the same five services the in-process
//! loop has, as session routes under `/v1/sessions/{session_id}/turns/{sequence}`,
//! open only while that activation is and only with the token minted for it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EventPage, ModelRequest, ModelResult, ToolInvocation, ToolResult};

/// Where an Environment reaches Brain's turn services for one turn, and the bearer
/// token that opens them. Sent with the turn, never journaled, dead when the turn ends.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnCallback {
    /// `{public url}/v1/sessions/{session_id}/turns/{sequence}`.
    #[schemars(length(min = 1, max = 2048), extend("format" = "uri"))]
    pub url: String,
    #[schemars(length(min = 1, max = 256))]
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnDispatchRequest {
    pub calls: Vec<ToolInvocation>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct TurnDispatchResult {
    pub results: Vec<ToolResult>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnEmitRequest {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct TurnEmitAck {
    /// The sequence Brain assigned to the committed Event.
    #[schemars(range(min = 1))]
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnTelemetry {
    pub record: serde_json::Value,
}

/// One call on the turn routes, as the API hands it to the open activation.
#[derive(Clone, Debug)]
pub enum TurnCall {
    Events { after: u64 },
    Model(ModelRequest),
    Dispatch(TurnDispatchRequest),
    Emit(TurnEmitRequest),
    Telemetry(TurnTelemetry),
}

#[derive(Clone, Debug)]
pub enum TurnAnswer {
    Events(EventPage),
    Model(ModelResult),
    Dispatch(TurnDispatchResult),
    Emit(TurnEmitAck),
    Telemetry,
}
