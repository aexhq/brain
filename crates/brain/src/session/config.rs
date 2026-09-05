use std::sync::Arc;

use brain_telemetry::TelemetryPublisher;

use crate::{LoopExecutor, ModelExecutor, ToolExecutor};

/// Everything a session needs that is not its own: the executors that perform its
/// effects, the limits it runs under, and where its live output goes. Built once by the
/// host and shared by every session it runs.
pub struct SessionRuntime {
    /// Model calls one turn may make before Brain refuses the next one.
    pub max_model_calls_per_turn: usize,
    /// How long one turn may run before Brain cancels it. Zero means no bound.
    pub max_turn_ms: u64,
    /// Deadline handed to every Tool invocation. The session enforces it by killing the
    /// call and recording a `timeout` outcome — the remote cannot be trusted to.
    pub tool_deadline_ms: u64,
    pub loop_executor: Arc<dyn LoopExecutor>,
    pub model_executor: Arc<dyn ModelExecutor>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    /// Live observations use session-scoped backlogs and never retain an actor.
    pub live: Arc<crate::Feed>,
    /// Where the loop's telemetry goes.
    pub telemetry: TelemetryPublisher,
}

/// Long enough for real tool work, short enough that a hung environment cannot pin a
/// turn forever.
pub const DEFAULT_TOOL_DEADLINE_MS: u64 = 120_000;

/// A turn that has not finished in this long is cancelled: a turn is minutes of tool
/// work at most, not hours.
pub const DEFAULT_MAX_TURN_MS: u64 = 30 * 60 * 1_000;

pub const DEFAULT_MAX_MODEL_CALLS_PER_TURN: usize = 128;
