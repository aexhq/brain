use brain_protocol::SessionId;
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
    pub schema_version: u32,
    pub session_id: SessionId,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub checksum: String,
}
