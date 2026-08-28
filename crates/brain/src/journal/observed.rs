use std::sync::Arc;

use brain_protocol::SessionId;
use brain_telemetry::{TelemetryKind, TelemetryPublisher, TelemetryRecord};

use crate::{
    KernelError,
    journal::{AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate},
};

pub(crate) struct ObservedJournal {
    inner: Arc<dyn JournalStore>,
    telemetry: TelemetryPublisher,
}

impl ObservedJournal {
    pub(crate) fn new(inner: Arc<dyn JournalStore>, telemetry: TelemetryPublisher) -> Self {
        Self { inner, telemetry }
    }

    fn publish(&self, record: &JournalRecord) {
        let _ = self.telemetry.try_publish(TelemetryRecord {
            kind: TelemetryKind::Event,
            name: record.kind.clone(),
            payload: serde_json::to_vec(&record.payload)
                .expect("journal payload is valid telemetry JSON"),
            session_id: Some(record.session_id.clone()),
            journal_id: Some(record.journal_id.clone()),
            event_id: Some(record.event_id()),
            operation_id: None,
        });
    }
}

impl JournalStore for ObservedJournal {
    fn create_session(
        &self,
        row: &SessionRow,
        record: AppendRecord,
    ) -> Result<JournalRecord, KernelError> {
        let saved = self.inner.create_session(row, record)?;
        self.publish(&saved);
        Ok(saved)
    }

    fn append(
        &self,
        session_id: &SessionId,
        expected_through: u64,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, KernelError> {
        let saved = self
            .inner
            .append(session_id, expected_through, records, update)?;
        for record in &saved {
            self.publish(record);
        }
        Ok(saved)
    }

    fn session(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError> {
        self.inner.session(session_id)
    }

    fn sessions(&self) -> Result<Vec<SessionRow>, KernelError> {
        self.inner.sessions()
    }

    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, KernelError> {
        self.inner.records_after(session_id, after, limit)
    }

    fn delete_ended(&self, session_id: &SessionId) -> Result<(), KernelError> {
        self.inner.delete_ended(session_id)
    }

    fn idempotency_get(
        &self,
        scope: &str,
        key: &str,
        digest: &str,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        self.inner.idempotency_get(scope, key, digest)
    }

    fn idempotency_put(
        &self,
        scope: &str,
        key: &str,
        digest: &str,
        response: &serde_json::Value,
    ) -> Result<(), KernelError> {
        self.inner.idempotency_put(scope, key, digest, response)
    }
}
