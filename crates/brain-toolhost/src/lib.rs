use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use brain::adapter::CallOutcome;
use brain::journal::ToolSelectorDoc;
use brain::tools::{
    ComponentToolRequest, TOOL_WORLD, ToolCapabilityFailure, ToolCapabilityHandler, ToolRegistry,
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

    fn store(&self, digest: &str, bytes: &[u8]) -> Result<()> {
        let target = self.path(digest);
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
    fn admit(
        &self,
        component_digest: &str,
        world: &str,
        component: &[u8],
        config: &serde_json::Map<String, Value>,
        grants: &[String],
        environment: Option<&str>,
    ) -> Result<ToolSelectorDoc> {
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
        self.store(component_digest, component)?;
        Ok(ToolSelectorDoc {
            component_digest: component_digest.into(),
            component_bytes: component.len() as u64,
            world: world.into(),
            config: config.clone(),
            grants: grants.to_vec(),
            environment: environment.map(str::to_owned),
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
        let binding = self
            .router
            .bind(
                "tool",
                request.call_id.clone(),
                Arc::new(ComponentCapabilities(capabilities)),
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

struct ComponentCapabilities(Arc<dyn ToolCapabilityHandler>);

#[async_trait]
impl CapabilityHandler for ComponentCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        self.0
            .call(&call.capability, &call.operation_id, call.request)
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
