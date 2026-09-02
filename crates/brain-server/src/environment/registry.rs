use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use brain::{CreatingSession, Kernel, KernelError, SessionHandle};
use brain_protocol::{
    AttachmentId, EnvironmentAttachment, EnvironmentCallResult, EnvironmentId,
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, Identity, Provision,
    ResolvedSessionRequest, SealedSessionConfig, SessionId, ToolBinding, ToolManifest,
};

use super::{DirectoryEntry, EnvironmentAdapter, EnvironmentDirectory};

/// Plaintext binding values per environment, carried beside the resolved request for
/// exactly as long as create runs. They go out on the attach wire and are journaled
/// only as identities.
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
        request: ResolvedSessionRequest,
        binding_values: SessionBindingValues,
    ) -> Result<(SessionHandle, SealedSessionConfig), KernelError> {
        match self.prepare(&mut creation, request, binding_values).await {
            Ok(sealed) => {
                let handle = creation.complete(sealed.clone())?;
                Ok((handle, sealed))
            }
            Err(error) => {
                let message = error.to_string();
                creation.fail("environment_preparation_failed", &message)?;
                Err(error)
            }
        }
    }

    async fn prepare(
        &self,
        creation: &mut CreatingSession,
        request: ResolvedSessionRequest,
        mut binding_values: SessionBindingValues,
    ) -> Result<SealedSessionConfig, KernelError> {
        let mut environments = Vec::with_capacity(request.environments.len());
        let mut attachments = std::collections::HashMap::new();
        for requirement in &request.environments {
            let entry = self.directory.resolve(requirement).await?;
            let setup = self
                .lifecycle(
                    creation,
                    &entry,
                    EnvironmentRequest::Setup {
                        configuration: requirement.configuration.clone(),
                    },
                    None,
                    "environment_setup",
                )
                .await?;
            let attachment_id = attachment_id(creation.session_id(), &requirement.environment_id)?;
            let bindings = binding_values
                .remove(&requirement.environment_id)
                .unwrap_or_default();
            let attach = EnvironmentRequest::Attach {
                provisions: provisions_for(&request, &requirement.environment_id),
                bindings,
            };
            let attached = self
                .lifecycle(
                    creation,
                    &entry,
                    attach,
                    Some(attachment_id.clone()),
                    "environment_attach",
                )
                .await?;
            // What the environment declares it executes and offers feeds the sealed
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
            attachments.insert(requirement.environment_id.clone(), attachment_id.clone());
            environments.push(EnvironmentAttachment {
                binding: entry.binding,
                attachment_id,
                runtimes,
                resources,
            });
        }
        let tool_bindings = request
            .tool_bindings
            .into_iter()
            .map(|tool| {
                // A client-hosted tool binds no environment: it is served by the
                // session's creator off the event feed.
                let Some(environment_id) = tool.environment_id else {
                    return Ok(ToolBinding {
                        name: tool.name,
                        environment: None,
                        attachment_id: None,
                        needs: tool.needs,
                        binding_names: tool.binding_names,
                        hosting: tool.hosting,
                        program: tool.program,
                    });
                };
                let attachment_id = attachments.get(&environment_id).cloned().ok_or_else(|| {
                    KernelError::InvalidState("Tool requested an unresolved Environment".into())
                })?;
                Ok(ToolBinding {
                    name: tool.name,
                    environment: environments
                        .iter()
                        .find(|environment| environment.binding.environment_id == environment_id)
                        .map(|environment| Some(environment.binding.clone()))
                        .ok_or_else(|| {
                            KernelError::InvalidState(
                                "Tool requested an unresolved Environment".into(),
                            )
                        })?,
                    attachment_id: Some(attachment_id),
                    needs: tool.needs,
                    binding_names: tool.binding_names,
                    hosting: tool.hosting,
                    program: tool.program,
                })
            })
            .collect::<Result<Vec<_>, KernelError>>()?;
        Ok(SealedSessionConfig {
            agentloop_identity: request.agentloop_identity,
            brain_configuration: request.brain_configuration,
            model: request.model,
            presentation: request.presentation,
            environments,
            tool_bindings,
        })
    }

    async fn lifecycle(
        &self,
        creation: &mut CreatingSession,
        entry: &DirectoryEntry,
        request: EnvironmentRequest,
        attachment_id: Option<AttachmentId>,
        kind: &str,
    ) -> Result<EnvironmentReceipt, KernelError> {
        // An attach carries binding values in plaintext, and plaintext never enters
        // the journal: the recorded intent replaces each value with its identity.
        let (operation_id, request_identity) = match redacted(&request)? {
            Some(journal_view) => creation.record_intent_redacted(kind, &request, &journal_view)?,
            None => creation.record_intent(kind, &request)?,
        };
        let operation = EnvironmentOperation {
            operation_id: operation_id.clone(),
            request_identity,
            environment_id: entry.binding.environment_id.clone(),
            session_id: creation.session_id().clone(),
            attachment_id,
            request,
        };
        let receipt = self
            .adapter
            .send(&entry.endpoint, &entry.binding, &operation)
            .await?;
        match &receipt {
            EnvironmentReceipt::Conflict { .. } => {
                return Err(KernelError::InvalidState(
                    "Environment rejected a lifecycle digest conflict".into(),
                ));
            }
            EnvironmentReceipt::Ambiguous { message } => {
                return Err(KernelError::Ambiguous(message.clone()));
            }
            EnvironmentReceipt::Failure { message, .. } => {
                return Err(KernelError::Executor(message.clone()));
            }
            EnvironmentReceipt::Progress { .. } => {
                return Err(KernelError::Executor(
                    "Environment returned progress without a terminal lifecycle receipt".into(),
                ));
            }
            EnvironmentReceipt::Accepted { .. }
            | EnvironmentReceipt::Result { .. }
            | EnvironmentReceipt::Outcome { .. } => {}
        }
        creation.record_result(kind, &operation_id, &receipt)?;
        Ok(receipt)
    }

    pub async fn execute(
        &self,
        binding: &brain_protocol::EnvironmentBinding,
        operation: &EnvironmentOperation<EnvironmentRequest>,
    ) -> Result<EnvironmentReceipt, KernelError> {
        let entry = self.directory.get(binding).await?;
        self.adapter
            .send(&entry.endpoint, &entry.binding, operation)
            .await
    }

    pub async fn call(
        &self,
        kernel: &Kernel,
        session_id: &SessionId,
        environment_id: &EnvironmentId,
        name: String,
        input: serde_json::Value,
    ) -> Result<EnvironmentCallResult, KernelError> {
        let sealed = kernel.sealed_config(session_id)?;
        let attachment = sealed
            .environments
            .iter()
            .find(|attachment| &attachment.binding.environment_id == environment_id)
            .ok_or_else(|| {
                KernelError::InvalidState("Environment is not attached to this session".into())
            })?;
        let entry = self.directory.get(&attachment.binding).await?;
        let request = EnvironmentRequest::Call { name, input };
        let (operation_id, request_identity) =
            kernel.record_external_intent(session_id, "environment_call", &request)?;
        let operation = EnvironmentOperation {
            operation_id: operation_id.clone(),
            request_identity,
            environment_id: environment_id.clone(),
            session_id: session_id.clone(),
            attachment_id: Some(attachment.attachment_id.clone()),
            request,
        };
        let receipt = self
            .adapter
            .send(&entry.endpoint, &entry.binding, &operation)
            .await?;
        kernel.record_external_result(session_id, "environment_call", &operation_id, &receipt)?;
        match receipt {
            EnvironmentReceipt::Result { output } => Ok(EnvironmentCallResult { output }),
            EnvironmentReceipt::Failure { message, .. } => Err(KernelError::Executor(message)),
            EnvironmentReceipt::Ambiguous { message } => Err(KernelError::Ambiguous(message)),
            EnvironmentReceipt::Conflict { .. } => Err(KernelError::InvalidState(
                "Environment rejected a call digest conflict".into(),
            )),
            _ => Err(KernelError::Executor(
                "Environment returned a nonterminal call receipt".into(),
            )),
        }
    }

    pub async fn release_session(
        &self,
        kernel: &Kernel,
        session_id: &brain_protocol::SessionId,
        sealed: &SealedSessionConfig,
    ) -> Result<(), KernelError> {
        for attachment in sealed.environments.iter().rev() {
            let entry = self.directory.get(&attachment.binding).await?;
            self.session_lifecycle(
                kernel,
                session_id,
                &entry,
                Some(attachment.attachment_id.clone()),
                EnvironmentRequest::Detach,
                "environment_detach",
            )
            .await?;
            if matches!(
                attachment.binding.lifecycle_policy,
                brain_protocol::LifecyclePolicy::Session
            ) {
                self.session_lifecycle(
                    kernel,
                    session_id,
                    &entry,
                    None,
                    EnvironmentRequest::Teardown,
                    "environment_teardown",
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn session_lifecycle(
        &self,
        kernel: &Kernel,
        session_id: &brain_protocol::SessionId,
        entry: &DirectoryEntry,
        attachment_id: Option<AttachmentId>,
        request: EnvironmentRequest,
        kind: &str,
    ) -> Result<(), KernelError> {
        let (operation_id, digest) = kernel.record_external_intent(session_id, kind, &request)?;
        let operation = EnvironmentOperation {
            operation_id: operation_id.clone(),
            request_identity: digest,
            environment_id: entry.binding.environment_id.clone(),
            session_id: session_id.clone(),
            attachment_id,
            request,
        };
        let receipt = self
            .adapter
            .send(&entry.endpoint, &entry.binding, &operation)
            .await?;
        match &receipt {
            EnvironmentReceipt::Accepted { .. } | EnvironmentReceipt::Result { .. } => {}
            EnvironmentReceipt::Conflict { .. } => {
                return Err(KernelError::InvalidState(
                    "Environment rejected a lifecycle digest conflict".into(),
                ));
            }
            EnvironmentReceipt::Ambiguous { message } => {
                return Err(KernelError::Ambiguous(message.clone()));
            }
            EnvironmentReceipt::Failure { message, .. } => {
                return Err(KernelError::Executor(message.clone()));
            }
            _ => {
                return Err(KernelError::Executor(
                    "Environment returned a nonterminal lifecycle receipt".into(),
                ));
            }
        }
        kernel.record_external_result(session_id, kind, &operation_id, &receipt)
    }
}

/// The provisioned-tool artifacts to hand this environment at attach: every bound tool
/// with a payload, its manifest rebuilt from the model-facing definition plus the
/// binding — the two halves the create request split apart.
fn provisions_for(
    request: &ResolvedSessionRequest,
    environment_id: &EnvironmentId,
) -> Vec<Provision> {
    request
        .tool_bindings
        .iter()
        .filter(|tool| tool.environment_id.as_ref() == Some(environment_id))
        .filter_map(|tool| {
            let program = tool.program.clone()?;
            let definition = request
                .presentation
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
/// the same request with each value replaced by its identity. `None` when the request
/// carries nothing the journal must not hold.
fn redacted(request: &EnvironmentRequest) -> Result<Option<EnvironmentRequest>, KernelError> {
    let EnvironmentRequest::Attach {
        provisions,
        bindings,
    } = request
    else {
        return Ok(None);
    };
    let mut identities = BTreeMap::new();
    for (name, value) in bindings {
        let identity =
            Identity::of(value).map_err(|error| KernelError::InvalidState(error.to_string()))?;
        identities.insert(name.clone(), identity.to_string());
    }
    Ok(Some(EnvironmentRequest::Attach {
        provisions: provisions.clone(),
        bindings: identities,
    }))
}

fn attachment_id(
    session_id: &brain_protocol::SessionId,
    environment_id: &brain_protocol::EnvironmentId,
) -> Result<AttachmentId, KernelError> {
    let identity = Identity::of(&(session_id, environment_id))
        .map_err(|error| KernelError::InvalidState(error.to_string()))?;
    Ok(AttachmentId::new(format!(
        "att_{}",
        &identity.to_string()[..24]
    )))
}
