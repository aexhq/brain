//! Environments as resources: created once with their configuration, attached by id
//! from any session, detached when a session ends, and closed when deleted or, if
//! managed, when nothing has used them for their idle TTL.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

use brain::{CreatingSession, JournalStore, Session};
use brain_protocol::{
    AttachmentId, CreateEnvironmentRequest, EnvironmentBinding, EnvironmentCallResult,
    EnvironmentId, EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, EnvironmentStatus,
    EnvironmentSummary, Provision, SessionConfig, SessionId, ToolManifest, codes,
};
use tokio::sync::broadcast;

use super::{EnvironmentAdapter, EnvironmentRecord, EnvironmentResources, resources};

/// Plaintext binding values per environment, carried beside the configuration for
/// exactly as long as create runs. They go out on the attach wire and never enter the
/// journal.
pub type SessionBindingValues = HashMap<EnvironmentId, BTreeMap<String, String>>;

/// Where an environment is reached.
#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    pub binding: EnvironmentBinding,
    pub endpoint: String,
}

/// Something that happened to an environment that its attached sessions should hear.
#[derive(Clone, Debug)]
pub struct EnvironmentNotice {
    pub environment_id: EnvironmentId,
    pub kind: EnvironmentNoticeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentNoticeKind {
    Closed,
    Unreachable,
}

pub struct EnvironmentRegistry {
    resources: Arc<EnvironmentResources>,
    endpoint: String,
    adapter: Arc<dyn EnvironmentAdapter>,
    notices: broadcast::Sender<EnvironmentNotice>,
    default_idle_ttl: Duration,
}

impl EnvironmentRegistry {
    pub fn new(
        resources: Arc<EnvironmentResources>,
        endpoint: impl Into<String>,
        adapter: Arc<dyn EnvironmentAdapter>,
        default_idle_ttl: Duration,
    ) -> Self {
        Self {
            resources,
            endpoint: endpoint.into(),
            adapter,
            notices: broadcast::Sender::new(256),
            default_idle_ttl,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EnvironmentNotice> {
        self.notices.subscribe()
    }

    pub fn resources(&self) -> &Arc<EnvironmentResources> {
        &self.resources
    }

    /// Creates an environment: records it, runs `setup`, and keeps what the environment
    /// declared it executes and offers.
    pub async fn create(
        &self,
        request: CreateEnvironmentRequest,
    ) -> Result<EnvironmentRecord, brain::Error> {
        if self.endpoint.trim().is_empty() {
            return Err(brain::Error::InvalidState(
                "no Environment endpoint is configured".into(),
            ));
        }
        let environment_id = request
            .environment_id
            .unwrap_or_else(|| EnvironmentId::new(brain::random_id("env")));
        self.resources.create(EnvironmentRecord {
            environment_id: environment_id.clone(),
            configuration: request.configuration.clone(),
            managed: request.managed,
            idle_ttl_ms: request.idle_ttl_ms,
            created_at_ms: resources::wall_clock_ms(),
            runtimes: Vec::new(),
            resources: Default::default(),
            operations: 0,
        })?;
        let receipt = match self
            .operation(
                &environment_id,
                EnvironmentRequest::Setup {
                    configuration: request.configuration,
                },
            )
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = self.resources.remove(&environment_id);
                return Err(error);
            }
        };
        let (runtimes, declared) = receipt_declaration(&receipt);
        self.resources.update(&environment_id, |record| {
            record.runtimes = runtimes;
            record.resources = declared;
        })
    }

    pub fn summary(
        &self,
        environment_id: &EnvironmentId,
        attached_sessions: Vec<SessionId>,
    ) -> Result<Option<EnvironmentSummary>, brain::Error> {
        let Some(record) = self.resources.get(environment_id)? else {
            return Ok(None);
        };
        let status = self
            .resources
            .status(environment_id)?
            .unwrap_or(EnvironmentStatus::Open);
        Ok(Some(EnvironmentSummary {
            environment_id: record.environment_id,
            status,
            managed: record.managed,
            idle_ttl_ms: record.idle_ttl_ms,
            attached_sessions,
            runtimes: record.runtimes,
            resources: record.resources,
            created_at_ms: record.created_at_ms,
        }))
    }

