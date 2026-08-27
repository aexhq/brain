use async_trait::async_trait;

use crate::TelemetryRecord;

#[async_trait]
pub trait TelemetrySink: Send + Sync + 'static {
    async fn publish(&self, record: &TelemetryRecord) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
