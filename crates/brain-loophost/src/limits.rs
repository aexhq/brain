/// Fixed MVP allowance for small orchestration Components. Fuel is a stable work
/// ceiling within one Wasmtime version, not a duration or a cross-machine CPU measure.
const DEFAULT_INVOCATION_FUEL: u64 = 10_000_000_000;

#[derive(Clone, Debug)]
pub struct LoopLimits {
    pub package_bytes: usize,
    pub turn_input_bytes: usize,
    pub turn_output_bytes: usize,
    pub linear_memory_bytes: usize,
    /// Wasmtime work units one invocation may consume. Host and WASI waits consume none.
    pub fuel: u64,
    /// Capacity per invocation class: this many Agentloops and this many native Tools.
    /// Each retains a fresh Store during host waits; the combined linear-memory ceiling
    /// is twice this count times `linear_memory_bytes`.
    pub concurrent_turns_per_worker: usize,
}

impl Default for LoopLimits {
    fn default() -> Self {
        Self {
            package_bytes: super::MAX_PACKAGE_BYTES,
            turn_input_bytes: super::MAX_TURN_INPUT_BYTES,
            turn_output_bytes: super::MAX_TURN_OUTPUT_BYTES,
            linear_memory_bytes: super::MAX_LINEAR_MEMORY_BYTES,
            fuel: DEFAULT_INVOCATION_FUEL,
            concurrent_turns_per_worker: super::DEFAULT_CONCURRENT_TURNS,
        }
    }
}
