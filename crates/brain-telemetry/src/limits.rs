use std::time::Duration;

/// What the telemetry queue may hold. Defaults are a starting point, not a measurement
/// of the machine; the deployment sets them.
#[derive(Clone, Debug, clap::Args)]
pub struct TelemetryLimits {
    /// Telemetry records the queue holds before it drops the newest; at least 1.
    #[arg(long, env = "BRAIN_MAX_TELEMETRY_RECORDS", default_value_t = TelemetryLimits::default().max_telemetry_records)]
    pub max_telemetry_records: usize,
    /// Bytes of telemetry the queue holds before it drops the newest; at least 1.
    #[arg(long, env = "BRAIN_MAX_TELEMETRY_BYTES", default_value_t = TelemetryLimits::default().max_telemetry_bytes)]
    pub max_telemetry_bytes: usize,
    /// Seconds a telemetry batch is retried against a failing sink before it is dropped.
    #[arg(long, env = "BRAIN_TELEMETRY_RETRY_SECS", default_value_t = TelemetryLimits::default().telemetry_retry_secs)]
    pub telemetry_retry_secs: u64,
}

impl Default for TelemetryLimits {
    fn default() -> Self {
        Self {
            max_telemetry_records: 4_096,
            max_telemetry_bytes: 8 * 1024 * 1024,
            telemetry_retry_secs: 30,
        }
    }
}

impl TelemetryLimits {
    pub fn retry_age(&self) -> Duration {
        Duration::from_secs(self.telemetry_retry_secs)
    }

    /// The queue is a capacity, not a ceiling that can be switched off.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_telemetry_records == 0 || self.max_telemetry_bytes == 0 {
            return Err(
                "BRAIN_MAX_TELEMETRY_RECORDS and BRAIN_MAX_TELEMETRY_BYTES must be at least 1"
                    .into(),
            );
        }
        Ok(())
    }
}
