use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use brain::{AppendRecord, Error, SessionStore, SessionUpdate, environment::ExecutionServices};
use brain_protocol::{
    EnvironmentControlRequest as Request, EnvironmentGrant, EnvironmentLifecycle, EnvironmentName,
    EnvironmentOperation, EnvironmentPermission as Permission, EnvironmentReceipt, EnvironmentRef,
    EnvironmentRequest, EnvironmentState, EnvironmentView, SessionId, codes,
};
use serde_json::{Value, json};

use crate::EnvironmentRegistry;

mod context;
mod observation;

#[derive(Default)]
pub(crate) struct Control {
    executions: Mutex<Weak<brain::ToolExecutions>>,
    stores: Mutex<HashMap<SessionId, Weak<dyn SessionStore>>>,
    locks: crate::locks::KeyedLocks<SessionId>,
    observed: Mutex<Option<ObservationListener>>,
    maximum_emitted_bytes: std::sync::atomic::AtomicUsize,
}

type ObservationListener = Arc<dyn Fn(SessionId, u64) + Send + Sync>;

#[async_trait]
impl brain::environment::EnvironmentControl for EnvironmentRegistry {
    async fn control(
        &self,
        store: Arc<dyn SessionStore>,
        caller: Option<&EnvironmentName>,
        grants: &[EnvironmentGrant],
        request: Request,
    ) -> Result<Value, Error> {
        EnvironmentRegistry::control(self, store, caller, grants, request).await
    }
}

#[cfg(test)]
#[path = "../control_tests.rs"]
mod tests;

