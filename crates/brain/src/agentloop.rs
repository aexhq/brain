use std::sync::Arc;

use async_trait::async_trait;
use brain_protocol::{SessionConfig, SessionId, TurnInput, TurnOutput};

use crate::{Error, TurnServices};

#[async_trait]
pub trait AgentHarness: Send + Sync + 'static {
    /// Reject unregistered implementations before admitting a session.
    fn validate(&self, _selection: &brain_protocol::AgentloopSelection) -> Result<(), Error> {
        Ok(())
    }
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
    async fn delete(&self, _session: &SessionId, _config: &SessionConfig) -> Result<(), Error> {
        Ok(())
    }

    async fn shutdown(&self) {}
}

/// Names are also native-state directory components, so paths are not accepted.
pub fn validate_agentloop_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(Error::InvalidState(
            "agentloop name must be 1-64 ASCII letters, digits, underscores or hyphens".into(),
        ));
    }
    Ok(())
}
