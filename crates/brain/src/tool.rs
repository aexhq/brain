use async_trait::async_trait;
use brain_protocol::{Outcome, ToolCancellation, ToolDispatch};

use crate::Error;

#[async_trait]
pub trait ToolServices: Send + Sync {
    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error>;
    fn telemetry(&self, record: serde_json::Value);
}

#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    /// Sends one `invoke` and returns its [`Outcome`]. The session maps the outcome onto
    /// the loop's tool result and owns the deadline; an `Err` is a transport-level
    /// failure, not a tool that ran and failed.
    async fn execute(
        &self,
        dispatch: ToolDispatch,
        services: &dyn ToolServices,
    ) -> Result<Outcome, Error>;
    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), Error>;
}