impl EnvironmentRegistry {
    pub fn bind_executions(&self, executions: &Arc<brain::ToolExecutions>) {
        *self
            .control
            .executions
            .lock()
            .expect("Environment execution table poisoned") = Arc::downgrade(executions);
    }
    pub async fn control(
        &self,
        store: Arc<dyn SessionStore>,
        caller: Option<&EnvironmentName>,
        grants: &[EnvironmentGrant],
        request: Request,
    ) -> Result<Value, Error> {
        let lock = self
            .control
            .locks
            .acquire(store.session_id().clone())
            .map_err(|error| Error::Journal(error.message))?;
        let guard = lock.lock().await;
        let config = brain::session_config(&*store)?;
        let mut views = brain::environment::environments(&*store, &config)?;
        if matches!(request, Request::List) {
            return Ok(json!(
                views
                    .values()
                    .filter(|view| allowed(grants, &view.template, Permission::Read, None))
                    .collect::<Vec<_>>()
            ));
        }
        let (mut view, permission, method) = match &request {
            Request::Create {
                template,
                name,
                configuration,
            } => {
                if !crate::service::valid_identifier(name.as_str()) {
                    return Err(Error::InvalidState("invalid Environment name".into()));
                }
                if !allowed(grants, template, Permission::Create, None) {
                    return Err(denied());
                }
                let source = config.environment(template).ok_or_else(denied)?;
                let declaration = source.template.as_ref().ok_or_else(denied)?;
                validate(&declaration.configuration_schema, configuration)?;
                if views
                    .get(name)
                    .is_some_and(|view| view.state != EnvironmentState::Deleted)
                {
                    return Err(Error::InvalidState(
                        "Environment name is already in use".into(),
                    ));
                }
                if views
                    .values()
                    .filter(|view| {
                        &view.template == template && view.state != EnvironmentState::Deleted
                    })
                    .count()
                    >= declaration.max_instances
                {
                    return Err(Error::InvalidState(
                        "Environment template instance limit reached".into(),
                    ));
                }
                (
                    EnvironmentView {
                        reference: EnvironmentRef {
                            name: name.clone(),
                            sequence: 0,
                        },
                        template: template.clone(),
                        lifecycle: source.lifecycle.ok_or_else(|| {
                            Error::InvalidState("Environment lifecycle is required".into())
                        })?,
                        state: EnvironmentState::Declared,
                        configuration: configuration.clone(),
                        methods: source.methods.clone(),
                        observation: None,
                        observed_at_ms: None,
                        pending_operation: None,
                        availability: None,
                        tools: config
                            .tools
                            .iter()
                            .filter(|tool| tool.placements.contains_key(template))
                            .map(|tool| tool.definition())
                            .collect(),
                    },
                    Permission::Create,
                    None,
                )
            }
            Request::List => unreachable!(),
            Request::Get { environment }
            | Request::Setup { environment }
            | Request::Update { environment, .. }
            | Request::Delete { environment }
            | Request::Call { environment, .. } => {
                let view = views
                    .remove(&environment.name)
                    .filter(|view| {
                        view.reference == *environment
                            && (view.state != EnvironmentState::Deleted
                                || matches!(request, Request::Get { .. }))
                    })
                    .ok_or_else(|| {
                        Error::NotFound("Environment reference is absent or stale".into())
                    })?;
                let permission = match request {
                    Request::Get { .. } => Permission::Read,
                    Request::Setup { .. } => Permission::Setup,
                    Request::Update { .. } => Permission::Update,
                    Request::Delete { .. } => Permission::Delete,
                    _ => Permission::Call,
                };
                let method = if let Request::Call { method, .. } = &request {
                    Some(method.clone())
                } else {
                    None
                };
                (view, permission, method)
            }
        };
        if !allowed(grants, &view.template, permission, method.as_deref()) {
            return Err(denied());
        }
        if matches!(request, Request::Get { .. }) {
            return Ok(json!(view));
        }
        if !matches!(
            store.session_summary()?.status,
            brain_protocol::SessionStatus::Creating
                | brain_protocol::SessionStatus::Idle
                | brain_protocol::SessionStatus::Running
        ) {
            return Err(Error::InvalidState(
                "session no longer accepts Environment operations".into(),
            ));
        }
        if (Some(&view.reference.name) == caller
            || (view.reference.name == config.agentloop.environment
                && !(caller.is_none()
                    && view.state == EnvironmentState::Declared
                    && matches!(request, Request::Setup { .. }))))
            && !matches!(request, Request::Call { .. })
        {
            return Err(Error::InvalidState(
                "an extension cannot replace its own execution Environment".into(),
            ));
        }
        let remote = match request {
            Request::Create { .. } => {
                view.reference.sequence =
                    append(&*store, codes::event::ENVIRONMENT_DECLARED, json!(view))?;
                if view.lifecycle == EnvironmentLifecycle::Manual {
                    return Ok(json!(view));
                }
                Some(EnvironmentRequest::Setup {
                    configuration: view.configuration.clone(),
                })
            }
            Request::Setup { .. } => {
                if view.state != EnvironmentState::Declared {
                    return Err(Error::InvalidState(
                        "Environment setup was already attempted; inspect its recorded outcome"
                            .into(),
                    ));
                }
                Some(EnvironmentRequest::Setup {
                    configuration: view.configuration.clone(),
                })
            }
            Request::Update { configuration, .. } => {
                if view.state != EnvironmentState::Declared {
                    return Err(Error::InvalidState("configuration updates require an Environment whose setup has not been attempted".into()));
                }
                let template = config
                    .environment(&view.template)
                    .ok_or_else(denied)?
                    .template
                    .as_ref()
                    .ok_or_else(|| {
                        Error::InvalidState(
                            "configuration updates require an admitted template schema".into(),
                        )
                    })?;
                validate(&template.configuration_schema, &configuration)?;
                append(
                    &*store,
                    codes::event::ENVIRONMENT_UPDATED,
                    json!({"environment": view.reference.name, "configuration": configuration}),
                )?;
                view.configuration = configuration;
                return Ok(json!(view));
            }
            Request::Delete { .. } => {
                if view.state == EnvironmentState::Declared {
                    append(
                        &*store,
                        codes::event::ENVIRONMENT_CLOSED,
                        json!({"environment": view.reference.name}),
                    )?;
                    view.state = EnvironmentState::Deleted;
                    return Ok(json!(view));
                }
                if matches!(
                    view.state,
                    EnvironmentState::Starting
                        | EnvironmentState::Deleting
                        | EnvironmentState::Unknown
                ) {
                    return Err(Error::InvalidState(
                        "Environment has an unresolved operation".into(),
                    ));
                }
                Some(EnvironmentRequest::Teardown)
            }
            Request::Call { method, input, .. } => {
                if matches!(
                    view.state,
                    EnvironmentState::Deleting | EnvironmentState::Detached
                ) {
                    return Err(Error::Conflict("Environment is closing or detached".into()));
                }
                let definition = view.methods.get(&method).ok_or_else(denied)?;
                validate(&definition.input_schema, &input)?;
                if definition.effect == brain_protocol::EnvironmentMethodEffect::Replace {
                    if Some(&view.reference.name) == caller
                        || view.reference.name == config.agentloop.environment
                        || active_tools(&*store, &view.reference.name)?
                    {
                        return Err(Error::Conflict(
                            "Environment replacement would interrupt an active extension".into(),
                        ));
                    }
                    if matches!(
                        view.state,
                        EnvironmentState::Starting
                            | EnvironmentState::Deleting
                            | EnvironmentState::Unknown
                    ) {
                        return Err(Error::Conflict(
                            "resolve the pending Environment operation before replacement".into(),
                        ));
                    }
                }
                Some(EnvironmentRequest::Call {
                    name: method,
                    input,
                })
            }
            _ => unreachable!(),
        };
        let request = remote.expect("remote operation");
        let kind = match request {
            EnvironmentRequest::Setup { .. } => codes::event::call::ENVIRONMENT_SETUP,
            EnvironmentRequest::Teardown => codes::event::call::ENVIRONMENT_TEARDOWN,
            EnvironmentRequest::Call { ref name, .. }
                if view.methods[name].effect
                    == brain_protocol::EnvironmentMethodEffect::Replace =>
            {
                codes::event::call::ENVIRONMENT_REPLACE
            }
            _ => codes::event::call::ENVIRONMENT_CALL,
        };
        let sequence = append(
            &*store,
            &format!("{kind}_started"),
            json!({
                "environment": view.reference.name, "environment_sequence": view.reference.sequence, "request": request,
            }),
        )?;
        let operation = EnvironmentOperation {
            template: Some(view.template.clone()),
            context: None,
            binding: None,
            reporter: None,
            configuration: serde_json::Value::Null,
            sequence,
            session_id: store.session_id().clone(),
            environment: view.reference.name.clone(),
            request,
        };
        let environment = brain::environment::descriptor(&config, &view)?;
        drop(guard);
        if matches!(operation.request, EnvironmentRequest::Teardown) {
            let executions = self
                .control
                .executions
                .lock()
                .expect("Environment execution table poisoned")
                .upgrade();
            if let Some(executions) = executions {
                executions
                    .group(store.session_id())
                    .cancel_environment(&view.reference.name)
                    .await;
            }
        }
        let result = self.send(&environment, &operation, None).await;
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                append(
                    &*store,
                    &format!("{kind}_failed"),
                    json!({"sequence": sequence, "code": error.code(), "message": error.to_string(), "ambiguous": matches!(error, Error::Ambiguous(_)), "domain": "transport"}),
                )?;
                return Err(error);
            }
        };
        match &receipt {
            EnvironmentReceipt::Failure {
                code,
                message,
                retryable,
                details,
            } => {
                append(
                    &*store,
                    &format!("{kind}_failed"),
                    json!({"sequence": sequence, "code": code, "message": message, "retryable": retryable, "details": details, "domain": "environment", "ambiguous": false}),
                )?;
                return Err(Error::Environment(brain_protocol::OutcomeError {
                    code: code.clone(),
                    message: message.clone(),
                    retryable: *retryable,
                    details: details.clone(),
                }));
            }
            EnvironmentReceipt::Unknown { message } => {
                append(
                    &*store,
                    &format!("{kind}_failed"),
                    json!({"sequence": sequence, "code": "unknown", "message": message, "domain": "environment", "ambiguous": true}),
                )?;
                return Err(Error::Ambiguous(message.clone()));
            }
            EnvironmentReceipt::Accepted { on_turn_end }
                if !matches!(operation.request, EnvironmentRequest::Call { .. })
                    && on_turn_end
                        .as_ref()
                        .is_none_or(|name| crate::service::valid_identifier(name)) => {}
            EnvironmentReceipt::Result { output } => {
                if let EnvironmentRequest::Call { name, .. } = &operation.request
                    && let Some(schema) = &view.methods[name].output_schema
                    && let Err(error) = validate(schema, output)
                {
                    append(
                        &*store,
                        &format!("{kind}_failed"),
                        json!({"sequence": sequence, "code": "environment_output_invalid", "message": error.to_string(), "domain": "environment", "ambiguous": true}),
                    )?;
                    return Err(Error::Ambiguous(format!(
                        "Environment method output is invalid: {error}"
                    )));
                }
            }
            _ => {
                append(
                    &*store,
                    &format!("{kind}_failed"),
                    json!({"sequence": sequence, "code": "unknown", "message": "nonterminal Environment receipt", "domain": "transport", "ambiguous": true}),
                )?;
                return Err(Error::Ambiguous("nonterminal Environment receipt".into()));
            }
        }
        append(
            &*store,
            &format!("{kind}_ended"),
            json!({"sequence": sequence, "result": receipt}),
        )?;
        if let EnvironmentRequest::Call { .. } = &operation.request {
            let EnvironmentReceipt::Result { output } = receipt else {
                return Err(Error::Executor(
                    "Environment method must return a result".into(),
                ));
            };
            return Ok(output);
        }
        Ok(json!(
            brain::environment::environments(&*store, &config)?.get(&view.reference.name)
        ))
    }
}

