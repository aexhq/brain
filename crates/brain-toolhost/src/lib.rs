use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use base64::Engine as _;
use brain::adapter::CallOutcome;
use brain::journal::ToolSelectorDoc;
use brain::tools::{
    ComponentToolAdmission, ComponentToolRequest, TOOL_WORLD, ToolCapabilityFailure,
    ToolCapabilityHandler, ToolRegistry,
};
use brain::{BrainError, Result};
use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter, ComponentSource,
    WorkerPool, WorkerRequest, component_digest, tool,
};
use brain_protocol::environment::TerminalOutcome;
use serde_json::Value;

static STAGING_NONCE: AtomicU64 = AtomicU64::new(1);

pub struct ComponentToolRegistry {
    store_dir: PathBuf,
    pool: Arc<WorkerPool>,
    router: Arc<CapabilityRouter>,
}

impl ComponentToolRegistry {
    pub fn new(
        store_dir: impl Into<PathBuf>,
        pool: Arc<WorkerPool>,
        router: Arc<CapabilityRouter>,
    ) -> std::io::Result<Self> {
        let store_dir = store_dir.into();
        std::fs::create_dir_all(&store_dir)?;
        Ok(Self {
            store_dir,
            pool,
            router,
        })
    }

    fn path(&self, digest: &str) -> PathBuf {
        self.store_dir.join(format!("{digest}.wasm"))
    }

    /// Immutable Environment bundle custody, content-addressed beside the component that names it.
    /// Only the digest is sealed, so this is the one place the executed bytes can come from.
    fn bundle_path(&self, digest: &str) -> PathBuf {
        self.store_dir.join(format!("{digest}.bundle"))
    }

    fn store(&self, target: PathBuf, digest: &str, bytes: &[u8]) -> Result<()> {
        if target.exists() {
            let existing = std::fs::read(&target)
                .map_err(|error| BrainError::Protocol(format!("Tool store read: {error}")))?;
            if component_digest(&existing) != digest {
                return Err(BrainError::Protocol(format!(
                    "Tool store entry {digest} has different bytes"
                )));
            }
            return Ok(());
        }
        let staged = target.with_extension(format!(
            "staging-{}-{}",
            std::process::id(),
            STAGING_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&staged, bytes)
            .and_then(|()| std::fs::rename(&staged, &target))
            .map_err(|error| BrainError::Protocol(format!("Tool store write: {error}")))
    }
}

#[async_trait]
impl ToolRegistry for ComponentToolRegistry {
    fn admit(&self, request: ComponentToolAdmission<'_>) -> Result<ToolSelectorDoc> {
        let ComponentToolAdmission {
            component_digest,
            world,
            component,
            config,
            grants,
            environment,
            bundle,
        } = request;
        if world != TOOL_WORLD {
            return Err(BrainError::Invalid(format!(
                "Tool world {world:?} is not supported; expected {TOOL_WORLD:?}"
            )));
        }
        let allowed = HashSet::from(["environment", "journal", "storage", "children", "parent"]);
        if grants.iter().any(|grant| !allowed.contains(grant.as_str())) {
            return Err(BrainError::Invalid("Tool declares an unknown grant".into()));
        }
        let has_environment = grants.iter().any(|grant| grant == "environment");
        if has_environment != environment.is_some() {
            return Err(BrainError::Invalid(
                "Tool must bind exactly one Environment when the environment grant is present"
                    .into(),
            ));
        }
        if let Some((digest, bytes)) = bundle {
            if bytes.is_empty() || brain_component_host::component_digest(bytes) != digest {
                return Err(BrainError::Invalid(
                    "Tool Environment bundle bytes do not match their declared digest".into(),
                ));
            }
            self.store(self.bundle_path(digest), digest, bytes)?;
        }
        self.store(self.path(component_digest), component_digest, component)?;
        Ok(ToolSelectorDoc {
            component_digest: component_digest.into(),
            component_bytes: component.len() as u64,
            world: world.into(),
            config: config.clone(),
            grants: grants.to_vec(),
            environment: environment.map(str::to_owned),
            bundle_digest: bundle.map(|(digest, _)| digest.to_owned()),
        })
    }

