use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use brain::environment::{
    COMPONENT_ENVIRONMENT_WORLD, ComponentEnvironmentInvocation, ComponentEnvironmentRegistry,
};
use brain::{BrainError, Result};
use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter, ComponentSource,
    ENVIRONMENT_WORLD, WorkerPool, WorkerRequest, component_digest, environment,
};
use serde_json::Value;

static STAGING_NONCE: AtomicU64 = AtomicU64::new(1);

pub struct WasmEnvironmentRegistry {
    store_dir: PathBuf,
    pool: Arc<WorkerPool>,
}

impl WasmEnvironmentRegistry {
    pub fn new(store_dir: impl Into<PathBuf>, pool: Arc<WorkerPool>) -> std::io::Result<Self> {
        let store_dir = store_dir.into();
        std::fs::create_dir_all(&store_dir)?;
        Ok(Self { store_dir, pool })
    }

    fn path(&self, digest: &str) -> PathBuf {
        self.store_dir.join(format!("{digest}.wasm"))
    }

    fn store(&self, digest: &str, bytes: &[u8]) -> Result<()> {
        let target = self.path(digest);
        if target.exists() {
            let existing = std::fs::read(&target).map_err(|error| {
                BrainError::Protocol(format!("Environment store read: {error}"))
            })?;
            if component_digest(&existing) != digest {
                return Err(BrainError::Protocol(format!(
                    "Environment store entry {digest} has different bytes"
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
            .map_err(|error| BrainError::Protocol(format!("Environment store write: {error}")))
    }
}

#[async_trait]
impl ComponentEnvironmentRegistry for WasmEnvironmentRegistry {
    fn admit(&self, digest: &str, world: &str, component: &[u8]) -> Result<()> {
        if world != COMPONENT_ENVIRONMENT_WORLD || world != ENVIRONMENT_WORLD {
            return Err(BrainError::Invalid(format!(
                "Environment world {world:?} is not supported; expected {ENVIRONMENT_WORLD:?}"
            )));
        }
        self.store(digest, component)
    }

    async fn invoke(
        &self,
        declaration: &brain_protocol::session::ComponentEnvironmentConfig,
        request: ComponentEnvironmentInvocation,
    ) -> Result<String> {
        if declaration.world != ENVIRONMENT_WORLD {
            return Err(BrainError::Journal(format!(
                "sealed Environment world {:?} is unsupported",
                declaration.world
            )));
        }
        let path = self.path(declaration.component_digest.as_str());
        if !path.is_file() {
            return Err(BrainError::Journal(format!(
                "Environment component {} is absent from this Brain store",
                declaration.component_digest.as_str()
            )));
        }
        let instance_id = format!(
            "{}:{}:{}",
            request.session_id,
            request.environment_id,
            declaration.component_digest.as_str()
        );
        let component = ComponentSource {
            path,
            sha256: declaration.component_digest.to_string(),
        };
        let resolved = self
            .pool
            .call(WorkerRequest::EnvironmentResolve {
                instance_id: instance_id.clone(),
                component,
                request: environment::aex::environment::types::ResolveRequest {
                    tenant_id: request.tenant_id,
                    session_id: request.session_id,
                    environment_id: request.environment_id,
                    config_json: serde_json::to_string(&declaration.config)?,
                    authority_json: "{}".into(),
                },
            })
            .await
            .map_err(environment_transport)?;
        let binding_json = required_string(&resolved, "binding_json")?.to_owned();
        let submitted = self
            .pool
            .call(WorkerRequest::EnvironmentSubmit {
                instance_id: instance_id.clone(),
                binding_json: binding_json.clone(),
                operation: environment::aex::environment::types::Operation {
                    operation_id: request.operation_id,
                    kind: "invoke".into(),
                    descriptor_json: request.descriptor_json,
                    bundle: request.bundle,
                    input_json: request.input_json,
                    deadline_at_ms: request.deadline_at_ms,
                },
            })
            .await
            .map_err(environment_transport)?;
        let provider_operation_id =
            required_string(&submitted, "provider_operation_id")?.to_owned();
        let mut cursor = None;
        loop {
            if brain::wall_ms() >= request.deadline_at_ms {
                let _ = self
                    .pool
                    .call(WorkerRequest::EnvironmentCancel {
                        instance_id: instance_id.clone(),
                        binding_json: binding_json.clone(),
                        provider_operation_id: provider_operation_id.clone(),
                    })
                    .await;
                return Err(BrainError::Transport(
                    "Environment component operation exceeded its deadline".into(),
                ));
            }
            let observed = self
                .pool
                .call(WorkerRequest::EnvironmentObserve {
                    instance_id: instance_id.clone(),
                    binding_json: binding_json.clone(),
                    provider_operation_id: provider_operation_id.clone(),
                    cursor: cursor.clone(),
                })
                .await
                .map_err(environment_transport)?;
            let observation: environment::aex::environment::types::Observation =
                serde_json::from_value(observed).map_err(|error| {
                    BrainError::Protocol(format!("Environment observation: {error}"))
                })?;
            cursor = Some(observation.cursor);
            match observation.state {
                environment::aex::environment::types::OperationState::Pending
                | environment::aex::environment::types::OperationState::Running => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                environment::aex::environment::types::OperationState::Unknown => {
                    return Err(BrainError::Environment(
                        "Environment component reported an unknown operation outcome".into(),
                    ));
                }
                environment::aex::environment::types::OperationState::Completed
                | environment::aex::environment::types::OperationState::Failed
                | environment::aex::environment::types::OperationState::Cancelled => {
                    let terminal = observation.terminal_json.ok_or_else(|| {
                        BrainError::Protocol(
                            "terminal Environment observation omitted terminal_json".into(),
                        )
                    })?;
                    self.pool
                        .call(WorkerRequest::EnvironmentAcknowledge {
                            instance_id,
                            binding_json,
                            provider_operation_id,
                            terminal_json: terminal.clone(),
                        })
                        .await
                        .map_err(environment_transport)?;
                    return Ok(terminal);
                }
            }
        }
    }
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BrainError::Protocol(format!("Environment response omitted {field}")))
}

fn environment_transport(error: anyhow::Error) -> BrainError {
    BrainError::Transport(format!("Environment component: {error}"))
}

pub async fn registry_with_component_store(
    store_dir: &Path,
    component_host: &Path,
    workers: usize,
    capabilities: Arc<dyn CapabilityHandler>,
) -> anyhow::Result<Arc<dyn ComponentEnvironmentRegistry>> {
    let router = CapabilityRouter::new(capabilities);
    let pool = WorkerPool::with_capabilities(component_host, workers, router).await?;
    Ok(Arc::new(WasmEnvironmentRegistry::new(store_dir, pool)?))
}

pub struct RejectEnvironmentCapabilities;

#[async_trait]
impl CapabilityHandler for RejectEnvironmentCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "capability_unbound".into(),
            message: format!(
                "Environment host capability {} is not configured",
                call.capability
            ),
            retryable: false,
        })
    }
}
