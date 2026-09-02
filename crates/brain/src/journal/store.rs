use brain_protocol::{SessionId, SessionStatus, SessionSummary};

use crate::{
    Error,
    journal::{AppendRecord, JournalRecord},
};

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub status: SessionStatus,
    pub through_sequence: u64,
    pub configuration: serde_json::Value,
    pub context: serde_json::Value,
}

/// What a caller outside the session is allowed to see about it. Building one borrows
/// the row rather than cloning it, so listing sessions never copies a conversation or a
/// configuration.
impl From<&SessionRow> for SessionSummary {
    fn from(row: &SessionRow) -> Self {
        Self {
            session_id: row.session_id.clone(),
            status: row.status.clone(),
            last_sequence: row.through_sequence,
            share_key: String::new(),
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
    ) -> Result<JournalRecord, Error>;
    fn append(
        &self,
        session_id: &SessionId,
        expected_through: u64,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, Error>;
    /// The whole row, including configuration and context. Only rehydrating a session
    /// actor needs this; everything else wants a summary.
    fn session_row(&self, session_id: &SessionId) -> Result<Option<SessionRow>, Error>;
    fn session_summary(&self, session_id: &SessionId) -> Result<Option<SessionSummary>, Error>;
    fn session_summaries(&self) -> Result<Vec<SessionSummary>, Error>;
    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, Error>;
    fn delete_ended(&self, session_id: &SessionId) -> Result<(), Error>;
    /// Whether this session was rebuilt from the journal and has not yet been handed its own
    /// records back. Answers true once and then false: the agentloop is told what it is
    /// continuing exactly once, and telling it twice would replay a conversation it is
    /// already holding.
    fn take_restored(&self, session_id: &SessionId) -> Result<bool, Error>;
}
