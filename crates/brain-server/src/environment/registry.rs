//! Session Environment bindings. Providers own physical lifetime; Brain records lifecycle calls.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use brain::{CreatingSession, Session, SessionStore};
use brain_protocol::{
    AttachmentId, EnvironmentBinding, EnvironmentCallResult, EnvironmentId, EnvironmentOperation,
    EnvironmentReceipt, EnvironmentRequest, EnvironmentStatus, Provision, SessionConfig,
    SessionEnvironment, ToolManifest, codes,
};

use super::{EnvironmentAdapter, EnvironmentRecord, EnvironmentResources, resources};

/// Plaintext binding values per environment, carried beside the configuration for
/// exactly as long as create runs. They go out on the attach wire and never enter the
/// journal.
pub type SessionBindingValues = HashMap<EnvironmentId, BTreeMap<String, String>>;

/// Where an environment is reached.
#[derive(Clone)]
pub struct DirectoryEntry {
    pub binding: EnvironmentBinding,
    pub endpoint: String,
    adapter: Arc<dyn EnvironmentAdapter>,
}

pub struct EnvironmentRegistry {
    resources: Arc<EnvironmentResources>,
    endpoint: String,
    adapter: Arc<dyn EnvironmentAdapter>,
    routes: HashMap<String, (String, Arc<dyn EnvironmentAdapter>)>,
}

impl EnvironmentRegistry {
    pub fn new(
        resources: Arc<EnvironmentResources>,
        endpoint: impl Into<String>,
        adapter: Arc<dyn EnvironmentAdapter>,
    ) -> Self {
        Self {
            resources,
            endpoint: endpoint.into(),
            adapter,
            routes: HashMap::new(),
        }
    }

    pub fn resources(&self) -> &Arc<EnvironmentResources> {
        &self.resources
    }

    /// Deployment-granted routing. Session configuration can select a driver, never its credentials.
    pub fn with_route(
        mut self,
        driver: String,
        endpoint: String,
        adapter: Arc<dyn EnvironmentAdapter>,
    ) -> Result<Self, brain::Error> {
        if driver.is_empty()
            || driver == "brain_wasm"
            || endpoint.trim().is_empty()
            || self.routes.contains_key(&driver)
        {
            return Err(brain::Error::InvalidState(
                "invalid or duplicate Environment route".into(),
            ));
        }
        self.routes.insert(driver, (endpoint, adapter));
        Ok(self)
    }

