use serde::{Deserialize, Serialize};

use crate::{
    AgentloopIdentity, EnvironmentAttachment, EnvironmentId, EventId, Identity, JournalId,
    LifecyclePolicy, ModelBinding, ModelSelection, OperationId, RequestedToolBinding, SessionId,
    ToolBinding, ToolDefinition,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPresentation {
    pub system: String,
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub agentloop_identity: AgentloopIdentity,
    pub brain_configuration: serde_json::Value,
    pub model: ModelSelection,
    pub presentation: ModelPresentation,
    pub environments: Vec<EnvironmentRequirement>,
    pub tool_bindings: Vec<RequestedToolBinding>,
    /// Prior events for this conversation, if the caller kept them.
    ///
    /// A session does not outlive the process that made it, so an application that wants
    /// one to continue holds the events it was already receiving and hands them back here.
    /// Brain writes them as the new session's opening records and tells the agentloop about
    /// them, so `GET /events` reads the whole conversation and the loop can pick up where
    /// it left off. Empty is an ordinary new session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEvent>,
}

/// One event an application kept and is handing back.
///
/// The shape `GET /v1/sessions/{id}/events` returns, less `event_id`: an id names an event
/// in a session, and this is being replayed into a different one, so Brain mints them
/// again rather than taking one that points somewhere else.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEvent {
    /// Where this sat in the conversation it came from. Kept because the caller has it and
    /// because it is what makes a gap or a reordering visible instead of silent.
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at_ms: Option<u64>,
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedSessionRequest {
    pub agentloop_identity: AgentloopIdentity,
    pub brain_configuration: serde_json::Value,
    pub model: ModelBinding,
    pub presentation: ModelPresentation,
    pub environments: Vec<EnvironmentRequirement>,
    pub tool_bindings: Vec<RequestedToolBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEvent>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRequirement {
    pub environment_id: EnvironmentId,
    pub configuration: serde_json::Value,
    pub lifecycle_policy: LifecyclePolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SealedSessionConfig {
    pub agentloop_identity: AgentloopIdentity,
    pub brain_configuration: serde_json::Value,
    pub model: ModelBinding,
    pub presentation: ModelPresentation,
    pub environments: Vec<EnvironmentAttachment>,
    pub tool_bindings: Vec<ToolBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub content: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Creating,
    Idle,
    Running,
    Ended,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub session_id: SessionId,
    pub journal_id: JournalId,
    pub status: SessionStatus,
    pub through_sequence: u64,
    pub presentation_identity: Identity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    pub event_id: EventId,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub event_type: String,
    pub data: serde_json::Value,
}

/// What a live subscription carries.
///
/// A subscription exists to say that a session moved. Most of what it carries is a journal
/// record, which has a sequence and can be read back later with `after`. Model output is
/// the exception: it arrives while the turn is still running, before the record that will
/// hold it exists.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum LiveEvent {
    /// A journal record, as it is appended.
    Recorded(Event),
    /// Model output as it arrives.
    ///
    /// Never journalled and never replayed. A client that reconnects is handed the
    /// completed message from the page rather than the tokens that built it, because
    /// recording a token is a durable write per token and the completed message is the
    /// durable truth. This is the difference between watching a turn and reading it.
    Streaming(StreamingEvent),
}

/// One piece of model output, mid-turn.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StreamingEvent {
    /// The model call this came from, so a client can tell two concurrent ones apart.
    pub operation_id: OperationId,
    /// `assistant_delta` for text, `tool_call_delta` for a tool call being assembled.
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventPage {
    pub events: Vec<Event>,
    pub next_cursor: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionList {
    pub sessions: Vec<Session>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionStatus {
    Admitted,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentloopAdmission {
    pub identity: AgentloopIdentity,
    pub status: AdmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::ApiError>,
}
