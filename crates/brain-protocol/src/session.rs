use serde::{Deserialize, Serialize};

use crate::{
    AgentloopDigest, EnvironmentAttachment, EnvironmentId, EventId, JournalId, LifecyclePolicy,
    ModelBinding, RequestedToolBinding, SessionId, ToolBinding, ToolDefinition,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPresentation {
    pub system: String,
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub agentloop_digest: AgentloopDigest,
    pub model: ModelBinding,
    pub presentation: ModelPresentation,
    pub environments: Vec<EnvironmentRequirement>,
    pub tool_bindings: Vec<RequestedToolBinding>,
    #[serde(default)]
    pub metadata: serde_json::Value,
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
    pub agentloop_digest: AgentloopDigest,
    pub model: ModelBinding,
    pub presentation: ModelPresentation,
    pub environments: Vec<EnvironmentAttachment>,
    pub tool_bindings: Vec<ToolBinding>,
    pub metadata: serde_json::Value,
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
    pub presentation_digest: String,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    pub event_id: EventId,
    pub sequence: u64,
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
    pub digest: AgentloopDigest,
    pub status: AdmissionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::ApiError>,
}
