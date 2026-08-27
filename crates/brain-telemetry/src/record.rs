use brain_protocol::{EventId, JournalId, OperationId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryKind {
    Event,
    Log,
    Metric,
    Trace,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TelemetryRecord {
    pub kind: TelemetryKind,
    pub name: String,
    pub payload: Vec<u8>,
    pub session_id: Option<SessionId>,
    pub journal_id: Option<JournalId>,
    pub event_id: Option<EventId>,
    pub operation_id: Option<OperationId>,
}

impl TelemetryRecord {
    pub fn encoded_len(&self) -> usize {
        self.name.len() + self.payload.len() + 256
    }
}
