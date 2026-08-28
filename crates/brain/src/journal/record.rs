use brain_protocol::{EventId, JournalId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppendRecord {
    pub kind: String,
    pub payload: serde_json::Value,
}

impl AppendRecord {
    pub fn new(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JournalRecord {
    pub session_id: SessionId,
    pub journal_id: JournalId,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: String,
    pub payload: serde_json::Value,
}

impl JournalRecord {
    pub fn event_id(&self) -> EventId {
        EventId::new(format!("evt_{}_{}", self.session_id, self.sequence))
    }
}
