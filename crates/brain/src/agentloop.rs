use async_trait::async_trait;
use brain_protocol::{ActivationInput, ActivationOutput, AgentloopIdentity, SessionId};

use crate::Error;

#[async_trait]
pub trait LoopExecutor: Send + Sync + 'static {
    /// Runs one activation. The session names which conversation this is so an executor
    /// may keep a warm instance per session; it must never reach the guest.
    async fn activate(
        &self,
        session: &SessionId,
        agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, Error>;
}
