use std::sync::Arc;

use brain_protocol::{Event, Identity, Session, SessionId};
use brain_telemetry::{TelemetryKind, TelemetryPublisher, TelemetryRecord};
use tokio::sync::broadcast;

use crate::{
    KernelError,
    journal::{AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate},
};

/// Records held for a subscriber that is not keeping up.
///
/// A live subscription must never be able to slow a turn down, so this is a bound on
/// memory and not a promise of delivery: a subscriber further behind than this loses the
/// records it missed and is told how many. Reading them back is what `after` is for — the
/// journal is the record, and the stream is a notification that it moved.
const LIVE_BACKLOG: usize = 1_024;

pub(crate) struct ObservedJournal {
    inner: Arc<dyn JournalStore>,
    telemetry: TelemetryPublisher,
    live: broadcast::Sender<(SessionId, Event)>,
}

impl ObservedJournal {
    pub(crate) fn new(inner: Arc<dyn JournalStore>, telemetry: TelemetryPublisher) -> Self {
        Self {
            inner,
            telemetry,
            live: broadcast::Sender::new(LIVE_BACKLOG),
        }
    }

    /// Every record appended from now on, as it is appended.
    ///
    /// Subscribing before reading a page is what closes the gap between the two: a record
    /// appended in between arrives on the subscription, and the reader drops what it has
    /// already seen by sequence.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<(SessionId, Event)> {
        self.live.subscribe()
    }

    fn publish(&self, record: &JournalRecord) {
        // Sent before telemetry, and never waited on: `send` returns immediately whether
        // or not anyone is listening, and drops for a receiver that has fallen behind.
        let _ = self.live.send((
            record.session_id.clone(),
            Event {
                event_id: record.event_id(),
                sequence: record.sequence,
                recorded_at_ms: record.recorded_at_ms,
                event_type: record.kind.clone(),
                data: record.payload.clone(),
            },
        ));
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

    fn session_row(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError> {
        self.inner.session_row(session_id)
    }

    fn session_summary(&self, session_id: &SessionId) -> Result<Option<Session>, KernelError> {
        self.inner.session_summary(session_id)
    }

    fn session_summaries(&self) -> Result<Vec<Session>, KernelError> {
        self.inner.session_summaries()
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
        request: &Identity,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        self.inner.idempotency_get(scope, key, request)
    }

    fn idempotency_put(
        &self,
        scope: &str,
        key: &str,
        request: &Identity,
        response: &serde_json::Value,
    ) -> Result<(), KernelError> {
        self.inner.idempotency_put(scope, key, request, response)
    }
}
