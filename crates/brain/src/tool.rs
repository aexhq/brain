use async_trait::async_trait;
use brain_protocol::{Outcome, ToolCancellation, ToolDispatch};

use crate::Error;

mod execution;
pub(crate) use execution::EmissionBudget;
pub use execution::{ToolExecutions, ToolGroup, ToolWakeup};

#[async_trait]
pub trait ToolServices: Send + Sync {
    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error>;
    async fn result(&self, outcome: Outcome) -> Result<u64, Error>;
    async fn returned(&self, outcome: Option<Outcome>) -> Result<u64, Error>;
    async fn finish(&self, outcome: Option<Outcome>) -> Result<u64, Error>;
    async fn closed(&self);
    fn telemetry(&self, record: serde_json::Value);
    fn cancelled(&self) -> bool {
        false
    }
}

#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    /// Runs the Environment entrypoint. Its return may supply data but does not finish
    /// the Tool execution. The Tool explicitly finishes through its services.
    async fn execute(
        &self,
        dispatch: ToolDispatch,
        services: std::sync::Arc<dyn ToolServices>,
    ) -> Result<Option<Outcome>, Error>;
    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), Error>;
}
