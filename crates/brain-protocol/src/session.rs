use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AgentloopIdentity, EnvironmentAttachRequest, EnvironmentAttachment, EnvironmentId, EventId,
    IDENTIFIER_PATTERN, MAX_TRANSCRIPT_ITEMS, Message, ModelBinding, ModelSelection, Program,
    RESOURCE_NAME_PATTERN, SessionId, ToolBinding, ToolDefinition, ToolHosting,
};

/// The contract identifier of the session API.
pub const SESSION_CONTRACT: &str = "session/v1";

/// The admitted loop package a session runs: which one, and how it is configured.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopRef {
    pub identity: AgentloopIdentity,
    pub configuration: serde_json::Value,
}

/// One tool as the SDK hands it over: its manifest fields plus the environment it
/// binds to. Brain splits the model-facing and dispatch-facing halves internally.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::bound_tool_rules)]
pub struct BoundTool {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub name: String,
    #[schemars(length(max = 8192))]
    pub description: String,
    #[schemars(schema_with = "crate::schema::json_object")]
    pub input_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "crate::schema::json_object")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(
        length(max = 64),
        inner(regex(pattern = RESOURCE_NAME_PATTERN)),
        extend("uniqueItems" = true)
    )]
    pub needs: Vec<String>,
    #[schemars(
        length(max = 64),
        inner(regex(pattern = IDENTIFIER_PATTERN)),
        extend("uniqueItems" = true)
    )]
    pub binding_names: Vec<String>,
    #[serde(default)]
    pub hosting: ToolHosting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
    /// Required unless `hosting` is `client`; a client-hosted tool binds no environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub agentloop: AgentloopRef,
    pub model: ModelSelection,
    /// The system prompt the agent loop starts from. The loop may send a different one
    /// on any model call.
    #[serde(default)]
    #[schemars(length(max = 131072))]
    pub system: String,
    /// The provider's structured-output request, applied to every model call unless the
    /// loop sends its own. Optional, and rejected at create for a provider that cannot
    /// carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[schemars(length(max = 128))]
    pub tools: Vec<BoundTool>,
    /// The environments this session attaches to, by id. Each must already exist.
    #[schemars(length(max = 128))]
    pub environments: Vec<EnvironmentAttachRequest>,
    /// A transcript to carry forward, if the caller has one: the messages the new
    /// session's first model call should already see. Brain journals them as the session's
    /// opening transcript. Empty is an ordinary new session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = MAX_TRANSCRIPT_ITEMS))]
    pub transcript: Vec<Message>,
    /// How long the session may sit idle before Brain suspends it: its task and memory
    /// are released and rebuilt from disk on the next request. Absent means the server's
    /// default; zero means never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ttl_ms: Option<u64>,
}

/// What a session was admitted with. Written at create and never changed afterwards: a
/// session can only ever do what it was granted. The environments and tool bindings are
/// filled in as the host attaches them, before the session is admitted.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionConfig {
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
    pub environments: Vec<EnvironmentAttachment>,
    pub tool_bindings: Vec<ToolBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ttl_ms: Option<u64>,
}

/// What an application hands a session on `send`. The shape is closed on purpose:
/// Brain owes every agentloop the same observation shape regardless of who wrote
/// the client, so free-form content is not accepted. Multimodal parts will extend
/// this record when they land — see the roadmap.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    #[schemars(length(min = 1))]
    pub message: String,
}

impl<T: Into<String>> From<T> for UserInput {
    fn from(message: T) -> Self {
        UserInput {
            message: message.into(),
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
    Ended,
    Failed,
}

/// What the API says about a session: its id, where it is, and how far its journal goes.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[schemars(transform = crate::schema::share_key_always_present)]
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
    #[schemars(regex(pattern = r"^sk\.ses_[A-Za-z0-9]{20,32}\.[0-9a-f]{64}$"))]
    pub share_key: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Event {
    pub event_id: EventId,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub recorded_at_ms: u64,
    #[schemars(schema_with = "crate::schema::identifier")]
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
    pub identity: AgentloopIdentity,
    pub status: AdmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::ApiError>,
}

/// The manifest inside an agentloop package: the contract the loop was built against
/// and the component it carries.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopManifest {
    #[schemars(schema_with = "crate::schema::agentloop_contract")]
    pub contract_version: String,
    pub component_identity: AgentloopIdentity,
    #[schemars(range(min = 1, max = 33554432))]
    pub component_bytes: usize,
    #[schemars(length(min = 1, max = 256))]
    pub toolchain: String,
}

/// What `POST /v1/agentloops` receives: the manifest and the component it describes,
/// base64-encoded.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopPackage {
    pub manifest: AgentloopManifest,
    pub component_base64: String,
}
