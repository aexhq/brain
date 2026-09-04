use brain_protocol::{Event, LiveEvent, SessionId};
use brain_telemetry::{TelemetryKind, TelemetryPublisher, TelemetryRecord};
use tokio::sync::broadcast;

use crate::journal::JournalRecord;

/// Records held for a subscriber that is not keeping up.
///
/// A live subscription must never be able to slow a turn down, so this is a bound on
/// memory and not a promise of delivery: a subscriber further behind than this loses the
/// records it missed and is told how many. Reading them back is what `after` is for — the
/// journal is the record, and the stream is a notification that it moved.
const LIVE_BACKLOG: usize = 1_024;

/// Where every session's records and live model output go as they happen: one feed per
/// process, shared by every session store, so a subscriber sees all of them in one place.
pub struct Feed {
    telemetry: TelemetryPublisher,
    live: broadcast::Sender<(SessionId, LiveEvent)>,
}

impl Feed {
    pub fn new(telemetry: TelemetryPublisher) -> Self {
        Self {
            telemetry,
            live: broadcast::Sender::new(LIVE_BACKLOG),
        }
    }

    /// Every record appended from now on, as it is appended.
    ///
    /// Subscribing before reading a page is what closes the gap between the two: a record
    /// appended in between arrives on the subscription, and the reader drops what it has
    /// already seen by sequence.
    pub fn subscribe(&self) -> broadcast::Receiver<(SessionId, LiveEvent)> {
        self.live.subscribe()
    }

    /// A handle for publishing model output as it arrives.
    ///
    /// Handed to a session's actor so that a turn in progress can be watched. It goes to
    /// subscribers and nowhere else: nothing here is written to a log, because the log's
    /// business is what a turn produced and a token is not that yet.
    pub fn live_sender(&self) -> broadcast::Sender<(SessionId, LiveEvent)> {
        self.live.clone()
    }

    pub(crate) fn publish(&self, record: &JournalRecord) {
        // Sent before telemetry, and never waited on: `send` returns immediately whether
        // or not anyone is listening, and drops for a receiver that has fallen behind.
        let _ = self.live.send((
            record.session_id.clone(),
            LiveEvent::Recorded(Event {
                event_id: record.event_id(),
                sequence: record.sequence,
                recorded_at_ms: record.recorded_at_ms,
                event_type: record.kind.clone(),
                data: record.payload.clone(),
            }),
        ));
        let _ = self.telemetry.try_publish(TelemetryRecord {
            kind: TelemetryKind::Event,
            name: record.kind.clone(),
            payload: serde_json::to_vec(&record.payload)
                .expect("journal payload is valid telemetry JSON"),
            session_id: Some(record.session_id.clone()),
            event_id: Some(record.event_id()),
        });
    }
}