    /// Opens an Environment as part of session admission, recording setup before send.
    pub async fn create_for_session(
        &self,
        creation: &mut CreatingSession,
        specification: &SessionEnvironment,
    ) -> Result<EnvironmentRecord, brain::Error> {
        let native = brain_wasm_resources(&specification.configuration)?;
        let routed = specification
            .configuration
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|driver| self.routes.contains_key(driver));
        if native.is_none() && !routed && self.endpoint.trim().is_empty() {
            return Err(brain::Error::InvalidState(
                "no Environment endpoint is configured".into(),
            ));
        }
        let environment_id = specification.environment_id.clone();
        self.resources.create(EnvironmentRecord {
            session_id: creation.session_id().clone(),
            environment_id: environment_id.clone(),
            configuration: specification.configuration.clone(),

            created_at_ms: resources::wall_clock_ms(),
            resources: Default::default(),
        })?;
        let entry = self.entry(&environment_id)?;
        let receipt = self
            .lifecycle(
                creation,
                &entry,
                EnvironmentRequest::Setup {
                    configuration: specification.configuration.clone(),
                },
                None,
                codes::event::call::ENVIRONMENT_SETUP,
            )
            .await?;
        let declared = receipt_declaration(&receipt);
        self.resources.update(&environment_id, |record| {
            record.resources = declared;
        })
    }

    pub fn ids(&self) -> Result<Vec<EnvironmentId>, brain::Error> {
        self.resources.ids()
    }

    /// Tears the environment down and forgets it. Attached sessions hear
    /// `environment_closed`.
    pub async fn close(
        &self,
        environment_id: &EnvironmentId,
        store: &dyn SessionStore,
        retry_failed: bool,
    ) -> Result<(), brain::Error> {
        let Some(record) = self.resources.get(environment_id)? else {
            return Ok(());
        };
        if &record.session_id != store.session_id() {
            return Err(brain::Error::InvalidState(
                "Environment belongs to another session".into(),
            ));
        }
        let previous = operation_outcome(
            store,
            environment_id,
            codes::event::call::ENVIRONMENT_TEARDOWN,
        )?;
        if previous
            .as_ref()
            .is_some_and(|attempt| attempt.outcome == Some(true))
        {
            return self.resources.remove(environment_id);
        }
        if let Some(attempt) = &previous
            && attempt.outcome.is_none()
        {
            store.append_sync(&[brain::AppendRecord::new(codes::event::ENVIRONMENT_TEARDOWN_FAILED,
                serde_json::json!({"sequence":attempt.sequence,"code":"interrupted","ambiguous":true,"message":"teardown result was not recorded"}))], brain::SessionUpdate::default())?;
        }
        if previous.is_some() && !retry_failed {
            return Err(brain::Error::Ambiguous(
                "Environment teardown needs an explicit retry".into(),
            ));
        }
        self.operation(environment_id, EnvironmentRequest::Teardown, store)
            .await?;
        self.resources.remove(environment_id)?;
        store.append_sync(
            &[brain::AppendRecord::new(
                codes::event::ENVIRONMENT_CLOSED,
                serde_json::json!({"environment_id": environment_id}),
            )],
            brain::SessionUpdate::default(),
        )?;
        Ok(())
    }

    pub async fn prepare_session(
        &self,
        mut creation: CreatingSession,
        config: SessionConfig,
        binding_values: SessionBindingValues,
    ) -> Result<(Session, SessionConfig), brain::Error> {
        match self.prepare(&mut creation, config, binding_values).await {
            Ok(config) => {
                let session = creation.complete(config.clone())?;
                Ok((session, config))
            }
            Err(error) => {
                let message = error.to_string();
                creation.fail(codes::failure::ENVIRONMENT_PREPARATION_FAILED, &message)?;
                Err(error)
            }
        }
    }

    /// Attaches the session to every environment the configuration names and fills in
    /// what each one answered with, so the configuration the session is admitted with
    /// says what was actually granted.
    async fn prepare(
        &self,
        creation: &mut CreatingSession,
        mut config: SessionConfig,
        mut binding_values: SessionBindingValues,
    ) -> Result<SessionConfig, brain::Error> {
        let provisions: Vec<Vec<Provision>> = config
            .environments
            .iter()
            .map(|attachment| provisions_for(&config, &attachment.environment_id))
            .collect();
        for (attachment, provisions) in config.environments.iter_mut().zip(provisions) {
            let record = self
                .resources
                .get(&attachment.environment_id)?
                .ok_or_else(|| {
                    brain::Error::InvalidState(format!(
                        "Environment `{}` does not exist",
                        attachment.environment_id
                    ))
                })?;
            let entry = self.entry(&attachment.environment_id)?;
            let attachment_id = AttachmentId::new(brain::random_id("att"));
            let bindings = binding_values
                .remove(&attachment.environment_id)
                .unwrap_or_default();
            let attached = self
                .lifecycle(
                    creation,
                    &entry,
                    EnvironmentRequest::Attach {
                        provisions,
                        bindings,
                    },
                    Some(attachment_id.clone()),
                    codes::event::call::ENVIRONMENT_ATTACH,
                )
                .await?;
            // What the environment declares it executes and offers feeds the
            // configuration's bind check; setup and attach both may report it, and a
            // resource attach declares again replaces setup's block.
            let mut resources = record.resources;
            resources.extend(receipt_declaration(&attached));
            attachment.binding = Some(entry.binding);
            attachment.attachment_id = Some(attachment_id);
            attachment.resources = resources;
        }
        let SessionConfig {
            environments,
            tool_bindings,
            ..
        } = &mut config;
        for tool in tool_bindings.iter_mut() {
            // A resident Tool binds no Environment; its registered host answers it.
            let Some(environment_id) = &tool.environment_id else {
                continue;
            };
            let attachment = environments
                .iter()
                .find(|attachment| &attachment.environment_id == environment_id)
                .filter(|attachment| attachment.attached())
                .ok_or_else(|| {
                    brain::Error::InvalidState("Tool requested an unresolved Environment".into())
                })?;
            tool.environment = attachment.binding.clone();
            tool.attachment_id = attachment.attachment_id.clone();
        }
        Ok(config)
    }

    async fn lifecycle(
        &self,
        creation: &mut CreatingSession,
        entry: &DirectoryEntry,
        request: EnvironmentRequest,
        attachment_id: Option<AttachmentId>,
        kind: &str,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        // An attach carries binding values in plaintext, and plaintext never enters
        // the journal: the record carries the names with their values struck out.
        let sequence = match redacted(&request) {
            Some(journal_view) => creation.record_call_started(kind, &journal_view)?,
            None => creation.record_call_started(kind, &request)?,
        };
        let operation = EnvironmentOperation {
            sequence,
            environment_id: entry.binding.environment_id.clone(),
            session_id: creation.session_id().clone(),
            attachment_id,
            request,
        };
        let sent = self.send(entry, &operation).await;
        match sent.and_then(|receipt| terminal(receipt, "lifecycle")) {
            Ok(receipt) => {
                creation.record_call_ended(kind, sequence, &receipt)?;
                Ok(receipt)
            }
            Err(error) => {
                creation.record_call_failed(kind, sequence, &error)?;
                if matches!(error, brain::Error::Ambiguous(_)) {
                    creation.record(codes::event::ENVIRONMENT_UNREACHABLE, serde_json::json!({"environment_id": operation.environment_id, "sequence": sequence}))?;
                }
                Err(error)
            }
        }
    }

    /// One operation on a session's behalf whose record the session already holds: a
    /// tool invoke or cancel.
    pub async fn execute(
        &self,
        binding: &EnvironmentBinding,
        operation: &EnvironmentOperation,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let entry = self.entry(&binding.environment_id)?;
        self.send(&entry, operation).await
    }

    pub fn brain_wasm_configuration(
        &self,
        environment_id: &EnvironmentId,
    ) -> Result<Option<serde_json::Value>, brain::Error> {
        let Some(record) = self.resources.get(environment_id)? else {
            return Err(brain::Error::NotFound(format!(
                "Environment `{environment_id}` does not exist"
            )));
        };
        Ok(brain_wasm_resources(&record.configuration)?.map(|_| record.configuration))
    }

    pub async fn call(
        &self,
        session: &Session,
        store: &dyn SessionStore,
        environment_id: &EnvironmentId,
        name: String,
        input: serde_json::Value,
    ) -> Result<EnvironmentCallResult, brain::Error> {
        let session_id = session.id();
        let config = brain::session_config(store)?;
        let attachment = config
            .environments
            .iter()
            .find(|attachment| &attachment.environment_id == environment_id)
            .filter(|attachment| attachment.attached())
            .ok_or_else(|| {
                brain::Error::InvalidState("Environment is not attached to this session".into())
            })?;
        let entry = self.entry(environment_id)?;
        let request = EnvironmentRequest::Call { name, input };
        let sequence = session
            .record_call_started(codes::event::call::ENVIRONMENT_CALL, &request)
            .await?;
        let operation = EnvironmentOperation {
            sequence,
            environment_id: environment_id.clone(),
            session_id: session_id.clone(),
            attachment_id: attachment.attachment_id.clone(),
            request,
        };
        let sent = self.send(&entry, &operation).await;
        match sent.and_then(|receipt| terminal(receipt, "call")) {
            Ok(EnvironmentReceipt::Result { output }) => {
                session
                    .record_call_ended(codes::event::call::ENVIRONMENT_CALL, sequence, &output)
                    .await?;
                Ok(EnvironmentCallResult { output })
            }
            Ok(_) => {
                let error = brain::Error::Executor(
                    "Environment returned a nonterminal call receipt".into(),
                );
                session
                    .record_call_failed(codes::event::call::ENVIRONMENT_CALL, sequence, &error)
                    .await?;
                if matches!(error, brain::Error::Ambiguous(_)) {
                    session.record(codes::event::ENVIRONMENT_UNREACHABLE, serde_json::json!({"environment_id": environment_id, "sequence": sequence})).await?;
                }
                Err(error)
            }
            Err(error) => {
                session
                    .record_call_failed(codes::event::call::ENVIRONMENT_CALL, sequence, &error)
                    .await?;
                if matches!(error, brain::Error::Ambiguous(_)) {
                    session.record(codes::event::ENVIRONMENT_UNREACHABLE, serde_json::json!({"environment_id": environment_id, "sequence": sequence})).await?;
                }
                Err(error)
            }
        }
    }

    /// Detaches the session from every environment it was attached to. The environments
    /// stay until the provider expires them or the owner requests teardown.
    pub async fn release_session(
        &self,
        session: &Session,
        config: &SessionConfig,
        store: &dyn SessionStore,
    ) -> Result<(), brain::Error> {
        for attachment in config.environments.iter().rev() {
            if !attachment.attached() {
                continue;
            }
            if let Some(attempt) = operation_outcome(
                store,
                &attachment.environment_id,
                codes::event::call::ENVIRONMENT_DETACH,
            )? {
                if attempt.outcome.is_none() {
                    session
                        .record_call_failed(
                            codes::event::call::ENVIRONMENT_DETACH,
                            attempt.sequence,
                            &brain::Error::Ambiguous("detach result was not recorded".into()),
                        )
                        .await?;
                }
                continue;
            }
            let entry = self.entry(&attachment.environment_id)?;
            self.session_lifecycle(
                session,
                &entry,
                attachment.attachment_id.clone(),
                EnvironmentRequest::Detach,
                codes::event::call::ENVIRONMENT_DETACH,
            )
            .await?;
        }
        Ok(())
    }

    async fn session_lifecycle(
        &self,
        session: &Session,
        entry: &DirectoryEntry,
        attachment_id: Option<AttachmentId>,
        request: EnvironmentRequest,
        kind: &str,
    ) -> Result<(), brain::Error> {
        let sequence = session.record(&format!("{kind}_started"), serde_json::json!({"environment_id":entry.binding.environment_id,"request":request})).await?;
        let operation = EnvironmentOperation {
            sequence,
            environment_id: entry.binding.environment_id.clone(),
            session_id: session.id().clone(),
            attachment_id,
            request,
        };
        let sent = self.send(entry, &operation).await;
        match sent.and_then(|receipt| terminal(receipt, "lifecycle")) {
            Ok(receipt) => session.record_call_ended(kind, sequence, &receipt).await,
            Err(error) => {
                session.record_call_failed(kind, sequence, &error).await?;
                if matches!(error, brain::Error::Ambiguous(_)) {
                    session.record(codes::event::ENVIRONMENT_UNREACHABLE, serde_json::json!({"environment_id": operation.environment_id, "sequence": sequence})).await?;
                }
                Ok(())
            }
        }
    }

    async fn operation(
        &self,
        environment_id: &EnvironmentId,
        request: EnvironmentRequest,
        store: &dyn SessionStore,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let saved = store.append_sync(
            &[brain::AppendRecord::new(
                codes::event::ENVIRONMENT_TEARDOWN_STARTED,
                serde_json::json!({"environment_id": environment_id, "request": request}),
            )],
            brain::SessionUpdate::default(),
        )?;
        let sequence = saved[0].sequence;
        let entry = self.entry(environment_id)?;
        let operation = EnvironmentOperation {
            sequence,
            environment_id: environment_id.clone(),
            session_id: store.session_id().clone(),
            attachment_id: None,
            request,
        };
        let result = self
            .send(&entry, &operation)
            .await
            .and_then(|receipt| terminal(receipt, "lifecycle"));
        let (kind, payload) = match &result {
            Ok(receipt) => (
                codes::event::ENVIRONMENT_TEARDOWN_ENDED,
                serde_json::json!({"sequence": sequence, "result": receipt}),
            ),
            Err(error) => (
                codes::event::ENVIRONMENT_TEARDOWN_FAILED,
                serde_json::json!({"sequence": sequence, "code": error.code(), "message": error.to_string(), "ambiguous": matches!(error, brain::Error::Ambiguous(_))}),
            ),
        };
        store.append_sync(
            &[brain::AppendRecord::new(kind, payload)],
            brain::SessionUpdate::default(),
        )?;
        result
    }

    /// Sends one operation and keeps the environment's reachability current: a transport
    /// failure marks it unreachable and tells its sessions; the next answer clears it.
    async fn send(
        &self,
        entry: &DirectoryEntry,
        operation: &EnvironmentOperation,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        if let Some(record) = self.resources.get(&entry.binding.environment_id)?
            && let Some(resources) = brain_wasm_resources(&record.configuration)?
        {
            return match &operation.request {
                EnvironmentRequest::Call { .. } | EnvironmentRequest::Invoke { .. } => {
                    Err(brain::Error::InvalidState(
                        "the Brain Wasm Environment accepts Components through native execution"
                            .into(),
                    ))
                }
                _ => Ok(EnvironmentReceipt::Accepted { resources }),
            };
        }
        let sent = entry
            .adapter
            .send(&entry.endpoint, &entry.binding, operation)
            .await;
        let environment_id = &entry.binding.environment_id;
        match &sent {
            Err(brain::Error::Ambiguous(_)) => {
                self.resources
                    .set_status(environment_id, EnvironmentStatus::Unreachable)?;
            }
            Ok(_) => {
                self.resources
                    .set_status(environment_id, EnvironmentStatus::Open)?;
            }
            Err(_) => {}
        }
        sent
    }

    fn entry(&self, environment_id: &EnvironmentId) -> Result<DirectoryEntry, brain::Error> {
        let record = self
            .resources
            .get(environment_id)?
            .ok_or_else(|| brain::Error::NotFound("Environment does not exist".into()))?;
        let route = record
            .configuration
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .and_then(|driver| self.routes.get(driver));
        let (endpoint, adapter) = route
            .map_or((&self.endpoint, &self.adapter), |(endpoint, adapter)| {
                (endpoint, adapter)
            });
        Ok(DirectoryEntry {
            binding: EnvironmentBinding {
                environment_id: environment_id.clone(),
                directory_generation: 1,
            },
            endpoint: endpoint.clone(),
            adapter: adapter.clone(),
        })
    }
}

