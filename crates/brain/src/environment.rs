use std::sync::Arc;

use async_trait::async_trait;

use brain_protocol::{Environment, EnvironmentOperation, EnvironmentReceipt};

mod state;
pub use state::{descriptor, environments};

/// Invocation-scoped services granted by the caller.
#[async_trait]
pub trait ExecutionServices: Send + Sync {
    fn methods(&self) -> Vec<&'static str>;
    fn controller(&self) -> Services {
        None
    }
    async fn call(
        &self,
        method: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, crate::Error>;
    fn cancelled(&self) -> bool;
    /// Explicit completion for executions that grant a finish service.
    async fn closed(&self) {
        std::future::pending::<()>().await;
    }
}

pub type Services = Option<Arc<dyn ExecutionServices>>;

#[async_trait]
pub trait EnvironmentControl: Send + Sync {
    async fn control(
        &self,
        store: Arc<dyn crate::SessionStore>,
        caller: Option<&brain_protocol::EnvironmentName>,
        grants: &[brain_protocol::EnvironmentGrant],
        request: brain_protocol::EnvironmentControlRequest,
    ) -> Result<serde_json::Value, crate::Error>;
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
        services: Services,
    ) -> Result<EnvironmentReceipt, crate::Error>;
}

pub fn unsupported(what: &str) -> EnvironmentReceipt {
    EnvironmentReceipt::Failure {
        code: "unsupported".into(),
        message: format!("this Environment does not {what}"),
        retryable: false,
        details: None,
    }
}
