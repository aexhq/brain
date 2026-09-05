use std::sync::Arc;

use async_trait::async_trait;
use brain_protocol::{AgentloopIdentity, SessionId, TurnInput, TurnOutput};

use crate::{Error, TurnServices};

#[async_trait]
pub trait LoopExecutor: Send + Sync + 'static {
    /// Runs one whole turn. The session names which conversation this is so an executor
    /// may keep a warm instance per session; it must never reach the guest. Everything
    /// the loop asks Brain to do during the turn goes through `services`.
    async fn turn(
        &self,
        session: &SessionId,
        agentloop: &AgentloopIdentity,
        environment: serde_json::Value,
        input: TurnInput,
        services: Arc<dyn TurnServices>,
    ) -> Result<TurnOutput, Error>;
}
