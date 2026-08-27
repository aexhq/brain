use brain_protocol::{JournalId, SessionId, SessionStatus};

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
    pub presentation_digest: String,
}

#[derive(Default)]
pub struct SessionUpdate<'a> {
    pub status: Option<SessionStatus>,
    pub context: Option<&'a serde_json::Value>,
    pub configuration: Option<&'a serde_json::Value>,
}

pub trait JournalStore: Send + Sync + 'static {
    fn journal_id(&self) -> &JournalId;
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
    fn session(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError>;
    fn sessions(&self) -> Result<Vec<SessionRow>, KernelError>;
    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, KernelError>;
    fn delete_ended(&self, session_id: &SessionId) -> Result<(), KernelError>;
    fn idempotency_get(
        &self,
        scope: &str,
        key: &str,
        digest: &str,
    ) -> Result<Option<serde_json::Value>, KernelError>;
    fn idempotency_put(
        &self,
        scope: &str,
        key: &str,
        digest: &str,
        response: &serde_json::Value,
    ) -> Result<(), KernelError>;
}
