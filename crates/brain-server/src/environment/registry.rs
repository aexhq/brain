use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use brain::{CreatingSession, JournalStore, Session};
use brain_protocol::codes;
use brain_protocol::{
    AttachmentId, EnvironmentCallResult, EnvironmentId, EnvironmentOperation, EnvironmentReceipt,
    EnvironmentRequest, Provision, SessionConfig, ToolManifest,
};

use super::{DirectoryEntry, EnvironmentAdapter, EnvironmentDirectory};

/// Plaintext binding values per environment, carried beside the configuration for
/// exactly as long as create runs. They go out on the attach wire and never enter the
/// journal.
pub type SessionBindingValues = HashMap<EnvironmentId, BTreeMap<String, String>>;

pub struct EnvironmentRegistry {
    directory: Arc<dyn EnvironmentDirectory>,
    adapter: Arc<dyn EnvironmentAdapter>,
}

impl EnvironmentRegistry {
    pub fn new(
        directory: Arc<dyn EnvironmentDirectory>,
        adapter: Arc<dyn EnvironmentAdapter>,
    ) -> Self {
        Self { directory, adapter }
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

    /// Attaches every environment the configuration names and fills in what each one
    /// answered with, so the configuration the session is admitted with says what was
    /// actually granted.
    async fn prepare(
        &self,
        creation: &mut CreatingSession,
        mut config: SessionConfig,
        mut binding_values: SessionBindingValues,
    ) -> Result<SessionConfig, brain::Error> {
        let provisions: Vec<(EnvironmentId, Vec<Provision>)> = config
            .environments
            .iter()
            .map(|attachment| {
                (
                    attachment.environment_id.clone(),
                    provisions_for(&config, &attachment.environment_id),
                )
            })
            .collect();
        for (attachment, (_, provisions)) in config.environments.iter_mut().zip(provisions) {
            let entry = self.directory.resolve(attachment).await?;
            let setup = self
                .lifecycle(
                    creation,
                    &entry,
                    EnvironmentRequest::Setup {
                        configuration: attachment.configuration.clone(),
                    },
                    None,
                    codes::event::call::ENVIRONMENT_SETUP,
                )
                .await?;
            let attachment_id = AttachmentId::new(brain::random_id("att"));
            let bindings = binding_values
                .remove(&attachment.environment_id)
                .unwrap_or_default();
            let attach = EnvironmentRequest::Attach {
                provisions,
                bindings,
            };
            let attached = self
                .lifecycle(
                    creation,
                    &entry,
                    attach,
                    Some(attachment_id.clone()),
                    codes::event::call::ENVIRONMENT_ATTACH,
                )
                .await?;
            // What the environment declares it executes and offers feeds the
            // configuration's bind check; setup and attach both may report it, and a
            // resource attach declares again replaces the setup's block.
            let (mut runtimes, mut resources) = receipt_declaration(&setup);
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
        let sent = self
            .adapter
            .send(&entry.endpoint, &entry.binding, &operation)
            .await;
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

    pub async fn execute(
        &self,
        binding: &brain_protocol::EnvironmentBinding,
        operation: &EnvironmentOperation<EnvironmentRequest>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let entry = self.directory.get(binding).await?;
        self.adapter
            .send(&entry.endpoint, &entry.binding, operation)
            .await
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
        let config = brain::session_config(store, session_id)?;
        let attachment = config
            .environments
            .iter()
            .find(|attachment| &attachment.environment_id == environment_id)
            .and_then(|attachment| {
                attachment
                    .binding
                    .as_ref()
                    .map(|binding| (attachment, binding))
            })
            .ok_or_else(|| {
                brain::Error::InvalidState("Environment is not attached to this session".into())
            })?;
        let (attachment, binding) = attachment;
        let entry = self.directory.get(binding).await?;
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
        let sent = self
            .adapter
            .send(&entry.endpoint, &entry.binding, &operation)
            .await;
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

    pub async fn release_session(
        &self,
        session: &Session,
        config: &SessionConfig,
    ) -> Result<(), brain::Error> {
        for attachment in config.environments.iter().rev() {
            let Some(binding) = &attachment.binding else {
                continue;
            };
            let entry = self.directory.get(binding).await?;
            self.session_lifecycle(
                session,
                &entry,
                attachment.attachment_id.clone(),
                EnvironmentRequest::Detach,
                codes::event::call::ENVIRONMENT_DETACH,
            )
            .await?;
            if matches!(
                attachment.lifecycle_policy,
                brain_protocol::LifecyclePolicy::Session
            ) {
                self.session_lifecycle(
                    session,
                    &entry,
                    None,
                    EnvironmentRequest::Teardown,
                    codes::event::call::ENVIRONMENT_TEARDOWN,
                )
                .await?;
            }
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
        let sent = self
            .adapter
            .send(&entry.endpoint, &entry.binding, &operation)
            .await;
        match sent.and_then(|receipt| terminal(receipt, "lifecycle")) {
            Ok(receipt) => session.record_call_ended(kind, sequence, &receipt).await,
            Err(error) => {
                session.record_call_failed(kind, sequence, &error).await?;
                Err(error)
            }
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
