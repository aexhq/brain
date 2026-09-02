use std::sync::Arc;

use async_trait::async_trait;
use brain::ToolExecutor;
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
    async fn execute(&self, dispatch: ToolDispatch) -> Result<Outcome, brain::Error> {
        // Client-hosted calls are parked by the session and answered over the API;
        // reaching this executor with one means the hosting split failed upstream.
        let (Some(environment), Some(attachment_id)) = (
            dispatch.binding.environment.clone(),
            dispatch.binding.attachment_id.clone(),
        ) else {
            return Err(brain::Error::InvalidState(
                "a client-hosted Tool call cannot be dispatched to an Environment".into(),
            ));
        };
        let operation = EnvironmentOperation {
            sequence: dispatch.sequence,
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
            EnvironmentReceipt::Failure { message, .. } => Err(brain::Error::Executor(message)),
            EnvironmentReceipt::Ambiguous { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Executor(
                "Environment returned a nonterminal Tool receipt".into(),
            )),
        }
    }

    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), brain::Error> {
        let (Some(environment), Some(attachment_id)) = (
            cancellation.binding.environment.clone(),
            cancellation.binding.attachment_id.clone(),
        ) else {
            return Err(brain::Error::InvalidState(
                "a client-hosted Tool call cannot be cancelled through an Environment".into(),
            ));
        };
        let operation = EnvironmentOperation {
            sequence: cancellation.sequence,
            environment_id: environment.environment_id.clone(),
            session_id: cancellation.session_id,
            attachment_id: Some(attachment_id),
            request: EnvironmentRequest::Cancel {
                target_sequence: cancellation.target_sequence,
            },
        };
        match self.environments.execute(&environment, &operation).await? {
            EnvironmentReceipt::Accepted { .. } | EnvironmentReceipt::Result { .. } => Ok(()),
            EnvironmentReceipt::Failure { message, .. } => Err(brain::Error::Executor(message)),
            EnvironmentReceipt::Ambiguous { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Executor(
                "Environment returned a nonterminal cancellation receipt".into(),
            )),
        }
    }
}