struct Attempt {
    sequence: u64,
    outcome: Option<bool>,
}

fn operation_outcome(
    store: &dyn SessionStore,
    environment_id: &EnvironmentId,
    kind: &str,
) -> Result<Option<Attempt>, brain::Error> {
    let mut after = 0;
    let mut attempt: Option<Attempt> = None;
    loop {
        let records = store.records_after(after, 1000)?;
        if records.is_empty() {
            return Ok(attempt);
        }
        for record in records {
            after = record.sequence;
            if record.kind == format!("{kind}_started")
                && record
                    .payload
                    .get("environment_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(environment_id.as_str())
            {
                attempt = Some(Attempt {
                    sequence: record.sequence,
                    outcome: None,
                });
            } else if let Some(attempt) = &mut attempt
                && record
                    .payload
                    .get("sequence")
                    .and_then(serde_json::Value::as_u64)
                    == Some(attempt.sequence)
            {
                if record.kind == format!("{kind}_ended") {
                    attempt.outcome = Some(true);
                }
                if record.kind == format!("{kind}_failed") {
                    attempt.outcome = Some(false);
                }
            }
        }
    }
}

fn brain_wasm_resources(
    configuration: &serde_json::Value,
) -> Result<Option<brain_protocol::Resources>, brain::Error> {
    let Some(object) = configuration.as_object() else {
        return Ok(None);
    };
    if object.get("driver").and_then(serde_json::Value::as_str) != Some("brain_wasm") {
        return Ok(None);
    }
    let allowed = ["driver", "network", "filesystem", "secrets"];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(brain::Error::InvalidState(
            "Brain Wasm Environment configuration has an unknown field".into(),
        ));
    }
    let mut resources = brain_protocol::Resources::new();
    let filesystem = object
        .get("filesystem")
        .map(|value| {
            let value = value.as_object().ok_or_else(|| {
                brain::Error::InvalidState("Brain Wasm filesystem must be an object".into())
            })?;
            if value
                .keys()
                .any(|key| key != "scratch" && key != "workspace")
            {
                return Err(brain::Error::InvalidState(
                    "Brain Wasm filesystem has an unknown field".into(),
                ));
            }
            Ok(value)
        })
        .transpose()?;
    let mut roots = Vec::new();
    for (name, root) in [("scratch", "/scratch"), ("workspace", "/workspace")] {
        let requested = filesystem
            .and_then(|value| value.get(name))
            .map(|value| {
                value.as_bool().ok_or_else(|| {
                    brain::Error::InvalidState(format!("Brain Wasm {name} must be boolean"))
                })
            })
            .transpose()?
            .unwrap_or(false);
        if requested {
            roots.push(root);
        }
    }
    if !roots.is_empty() {
        resources.insert("fs".into(), serde_json::json!({"roots": roots}));
    }
    let network = object
        .get("network")
        .map(|value| {
            let value = value.as_object().ok_or_else(|| {
                brain::Error::InvalidState("Brain Wasm network must be an object".into())
            })?;
            if value.keys().any(|key| key != "allow") {
                return Err(brain::Error::InvalidState(
                    "Brain Wasm network has an unknown field".into(),
                ));
            }
            Ok(value)
        })
        .transpose()?;
    let allow = network
        .and_then(|value| value.get("allow"))
        .map(|value| {
            value.as_array().cloned().ok_or_else(|| {
                brain::Error::InvalidState("Brain Wasm network allow must be an array".into())
            })
        })
        .transpose()?
        .unwrap_or_default();
    if allow.len() > 256
        || allow.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|value| !valid_network_target(value))
        })
    {
        return Err(brain::Error::InvalidState(
            "Brain Wasm network allow must contain HTTP(S) origins or authorities".into(),
        ));
    }
    resources.insert("net".into(), serde_json::json!({"allow": allow}));
    let secrets = object
        .get("secrets")
        .map(|value| {
            value.as_array().cloned().ok_or_else(|| {
                brain::Error::InvalidState("Brain Wasm secrets must be an array".into())
            })
        })
        .transpose()?
        .unwrap_or_default();
    if secrets.len() > 64
        || secrets.iter().any(|value| {
            value.as_str().is_none_or(|value| {
                value.is_empty()
                    || value.len() > 128
                    || !value.bytes().enumerate().all(|(index, byte)| {
                        byte.is_ascii_alphanumeric()
                            || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
                    })
            })
        })
    {
        return Err(brain::Error::InvalidState(
            "Brain Wasm secrets must be an array of names".into(),
        ));
    }
    if !secrets.is_empty() {
        resources.insert("secrets".into(), serde_json::json!({"names": secrets}));
    }
    Ok(Some(resources))
}

