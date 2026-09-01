use std::sync::Arc;

use async_trait::async_trait;
use brain::{KernelError, ToolExecutor};
use brain_protocol::{
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, Outcome, ToolCancellation,
    ToolDispatch,
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
    async fn execute(&self, dispatch: ToolDispatch) -> Result<Outcome, KernelError> {
        // Client-hosted calls are parked by the kernel and answered over the API;
        // reaching this executor with one means the hosting split failed upstream.
        let (Some(environment), Some(attachment_id)) = (
            dispatch.binding.environment.clone(),
            dispatch.binding.attachment_id.clone(),
        ) else {
            return Err(KernelError::InvalidState(
                "a client-hosted Tool call cannot be dispatched to an Environment".into(),
            ));
        };
        let operation = EnvironmentOperation {
            operation_id: dispatch.operation_id,
            request_identity: dispatch.request_identity,
            environment_id: environment.environment_id.clone(),
            session_id: dispatch.session_id,
            attachment_id: Some(attachment_id),
            request: EnvironmentRequest::Invoke {
                call_id: dispatch.invocation.call_id,
                tool: dispatch.binding.name,
                input: dispatch.invocation.input,
                deadline_ms: dispatch.deadline_ms,
            },
        };
        match self.environments.execute(&environment, &operation).await? {
            EnvironmentReceipt::Outcome { outcome } => Ok(outcome),
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
        let (Some(environment), Some(attachment_id)) = (
            cancellation.binding.environment.clone(),
            cancellation.binding.attachment_id.clone(),
        ) else {
            return Err(KernelError::InvalidState(
                "a client-hosted Tool call cannot be cancelled through an Environment".into(),
            ));
        };
        let operation = EnvironmentOperation {
            operation_id: cancellation.operation_id,
            request_identity: cancellation.request_identity,
            environment_id: environment.environment_id.clone(),
            session_id: cancellation.session_id,
            attachment_id: Some(attachment_id),
            request: EnvironmentRequest::Cancel {
                target_operation_id: cancellation.target_operation_id,
            },
        };
        match self.environments.execute(&environment, &operation).await? {
            EnvironmentReceipt::Accepted { .. } | EnvironmentReceipt::Result { .. } => Ok(()),
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
