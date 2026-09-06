use std::sync::Arc;

use async_trait::async_trait;
use brain_protocol::{AgentloopRef, Environment, SessionId, TurnInput, TurnOutput};

use crate::{Error, TurnServices};

#[async_trait]
pub trait LoopExecutor: Send + Sync + 'static {
    /// Runs one whole turn in the Environment the Agentloop names. `sequence` is the
    /// `activation_started` record's, which with the session id names the turn on the
    /// Environment's wire. Everything the loop asks Brain to do during the turn goes
    /// through `services`.
    async fn turn(
        &self,
        session: &SessionId,
        sequence: u64,
        agentloop: &AgentloopRef,
        environment: &Environment,
        input: TurnInput,
        services: Arc<dyn TurnServices>,
    ) -> Result<TurnOutput, Error>;
}
