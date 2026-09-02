use std::sync::Arc;

use brain_protocol::{LiveEvent, SessionId};
use tokio::sync::broadcast;

use crate::{LoopExecutor, ModelExecutor, ToolExecutor};

/// Everything a session needs that is not its own: the executors that perform its
/// effects, the limits it runs under, and where its live output goes. Built once by the
/// host and shared by every session it runs.
pub struct SessionConfig {
    pub max_decisions_per_turn: usize,
    /// Deadline handed to every Tool invocation. The session enforces it by killing the
    /// call and recording a `timeout` outcome — the remote cannot be trusted to.
    pub tool_deadline_ms: u64,
    pub loop_executor: Arc<dyn LoopExecutor>,
    pub model_executor: Arc<dyn ModelExecutor>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    /// Where model output goes while a turn is still running. Not the journal: a
    /// token is not yet something the turn produced. `ObservedJournal::live_sender`
    /// hands out the sender that puts these beside the records on one feed.
    pub live: broadcast::Sender<(SessionId, LiveEvent)>,
}

/// Long enough for real tool work, short enough that a hung environment cannot pin a
/// turn forever.
pub const DEFAULT_TOOL_DEADLINE_MS: u64 = 120_000;
