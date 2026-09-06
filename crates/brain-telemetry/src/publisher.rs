use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{
    TelemetryLimits, TelemetryMetrics, TelemetryRecord, TelemetryWorker, queue::BoundedQueue,
};

#[derive(Clone)]
pub struct TelemetryPublisher {
    queue: Arc<Mutex<BoundedQueue>>,
    notify: Arc<Notify>,
    metrics: TelemetryMetrics,
}

/// A channel under the default limits.
pub fn telemetry_channel() -> (TelemetryPublisher, TelemetryWorker) {
    telemetry_channel_with(&TelemetryLimits::default())
}

pub fn telemetry_channel_with(limits: &TelemetryLimits) -> (TelemetryPublisher, TelemetryWorker) {
    let queue = Arc::new(Mutex::new(BoundedQueue::new(
        limits.max_telemetry_records,
        limits.max_telemetry_bytes,
    )));
    let notify = Arc::new(Notify::new());
    let metrics = TelemetryMetrics::default();
    (
        TelemetryPublisher {
            queue: queue.clone(),
            notify: notify.clone(),
            metrics: metrics.clone(),
        },
        TelemetryWorker::new(queue, notify, metrics, limits.retry_age()),
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
            sequence: None,
        }
    }

    #[test]
    fn rejects_records_without_exceeding_byte_or_count_bounds() {
        let limits = TelemetryLimits::default();
        let (publisher, _worker) = telemetry_channel_with(&limits);
        assert!(!publisher.try_publish(record(limits.max_telemetry_bytes)));
        for _ in 0..limits.max_telemetry_records {
            assert!(publisher.try_publish(record(0)));
        }
        assert!(!publisher.try_publish(record(0)));
        assert_eq!(
            publisher.metrics().queued_records(),
            limits.max_telemetry_records
        );
        assert_eq!(publisher.metrics().dropped_records(), 2);
    }
}
