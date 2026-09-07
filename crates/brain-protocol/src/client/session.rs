use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Environment, EnvironmentName, Message, ModelBinding, ModelSelection, SessionId, Tool,
    ToolDefinition,
};

/// The contract identifier of the session API.
pub const SESSION_CONTRACT: &str = "session/v1";

/// The admitted Agentloop a session runs: which one, how it is configured, which
/// Environment of the session runs it, and what it needs there.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopRef {
    pub implementation: serde_json::Value,
    pub configuration: serde_json::Value,
    pub environment: EnvironmentName,
    /// What the Agentloop needs from its Environment, as URIs. Brain hands them to the
    /// Environment at setup and with every turn, and reads none of them.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::needs")]
    pub needs: Vec<String>,
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub agentloop: AgentloopRef,
    pub model: ModelSelection,
    /// The system prompt the agent loop starts from. The loop may send a different one
    /// on any model call.
    #[serde(default)]
    pub system: String,
    /// The provider's structured-output request, applied to every model call unless the
    /// loop sends its own. Optional, and rejected at create for a provider that cannot
    /// carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    pub tools: Vec<Tool>,
    /// The Environments of this session, set up as part of this create. Every Tool and
    /// the Agentloop name one of them.
    pub environments: Vec<Environment>,
    /// A transcript to carry forward, if the caller has one: the messages the new
    /// session's first model call should already see. Brain journals them as the session's
    /// opening transcript. Empty is an ordinary new session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transcript: Vec<Message>,
    /// How long the session may sit idle before Brain suspends it: its task and memory
    /// are released and rebuilt from disk on the next request. Absent means the server's
    /// default; zero means never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ttl_ms: Option<u64>,
}

/// What a session was admitted with. Written at create and never changed afterwards: a
/// session can only ever do what it was granted. Credentials never enter it: the model
/// key and an Environment's credential are sealed by the server beside the session.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionConfig {
    pub agentloop: AgentloopRef,
    pub model: ModelBinding,
    #[serde(default)]
    pub system: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    pub tools: Vec<Tool>,
    pub environments: Vec<Environment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ttl_ms: Option<u64>,
}

impl SessionConfig {
    pub fn environment(&self, name: &EnvironmentName) -> Option<&Environment> {
        self.environments
            .iter()
            .find(|environment| &environment.name == name)
    }

    pub fn tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    /// What the model may be told about each Tool, in declaration order.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(Tool::definition).collect()
    }
}

/// What an application hands a session on `send`.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    #[schemars(length(min = 1))]
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<crate::Media>,
}

impl<T: Into<String>> From<T> for UserInput {
    fn from(message: T) -> Self {
        UserInput {
            message: message.into(),
            media: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub input: UserInput,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Creating,
    Idle,
    Running,
    Ending,
    Ended,
    Failed,
}

/// What the API says about a session: its id, where it is, and how far its journal goes.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub status: SessionStatus,
    /// Sequence of the last journal record committed for this session — the journal is
    /// complete through here, so it is where a `GET /events` cursor starts.
    pub last_sequence: u64,
}

/// Canonical transcript as of a committed journal sequence, available without execution.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SessionTranscript {
    pub messages: Vec<crate::Message>,
    pub through_sequence: u64,
}

/// One journal record as a client reads it. `(session_id, sequence)` names it; there
/// is no other identifier.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Event {
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub recorded_at_ms: u64,
    #[schemars(schema_with = "crate::schema::identifier")]
    pub event_type: String,
    pub data: serde_json::Value,
    /// Absent on kernel records and historical extension records without attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<EventOrigin>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventOrigin {
    Agentloop { sequence: u64 },
    Tool { sequence: u64 },
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
    /// The sequence of the `model_call_started` record this output belongs to.
    pub sequence: u64,
    /// `assistant_delta` for text, `tool_call_delta` for a tool call being assembled.
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EventPage {
    #[schemars(length(max = 1000))]
    pub events: Vec<Event>,
    pub next_cursor: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SessionList {
    pub sessions: Vec<SessionSummary>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionStatus {
    Admitted,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct AgentloopAdmission {
    pub id: crate::AgentloopId,
    pub status: AdmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::ApiError>,
}
