use std::sync::Arc;

use async_trait::async_trait;

use brain_protocol::{Environment, EnvironmentOperation, EnvironmentReceipt};

/// Invocation-scoped services granted by the caller.
#[async_trait]
pub trait ExecutionServices: Send + Sync {
    fn methods(&self) -> &'static [&'static str];
    async fn call(
        &self,
        method: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, crate::Error>;
    fn cancelled(&self) -> bool;
}

pub type Services = Option<Arc<dyn ExecutionServices>>;

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
        services: Services,
    ) -> Result<EnvironmentReceipt, crate::Error>;
}

pub fn unsupported(what: &str) -> EnvironmentReceipt {
    EnvironmentReceipt::Failure {
        code: "unsupported".into(),
        message: format!("this Environment does not {what}"),
        retryable: false,
    }
}
