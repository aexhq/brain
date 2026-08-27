use brain_protocol::{ContextEnvelope, SessionStatus};

use crate::{KernelError, journal::JournalRecord};

#[derive(Clone, Debug)]
pub struct FoldedSession {
    pub through_sequence: u64,
    pub status: SessionStatus,
    pub context: ContextEnvelope,
    pub pending_operations: Vec<serde_json::Value>,
}

pub fn fold_records(
    initial: ContextEnvelope,
    records: &[JournalRecord],
) -> Result<FoldedSession, KernelError> {
    let mut folded = FoldedSession {
        through_sequence: 0,
        status: SessionStatus::Idle,
        context: initial,
        pending_operations: Vec::new(),
    };
    for record in records {
        if record.sequence != folded.through_sequence + 1 {
            return Err(KernelError::Journal(format!(
                "journal sequence gap at {}",
                record.sequence
            )));
        }
        folded.through_sequence = record.sequence;
        match record.kind.as_str() {
            "turn_started" => folded.status = SessionStatus::Running,
            "turn_finished" | "turn_failed" => folded.status = SessionStatus::Idle,
            "session_ended" => folded.status = SessionStatus::Ended,
            "context_updated" => {
                folded.context = serde_json::from_value(record.payload.clone())
                    .map_err(|error| KernelError::Journal(error.to_string()))?;
            }
            kind if kind.ends_with("_intent") => {
                folded.pending_operations.push(record.payload.clone())
            }
            kind if kind.ends_with("_result") || kind.ends_with("_ambiguous") => {
                if let Some(operation_id) = record.payload.get("operation_id") {
                    folded
                        .pending_operations
                        .retain(|pending| pending.get("operation_id") != Some(operation_id));
                }
            }
            _ => {}
        }
    }
    Ok(folded)
}
