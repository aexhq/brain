use std::sync::Arc;

use async_trait::async_trait;
use brain_protocol::{SessionConfig, SessionId, TurnInput, TurnOutput};

use crate::{Error, TurnServices};

#[async_trait]
pub trait AgentHarness: Send + Sync + 'static {
    fn ready(&self) -> bool {
        true
    }
    /// Runs an activation using the implementation supplied at startup. Model and
    /// tool effects go through the activation-scoped services, which commit intent
    /// and terminal records before returning. Native session state belongs here.
    async fn turn(
        &self,
        session: &SessionId,
        sequence: u64,
        config: &SessionConfig,
        input: TurnInput,
        services: Arc<dyn TurnServices>,
    ) -> Result<TurnOutput, Error>;

    /// Release a native session while keeping its persistent state for resume.
    async fn close(&self, _session: &SessionId) -> Result<(), Error> {
        Ok(())
    }

    /// Remove native state after the session has ended.
    async fn delete(&self, _session: &SessionId) -> Result<(), Error> {
        Ok(())
    }

    async fn shutdown(&self) {}
}
