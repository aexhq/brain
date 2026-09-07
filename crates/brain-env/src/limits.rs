use std::{path::PathBuf, time::Duration};

/// What one Environment worker may hold and spend. Brain ships a default for each and never
/// treats it as a fact about the machine: the host that starts Brain sets them. Zero
/// on any ceiling means no bound.
///
/// The supervisor renders these as the worker's command line and the worker parses
/// them with this same definition, so both sides hold one set of values.
#[derive(Clone, Debug, PartialEq, Eq, clap::Args)]
pub struct EnvLimits {
    /// Bytes an Agentloop or Tool Component package may hold.
    #[arg(long, env = "BRAIN_MAX_PACKAGE_BYTES", default_value_t = EnvLimits::default().max_package_bytes)]
    pub max_package_bytes: usize,
    /// Bytes one execution input may hold, including an Agentloop's transcript and events.
    #[arg(long, env = "BRAIN_MAX_EXECUTION_INPUT_BYTES", default_value_t = EnvLimits::default().max_execution_input_bytes)]
    pub max_execution_input_bytes: usize,
    /// Bytes one execution output may hold.
    #[arg(long, env = "BRAIN_MAX_EXECUTION_OUTPUT_BYTES", default_value_t = EnvLimits::default().max_execution_output_bytes)]
    pub max_execution_output_bytes: usize,
    /// Bytes of linear memory one Wasm instance may grow to.
    #[arg(long, env = "BRAIN_MAX_LINEAR_MEMORY_BYTES", default_value_t = EnvLimits::default().max_linear_memory_bytes)]
    pub max_linear_memory_bytes: usize,
    /// Wasmtime work units one invocation may consume; host and WASI waits consume none.
    /// A stable ceiling within one Wasmtime version, not a duration.
    #[arg(long, env = "BRAIN_MAX_FUEL", default_value_t = EnvLimits::default().max_fuel)]
    pub max_fuel: u64,
    /// Executions per worker in each of two classes: those granted dispatch, and leaf calls. Each
    /// holds a fresh Store while it runs, so guest memory is bounded by twice this count
    /// times the linear memory ceiling.
    #[arg(long, env = "BRAIN_MAX_CONCURRENT_EXECUTIONS", default_value_t = EnvLimits::default().max_concurrent_executions)]
    pub max_concurrent_executions: usize,
    /// Core instances, memories, and tables one guest may hold: its own modules plus the
    /// shims Wasmtime builds for its imports.
    #[arg(long, env = "BRAIN_MAX_CORE_INSTANCES", default_value_t = EnvLimits::default().max_core_instances)]
    pub max_core_instances: usize,
    /// Seconds one outbound HTTP request from a native Component may take.
    #[arg(long, env = "BRAIN_MAX_NATIVE_HTTP_SECS", default_value_t = EnvLimits::default().max_native_http_secs)]
    pub max_native_http_secs: u64,
}

impl Default for EnvLimits {
    fn default() -> Self {
        Self {
            max_package_bytes: 32 * 1024 * 1024,
            max_execution_input_bytes: 32 * 1024 * 1024,
            max_execution_output_bytes: 32 * 1024 * 1024,
            max_linear_memory_bytes: 128 * 1024 * 1024,
            max_fuel: 10_000_000_000,
            // Eight instances of the 128 MiB ceiling bound guest memory to 1 GiB per
            // invocation class.
            max_concurrent_executions: 8,
            max_core_instances: 8,
            max_native_http_secs: 120,
        }
    }
}

impl EnvLimits {
    /// The limits as the worker's command line, parsed back by [`WorkerArgs`].
    pub fn args(&self) -> Vec<String> {
        vec![
            format!("--max-package-bytes={}", self.max_package_bytes),
            format!(
                "--max-execution-input-bytes={}",
                self.max_execution_input_bytes
            ),
            format!(
                "--max-execution-output-bytes={}",
                self.max_execution_output_bytes
            ),
            format!("--max-linear-memory-bytes={}", self.max_linear_memory_bytes),
            format!("--max-fuel={}", self.max_fuel),
            format!(
                "--max-concurrent-executions={}",
                self.max_concurrent_executions
            ),
            format!("--max-core-instances={}", self.max_core_instances),
            format!("--max-native-http-secs={}", self.max_native_http_secs),
        ]
    }

    /// How long a native Component's outbound HTTP request may take, or `None` for no
    /// bound.
    pub fn max_native_http(&self) -> Option<Duration> {
        (self.max_native_http_secs != 0).then(|| Duration::from_secs(self.max_native_http_secs))
    }

    /// How long the worker may go without a frame before the supervisor gives up on
    /// it: one bounded native HTTP wait plus slack. Runaway guest compute is stopped
    /// independently by fuel.
    pub fn worker_liveness(&self) -> Duration {
        match self.max_native_http() {
            Some(wait) => wait + Duration::from_secs(5),
            None => Duration::MAX,
        }
    }

    /// The largest request frame the worker accepts: a package arrives base64-encoded,
    /// everything else is a turn input plus its envelope.
    pub fn max_request_frame_bytes(&self) -> usize {
        ceiling(self.max_package_bytes)
            .saturating_mul(2)
            .max(self.max_turn_frame_bytes())
    }

    /// A turn or Tool request frame: the input plus its envelope.
    pub fn max_turn_frame_bytes(&self) -> usize {
        ceiling(self.max_execution_input_bytes).saturating_add(1_024)
    }

    /// A response frame from the worker: the output plus its envelope.
    pub fn max_response_frame_bytes(&self) -> usize {
        ceiling(self.max_execution_output_bytes).saturating_add(1_024)
    }
}

/// The worker's command line: the socket to serve and the limits to serve it under.
#[derive(Debug, clap::Parser)]
#[command(name = "brain-env-worker")]
pub struct WorkerArgs {
    pub socket: PathBuf,
    #[command(flatten)]
    pub limits: EnvLimits,
}

/// A byte or count ceiling as the code compares against it: zero means no bound.
pub fn ceiling(value: usize) -> usize {
    if value == 0 { usize::MAX } else { value }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[test]
    fn the_rendered_command_line_parses_back_to_the_same_limits() {
        let limits = EnvLimits {
            max_package_bytes: 1,
            max_execution_input_bytes: 2,
            max_execution_output_bytes: 3,
            max_linear_memory_bytes: 4,
            max_fuel: 5,
            max_concurrent_executions: 6,
            max_core_instances: 7,
            max_native_http_secs: 8,
        };
        let mut argv = vec!["brain-env-worker".to_owned(), "/run/worker.sock".to_owned()];
        argv.extend(limits.args());
        let parsed = WorkerArgs::parse_from(argv);
        assert_eq!(parsed.socket, PathBuf::from("/run/worker.sock"));
        assert_eq!(parsed.limits, limits);
    }

    #[test]
    fn the_worker_defaults_match_the_supervisor_defaults() {
        let parsed = WorkerArgs::parse_from(["brain-env-worker", "sock"]);
        assert_eq!(parsed.limits, EnvLimits::default());
    }
}
