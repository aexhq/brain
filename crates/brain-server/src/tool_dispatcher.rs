use std::sync::Arc;

use async_trait::async_trait;
use brain::{ToolExecutor, ToolServices};
use brain_protocol::{
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, Outcome, ToolCancellation,
    ToolDispatch,
};

use crate::{EnvironmentRegistry, Services};

/// Every Tool call goes to the Environment the Tool names; the registry knows how.
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
    async fn execute(
        &self,
        dispatch: ToolDispatch,
        services: &dyn ToolServices,
    ) -> Result<Outcome, brain::Error> {
        let operation = EnvironmentOperation {
            sequence: dispatch.sequence,
            environment: dispatch.tool.environment.clone(),
            session_id: dispatch.session_id,
            request: EnvironmentRequest::Invoke {
                tool: dispatch.tool.name,
                implementation: dispatch.tool.implementation,
                needs: dispatch.tool.needs,
                input: dispatch.invocation.input,
                deadline_ms: dispatch.deadline_ms,
            },
        };
        match self
            .environments
            .execute(&dispatch.environment, &operation, Services::Tool(services))
            .await?
        {
            EnvironmentReceipt::Outcome { outcome } => Ok(outcome),
            EnvironmentReceipt::Failure { message, .. } => Err(brain::Error::Executor(message)),
            EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Executor(
                "Environment returned a nonterminal Tool receipt".into(),
            )),
        }
    }

    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), brain::Error> {
        let operation = EnvironmentOperation {
            sequence: cancellation.sequence,
            environment: cancellation.environment.name.clone(),
            session_id: cancellation.session_id,
            request: EnvironmentRequest::Cancel {
                target_sequence: cancellation.target_sequence,
            },
        };
        match self
            .environments
            .execute(&cancellation.environment, &operation, Services::None)
            .await?
        {
            EnvironmentReceipt::Accepted | EnvironmentReceipt::Result { .. } => Ok(()),
            EnvironmentReceipt::Failure { message, .. } => Err(brain::Error::Executor(message)),
            EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Executor(
                "Environment returned a nonterminal cancellation receipt".into(),
            )),
        }
    }
}
