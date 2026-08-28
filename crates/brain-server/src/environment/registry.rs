use std::sync::Arc;

use brain::{CreatingSession, Kernel, KernelError, SessionHandle};
use brain_protocol::{
    AttachmentId, EnvironmentAttachment, EnvironmentCallResult, EnvironmentId,
    EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, Identity, ResolvedSessionRequest,
    SealedSessionConfig, SessionId, ToolBinding,
};

use super::{DirectoryEntry, EnvironmentAdapter, EnvironmentDirectory};

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
    ) -> Result<(SessionHandle, SealedSessionConfig), KernelError> {
        match self.prepare(&mut creation, request).await {
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
    ) -> Result<SealedSessionConfig, KernelError> {
        let mut environments = Vec::with_capacity(request.environments.len());
        let mut attachments = std::collections::HashMap::new();
        for requirement in &request.environments {
            let entry = self.directory.resolve(requirement).await?;
            self.lifecycle(
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
            self.lifecycle(
                creation,
                &entry,
                EnvironmentRequest::Attach {
                    grants: serde_json::json!({}),
                },
                Some(attachment_id.clone()),
                "environment_attach",
            )
            .await?;
            attachments.insert(requirement.environment_id.clone(), attachment_id.clone());
            environments.push(EnvironmentAttachment {
                binding: entry.binding,
                attachment_id,
            });
        }
        let tool_bindings = request
            .tool_bindings
            .into_iter()
            .map(|tool| {
                let attachment_id =
                    attachments
                        .get(&tool.environment_id)
                        .cloned()
                        .ok_or_else(|| {
                            KernelError::InvalidState(
                                "Tool requested an unresolved Environment".into(),
                            )
                        })?;
                Ok(ToolBinding {
                    name: tool.name,
                    environment: environments
                        .iter()
                        .find(|environment| {
                            environment.binding.environment_id == tool.environment_id
                        })
                        .map(|environment| environment.binding.clone())
                        .ok_or_else(|| {
                            KernelError::InvalidState(
                                "Tool requested an unresolved Environment".into(),
                            )
                        })?,
                    attachment_id,
                    remote_tool_id: tool.remote_tool_id,
                    tool_configuration: tool.tool_configuration,
                    grant: tool.grant,
                })
            })
            .collect::<Result<Vec<_>, KernelError>>()?;
        Ok(SealedSessionConfig {
            agentloop_digest: request.agentloop_digest,
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
        let (operation_id, request_digest) = creation.record_intent(kind, &request)?;
        let operation = EnvironmentOperation {
            operation_id: operation_id.clone(),
            request_digest,
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
            EnvironmentReceipt::Accepted
            | EnvironmentReceipt::Result { .. }
            | EnvironmentReceipt::ToolResult { .. } => {}
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
        let (operation_id, request_digest) =
            kernel.record_external_intent(session_id, "environment_call", &request)?;
        let operation = EnvironmentOperation {
            operation_id: operation_id.clone(),
            request_digest,
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
            request_digest: digest,
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
            EnvironmentReceipt::Accepted | EnvironmentReceipt::Result { .. } => {}
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
