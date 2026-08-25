use std::collections::HashMap;
use std::sync::Arc;

use brain_protocol::environment::TerminalOutcome;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::customer::{CustomerCoordinator, CustomerTerminalReceipt};
use crate::{BrainError, Result};

const MAX_COMPONENT_OPERATIONS: usize = 1_024;

#[derive(Clone)]
pub struct CustomerComponentDriver {
    coordinator: Arc<CustomerCoordinator>,
    operations: Arc<Mutex<HashMap<String, Operation>>>,
}

#[derive(Clone)]
enum Operation {
    Running {
        identity: OperationIdentity,
        cancel: CancellationToken,
    },
    Terminal {
        identity: OperationIdentity,
        state: &'static str,
        value: Value,
        receipt: Option<CustomerTerminalReceipt>,
    },
}

#[derive(Clone, PartialEq)]
struct OperationIdentity {
    binding: Binding,
    registration: String,
    name: String,
    contract_digest: String,
    input: Value,
    deadline_at_ms: u64,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Binding {
    driver: String,
    configuration: Value,
    policy: Value,
    tenant_id: String,
    session_id: String,
    root_id: String,
    parent_id: Option<String>,
    environment_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppConfiguration {
    registration: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitBody {
    binding: Binding,
    operation: ComponentOperation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentOperation {
    operation_id: String,
    kind: String,
    descriptor_json: String,
    bundle_base64: Option<String>,
    input_json: String,
    deadline_at_ms: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolDescriptor {
    registration: String,
    name: String,
    contract_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationBody {
    binding: Binding,
    provider_operation_id: String,
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcknowledgeBody {
    binding: Binding,
    provider_operation_id: String,
    terminal: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseBody {
    binding: Binding,
}

impl CustomerComponentDriver {
    pub fn new(coordinator: Arc<CustomerCoordinator>) -> Arc<Self> {
        Arc::new(Self {
            coordinator,
            operations: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn dispatch(
        self: &Arc<Self>,
        operation_id: &str,
        action: &str,
        request: Value,
        deadline_at_ms: u64,
    ) -> Result<Value> {
        valid_id("operation id", operation_id)?;
        match action {
            "submit" => self.submit(operation_id, request, deadline_at_ms).await,
            "observe" => self.observe(operation_id, request).await,
            "cancel" => self.cancel(operation_id, request).await,
            "acknowledge" => self.acknowledge(operation_id, request).await,
            "release" => self.release(request).await,
            _ => Err(BrainError::Invalid(
                "unknown customer Environment action".into(),
            )),
        }
    }

    async fn submit(
        self: &Arc<Self>,
        operation_id: &str,
        request: Value,
        dispatch_deadline_at_ms: u64,
    ) -> Result<Value> {
        let body: SubmitBody = parse(request)?;
        let client_id = validate_binding(&body.binding)?;
        if body.operation.operation_id != operation_id
            || body.operation.kind != "invoke"
            || body.operation.bundle_base64.is_some()
        {
            return Err(BrainError::Invalid(
                "invalid customer Environment operation".into(),
            ));
        }
        let deadline_at_ms =
            body.operation.deadline_at_ms.parse::<u64>().map_err(|_| {
                BrainError::Invalid("customer operation deadline is invalid".into())
            })?;
        if deadline_at_ms != dispatch_deadline_at_ms || deadline_at_ms <= crate::wall_ms() {
            return Err(BrainError::Invalid(
                "customer operation deadline is invalid or elapsed".into(),
            ));
        }
        let descriptor: ToolDescriptor = serde_json::from_str(&body.operation.descriptor_json)
            .map_err(|_| BrainError::Invalid("customer Tool descriptor is invalid".into()))?;
        valid_id("Tool registration", &descriptor.registration)?;
        valid_id("Tool name", &descriptor.name)?;
        if !is_digest(&descriptor.contract_digest) {
            return Err(BrainError::Invalid(
                "customer Tool contract digest is invalid".into(),
            ));
        }
        let input = serde_json::from_str(&body.operation.input_json)
            .map_err(|_| BrainError::Invalid("customer Tool input is invalid JSON".into()))?;
        let identity = OperationIdentity {
            binding: body.binding,
            registration: descriptor.registration,
            name: descriptor.name,
            contract_digest: descriptor.contract_digest,
            input,
            deadline_at_ms,
        };
        let cancel = CancellationToken::new();
        {
            let mut operations = self.operations.lock().await;
            if let Some(existing) = operations.get(operation_id) {
                if operation_identity(existing) != &identity {
                    return Err(BrainError::Invalid(
                        "customer operation id was reused with different input".into(),
                    ));
                }
                return Ok(json!({"provider_operation_id": operation_id}));
            }
            if operations.len() >= MAX_COMPONENT_OPERATIONS {
                return Err(BrainError::Overloaded);
            }
            operations.insert(
                operation_id.to_owned(),
                Operation::Running {
                    identity: identity.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        let driver = Arc::clone(self);
        let provider_operation_id = operation_id.to_owned();
        tokio::spawn(async move {
            let execution = driver
                .coordinator
                .execute(
                    &identity.binding.tenant_id,
                    &client_id,
                    1,
                    &identity.binding.session_id,
                    &provider_operation_id,
                    &identity.registration,
                    &identity.name,
                    &identity.contract_digest,
                    identity.input.clone(),
                    identity.deadline_at_ms,
                    cancel,
                )
                .await;
            let state = match execution.outcome.outcome {
                TerminalOutcome::Completed if !execution.outcome.is_error => "completed",
                TerminalOutcome::Cancelled => "cancelled",
                _ => "failed",
            };
            let value_json = execution
                .outcome
                .value
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .unwrap_or_default()
                .unwrap_or_else(|| "null".into());
            let terminal = json!({
                "value_json": value_json,
                "content": execution.outcome.content,
                "is_error": execution.outcome.is_error,
            });
            let mut operations = driver.operations.lock().await;
            if operations
                .get(&provider_operation_id)
                .is_some_and(|current| operation_identity(current) == &identity)
            {
                operations.insert(
                    provider_operation_id,
                    Operation::Terminal {
                        identity,
                        state,
                        value: terminal,
                        receipt: execution.terminal_receipt,
                    },
                );
            }
        });
        Ok(json!({"provider_operation_id": operation_id}))
    }

    async fn observe(&self, operation_id: &str, request: Value) -> Result<Value> {
        let body: OperationBody = parse(request)?;
        validate_operation_body(operation_id, &body)?;
        let operations = self.operations.lock().await;
        let operation = operations.get(operation_id).ok_or_else(|| {
            BrainError::Invalid("customer Environment operation is unknown".into())
        })?;
        validate_operation_binding(operation, &body.binding)?;
        match operation {
            Operation::Running { .. } => Ok(json!({
                "state": "running",
                "cursor": body.cursor.unwrap_or_default(),
                "chunks": [],
            })),
            Operation::Terminal { state, value, .. } => Ok(json!({
                "state": state,
                "cursor": "terminal",
                "chunks": [],
                "terminal_json": value,
            })),
        }
    }

    async fn cancel(&self, operation_id: &str, request: Value) -> Result<Value> {
        let body: OperationBody = parse(request)?;
        validate_operation_body(operation_id, &body)?;
        let operations = self.operations.lock().await;
        let operation = operations.get(operation_id).ok_or_else(|| {
            BrainError::Invalid("customer Environment operation is unknown".into())
        })?;
        validate_operation_binding(operation, &body.binding)?;
        if let Operation::Running { cancel, .. } = operation {
            cancel.cancel();
        }
        Ok(json!({}))
    }

    async fn acknowledge(&self, operation_id: &str, request: Value) -> Result<Value> {
        let body: AcknowledgeBody = parse(request)?;
        if body.provider_operation_id != operation_id {
            return Err(BrainError::Invalid(
                "customer Environment operation identity does not match".into(),
            ));
        }
        validate_binding(&body.binding)?;
        let (identity, terminal, receipt) = {
            let operations = self.operations.lock().await;
            let operation = operations.get(operation_id).ok_or_else(|| {
                BrainError::Invalid("customer Environment operation is unknown".into())
            })?;
            validate_operation_binding(operation, &body.binding)?;
            match operation {
                Operation::Terminal {
                    identity,
                    value,
                    receipt,
                    ..
                } if value == &body.terminal => (identity.clone(), value.clone(), receipt.clone()),
                Operation::Terminal { .. } => {
                    return Err(BrainError::Invalid(
                        "customer terminal acknowledgement does not match".into(),
                    ));
                }
                Operation::Running { .. } => {
                    return Err(BrainError::Invalid(
                        "customer Environment operation is not terminal".into(),
                    ));
                }
            }
        };
        if let Some(receipt) = &receipt {
            self.coordinator.acknowledge_terminal(receipt).await?;
        }
        let mut operations = self.operations.lock().await;
        if matches!(
            operations.get(operation_id),
            Some(Operation::Terminal { identity: current, value, .. })
                if current == &identity && value == &terminal
        ) {
            operations.remove(operation_id);
        }
        Ok(json!({}))
    }

    async fn release(&self, request: Value) -> Result<Value> {
        let body: ReleaseBody = parse(request)?;
        validate_binding(&body.binding)?;
        let mut operations = self.operations.lock().await;
        operations.retain(|_, operation| {
            let identity = operation_identity(operation);
            let matches = identity.binding.tenant_id == body.binding.tenant_id
                && identity.binding.session_id == body.binding.session_id
                && identity.binding.environment_id == body.binding.environment_id;
            if matches && let Operation::Running { cancel, .. } = operation {
                cancel.cancel();
            }
            !matches
        });
        Ok(json!({}))
    }
}

fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|_| BrainError::Invalid("invalid customer Environment request".into()))
}

fn validate_binding(binding: &Binding) -> Result<String> {
    if binding.driver != "customer" {
        return Err(BrainError::Invalid(
            "customer Environment binding selected a different driver".into(),
        ));
    }
    valid_id("tenant id", &binding.tenant_id)?;
    valid_id("session id", &binding.session_id)?;
    valid_id("root id", &binding.root_id)?;
    if let Some(parent_id) = &binding.parent_id {
        valid_id("parent id", parent_id)?;
    }
    valid_id("Environment id", &binding.environment_id)?;
    let configuration: AppConfiguration = serde_json::from_value(binding.configuration.clone())
        .map_err(|_| BrainError::Invalid("customer Environment configuration is invalid".into()))?;
    valid_id("client id", &configuration.registration)?;
    Ok(configuration.registration)
}

fn validate_operation_body(operation_id: &str, body: &OperationBody) -> Result<()> {
    if body.provider_operation_id != operation_id {
        return Err(BrainError::Invalid(
            "customer Environment operation identity does not match".into(),
        ));
    }
    validate_binding(&body.binding)?;
    if body
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.len() > 128)
    {
        return Err(BrainError::Invalid(
            "customer Environment cursor is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_operation_binding(operation: &Operation, binding: &Binding) -> Result<()> {
    if &operation_identity(operation).binding != binding {
        return Err(BrainError::Invalid(
            "customer Environment operation binding does not match".into(),
        ));
    }
    Ok(())
}

fn operation_identity(operation: &Operation) -> &OperationIdentity {
    match operation {
        Operation::Running { identity, .. } | Operation::Terminal { identity, .. } => identity,
    }
}

fn valid_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    {
        return Err(BrainError::Invalid(format!(
            "{label} must contain 1 through 128 safe ASCII bytes"
        )));
    }
    Ok(())
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::customer::{
        CustomerCommand, CustomerEnvironmentIngressPort, CustomerGatewayInput,
        CustomerGatewayRoute, CustomerObservation, CustomerTransportConfig, frame_proof,
    };
    use tokio::sync::mpsc;

    async fn connected() -> (
        Arc<CustomerComponentDriver>,
        crate::customer::CustomerGrant,
        mpsc::Receiver<CustomerCommand>,
        u64,
    ) {
        let coordinator = CustomerCoordinator::new(
            CustomerTransportConfig::new(
                "ws://127.0.0.1:3210/v1/customer-environment/socket",
                "http://127.0.0.1:3210",
            )
            .unwrap(),
            None,
        );
        let grant = coordinator.grant("tenant", "app").await.unwrap();
        let proof = frame_proof(&grant.protocol);
        coordinator
            .receive(CustomerGatewayInput {
                route: CustomerGatewayRoute::Connect,
                connection_id: "connection".into(),
                request_id: "connect".into(),
                route_key: "$connect".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: Some(grant.protocol.clone()),
                body: None,
            })
            .await
            .unwrap();
        let (sender, mut receiver) = mpsc::channel(8);
        coordinator
            .bind_local_sender("connection", sender)
            .await
            .unwrap();
        coordinator
            .receive(CustomerGatewayInput {
                route: CustomerGatewayRoute::Message,
                connection_id: "connection".into(),
                request_id: "register".into(),
                route_key: "$default".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: None,
                body: Some(
                    json!({
                        "type":"register", "client_id":"app", "process_id":"process:test",
                        "proof":proof
                    })
                    .to_string(),
                ),
            })
            .await
            .unwrap();
        let Some(CustomerCommand::Ready { epoch }) = receiver.recv().await else {
            panic!("ready")
        };
        coordinator
            .receive(CustomerGatewayInput {
                route: CustomerGatewayRoute::Message,
                connection_id: "connection".into(),
                request_id: "tools".into(),
                route_key: "$default".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: None,
                body: Some(
                    json!({
                        "type":"register_tools", "epoch":epoch, "batch_id":"batch",
                        "proof":proof,
                        "registrations":[{
                            "registration":"lookup", "name":"lookup",
                            "contract_digest":"a".repeat(64)
                        }]
                    })
                    .to_string(),
                ),
            })
            .await
            .unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(CustomerCommand::Registered { .. })
        ));
        (
            CustomerComponentDriver::new(coordinator),
            grant,
            receiver,
            epoch,
        )
    }

    fn binding() -> Value {
        json!({
            "driver":"customer",
            "configuration":{"registration":"app"},
            "policy":{},
            "tenant_id":"tenant",
            "session_id":"ses_test",
            "root_id":"ses_test",
            "parent_id":null,
            "environment_id":"application"
        })
    }

    #[tokio::test]
    async fn generic_lifecycle_routes_one_source_free_callback_and_acks_exact_terminal() {
        let (driver, grant, mut commands, epoch) = connected().await;
        let deadline = crate::wall_ms() + 5_000;
        let submit = json!({
            "binding":binding(),
            "operation":{
                "operation_id":"operation",
                "kind":"invoke",
                "descriptor_json":json!({
                    "registration":"lookup",
                    "name":"lookup",
                    "contract_digest":"a".repeat(64)
                }).to_string(),
                "input_json":json!({"id":7}).to_string(),
                "deadline_at_ms":deadline.to_string()
            }
        });
        assert_eq!(
            driver
                .dispatch("operation", "submit", submit, deadline)
                .await
                .unwrap(),
            json!({"provider_operation_id":"operation"})
        );
        let Some(CustomerCommand::Offer(offer)) = commands.recv().await else {
            panic!("offer")
        };
        driver
            .coordinator
            .observation(
                &grant.grant_id,
                &grant.observation_token,
                CustomerObservation::Receipt {
                    epoch,
                    operation_id: offer.operation_id.clone(),
                    request_digest: offer.request_digest.clone(),
                    replayed: false,
                },
            )
            .await
            .unwrap();
        driver
            .coordinator
            .observation(
                &grant.grant_id,
                &grant.observation_token,
                CustomerObservation::Terminal {
                    epoch,
                    operation_id: offer.operation_id,
                    request_digest: offer.request_digest,
                    ok: true,
                    output: Some(json!({"ok":true})),
                    error: None,
                },
            )
            .await
            .unwrap();
        let terminal = loop {
            let observation = driver
                .dispatch(
                    "operation",
                    "observe",
                    json!({
                        "binding":binding(),
                        "provider_operation_id":"operation",
                        "cursor":null
                    }),
                    u64::MAX,
                )
                .await
                .unwrap();
            if observation["state"] == "completed" {
                break observation["terminal_json"].clone();
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(terminal["value_json"], r#"{"ok":true}"#);
        driver
            .dispatch(
                "operation",
                "acknowledge",
                json!({
                    "binding":binding(),
                    "provider_operation_id":"operation",
                    "terminal":terminal
                }),
                u64::MAX,
            )
            .await
            .unwrap();
        assert!(matches!(
            commands.recv().await,
            Some(CustomerCommand::Ack { .. })
        ));
        assert!(
            driver
                .dispatch(
                    "operation",
                    "observe",
                    json!({
                        "binding":binding(),
                        "provider_operation_id":"operation",
                        "cursor":null
                    }),
                    u64::MAX,
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn callback_driver_rejects_uploaded_implementation_bytes() {
        let (driver, _, _, _) = connected().await;
        let deadline = crate::wall_ms() + 5_000;
        let result = driver
            .dispatch(
                "operation",
                "submit",
                json!({
                    "binding":binding(),
                    "operation":{
                        "operation_id":"operation",
                        "kind":"invoke",
                        "descriptor_json":json!({
                            "registration":"lookup", "name":"lookup",
                            "contract_digest":"a".repeat(64)
                        }).to_string(),
                        "bundle_base64":"dXNlciBzb3VyY2U=",
                        "input_json":"{}",
                        "deadline_at_ms":deadline.to_string()
                    }
                }),
                deadline,
            )
            .await;
        assert!(matches!(result, Err(BrainError::Invalid(_))));
    }
}
