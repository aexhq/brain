use brain_protocol::{Event, LiveEvent, SessionId};
use brain_telemetry::{TelemetryKind, TelemetryPublisher, TelemetryRecord};
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::broadcast;

use crate::journal::SessionRecord;

/// Records held for a subscriber that is not keeping up.
///
/// A live subscription must never be able to slow a turn down, so this is a bound on
/// memory and not a promise of delivery: a subscriber further behind than this loses the
/// records it missed and is told how many. Reading them back is what `after` is for — the
/// journal is the record, and the stream is a notification that it moved.
const LIVE_BACKLOG: usize = 1_024;

/// Where every session's records and live model output go as they happen: one feed per
/// process, with independent backlogs for each subscribed session.
pub struct Feed {
    telemetry: TelemetryPublisher,
    live: Mutex<HashMap<SessionId, broadcast::Sender<(SessionId, LiveEvent)>>>,
}

impl Feed {
    pub fn new(telemetry: TelemetryPublisher) -> Self {
        Self {
            telemetry,
            live: Mutex::new(HashMap::new()),
        }
    }

    /// Every record appended from now on, as it is appended.
    ///
    /// Subscribing before reading a page is what closes the gap between the two: a record
    /// appended in between arrives on the subscription, and the reader drops what it has
    /// already seen by sequence.
    pub fn subscribe(&self, session_id: &SessionId) -> broadcast::Receiver<(SessionId, LiveEvent)> {
        let mut live = self.live.lock().expect("live feed poisoned");
        live.retain(|_, sender| sender.receiver_count() > 0);
        live.entry(session_id.clone())
            .or_insert_with(|| broadcast::Sender::new(LIVE_BACKLOG))
            .subscribe()
    }

    pub fn send(&self, (session_id, event): (SessionId, LiveEvent)) {
        let mut live = self.live.lock().expect("live feed poisoned");
        if let Some(sender) = live.get(&session_id)
            && sender.send((session_id.clone(), event)).is_err()
        {
            live.remove(&session_id);
        }
    }

    pub(crate) fn publish(&self, record: &SessionRecord) {
        // Sent before telemetry, and never waited on: `send` returns immediately whether
        // or not anyone is listening, and drops for a receiver that has fallen behind.
        self.send((
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_sessions_do_not_consume_a_subscribers_backlog() {
        let (telemetry, _) = brain_telemetry::telemetry_channel();
        let feed = Feed::new(telemetry);
        let quiet = SessionId::new("quiet");
        let noisy = SessionId::new("noisy");
        let mut receiver = feed.subscribe(&quiet);
        let _noisy = feed.subscribe(&noisy);
        let event = LiveEvent::Streaming(brain_protocol::StreamingEvent {
            sequence: 1,
            event_type: "assistant_delta".into(),
            data: serde_json::json!({}),
        });
        feed.send((quiet.clone(), event.clone()));
        for _ in 0..LIVE_BACKLOG * 2 {
            feed.send((noisy.clone(), event.clone()));
        }
        assert_eq!(receiver.try_recv().unwrap().0, quiet);
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
}