    async fn invoke(
        &self,
        selector: &ToolSelectorDoc,
        request: ComponentToolRequest,
        capabilities: Arc<dyn ToolCapabilityHandler>,
    ) -> Result<CallOutcome> {
        if selector.world != TOOL_WORLD {
            return Err(BrainError::Journal(format!(
                "sealed Tool world {:?} is unsupported",
                selector.world
            )));
        }
        let path = self.path(&selector.component_digest);
        if !path.is_file() {
            return Err(BrainError::Journal(format!(
                "Tool component {} is absent from this Brain store",
                selector.component_digest
            )));
        }
        let bundle = match &selector.bundle_digest {
            None => None,
            Some(digest) => {
                let bytes = std::fs::read(self.bundle_path(digest)).map_err(|error| {
                    BrainError::Journal(format!(
                        "sealed Tool Environment bundle {digest} is unreadable in this Brain store: {error}"
                    ))
                })?;
                if component_digest(&bytes) != digest.as_str() {
                    return Err(BrainError::Journal(format!(
                        "stored Tool Environment bundle {digest} has different bytes"
                    )));
                }
                Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
            }
        };
        let binding = self
            .router
            .bind(
                brain_component_host::TOOL_COMPONENT,
                request.call_id.clone(),
                Arc::new(ComponentCapabilities {
                    handler: capabilities,
                    bundle_base64: bundle,
                }),
            )
            .map_err(|error| BrainError::Transport(error.to_string()))?;
        let started = std::time::Instant::now();
        let value = self
            .pool
            .call(WorkerRequest::Tool {
                component: ComponentSource {
                    path,
                    sha256: selector.component_digest.clone(),
                },
                request: tool::aex::tool::types::Invocation {
                    metadata: tool::aex::tool::types::CallMetadata {
                        tenant_id: request.tenant_id,
                        session_id: request.session_id,
                        turn_id: request.turn_id,
                        call_id: request.call_id,
                        tool_name: request.tool_name,
                    },
                    input_json: serde_json::to_string(&request.input)?,
                    config_json: serde_json::to_string(&selector.config)?,
                    deadline_at_ms: request.deadline_at_ms,
                },
                grants: selector.grants.clone(),
            })
            .await
            .map_err(|error| BrainError::Transport(format!("Tool component: {error}")))?;
        drop(binding);
        let outcome: tool::aex::tool::types::Outcome = serde_json::from_value(value)
            .map_err(|error| BrainError::Protocol(format!("Tool component outcome: {error}")))?;
        let structured = serde_json::from_str(&outcome.value_json)
            .map_err(|error| BrainError::Protocol(format!("Tool value_json: {error}")))?;
        Ok(CallOutcome {
            outcome: if outcome.is_error {
                TerminalOutcome::Failed
            } else {
                TerminalOutcome::Completed
            },
            content: outcome.content,
            value: Some(structured),
            is_error: outcome.is_error,
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            truncated: false,
            terminal: None,
        })
    }
}

struct ComponentCapabilities {
    handler: Arc<dyn ToolCapabilityHandler>,
    bundle_base64: Option<String>,
}

#[async_trait]
impl CapabilityHandler for ComponentCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        let mut request = call.request;
        // The executed bundle comes from the seal, never from the guest, so a Tool component
        // cannot run code that was not admitted at create.
        if call.capability == "tool.environment.invoke"
            && let Some(object) = request.as_object_mut()
            && let Some(bundle) = &self.bundle_base64
        {
            object.insert("bundle_base64".into(), Value::String(bundle.clone()));
        }
        self.handler
            .call(&call.capability, &call.operation_id, request)
            .await
            .map_err(|failure: ToolCapabilityFailure| CapabilityFailure {
                code: failure.code,
                message: failure.message,
                retryable: failure.retryable,
            })
    }
}

pub async fn registry_with_component_store(
    store_dir: &Path,
    component_host: &Path,
    workers: usize,
) -> anyhow::Result<Arc<dyn ToolRegistry>> {
    let router = CapabilityRouter::new(Arc::new(RejectCapabilities));
    let pool = WorkerPool::with_capabilities(component_host, workers, router.clone()).await?;
    Ok(Arc::new(ComponentToolRegistry::new(
        store_dir, pool, router,
    )?))
}

struct RejectCapabilities;

#[async_trait]
impl CapabilityHandler for RejectCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "capability_unbound".into(),
            message: format!(
                "no kernel capability handler is bound for {} instance {:?}",
                call.world, call.instance_id
            ),
            retryable: true,
        })
    }
}