    pub fn ids(&self) -> Result<Vec<EnvironmentId>, brain::Error> {
        self.resources.ids()
    }

    /// How long a managed environment may sit unattached before it is closed; `None`
    /// for an unmanaged one or one whose TTL is zero.
    pub fn idle_ttl(
        &self,
        environment_id: &EnvironmentId,
    ) -> Result<Option<Duration>, brain::Error> {
        let Some(record) = self.resources.get(environment_id)? else {
            return Ok(None);
        };
        if !record.managed {
            return Ok(None);
        }
        let ttl = record
            .idle_ttl_ms
            .map(Duration::from_millis)
            .unwrap_or(self.default_idle_ttl);
        Ok((!ttl.is_zero()).then_some(ttl))
    }

    pub fn idle_since(
        &self,
        environment_id: &EnvironmentId,
    ) -> Result<Option<std::time::Instant>, brain::Error> {
        self.resources.idle_since(environment_id)
    }

    /// Tears the environment down and forgets it. Attached sessions hear
    /// `environment_closed`.
    pub async fn close(&self, environment_id: &EnvironmentId) -> Result<(), brain::Error> {
        if self.resources.get(environment_id)?.is_none() {
            return Err(brain::Error::NotFound(format!(
                "Environment `{environment_id}` does not exist"
            )));
        }
        // Best effort: an environment that cannot be reached for its teardown is still
        // forgotten here, and the notice says which.
        if let Err(error) = self
            .operation(environment_id, EnvironmentRequest::Teardown)
            .await
        {
            tracing::warn!(%environment_id, %error, "Environment teardown failed; forgetting it anyway");
        }
        self.resources.remove(environment_id)?;
        let _ = self.notices.send(EnvironmentNotice {
            environment_id: environment_id.clone(),
            kind: EnvironmentNoticeKind::Closed,
        });
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
            let entry = self.entry(&attachment.environment_id);
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
            let (mut runtimes, mut resources) = (record.runtimes, record.resources);
            let (attach_runtimes, attach_resources) = receipt_declaration(&attached);
            for runtime in attach_runtimes {
                if !runtimes.contains(&runtime) {
                    runtimes.push(runtime);
                }
            }
            runtimes.sort_unstable();
            resources.extend(attach_resources);
            attachment.binding = Some(entry.binding);
            attachment.attachment_id = Some(attachment_id);
            attachment.runtimes = runtimes;
            attachment.resources = resources;
        }
        let SessionConfig {
            environments,
            tool_bindings,
            ..
        } = &mut config;
        for tool in tool_bindings.iter_mut() {
            // A client-hosted tool binds no environment: it is served by the
            // session's creator off the event feed.
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
                Err(error)
            }
        }
    }

    /// One operation on a session's behalf whose record the session already holds: a
    /// tool invoke or cancel.
    pub async fn execute(
        &self,
        binding: &EnvironmentBinding,
        operation: &EnvironmentOperation<EnvironmentRequest>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let entry = self.entry(&binding.environment_id);
        self.send(&entry, operation).await
    }

    pub async fn call(
        &self,
        session: &Session,
        store: &dyn JournalStore,
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
        let entry = self.entry(environment_id);
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
                Err(error)
            }
            Err(error) => {
                session
                    .record_call_failed(codes::event::call::ENVIRONMENT_CALL, sequence, &error)
                    .await?;
                Err(error)
            }
        }
    }

    /// Detaches the session from every environment it was attached to. The environments
    /// stay: closing one is the owner's or the idle sweeper's decision.
    pub async fn release_session(
        &self,
        session: &Session,
        config: &SessionConfig,
    ) -> Result<(), brain::Error> {
        for attachment in config.environments.iter().rev() {
            if !attachment.attached() {
                continue;
            }
            let entry = self.entry(&attachment.environment_id);
            self.session_lifecycle(
                session,
                &entry,
                attachment.attachment_id.clone(),
                EnvironmentRequest::Detach,
                codes::event::call::ENVIRONMENT_DETACH,
            )
            .await?;
            self.resources.touch_idle(&attachment.environment_id)?;
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
        let sequence = session.record_call_started(kind, &request).await?;
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
                Err(error)
            }
        }
    }

    /// An operation on the environment's own behalf: setup at create, teardown at close.
    /// Numbered by the environment's own counter and addressed to it, since no session
    /// is involved.
    async fn operation(
        &self,
        environment_id: &EnvironmentId,
        request: EnvironmentRequest,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let sequence = self.resources.next_operation(environment_id)?;
        let entry = self.entry(environment_id);
        let operation = EnvironmentOperation {
            sequence,
            environment_id: environment_id.clone(),
            session_id: SessionId::new(environment_id.as_str()),
            attachment_id: None,
            request,
        };
        self.send(&entry, &operation)
            .await
            .and_then(|receipt| terminal(receipt, "lifecycle"))
    }

    /// Sends one operation and keeps the environment's reachability current: a transport
    /// failure marks it unreachable and tells its sessions; the next answer clears it.
    async fn send(
        &self,
        entry: &DirectoryEntry,
        operation: &EnvironmentOperation<EnvironmentRequest>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let sent = self
            .adapter
            .send(&entry.endpoint, &entry.binding, operation)
            .await;
        let environment_id = &entry.binding.environment_id;
        match &sent {
            Err(brain::Error::Ambiguous(_)) => {
                if self
                    .resources
                    .set_status(environment_id, EnvironmentStatus::Unreachable)?
                {
                    let _ = self.notices.send(EnvironmentNotice {
                        environment_id: environment_id.clone(),
                        kind: EnvironmentNoticeKind::Unreachable,
                    });
                }
            }
            Ok(_) => {
                self.resources
                    .set_status(environment_id, EnvironmentStatus::Open)?;
            }
            Err(_) => {}
        }
        sent
    }

    fn entry(&self, environment_id: &EnvironmentId) -> DirectoryEntry {
        DirectoryEntry {
            binding: EnvironmentBinding {
                environment_id: environment_id.clone(),
                directory_generation: 1,
            },
            endpoint: self.endpoint.clone(),
        }
    }
}

