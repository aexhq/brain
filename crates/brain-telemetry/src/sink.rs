use async_trait::async_trait;

use crate::TelemetryRecord;

#[async_trait]
pub trait TelemetrySink: Send + Sync + 'static {
    /// Accept one ordered batch. Returning an error retries the whole batch, so sinks
    /// must make duplicate delivery harmless.
    async fn publish_batch(
        &self,
        records: &[TelemetryRecord],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
