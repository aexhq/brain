use brain_protocol::{EventId, JournalId, OperationId, SessionId};
use serde::{Deserialize, Serialize};

pub const DELIVERY_DROPPED_NAME: &str = "telemetry_delivery_dropped";

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

    pub(crate) fn delivery_dropped(&self) -> Self {
        Self {
            kind: TelemetryKind::Metric,
            name: DELIVERY_DROPPED_NAME.into(),
            payload: self.name.as_bytes().to_vec(),
            session_id: self.session_id.clone(),
            journal_id: self.journal_id.clone(),
            event_id: self.event_id.clone(),
            operation_id: self.operation_id.clone(),
        }
    }
}
