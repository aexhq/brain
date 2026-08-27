use std::sync::Arc;

use async_trait::async_trait;
use brain::{KernelError, ToolExecutor};
use brain_protocol::{
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, ToolDispatch, ToolResult,
};

use crate::EnvironmentRegistry;

pub struct ServerToolExecutor {
    environments: Arc<EnvironmentRegistry>,
}

impl ServerToolExecutor {
    pub fn new(environments: Arc<EnvironmentRegistry>) -> Self {
        Self { environments }
    }
}

#[async_trait]
impl ToolExecutor for ServerToolExecutor {
    async fn execute(&self, dispatch: ToolDispatch) -> Result<ToolResult, KernelError> {
        let operation = EnvironmentOperation {
            operation_id: dispatch.operation_id,
            request_digest: dispatch.request_digest,
            environment_id: dispatch.binding.environment_id,
            session_id: dispatch.session_id,
            attachment_id: Some(dispatch.binding.attachment_id),
            request: EnvironmentRequest::Execute {
                tool: dispatch.invocation,
                remote_tool_id: dispatch.binding.remote_tool_id,
                grant: dispatch.binding.grant,
            },
        };
        match self.environments.execute(&operation).await? {
            EnvironmentReceipt::ToolResult { result } => Ok(result),
            EnvironmentReceipt::Failure { message, .. } => Err(KernelError::Executor(message)),
            EnvironmentReceipt::Ambiguous { message } => Err(KernelError::Ambiguous(message)),
            EnvironmentReceipt::Conflict { .. } => Err(KernelError::InvalidState(
                "Environment reported a Tool digest conflict".into(),
            )),
            _ => Err(KernelError::Executor(
                "Environment returned a nonterminal Tool receipt".into(),
            )),
        }
    }
}
