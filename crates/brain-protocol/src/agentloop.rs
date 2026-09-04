use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{Event, Message, SessionId, ToolDefinition, UserInput};

pub const AGENTLOOP_CONTRACT_VERSION: &str = "agentloop/v1";

/// The most items a transcript may hold.
pub const MAX_TRANSCRIPT_ITEMS: usize = 4_096;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeEnvelope {
    pub logical_time_ms: u64,
    pub deterministic_seed: Vec<u8>,
}

impl RuntimeEnvelope {
    /// The runtime a turn sees. Both fields come from where the turn sits -- which
    /// session, how far in -- so replaying a position hands the loop the same clock and
    /// the same randomness it had the first time.
    pub fn at(session_id: &SessionId, logical_position: u64) -> Self {
        let mut seed = Sha256::new();
        seed.update(b"aex-brain-seed-v2 ");
        seed.update(session_id.as_str().as_bytes());
        seed.update(logical_position.to_be_bytes());
        Self {
            logical_time_ms: logical_position,
            deterministic_seed: seed.finalize().to_vec(),
        }
    }
}

/// What Brain hands the loop for one turn.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TurnInput {
    pub input: UserInput,
    /// The transcript as it stands: what the next model call would see.
    pub transcript: Vec<Message>,
    /// The loop's slots by name, as it last returned them.
    pub slots: BTreeMap<String, serde_json::Value>,
    /// Every record on the session's feed since the loop last ran, oldest first, so a
    /// loop sees what happened to its environments and tools between turns.
    pub events: Vec<Event>,
    pub configuration: serde_json::Value,
    /// The system prompt the session was created with. Used on every model call unless
    /// the loop sends its own.
    pub system: String,
    /// The tools the session was created with: offered whole on every model call unless
    /// the loop names a subset. Brain admitted and provisioned exactly these.
    pub tools: Vec<ToolDefinition>,
    pub runtime: RuntimeEnvelope,
}

/// What the loop hands back when the turn is done.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TurnOutput {
    pub transcript: Vec<Message>,
    /// Slots to keep. A name the loop leaves out keeps its previous value.
    #[serde(default)]
    pub slots: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

/// Why a turn, or one of the host calls inside it, failed.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TurnError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

impl TurnError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runtime_envelope_is_fixed_by_where_the_turn_sits() {
        let session = SessionId::new("ses_example");
        let envelope = RuntimeEnvelope::at(&session, 7);
        assert_eq!(envelope.logical_time_ms, 7);
        assert_eq!(
            envelope.deterministic_seed,
            RuntimeEnvelope::at(&session, 7).deterministic_seed
        );
        for other in [
            RuntimeEnvelope::at(&session, 8),
            RuntimeEnvelope::at(&SessionId::new("ses_other"), 7),
        ] {
            assert_ne!(envelope.deterministic_seed, other.deterministic_seed);
        }
    }
}
