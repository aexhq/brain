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
                    .drain()
            };
            if queued.is_empty() {
                self.notify.notified().await;
                continue;
            }
            self.metrics
                .removed(queued.len(), queued.iter().map(|record| record.bytes).sum());
            self.deliver(&sink, queued, MAX_RETRY_AGE).await;
        }
    }

    async fn deliver(
        &self,
        sink: &Arc<dyn TelemetrySink>,
        queued: Vec<QueuedRecord>,
        max_retry_age: Duration,
    ) {
        let enqueued_at = queued
            .first()
            .expect("delivery batches are never empty")
            .enqueued_at;
        let records = queued
            .iter()
            .map(|queued| queued.record.clone())
            .collect::<Vec<_>>();
        let mut attempt = 0;
        loop {
            if sink.publish_batch(&records).await.is_ok() {
                return;
            }
            let Some(delay) = retry_delay(attempt, enqueued_at, max_retry_age) else {
                self.metrics.dropped_by(queued.len());
                for queued in &queued {
                    if queued.record.name == DELIVERY_DROPPED_NAME {
                        continue;
                    }
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
        records: AtomicUsize,
    }

    #[async_trait]
    impl TelemetrySink for FailingSink {
        async fn publish_batch(
            &self,
            records: &[TelemetryRecord],
        ) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.records.store(records.len(), Ordering::Relaxed);
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
            name: "turn_ended".into(),
            payload: Vec::new(),
            session_id: None,
            event_id: None,
        }
    }

    fn drain(worker: &TelemetryWorker) -> Vec<QueuedRecord> {
        let queued = worker
            .queue
            .lock()
            .expect("telemetry queue mutex poisoned")
            .drain();
        worker
            .metrics
            .removed(queued.len(), queued.iter().map(|record| record.bytes).sum());
        queued
    }

    #[tokio::test]
    async fn batches_records_and_retries_a_transient_sink_failure() {
        let (publisher, worker) = telemetry_channel();
        assert!(publisher.try_publish(record()));
        assert!(publisher.try_publish(record()));
        let queued = drain(&worker);
        let sink = Arc::new(FailingSink {
            failures: 2,
            attempts: AtomicUsize::new(0),
            records: AtomicUsize::new(0),
        });
        worker
            .deliver(
                &(sink.clone() as Arc<dyn TelemetrySink>),
                queued,
                Duration::from_secs(1),
            )
            .await;
        assert_eq!(sink.attempts.load(Ordering::Relaxed), 3);
        assert_eq!(sink.records.load(Ordering::Relaxed), 2);
        assert_eq!(publisher.metrics().dropped_records(), 0);
        assert_eq!(publisher.metrics().queued_records(), 0);
    }

    #[tokio::test]
    async fn reports_retry_exhaustion_without_recursive_dead_letters() {
        let (publisher, worker) = telemetry_channel();
        assert!(publisher.try_publish(record()));
        assert!(publisher.try_publish(record()));
        let sink: Arc<dyn TelemetrySink> = Arc::new(FailingSink {
            failures: usize::MAX,
            attempts: AtomicUsize::new(0),
            records: AtomicUsize::new(0),
        });
        worker.deliver(&sink, drain(&worker), Duration::ZERO).await;
        assert_eq!(publisher.metrics().dropped_records(), 2);
        let dead_letters = drain(&worker);
        assert_eq!(dead_letters.len(), 2);
        assert_eq!(dead_letters[0].record.name, DELIVERY_DROPPED_NAME);
        assert_eq!(dead_letters[0].record.payload, b"turn_ended");

        worker.deliver(&sink, dead_letters, Duration::ZERO).await;
        assert_eq!(publisher.metrics().dropped_records(), 4);
        assert!(
            worker
                .queue
                .lock()
                .expect("telemetry queue mutex poisoned")
                .drain()
                .is_empty()
        );
    }
}
