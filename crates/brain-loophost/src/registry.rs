use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use brain::BrainError;
use brain::agentloop::{Agentloop, AgentloopRegistry};
use brain::journal::AgentloopSelectorDoc;
use brain_component_host::{
    AGENTLOOP_WORLD, CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter,
    ComponentSource, WorkerPool, component_digest,
};
use serde_json::Value;

use crate::component::ComponentAgentloop;

static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);

pub struct ComponentAgentloopRegistry {
    store_dir: PathBuf,
    pool: Arc<WorkerPool>,
    router: Arc<CapabilityRouter>,
    engines: Mutex<HashMap<String, Arc<ComponentAgentloop>>>,
}

impl ComponentAgentloopRegistry {
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
            engines: Mutex::new(HashMap::new()),
        })
    }

    fn component_path(&self, digest: &str) -> PathBuf {
        self.store_dir.join(format!("{digest}.wasm"))
    }

    fn store_component(&self, digest: &str, bytes: &[u8]) -> brain::Result<()> {
        let target = self.component_path(digest);
        if target.exists() {
            let existing = std::fs::read(&target)
                .map_err(|error| BrainError::Journal(format!("component store read: {error}")))?;
            if component_digest(&existing) != digest {
                return Err(BrainError::Journal(format!(
                    "component store entry {digest} has different bytes"
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
            .map_err(|error| BrainError::Journal(format!("component store write: {error}")))
    }

    fn component(&self, selector: &AgentloopSelectorDoc) -> brain::Result<Arc<ComponentAgentloop>> {
        if selector.world != AGENTLOOP_WORLD {
            return Err(BrainError::Invalid(format!(
                "Agentloop world {:?} is not supported; expected {AGENTLOOP_WORLD:?}",
                selector.world
            )));
        }
        let config_json = serde_json::to_string(&selector.config)
            .map_err(|error| BrainError::Journal(format!("Agentloop config: {error}")))?;
        let cache_key = format!(
            "{}:{}",
            selector.component_digest,
            component_digest(config_json.as_bytes())
        );
        if let Some(engine) = self
            .engines
            .lock()
            .expect("Agentloop engines")
            .get(&cache_key)
        {
            return Ok(engine.clone());
        }
        let path = self.component_path(&selector.component_digest);
        if !path.is_file() {
            return Err(BrainError::Invalid(format!(
                "Agentloop component {} is absent from this Brain store",
                selector.component_digest
            )));
        }
        let engine = Arc::new(ComponentAgentloop::new(
            self.pool.clone(),
            self.router.clone(),
            ComponentSource {
                path,
                sha256: selector.component_digest.clone(),
            },
            config_json,
        ));
        self.engines
            .lock()
            .expect("Agentloop engines")
            .insert(cache_key, engine.clone());
        Ok(engine)
    }
}

impl AgentloopRegistry for ComponentAgentloopRegistry {
    fn resolve(&self, selector: &AgentloopSelectorDoc) -> brain::Result<Arc<dyn Agentloop>> {
        self.component(selector)
            .map(|agentloop| agentloop as Arc<dyn Agentloop>)
    }

    fn admit(
        &self,
        component_digest: &str,
        world: &str,
        component: &[u8],
        config: &serde_json::Map<String, Value>,
    ) -> brain::Result<AgentloopSelectorDoc> {
        if world != AGENTLOOP_WORLD {
            return Err(BrainError::Invalid(format!(
                "Agentloop world {world:?} is not supported; expected {AGENTLOOP_WORLD:?}"
            )));
        }
        self.store_component(component_digest, component)?;
        Ok(AgentloopSelectorDoc {
            component_digest: component_digest.to_owned(),
            component_bytes: component.len() as u64,
            world: world.to_owned(),
            config: config.clone(),
        })
    }
}

pub async fn services_with_component_store(
    store_dir: &Path,
    component_host: &Path,
    workers: usize,
) -> anyhow::Result<brain::session::BrainServices> {
    let router = CapabilityRouter::new(Arc::new(RejectCapabilities));
    let pool = WorkerPool::with_capabilities(component_host, workers, router.clone()).await?;
    let registry = ComponentAgentloopRegistry::new(store_dir, pool, router)?;
    Ok(brain::session::BrainServices {
        agentloop_registry: Some(Arc::new(registry)),
        ..brain::session::BrainServices::default()
    })
}

struct RejectCapabilities;

#[async_trait]
impl CapabilityHandler for RejectCapabilities {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
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
