<<<<<<< HEAD
=======
//! The loop-host registry resolves sealed selectors to runnable loops and owns the loop
//! store. A caller uploads the deterministic source bundle; its sealed
//! identity is (source sha256, toolchain), and this composition componentizes it once
//! server-side — cached content-addressed on disk — because componentization itself is
//! non-deterministic.

>>>>>>> origin/main
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use brain::BrainError;
use brain::agentloop::{Agentloop, AgentloopRegistry};
use brain::journal::AgentloopSelectorDoc;
<<<<<<< HEAD
use brain_component_host::{
    AGENTLOOP_WORLD, CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter,
    ComponentSource, WorkerPool, component_digest,
};
use serde_json::Value;
=======
>>>>>>> origin/main

use crate::component::ComponentAgentloop;

<<<<<<< HEAD
static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);

pub struct ComponentAgentloopRegistry {
    store_dir: PathBuf,
    pool: Arc<WorkerPool>,
    router: Arc<CapabilityRouter>,
    engines: Mutex<HashMap<String, Arc<ComponentAgentloop>>>,
=======
/// The one loop toolchain this build supports: the pinned guest engine plus componentizer.
/// A different toolchain string is a different sealed identity and is refused, never guessed.
/// External builders must seal source bundles with this exact value.
pub const LOOP_TOOLCHAIN: &str = "starlingmonkey-componentize-js-0.22.0";

/// Componentization is minutes of CPU at worst; a componentizer that exceeds this wall is
/// wedged, and the admission fails loudly instead of stalling the create indefinitely.
const COMPONENTIZE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Staging-file uniqueness within one process: pid alone collides when two admissions of the
/// same digest run concurrently on the async create path.
static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);

/// Selector-to-loop resolution over a content-addressed loop store.
pub struct LoophostRegistry {
    store_dir: PathBuf,
    /// The directory holding `componentize-one.mjs` with the pinned componentizer installed.
    toolchain_dir: PathBuf,
    engines: Mutex<HashMap<String, Arc<WasmAgentloop>>>,
    admissions: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
>>>>>>> origin/main
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
            admissions: Mutex::new(HashMap::new()),
        })
    }

<<<<<<< HEAD
=======
    fn load_component(
        &self,
        cache_key: &str,
        component: &Path,
        what: &str,
    ) -> Result<Arc<WasmAgentloop>, BrainError> {
        if let Some(engine) = self.engines.lock().expect("loop engines").get(cache_key) {
            return Ok(engine.clone());
        }
        let engine = Arc::new(
            WasmAgentloop::from_component_file(component).map_err(|error| {
                BrainError::Agentloop(format!("{what} failed to load: {error}"))
            })?,
        );
        self.engines
            .lock()
            .expect("loop engines")
            .insert(cache_key.to_string(), engine.clone());
        Ok(engine)
    }

    fn source_path(&self, digest: &str) -> PathBuf {
        self.store_dir.join("source").join(format!("{digest}.mjs"))
    }

>>>>>>> origin/main
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
<<<<<<< HEAD
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
=======
        std::fs::rename(&staged, &component)
            .map_err(|error| BrainError::Journal(format!("loop store publish: {error}")))?;
        Ok(())
    }

    async fn admit_bundle(&self, digest: &str, bundle: &[u8]) -> Result<(), BrainError> {
        let lock = self
            .admissions
            .lock()
            .expect("loop admissions")
            .entry(digest.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;
        self.store_source(digest, bundle)?;
        self.componentize(digest).await
    }

    fn custom_loop(&self, digest: &str) -> Result<Arc<WasmAgentloop>, BrainError> {
        let component = self.component_path(digest);
        if !component.exists() {
>>>>>>> origin/main
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
<<<<<<< HEAD
        self.component(selector)
            .map(|agentloop| agentloop as Arc<dyn Agentloop>)
    }

    fn admit(
=======
        if selector.toolchain != LOOP_TOOLCHAIN {
            return Err(BrainError::Invalid(format!(
                "loop toolchain {:?} is not supported; this composition runs {LOOP_TOOLCHAIN:?}",
                selector.toolchain
            )));
        }
        self.custom_loop(&selector.source_bundle_sha256)
            .map(|agentloop| agentloop as Arc<dyn Agentloop>)
    }

    fn admit_custom(
>>>>>>> origin/main
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
<<<<<<< HEAD
        self.store_component(component_digest, component)?;
        Ok(AgentloopSelectorDoc {
            component_digest: component_digest.to_owned(),
            component_bytes: component.len() as u64,
            world: world.to_owned(),
            config: config.clone(),
=======
        // Componentize now, so a bundle the toolchain rejects fails the create with the
        // author's diagnostic instead of failing the session's first turn. The runtime is
        // available here: admission happens inside the async create path.
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| BrainError::Journal("loop admission requires a tokio runtime".into()))?;
        tokio::task::block_in_place(|| {
            handle.block_on(self.admit_bundle(source_bundle_sha256, bundle))
        })?;
        Ok(AgentloopSelectorDoc {
            source_bundle_sha256: source_bundle_sha256.to_string(),
            source_bundle_bytes: bundle.len() as u64,
            toolchain: toolchain.to_string(),
>>>>>>> origin/main
        })
    }
}

<<<<<<< HEAD
pub async fn services_with_component_store(
=======
/// Convenience for compositions: a content-addressed loop store behind one registry.
pub fn services_with_loop_store(
>>>>>>> origin/main
    store_dir: &Path,
    component_host: &Path,
    workers: usize,
) -> anyhow::Result<brain::session::BrainServices> {
<<<<<<< HEAD
    let router = CapabilityRouter::new(Arc::new(RejectCapabilities));
    let pool = WorkerPool::with_capabilities(component_host, workers, router.clone()).await?;
    let registry = ComponentAgentloopRegistry::new(store_dir, pool, router)?;
=======
    let registry = LoophostRegistry::new(store_dir, toolchain_dir)?;
>>>>>>> origin/main
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
