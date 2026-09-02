use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    AgentloopIdentity, EnvironmentAttachment, EnvironmentId, EventId, Identity, LifecyclePolicy,
    ModelBinding, ModelSelection, Program, RequestedToolBinding, SessionId, ToolBinding,
    ToolDefinition, ToolHosting,
};

/// The admitted loop package a session runs: which one, and how it is configured.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopRef {
    pub identity: AgentloopIdentity,
    pub configuration: serde_json::Value,
}

/// One tool as the SDK hands it over: its manifest fields plus the environment it
/// binds to. Brain splits the model-facing and dispatch-facing halves internally.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub needs: Vec<String>,
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
    /// Required unless `hosting` is `client`; a client-hosted tool binds no environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
}

#[derive(Clone, Deserialize, Serialize)]
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
    pub tools: Vec<BoundTool>,
    pub environments: Vec<EnvironmentRequirement>,
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
    #[serde(default)]
    pub system: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    /// What the model may be told about each tool. Offered whole unless the agentloop
    /// names a subset on a model call; the bindings say where a call goes.
    pub tools: Vec<ToolDefinition>,
    pub environments: Vec<ResolvedEnvironment>,
    pub tool_bindings: Vec<RequestedToolBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryEvent>,
}

/// What a session create names about one environment, as it arrives on the wire.
/// `bindings` carries plaintext values and exists only here: the resolved request the
/// session journals carries their identities instead.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRequirement {
    pub environment_id: EnvironmentId,
    pub configuration: serde_json::Value,
    pub lifecycle_policy: LifecyclePolicy,
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
}

/// The journal-safe form of an environment requirement: binding values are sealed
/// outside the journal, which keeps only their identities — the same custody model as
/// model credentials.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedEnvironment {
    pub environment_id: EnvironmentId,
    pub configuration: serde_json::Value,
    pub lifecycle_policy: LifecyclePolicy,
    #[serde(default)]
    pub binding_identities: BTreeMap<String, Identity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SealedSessionConfig {
    pub agentloop_identity: AgentloopIdentity,
    pub brain_configuration: serde_json::Value,
    pub model: ModelBinding,
    #[serde(default)]
    pub system: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    pub tools: Vec<ToolDefinition>,
    pub environments: Vec<EnvironmentAttachment>,
    pub tool_bindings: Vec<ToolBinding>,
}

/// What an application hands a session on `send`. The shape is closed on purpose:
/// Brain owes every agentloop the same observation shape regardless of who wrote
/// the client, so free-form content is not accepted. Multimodal parts will extend
/// this record when they land — see the roadmap.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    pub message: String,
}

impl<T: Into<String>> From<T> for UserInput {
    fn from(message: T) -> Self {
        UserInput {
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub input: UserInput,
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

/// What the API says about a session: its id, where it is, and how far its journal goes.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub status: SessionStatus,
    /// Sequence of the last journal record committed for this session — the journal is
    /// complete through here, so it is where a `GET /events` cursor starts.
    pub last_sequence: u64,
    /// The scoped credential that authorizes answering this session's client-hosted
    /// tools: the serve feed and the tool-results endpoint, nothing else. Hand it to
    /// the process that serves a tool; it spends nothing and reads nothing else.
    /// Minted by the serving layer — the session leaves it empty.
    #[serde(default)]
    pub share_key: String,
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
    /// The sequence of the `model_call_started` record this output belongs to.
    pub sequence: u64,
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
    pub sessions: Vec<SessionSummary>,
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
