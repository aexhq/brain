use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LoopLimits {
    pub package_bytes: usize,
    pub activation_input_bytes: usize,
    pub activation_output_bytes: usize,
    pub linear_memory_bytes: usize,
    pub wall_time: Duration,
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
            queued_activations_per_worker: 2,
        }
    }
}