fn allowed(
    grants: &[EnvironmentGrant],
    template: &EnvironmentName,
    permission: Permission,
    method: Option<&str>,
) -> bool {
    grants.iter().any(|grant| {
        &grant.environment == template
            && grant.permissions.contains(&permission)
            && method.is_none_or(|method| grant.methods.iter().any(|allowed| allowed == method))
    })
}

fn denied() -> Error {
    Error::InvalidState("Environment operation is not granted".into())
}

fn validate(schema: &Value, input: &Value) -> Result<(), Error> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| Error::InvalidState(error.to_string()))?;
    validator
        .validate(input)
        .map_err(|error| Error::InvalidState(error.to_string()))
}

fn append(store: &dyn SessionStore, kind: &str, payload: Value) -> Result<u64, Error> {
    Ok(store.append_sync(
        &[AppendRecord::new(kind, payload)],
        SessionUpdate::default(),
    )?[0]
        .sequence)
}

fn active_tools(store: &dyn SessionStore, environment: &EnvironmentName) -> Result<bool, Error> {
    let mut active = std::collections::HashSet::new();
    let mut after = 0;
    loop {
        let records = store.records_after(after, 1000)?;
        if records.is_empty() {
            return Ok(!active.is_empty());
        }
        for record in records {
            after = record.sequence;
            if record.kind == codes::event::TOOL_CALL_STARTED
                && record.payload["environment"].as_str() == Some(environment.as_str())
            {
                active.insert(record.sequence);
            } else if matches!(
                record.kind.as_str(),
                codes::event::TOOL_CALL_ENDED | codes::event::TOOL_CALL_FAILED
            ) && let Some(sequence) = record.payload["sequence"].as_u64()
            {
                active.remove(&sequence);
            }
        }
    }
}
