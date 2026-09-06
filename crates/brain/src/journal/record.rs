use brain_protocol::{Event, SessionId};
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

/// One journal record. `(session_id, sequence)` names it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionRecord {
    pub session_id: SessionId,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: String,
    pub payload: serde_json::Value,
}

impl SessionRecord {
    /// The record as a client reads it.
    pub fn into_event(self) -> Event {
        Event {
            sequence: self.sequence,
            recorded_at_ms: self.recorded_at_ms,
            event_type: self.kind,
            data: self.payload,
        }
    }
}