/// A receipt that ended the operation, or the error it ended with. A failure receipt is
/// the environment saying the effect failed; an ambiguous one says it does not know; a
/// progress receipt where a terminal one was owed is a broken environment.
fn terminal(receipt: EnvironmentReceipt, what: &str) -> Result<EnvironmentReceipt, brain::Error> {
    match receipt {
        EnvironmentReceipt::Ambiguous { message } => Err(brain::Error::Ambiguous(message)),
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
            let program = tool.program.clone()?;
            let definition = config
                .tools
                .iter()
                .find(|definition| definition.name == tool.name)?;
            Some(Provision {
                payload_identity: *program.identity(),
                manifest: ToolManifest {
                    name: tool.name.clone(),
                    description: definition.description.clone(),
                    input_schema: definition.input_schema.clone(),
                    output_schema: definition.output_schema.clone(),
                    needs: tool.needs.clone(),
                    binding_names: tool.binding_names.clone(),
                    program,
                },
            })
        })
        .collect()
}

fn receipt_declaration(
    receipt: &EnvironmentReceipt,
) -> (Vec<brain_protocol::Runtime>, brain_protocol::Resources) {
    match receipt {
        EnvironmentReceipt::Accepted {
            runtimes,
            resources,
        } => (runtimes.clone(), resources.clone()),
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
