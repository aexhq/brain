use async_trait::async_trait;
use brain_protocol::{ActivationInput, ActivationOutput, AgentloopDigest};

use crate::KernelError;

#[async_trait]
pub trait LoopExecutor: Send + Sync + 'static {
    async fn activate(
        &self,
        agentloop: &AgentloopDigest,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError>;
}
