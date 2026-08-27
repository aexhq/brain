use std::{path::PathBuf, sync::Arc};

use crate::{LoopExecutor, ModelExecutor, ToolExecutor};

pub struct KernelConfig {
    pub data_dir: PathBuf,
    pub max_decisions_per_turn: usize,
    pub loop_executor: Arc<dyn LoopExecutor>,
    pub model_executor: Arc<dyn ModelExecutor>,
    pub tool_executor: Arc<dyn ToolExecutor>,
}
