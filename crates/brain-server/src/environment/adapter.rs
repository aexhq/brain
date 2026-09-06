use std::sync::Arc;

use async_trait::async_trait;
use brain::{ToolServices, TurnServices};
use brain_protocol::{Environment, EnvironmentOperation, EnvironmentReceipt};

/// What an operation may reach back into while it runs. A Tool call emits Events and
/// telemetry; a turn also calls the model and dispatches Tools; setup, call, cancel,
/// detach, and teardown reach back into nothing.
pub enum Services<'a> {
    None,
    Tool(&'a dyn ToolServices),
    Turn(&'a Arc<dyn TurnServices>),
}

/// One Environment as Brain reaches it. Three implementations, one per driver, in the
/// same shape: `brain.rs`, `host.rs`, `http.rs`.
#[async_trait]
pub trait EnvironmentAdapter: Send + Sync + 'static {
    /// Performs one operation and answers with its terminal receipt. An `Err` is a
    /// failure on this side of the Environment; `Ambiguous` when the operation may have
    /// happened anyway. An operation the Environment cannot carry is answered with an
    /// `unsupported` failure receipt rather than an error.
    async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services<'_>,
    ) -> Result<EnvironmentReceipt, brain::Error>;
}

pub(crate) fn unsupported(what: &str) -> EnvironmentReceipt {
    EnvironmentReceipt::Failure {
        code: "unsupported".into(),
        message: format!("this Environment does not {what}"),
        retryable: false,
    }
}
