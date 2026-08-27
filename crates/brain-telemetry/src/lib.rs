//! Bounded, best-effort live telemetry for Brain.

mod metrics;
mod publisher;
mod queue;
mod record;
mod retry;
mod sink;
mod worker;

pub use metrics::TelemetryMetrics;
pub use publisher::{TelemetryPublisher, telemetry_channel};
pub use record::{DELIVERY_DROPPED_NAME, TelemetryKind, TelemetryRecord};
pub use sink::TelemetrySink;
pub use worker::TelemetryWorker;

pub const MAX_QUEUE_RECORDS: usize = 4_096;
pub const MAX_QUEUE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RETRY_AGE: std::time::Duration = std::time::Duration::from_secs(30);
