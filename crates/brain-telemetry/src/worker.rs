use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{
    MAX_RETRY_AGE, TelemetryMetrics, TelemetrySink, queue::BoundedQueue, retry::retry_delay,
};

pub struct TelemetryWorker {
    queue: Arc<Mutex<BoundedQueue>>,
    notify: Arc<Notify>,
    metrics: TelemetryMetrics,
}

impl TelemetryWorker {
    pub(crate) fn new(
        queue: Arc<Mutex<BoundedQueue>>,
        notify: Arc<Notify>,
        metrics: TelemetryMetrics,
    ) -> Self {
        Self {
            queue,
            notify,
            metrics,
        }
    }

    pub async fn run(self, sink: Arc<dyn TelemetrySink>) {
        loop {
            let queued = {
                self.queue
                    .lock()
                    .expect("telemetry queue mutex poisoned")
                    .pop()
            };
            let Some(queued) = queued else {
                self.notify.notified().await;
                continue;
            };
            self.metrics.removed(queued.bytes);
            let mut attempt = 0;
            loop {
                if sink.publish(&queued.record).await.is_ok() {
                    break;
                }
                let Some(delay) = retry_delay(attempt, queued.enqueued_at, MAX_RETRY_AGE) else {
                    self.metrics.dropped();
                    break;
                };
                attempt += 1;
                tokio::time::sleep(delay).await;
            }
        }
    }
}
