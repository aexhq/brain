use serde::{Deserialize, Serialize};

use crate::{AgentloopDigest, EventId, JournalId, ModelBinding, SessionId, ToolBinding, ToolDefinition};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelPresentation {
    pub system: String,
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateSessionRequest {
    pub agentloop_digest: AgentloopDigest,
    pub model: ModelBinding,
    pub presentation: ModelPresentation,
    pub tool_bindings: Vec<ToolBinding>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessageRequest {
    pub content: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus { Idle, Running, Ended, Failed }

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
