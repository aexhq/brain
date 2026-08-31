use std::collections::HashMap;

use brain_protocol::{Identity, JournalId, Session, SessionId, SessionStatus};

use crate::{
    KernelError,
    journal::{AppendRecord, JournalRecord},
};

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub journal_id: JournalId,
    pub status: SessionStatus,
    pub through_sequence: u64,
    pub configuration: serde_json::Value,
    pub context: serde_json::Value,
    pub presentation_identity: Identity,
}

/// What a caller outside the kernel is allowed to see about a session. Building one
/// borrows the row rather than cloning it, so listing sessions never copies a
/// conversation or a configuration.
impl From<&SessionRow> for Session {
    fn from(row: &SessionRow) -> Self {
        Self {
            session_id: row.session_id.clone(),
            journal_id: row.journal_id.clone(),
            status: row.status.clone(),
            last_sequence: row.through_sequence,
            config_hash: row.presentation_identity,
        }
    }
}

#[derive(Default)]
pub struct SessionUpdate<'a> {
    pub status: Option<SessionStatus>,
    pub context: Option<&'a serde_json::Value>,
    pub configuration: Option<&'a serde_json::Value>,
}

pub trait JournalStore: Send + Sync + 'static {
    fn create_session(
        &self,
        row: &SessionRow,
        record: AppendRecord,
    ) -> Result<JournalRecord, KernelError>;
    fn append(
        &self,
        session_id: &SessionId,
        expected_through: u64,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, KernelError>;
    /// The whole row, including configuration and context. Only rehydrating a session
    /// actor needs this; everything else wants a summary.
    fn session_row(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError>;
    fn session_summary(&self, session_id: &SessionId) -> Result<Option<Session>, KernelError>;
    fn session_summaries(&self) -> Result<Vec<Session>, KernelError>;
    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, KernelError>;
    fn delete_ended(&self, session_id: &SessionId) -> Result<(), KernelError>;
    /// Whether this session was rebuilt from the journal and has not yet been handed its own
    /// records back. Answers true once and then false: the agentloop is told what it is
    /// continuing exactly once, and telling it twice would replay a conversation it is
    /// already holding.
    fn take_restored(&self, session_id: &SessionId) -> Result<bool, KernelError>;
    /// Gives restored sessions back the journal ids the server minted for them.
    ///
    /// A `journal_id` never appears in a session's own records, and `operation_id` hashes
    /// it one way, so it cannot be derived from the conversation. A restored session whose
    /// id the server no longer has keeps its placeholder and is readable but cannot take
    /// another turn — the tail of a best-effort write is allowed to be missing, and this
    /// is what that looks like from the inside.
    fn adopt_journal_ids(&self, journals: &HashMap<String, String>) -> Result<(), KernelError>;
    /// The answer already recorded under `key`, if the request is the same one.
    /// Reusing a key for different content is an error, not a miss.
    fn idempotency_get(
        &self,
        scope: &str,
        key: &str,
        request: &Identity,
    ) -> Result<Option<serde_json::Value>, KernelError>;
    fn idempotency_put(
        &self,
        scope: &str,
        key: &str,
        request: &Identity,
        response: &serde_json::Value,
    ) -> Result<(), KernelError>;
}
