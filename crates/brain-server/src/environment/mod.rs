//! Concrete Environment transports assembled by the server.
mod host;
mod http;
pub use brain::environment::{EnvironmentAdapter, Services};
pub use brain_env::{BrainEnvironment, NativePolicy};
pub use brain_sessions::EnvironmentRegistry;
pub use host::HostEnvironment;
pub use http::{HttpEnvironmentAdapter, validate_url};

pub struct EnvironmentRouter {
    pub brain: std::sync::Arc<BrainEnvironment>,
    pub hosts: HostEnvironment,
    pub http: std::sync::Arc<HttpEnvironmentAdapter>,
}

#[async_trait::async_trait]
impl EnvironmentAdapter for EnvironmentRouter {
    async fn execute(
        &self,
        environment: &brain_protocol::Environment,
        operation: &brain_protocol::EnvironmentOperation,
        services: Services,
    ) -> Result<brain_protocol::EnvironmentReceipt, brain::Error> {
        use brain_protocol::Driver;
        let adapter: &dyn EnvironmentAdapter = match environment.driver {
            Driver::Brain {} => &*self.brain,
            Driver::Host { .. } => &self.hosts,
            Driver::Http { .. } => &*self.http,
        };
        adapter.execute(environment, operation, services).await
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
