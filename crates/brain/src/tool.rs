use async_trait::async_trait;
use brain_protocol::{ToolCancellation, ToolDispatch, ToolResult};

use crate::KernelError;

#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    async fn execute(&self, dispatch: ToolDispatch) -> Result<ToolResult, KernelError>;
    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), KernelError>;
}
