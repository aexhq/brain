use async_trait::async_trait;
use brain_protocol::{Outcome, ToolCancellation, ToolDispatch};

use crate::KernelError;

#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    /// Sends one `invoke` and returns its [`Outcome`]. The kernel maps the outcome onto
    /// the loop's tool result and owns the deadline; an `Err` is a transport-level
    /// failure, not a tool that ran and failed.
    async fn execute(&self, dispatch: ToolDispatch) -> Result<Outcome, KernelError>;
    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), KernelError>;
}
