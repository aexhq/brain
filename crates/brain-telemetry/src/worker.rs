use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Notify;

use crate::{
    DELIVERY_DROPPED_NAME, MAX_RETRY_AGE, TelemetryMetrics, TelemetrySink,
    queue::{BoundedQueue, QueuedRecord},
    retry::retry_delay,
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
            self.deliver(&sink, queued, MAX_RETRY_AGE).await;
        }
    }

    async fn deliver(
        &self,
        sink: &Arc<dyn TelemetrySink>,
        queued: QueuedRecord,
        max_retry_age: Duration,
    ) {
        let mut attempt = 0;
        loop {
            if sink.publish(&queued.record).await.is_ok() {
                return;
            }
            let Some(delay) = retry_delay(attempt, queued.enqueued_at, max_retry_age) else {
                self.metrics.dropped();
                if queued.record.name != DELIVERY_DROPPED_NAME {
                    let accepted = self
                        .queue
                        .lock()
                        .expect("telemetry queue mutex poisoned")
                        .try_push(queued.record.delivery_dropped());
                    match accepted {
                        Some(bytes) => self.metrics.accepted(bytes),
                        None => self.metrics.dropped(),
                    }
                }
                return;
            };
            attempt += 1;
            tokio::time::sleep(delay).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;

    use super::*;
    use crate::{TelemetryKind, TelemetryRecord, telemetry_channel};

    struct FailingSink {
        failures: usize,
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl TelemetrySink for FailingSink {
        async fn publish(&self, _: &TelemetryRecord) -> Result<(), Box<dyn Error + Send + Sync>> {
            let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
            if attempt < self.failures {
                Err("sink unavailable".into())
            } else {
                Ok(())
            }
        }
    }

    fn record() -> TelemetryRecord {
        TelemetryRecord {
            kind: TelemetryKind::Event,
            name: "turn_finished".into(),
            payload: Vec::new(),
            session_id: None,
            journal_id: None,
            event_id: None,
            operation_id: None,
        }
    }

    fn pop(worker: &TelemetryWorker) -> QueuedRecord {
        let queued = worker
            .queue
            .lock()
            .expect("telemetry queue mutex poisoned")
            .pop()
            .expect("test record is queued");
        worker.metrics.removed(queued.bytes);
        queued
    }

    #[tokio::test]
    async fn retries_a_transient_sink_failure() {
        let (publisher, worker) = telemetry_channel();
        assert!(publisher.try_publish(record()));
        let queued = pop(&worker);
        let sink = Arc::new(FailingSink {
            failures: 2,
            attempts: AtomicUsize::new(0),
        });
        worker
            .deliver(
                &(sink.clone() as Arc<dyn TelemetrySink>),
                queued,
                Duration::from_secs(1),
            )
            .await;
        assert_eq!(sink.attempts.load(Ordering::Relaxed), 3);
        assert_eq!(publisher.metrics().dropped_records(), 0);
    }

    #[tokio::test]
    async fn reports_retry_exhaustion_without_recursive_dead_letters() {
        let (publisher, worker) = telemetry_channel();
        assert!(publisher.try_publish(record()));
        let sink: Arc<dyn TelemetrySink> = Arc::new(FailingSink {
            failures: usize::MAX,
            attempts: AtomicUsize::new(0),
        });
        worker.deliver(&sink, pop(&worker), Duration::ZERO).await;
        assert_eq!(publisher.metrics().dropped_records(), 1);
        let dead_letter = pop(&worker);
        assert_eq!(dead_letter.record.name, DELIVERY_DROPPED_NAME);
        assert_eq!(dead_letter.record.payload, b"turn_finished");

        worker.deliver(&sink, dead_letter, Duration::ZERO).await;
        assert_eq!(publisher.metrics().dropped_records(), 2);
        assert!(
            worker
                .queue
                .lock()
                .expect("telemetry queue mutex poisoned")
                .pop()
                .is_none()
        );
    }
}
