use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LoopLimits {
    pub package_bytes: usize,
    pub turn_input_bytes: usize,
    pub turn_output_bytes: usize,
    pub linear_memory_bytes: usize,
    /// The guest's own compute per turn. Time the guest spends waiting on a host call
    /// (a model answering, a tool running) is not charged against it.
    pub wall_time: Duration,
    /// Turns a worker runs at once. Each holds a live Wasm instance and a thread for as
    /// long as it runs, so this times `linear_memory_bytes` is the worst case a worker
    /// can hold.
    pub concurrent_turns_per_worker: usize,
    /// How many more may be waiting for one of those slots before the pool says no.
    /// Beyond this a turn is refused rather than queued, so a busy Brain reports that
    /// it is busy instead of building a backlog nobody is waiting on any more.
    pub queued_turns_per_worker: usize,
}

impl Default for LoopLimits {
    fn default() -> Self {
        Self {
            package_bytes: super::MAX_PACKAGE_BYTES,
            turn_input_bytes: super::MAX_TURN_INPUT_BYTES,
            turn_output_bytes: super::MAX_TURN_OUTPUT_BYTES,
            linear_memory_bytes: super::MAX_LINEAR_MEMORY_BYTES,
            wall_time: Duration::from_secs(2),
            concurrent_turns_per_worker: super::DEFAULT_CONCURRENT_TURNS,
            queued_turns_per_worker: 8,
        }
    }
}
