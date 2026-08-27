use std::sync::Arc;

use async_trait::async_trait;
use brain::{KernelError, ToolExecutor};
use brain_protocol::{
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, ToolCancellation, ToolDispatch,
    ToolResult,
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
        let environment = dispatch.binding.environment.clone();
        let operation = EnvironmentOperation {
            operation_id: dispatch.operation_id,
            request_digest: dispatch.request_digest,
            environment_id: environment.environment_id.clone(),
            session_id: dispatch.session_id,
            attachment_id: Some(dispatch.binding.attachment_id),
            request: EnvironmentRequest::Execute {
                tool: dispatch.invocation,
                remote_tool_id: dispatch.binding.remote_tool_id,
                grant: dispatch.binding.grant,
            },
        };
        match self.environments.execute(&environment, &operation).await? {
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

    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), KernelError> {
        let environment = cancellation.binding.environment.clone();
        let operation = EnvironmentOperation {
            operation_id: cancellation.operation_id,
            request_digest: cancellation.request_digest,
            environment_id: environment.environment_id.clone(),
            session_id: cancellation.session_id,
            attachment_id: Some(cancellation.binding.attachment_id),
            request: EnvironmentRequest::Cancel {
                target_operation_id: cancellation.target_operation_id,
            },
        };
        match self.environments.execute(&environment, &operation).await? {
            EnvironmentReceipt::Accepted | EnvironmentReceipt::Result { .. } => Ok(()),
            EnvironmentReceipt::Failure { message, .. } => Err(KernelError::Executor(message)),
            EnvironmentReceipt::Ambiguous { message } => Err(KernelError::Ambiguous(message)),
            EnvironmentReceipt::Conflict { .. } => Err(KernelError::InvalidState(
                "Environment reported a cancellation digest conflict".into(),
            )),
            _ => Err(KernelError::Executor(
                "Environment returned a nonterminal cancellation receipt".into(),
            )),
        }
    }
}
