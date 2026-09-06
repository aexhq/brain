use std::sync::Arc;

use brain_telemetry::TelemetryPublisher;

use crate::{LoopExecutor, ModelExecutor, ToolExecutor};

/// Everything a session needs that is not its own: the executors that perform its
/// effects, the limits it runs under, and where its live output goes. Built once by the
/// host and shared by every session it runs.
pub struct SessionRuntime {
    /// The budgets every turn runs under. The Tool deadline is enforced by the session:
    /// it kills the call and records a `timeout` outcome, since the remote cannot be
    /// trusted to.
    pub limits: crate::Limits,
    pub loop_executor: Arc<dyn LoopExecutor>,
    pub model_executor: Arc<dyn ModelExecutor>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    /// Live observations use session-scoped backlogs and never retain an actor.
    pub live: Arc<crate::Feed>,
    /// Where the loop's telemetry goes.
    pub telemetry: TelemetryPublisher,
}
