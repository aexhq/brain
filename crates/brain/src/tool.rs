use async_trait::async_trait;
use brain_protocol::{ToolDispatch, ToolResult};

use crate::KernelError;

#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    async fn execute(&self, dispatch: ToolDispatch) -> Result<ToolResult, KernelError>;
}
