use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{MAX_QUEUE_BYTES, MAX_QUEUE_RECORDS, TelemetryMetrics, TelemetryRecord, TelemetryWorker, queue::BoundedQueue};

#[derive(Clone)]
pub struct TelemetryPublisher {
    queue: Arc<Mutex<BoundedQueue>>,
    notify: Arc<Notify>,
    metrics: TelemetryMetrics,
}

pub fn telemetry_channel() -> (TelemetryPublisher, TelemetryWorker) {
    let queue = Arc::new(Mutex::new(BoundedQueue::new(MAX_QUEUE_RECORDS, MAX_QUEUE_BYTES)));
    let notify = Arc::new(Notify::new());
    let metrics = TelemetryMetrics::default();
    (
        TelemetryPublisher { queue: queue.clone(), notify: notify.clone(), metrics: metrics.clone() },
        TelemetryWorker::new(queue, notify, metrics),
    )
}

impl TelemetryPublisher {
    pub fn try_publish(&self, record: TelemetryRecord) -> bool {
        let accepted = self.queue.lock().expect("telemetry queue mutex poisoned").try_push(record);
        match accepted {
            Ok(bytes) => {
                self.metrics.accepted(bytes);
                self.notify.notify_one();
                true
            }
            Err(_) => {
                self.metrics.dropped();
                false
            }
        }
    }

    pub fn metrics(&self) -> TelemetryMetrics { self.metrics.clone() }
}
