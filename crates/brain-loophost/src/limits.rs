use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LoopLimits {
    pub package_bytes: usize,
    pub activation_input_bytes: usize,
    pub activation_output_bytes: usize,
    pub linear_memory_bytes: usize,
    pub wall_time: Duration,
    /// Activations a worker runs at once. Each is a live Wasm instance, so this times
    /// `linear_memory_bytes` is the worst case a worker can hold, and concurrency here
    /// has to stay explicitly bounded for that reason: 48 at once measured 1.03 GiB.
    pub concurrent_activations_per_worker: usize,
    /// How many more may be waiting for one of those slots before the pool says no.
    /// Beyond this an activation is refused rather than queued, so a busy Brain reports
    /// that it is busy instead of building a backlog nobody is waiting on any more.
    pub queued_activations_per_worker: usize,
}

impl Default for LoopLimits {
    fn default() -> Self {
        Self {
            package_bytes: super::MAX_PACKAGE_BYTES,
            activation_input_bytes: super::MAX_ACTIVATION_INPUT_BYTES,
            activation_output_bytes: super::MAX_ACTIVATION_OUTPUT_BYTES,
            linear_memory_bytes: super::MAX_LINEAR_MEMORY_BYTES,
            wall_time: Duration::from_secs(2),
            concurrent_activations_per_worker: super::DEFAULT_CONCURRENT_ACTIVATIONS,
            queued_activations_per_worker: 2,
        }
    }
}
