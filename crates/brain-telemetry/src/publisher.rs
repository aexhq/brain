use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{
    MAX_QUEUE_BYTES, MAX_QUEUE_RECORDS, TelemetryMetrics, TelemetryRecord, TelemetryWorker,
    queue::BoundedQueue,
};

#[derive(Clone)]
pub struct TelemetryPublisher {
    queue: Arc<Mutex<BoundedQueue>>,
    notify: Arc<Notify>,
    metrics: TelemetryMetrics,
}

pub fn telemetry_channel() -> (TelemetryPublisher, TelemetryWorker) {
    let queue = Arc::new(Mutex::new(BoundedQueue::new(
        MAX_QUEUE_RECORDS,
        MAX_QUEUE_BYTES,
    )));
    let notify = Arc::new(Notify::new());
    let metrics = TelemetryMetrics::default();
    (
        TelemetryPublisher {
            queue: queue.clone(),
            notify: notify.clone(),
            metrics: metrics.clone(),
        },
        TelemetryWorker::new(queue, notify, metrics),
    )
}

impl TelemetryPublisher {
    pub fn try_publish(&self, record: TelemetryRecord) -> bool {
        let accepted = self
            .queue
            .lock()
            .expect("telemetry queue mutex poisoned")
            .try_push(record);
        match accepted {
            Some(bytes) => {
                self.metrics.accepted(bytes);
                self.notify.notify_one();
                true
            }
            None => {
                self.metrics.dropped();
                false
            }
        }
    }

    pub fn metrics(&self) -> TelemetryMetrics {
        self.metrics.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TelemetryKind, TelemetryRecord};

    fn record(payload: usize) -> TelemetryRecord {
        TelemetryRecord {
            kind: TelemetryKind::Log,
            name: "test".into(),
            payload: vec![0; payload],
            session_id: None,
            event_id: None,
        }
    }

    #[test]
    fn rejects_records_without_exceeding_byte_or_count_bounds() {
        let (publisher, _worker) = telemetry_channel();
        assert!(!publisher.try_publish(record(MAX_QUEUE_BYTES)));
        for _ in 0..MAX_QUEUE_RECORDS {
            assert!(publisher.try_publish(record(0)));
        }
        assert!(!publisher.try_publish(record(0)));
        assert_eq!(publisher.metrics().queued_records(), MAX_QUEUE_RECORDS);
        assert_eq!(publisher.metrics().dropped_records(), 2);
    }
}
