//! Bounded, best-effort live telemetry for Brain.

mod limits;
mod metrics;
mod publisher;
mod queue;
mod record;
mod retry;
mod sink;
mod worker;

pub use limits::TelemetryLimits;
pub use metrics::TelemetryMetrics;
pub use publisher::{TelemetryPublisher, telemetry_channel, telemetry_channel_with};
pub use record::{DELIVERY_DROPPED_NAME, TelemetryKind, TelemetryRecord};
pub use sink::TelemetrySink;
pub use worker::TelemetryWorker;
