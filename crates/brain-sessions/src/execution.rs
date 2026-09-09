use crate::EnvironmentRegistry;
use async_trait::async_trait;
use brain::{
    LoopExecutor, ToolExecutor, ToolServices, TurnServices, environment::ExecutionServices,
};
use brain_protocol::{
    AgentloopRef, Environment, EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest,
    Outcome, SessionId, ToolCancellation, ToolDispatch, codes,
};
use serde_json::{Value, json};

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
use std::sync::Arc;

/// Runs each turn in the Environment the Agentloop names, through the same interface
/// every other operation on that Environment uses. An Environment outside this process
/// is handed the turn's routes and the token that opens them.
pub struct EnvironmentLoopExecutor {
    pub environments: Arc<EnvironmentRegistry>,
    pub deadline_ms: u64,
}

#[async_trait]
impl LoopExecutor for EnvironmentLoopExecutor {
    async fn turn(
        &self,
        session: &SessionId,
        sequence: u64,
        agentloop: &AgentloopRef,
        environment: &Environment,
        input: brain_protocol::TurnInput,
        services: Arc<dyn brain::TurnServices>,
    ) -> Result<brain_protocol::TurnOutput, brain::Error> {
        let operation = EnvironmentOperation {
            sequence,
            environment: environment.name.clone(),
            session_id: session.clone(),
            request: EnvironmentRequest::Execute {
                implementation: agentloop.implementation.clone(),
                input: serde_json::to_value(input).map_err(json_error)?,
                deadline_ms: self.deadline_ms,
                callback: None,
            },
        };
        match self
            .environments
            .execute(
                environment,
                &operation,
                Some(Arc::new(SessionServices::Loop(services))),
            )
            .await?
        {
            EnvironmentReceipt::Result { output } => {
                serde_json::from_value(output).map_err(json_error)
            }
            EnvironmentReceipt::Failure {
                code,
                message,
                retryable,
                ..
            } => {
                if code == codes::failure::CANCELLED {
                    return Err(brain::Error::Cancelled(message));
                }
                Err(brain::Error::Loop(brain_protocol::TurnError {
                    code,
                    message,
                    retryable,
                }))
            }
            EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Ambiguous(
                "Environment returned a nonterminal turn receipt".into(),
            )),
        }
    }
}

/// Every Tool call goes to the Environment the Tool names; the registry knows how.
pub struct SessionToolExecutor {
    environments: Arc<EnvironmentRegistry>,
}

impl SessionToolExecutor {
    pub fn new(environments: Arc<EnvironmentRegistry>) -> Self {
        Self { environments }
    }
}

#[async_trait]
impl ToolExecutor for SessionToolExecutor {
    async fn execute(
        &self,
        dispatch: ToolDispatch,
        services: std::sync::Arc<dyn ToolServices>,
    ) -> Result<Outcome, brain::Error> {
        let operation = EnvironmentOperation {
            sequence: dispatch.sequence,
            environment: dispatch.environment.name.clone(),
            session_id: dispatch.session_id,
            request: EnvironmentRequest::Execute {
                implementation: dispatch.placement.implementation,
                callback: None,
                input: dispatch.invocation.input,
                deadline_ms: dispatch.deadline_ms,
            },
        };
        match self
            .environments
            .execute(
                &dispatch.environment,
                &operation,
                Some(Arc::new(SessionServices::Tool(services))),
            )
            .await?
        {
            EnvironmentReceipt::Result { output } => Ok(Outcome::Ok { value: output }),
            EnvironmentReceipt::Failure {
                code,
                message,
                retryable,
                details,
            } => Ok(Outcome::Error {
                error: brain_protocol::OutcomeError {
                    code,
                    message,
                    retryable,
                    details,
                },
            }),
            EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Ambiguous(
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
            .execute(&cancellation.environment, &operation, None)
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

pub enum SessionServices {
    Loop(Arc<dyn TurnServices>),
    Tool(Arc<dyn ToolServices>),
}

#[async_trait]
impl ExecutionServices for SessionServices {
    fn methods(&self) -> &'static [&'static str] {
        match self {
            Self::Loop(_) => &[
                "events",
                "model",
                "dispatch",
                "emit",
                "telemetry",
                "set_transcript",
                "kv_put",
                "kv_read",
                "kv_delete",
            ],
            Self::Tool(_) => &["emit", "telemetry"],
        }
    }
    async fn call(&self, method: &str, input: Value) -> Result<Value, brain::Error> {
        if self.cancelled() {
            return Err(brain::Error::Cancelled("execution cancelled".into()));
        }
        match (self, method) {
            (Self::Loop(services), "set_transcript") => Ok(json!(
                services
                    .set_transcript(serde_json::from_value(input).map_err(json_error)?)
                    .await?
            )),
            (Self::Loop(services), "kv_put") => Ok(json!(
                services
                    .kv_put(serde_json::from_value(input).map_err(json_error)?)
                    .await?
            )),
            (Self::Loop(services), "kv_read") => {
                let key = serde_json::from_value(input).map_err(json_error)?;
                Ok(match services.kv_read(key).await? {
                    Some(value) => json!({"value": value}),
                    None => json!({}),
                })
            }
            (Self::Loop(services), "kv_delete") => Ok(json!(
                services
                    .kv_delete(serde_json::from_value(input).map_err(json_error)?)
                    .await?
            )),
            (Self::Loop(services), "events") => serde_json::to_value(
                services
                    .events(serde_json::from_value(input).map_err(json_error)?)
                    .await?,
            )
            .map_err(json_error),
            (Self::Loop(services), "model") => serde_json::to_value(
                services
                    .model(serde_json::from_value(input).map_err(json_error)?)
                    .await?,
            )
            .map_err(json_error),
            (Self::Loop(services), "dispatch") => serde_json::to_value(
                services
                    .dispatch(serde_json::from_value(input).map_err(json_error)?)
                    .await?,
            )
            .map_err(json_error),
            (_, "emit") => {
                let event: brain_protocol::TurnEmitRequest =
                    serde_json::from_value(input).map_err(json_error)?;
                let sequence = match self {
                    Self::Loop(s) => s.emit(event.event_type, event.data).await?,
                    Self::Tool(s) => s.emit(event.event_type, event.data).await?,
                };
                Ok(json!(sequence))
            }
            (_, "telemetry") => {
                match self {
                    Self::Loop(s) => s.telemetry(input),
                    Self::Tool(s) => s.telemetry(input),
                };
                Ok(Value::Null)
            }
            _ => Err(brain::Error::InvalidState(
                "execution service is not granted".into(),
            )),
        }
    }
    fn cancelled(&self) -> bool {
        match self {
            Self::Loop(s) => s.cancelled(),
            Self::Tool(s) => s.cancelled(),
        }
    }
}

fn json_error(error: serde_json::Error) -> brain::Error {
    brain::Error::InvalidState(error.to_string())
}
