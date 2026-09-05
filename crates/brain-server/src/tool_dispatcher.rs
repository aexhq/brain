use std::sync::Arc;

use async_trait::async_trait;
use brain::{ToolExecutor, ToolServices};
use brain_loophost::{HostCall, LoopError, NativeToolInput, TurnBridge, WorkerPool};
use brain_protocol::{
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, Outcome, ToolCancellation,
    ToolDispatch, ToolIdentity, TurnError,
};

use crate::{EnvironmentRegistry, ResidentHosts};

pub struct ServerToolExecutor {
    environments: Arc<EnvironmentRegistry>,
    resident_hosts: ResidentHosts,
    components: Arc<WorkerPool>,
}

impl ServerToolExecutor {
    pub fn new(
        environments: Arc<EnvironmentRegistry>,
        resident_hosts: ResidentHosts,
        components: Arc<WorkerPool>,
    ) -> Self {
        Self {
            environments,
            resident_hosts,
            components,
        }
    }
}

#[async_trait]
impl ToolExecutor for ServerToolExecutor {
    async fn execute(
        &self,
        dispatch: ToolDispatch,
        services: &dyn ToolServices,
    ) -> Result<Outcome, brain::Error> {
        if matches!(
            dispatch.binding.hosting,
            brain_protocol::ToolHosting::Resident
        ) {
            return self.resident_hosts.execute(dispatch, services).await;
        }
        if let Some(environment_id) = &dispatch.binding.environment_id
            && let Some(environment) = self.environments.brain_wasm_configuration(environment_id)?
        {
            let implementation = dispatch.binding.implementation.as_ref().ok_or_else(|| {
                brain::Error::InvalidState("native Tool has no implementation".into())
            })?;
            let identity = implementation
                .get("identity")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    brain::Error::InvalidState(
                        "native Tool implementation has no Component identity".into(),
                    )
                })?;
            if implementation
                .get("type")
                .and_then(serde_json::Value::as_str)
                != Some("brain_component")
            {
                return Err(brain::Error::InvalidState(
                    "the Brain Wasm Environment requires a Component implementation".into(),
                ));
            }
            let configuration = implementation
                .get("configuration")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let bridge = NativeToolBridge { services };
            return match self
                .components
                .tool(
                    dispatch.session_id.to_string(),
                    ToolIdentity::new(identity),
                    environment,
                    NativeToolInput {
                        call_id: dispatch.invocation.call_id,
                        input: dispatch.invocation.input,
                        configuration,
                        deadline_at_ms: wall_clock_ms().saturating_add(dispatch.deadline_ms),
                    },
                    &bridge,
                )
                .await
            {
                Ok(value) => Ok(Outcome::Ok { value }),
                Err(LoopError::Turn(error)) => Ok(Outcome::Error {
                    error: brain_protocol::OutcomeError {
                        code: error.code,
                        message: error.message,
                        details: None,
                    },
                }),
                Err(LoopError::Overloaded) => Err(brain::Error::Overloaded(
                    "native worker is at capacity".into(),
                )),
                Err(LoopError::Failed(message)) => Err(brain::Error::Ambiguous(message)),
            };
        }
        let (Some(environment), Some(attachment_id)) = (
            dispatch.binding.environment.clone(),
            dispatch.binding.attachment_id.clone(),
        ) else {
            return Err(brain::Error::InvalidState(
                "a provisioned Tool call has no attached Environment".into(),
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
            EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Executor(
                "Environment returned a nonterminal Tool receipt".into(),
            )),
        }
    }

    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), brain::Error> {
        if matches!(
            cancellation.binding.hosting,
            brain_protocol::ToolHosting::Resident
        ) {
            return self.resident_hosts.cancel(cancellation).await;
        }
        let (Some(environment), Some(attachment_id)) = (
            cancellation.binding.environment.clone(),
            cancellation.binding.attachment_id.clone(),
        ) else {
            return Err(brain::Error::InvalidState(
                "a provisioned Tool call has no attached Environment".into(),
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
            EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
            _ => Err(brain::Error::Executor(
                "Environment returned a nonterminal cancellation receipt".into(),
            )),
        }
    }
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

struct NativeToolBridge<'a> {
    services: &'a dyn ToolServices,
}

#[async_trait]
impl TurnBridge for NativeToolBridge<'_> {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::Emit { kind, payload_json } => {
                let payload = serde_json::from_str(&payload_json)
                    .map_err(|error| TurnError::new("invalid_event", error.to_string()))?;
                self.services
                    .emit(kind, payload)
                    .await
                    .map(|sequence| sequence.to_string())
                    .map_err(|error| TurnError::new(error.code(), error.to_string()))
            }
            HostCall::Telemetry { record_json } => {
                if let Ok(record) = serde_json::from_str(&record_json) {
                    self.services.telemetry(record);
                }
                Ok(String::new())
            }
            _ => Err(TurnError::new(
                "unsupported_host_call",
                "a Tool may only emit Events or telemetry",
            )),
        }
    }

    fn cancelled(&self) -> bool {
        false
    }
}