fn valid_network_target(value: &str) -> bool {
    let explicit = value.contains("://");
    let candidate = if explicit {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let Ok(url) = url::Url::parse(&candidate) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host().is_some()
}

/// A receipt that ended the operation, or the error it ended with. A failure receipt is
/// the environment saying the effect failed; an ambiguous one says it does not know; a
/// progress receipt where a terminal one was owed is a broken environment.
fn terminal(receipt: EnvironmentReceipt, what: &str) -> Result<EnvironmentReceipt, brain::Error> {
    match receipt {
        EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
        EnvironmentReceipt::Failure { message, .. } => Err(brain::Error::Executor(message)),
        EnvironmentReceipt::Progress { .. } => Err(brain::Error::Executor(format!(
            "Environment returned progress without a terminal {what} receipt"
        ))),
        receipt => Ok(receipt),
    }
}

/// The provisioned-tool artifacts to hand this environment at attach: every bound tool
/// with a payload, its manifest rebuilt from the model-facing definition plus the
/// binding — the two halves the create request split apart.
fn provisions_for(config: &SessionConfig, environment_id: &EnvironmentId) -> Vec<Provision> {
    config
        .tool_bindings
        .iter()
        .filter(|tool| tool.environment_id.as_ref() == Some(environment_id))
        .filter_map(|tool| {
            let implementation = tool.implementation.clone()?;
            let definition = config
                .tools
                .iter()
                .find(|definition| definition.name == tool.name)?;
            Some(Provision {
                manifest: ToolManifest {
                    name: tool.name.clone(),
                    description: definition.description.clone(),
                    input_schema: definition.input_schema.clone(),
                    output_schema: definition.output_schema.clone(),
                    needs: tool.needs.clone(),
                    binding_names: tool.binding_names.clone(),
                    implementation,
                },
            })
        })
        .collect()
}

fn receipt_declaration(receipt: &EnvironmentReceipt) -> brain_protocol::Resources {
    match receipt {
        EnvironmentReceipt::Accepted { resources } => resources.clone(),
        _ => Default::default(),
    }
}

/// The journal view of a request whose wire form carries plaintext binding values:
/// the same request with every value struck out. `None` when the request carries
/// nothing the journal must not hold.
fn redacted(request: &EnvironmentRequest) -> Option<EnvironmentRequest> {
    let EnvironmentRequest::Attach {
        provisions,
        bindings,
    } = request
    else {
        return None;
    };
    Some(EnvironmentRequest::Attach {
        provisions: provisions.clone(),
        bindings: bindings
            .keys()
            .map(|name| (name.clone(), "<redacted>".to_owned()))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::brain_wasm_resources;

    #[tokio::test]
    async fn teardown_is_journaled_and_failure_is_retained_without_automatic_retry() {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Adapter {
            store: Arc<brain::LocalSessionStore>,
            calls: AtomicUsize,
        }
        #[async_trait::async_trait]
        impl EnvironmentAdapter for Adapter {
            async fn send(
                &self,
                _: &str,
                _: &EnvironmentBinding,
                operation: &EnvironmentOperation,
            ) -> Result<EnvironmentReceipt, brain::Error> {
                assert_eq!(&operation.session_id, self.store.session_id());
                let records = self.store.records_after(0, 100).unwrap();
                assert!(
                    records
                        .iter()
                        .any(|record| record.sequence == operation.sequence
                            && record.kind == codes::event::ENVIRONMENT_TEARDOWN_STARTED)
                );
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(brain::Error::Ambiguous("connection lost".into()))
                } else {
                    Ok(EnvironmentReceipt::Accepted {
                        resources: Default::default(),
                    })
                }
            }
        }
        let path = std::env::temp_dir().join(format!("brain-teardown-{}", rand::random::<u64>()));
        let (telemetry, _) = brain_telemetry::telemetry_channel();
        let store = brain::LocalSessionStore::create(
            &path.join("session"),
            brain_protocol::SessionId::new("ses_owner"),
            &serde_json::json!({}),
            brain::Writer::spawn(),
            Arc::new(brain::Feed::new(telemetry)),
        )
        .unwrap();
        let resources = Arc::new(EnvironmentResources::open(&path.join("environments")).unwrap());
        let id = EnvironmentId::new("env_owned");
        resources
            .create(EnvironmentRecord {
                session_id: store.session_id().clone(),
                environment_id: id.clone(),
                configuration: serde_json::json!({}),

                created_at_ms: 0,
                resources: Default::default(),
            })
            .unwrap();
        let adapter = Arc::new(Adapter {
            store: store.clone(),
            calls: AtomicUsize::new(0),
        });
        let registry =
            EnvironmentRegistry::new(resources.clone(), "http://environment", adapter.clone());
        assert!(registry.close(&id, &*store, false).await.is_err());
        assert!(resources.get(&id).unwrap().is_some());
        assert!(registry.close(&id, &*store, false).await.is_err());
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
        assert!(
            store
                .records_after(0, 100)
                .unwrap()
                .iter()
                .any(
                    |record| record.kind == codes::event::ENVIRONMENT_TEARDOWN_FAILED
                        && record.payload["ambiguous"] == true
                )
        );
        registry.close(&id, &*store, true).await.unwrap();
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
        assert!(resources.get(&id).unwrap().is_none());
    }

    #[tokio::test]
    async fn deployment_routes_keep_endpoints_and_adapters_separate() {
        use super::*;
        struct Route(&'static str);
        #[async_trait::async_trait]
        impl EnvironmentAdapter for Route {
            async fn send(
                &self,
                endpoint: &str,
                _: &EnvironmentBinding,
                _: &EnvironmentOperation,
            ) -> Result<EnvironmentReceipt, brain::Error> {
                assert_eq!(endpoint, self.0);
                Ok(EnvironmentReceipt::Result {
                    output: serde_json::json!(endpoint),
                })
            }
        }
        let path = std::env::temp_dir().join(format!("brain-routes-{}", rand::random::<u64>()));
        let resources = Arc::new(EnvironmentResources::open(&path).unwrap());
        let mut registry =
            EnvironmentRegistry::new(resources.clone(), "gateway", Arc::new(Route("gateway")));
        for driver in ["first", "second"] {
            registry = registry
                .with_route(driver.into(), driver.into(), Arc::new(Route(driver)))
                .unwrap();
            resources
                .create(EnvironmentRecord {
                    session_id: brain_protocol::SessionId::new("ses_routes"),
                    environment_id: EnvironmentId::new(driver),
                    configuration: serde_json::json!({"driver": driver, "endpoint": "ungranted"}),
                    created_at_ms: 0,
                    resources: Default::default(),
                })
                .unwrap();
        }
        for driver in ["first", "second"] {
            let entry = registry.entry(&EnvironmentId::new(driver)).unwrap();
            let operation = EnvironmentOperation {
                sequence: 1,
                environment_id: entry.binding.environment_id.clone(),
                session_id: brain_protocol::SessionId::new("ses_routes"),
                attachment_id: None,
                request: EnvironmentRequest::Call {
                    name: "status".into(),
                    input: serde_json::json!({}),
                },
            };
            let receipt = registry.execute(&entry.binding, &operation).await.unwrap();
            assert!(matches!(receipt, EnvironmentReceipt::Result { output } if output == driver));
        }
        assert!(
            registry
                .with_route("first".into(), "changed".into(), Arc::new(Route("changed")))
                .is_err()
        );
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn brain_wasm_filesystem_roots_are_explicit() {
        let defaults = brain_wasm_resources(&serde_json::json!({"driver": "brain_wasm"}))
            .unwrap()
            .unwrap();
        assert!(!defaults.contains_key("fs"));

        let requested = brain_wasm_resources(&serde_json::json!({
            "driver": "brain_wasm",
            "filesystem": {"scratch": true, "workspace": true}
        }))
        .unwrap()
        .unwrap();
        assert_eq!(
            requested["fs"],
            serde_json::json!({"roots": ["/scratch", "/workspace"]})
        );
    }
}
