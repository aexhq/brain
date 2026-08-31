use std::{path::PathBuf, sync::Arc};

use crate::{LoopExecutor, ModelExecutor, ToolExecutor};

pub struct KernelConfig {
    pub data_dir: PathBuf,
    pub max_decisions_per_turn: usize,
    /// Deadline handed to every Tool invocation. The kernel enforces it by killing the
    /// call and recording a `timeout` outcome — the remote cannot be trusted to.
    pub tool_deadline_ms: u64,
    pub loop_executor: Arc<dyn LoopExecutor>,
    pub model_executor: Arc<dyn ModelExecutor>,
    pub tool_executor: Arc<dyn ToolExecutor>,
}

/// Long enough for real tool work, short enough that a hung environment cannot pin a
/// turn forever.
pub const DEFAULT_TOOL_DEADLINE_MS: u64 = 120_000;
