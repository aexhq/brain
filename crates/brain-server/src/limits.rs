use std::time::Duration;

/// The budgets the server itself enforces around Environments, hosts, and request
/// claims. Brain ships a default for each and never treats it as a fact about the
/// machine: the deployment sets them. Zero on any ceiling means no bound, except the
/// host command queue, which must be at least 1.
#[derive(Clone, Debug, clap::Args)]
pub struct ServerLimits {
    /// Bytes one HTTP Environment response may hold.
    #[arg(long, env = "BRAIN_MAX_ENVIRONMENT_RESPONSE_BYTES", default_value_t = ServerLimits::default().max_environment_response_bytes)]
    pub max_environment_response_bytes: usize,
    /// Seconds one HTTP Environment call other than a turn may take.
    #[arg(long, env = "BRAIN_MAX_ENVIRONMENT_SECS", default_value_t = ServerLimits::default().max_environment_secs)]
    pub max_environment_secs: u64,
    /// Seconds connecting to an HTTP Environment may take.
    #[arg(long, env = "BRAIN_MAX_ENVIRONMENT_CONNECT_SECS", default_value_t = ServerLimits::default().max_environment_connect_secs)]
    pub max_environment_connect_secs: u64,
    /// Registered hosts the server keeps before it refuses a registration.
    #[arg(long, env = "BRAIN_MAX_HOSTS", default_value_t = ServerLimits::default().max_hosts)]
    pub max_hosts: usize,
    /// Commands queued for one connected host before the server waits; at least 1.
    #[arg(long, env = "BRAIN_MAX_HOST_COMMANDS", default_value_t = ServerLimits::default().max_host_commands)]
    pub max_host_commands: usize,
    /// Seconds a registered host with no sessions may stay disconnected before its
    /// registration is dropped.
    #[arg(long, env = "BRAIN_HOST_UNCONNECTED_SECS", default_value_t = ServerLimits::default().host_unconnected_secs)]
    pub host_unconnected_secs: u64,
    /// Seconds a completed keyed request's answer is kept for replay.
    #[arg(long, env = "BRAIN_REQUEST_RETENTION_SECS", default_value_t = ServerLimits::default().request_retention_secs)]
    pub request_retention_secs: u64,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            // A Tool that returns a file returns it whole; this matches the request cap.
            max_environment_response_bytes: 32 * 1024 * 1024,
            max_environment_secs: 120,
            max_environment_connect_secs: 5,
            max_hosts: 4_096,
            max_host_commands: 128,
            host_unconnected_secs: 60,
            request_retention_secs: 24 * 60 * 60,
        }
    }
}

impl ServerLimits {
    pub fn max_environment(&self) -> Option<Duration> {
        secs(self.max_environment_secs)
    }

    pub fn max_environment_connect(&self) -> Option<Duration> {
        secs(self.max_environment_connect_secs)
    }

    /// How long an unconnected host without sessions is kept. No bound keeps it forever.
    pub fn host_unconnected(&self) -> Duration {
        secs(self.host_unconnected_secs).unwrap_or(Duration::MAX)
    }

    /// How long a completed answer is kept. No bound keeps it forever.
    pub fn request_retention(&self) -> Duration {
        secs(self.request_retention_secs).unwrap_or(Duration::MAX)
    }

    /// The host command queue is a bounded channel and cannot be unbounded.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_host_commands == 0 {
            return Err("BRAIN_MAX_HOST_COMMANDS must be at least 1".into());
        }
        Ok(())
    }
}

fn secs(value: u64) -> Option<Duration> {
    (value != 0).then(|| Duration::from_secs(value))
}

/// A byte or count ceiling as the code compares against it: zero means no bound.
pub(crate) fn ceiling(value: usize) -> usize {
    if value == 0 { usize::MAX } else { value }
}
