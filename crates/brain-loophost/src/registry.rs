//! The loop-host registry resolves sealed selectors to runnable loops and owns the loop
//! store. A caller uploads the deterministic source bundle; its sealed
//! identity is (source sha256, toolchain), and this composition componentizes it once
//! server-side — cached content-addressed on disk — because componentization itself is
//! non-deterministic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use brain::BrainError;
use brain::agentloop::{Agentloop, AgentloopRegistry};
use brain::journal::AgentloopSelectorDoc;

use crate::WasmAgentloop;

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
}

impl LoophostRegistry {
    pub fn new(
        store_dir: impl Into<PathBuf>,
        toolchain_dir: impl Into<PathBuf>,
    ) -> std::io::Result<Self> {
        let store_dir = store_dir.into();
        std::fs::create_dir_all(store_dir.join("source"))?;
        std::fs::create_dir_all(store_dir.join("component"))?;
        Ok(Self {
            store_dir,
            toolchain_dir: toolchain_dir.into(),
            engines: Mutex::new(HashMap::new()),
            admissions: Mutex::new(HashMap::new()),
        })
    }

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

    fn component_path(&self, digest: &str) -> PathBuf {
        self.store_dir
            .join("component")
            .join(format!("{digest}-{LOOP_TOOLCHAIN}.wasm"))
    }

    fn staging_path(target: &Path) -> PathBuf {
        target.with_extension(format!(
            "staging-{}-{}",
            std::process::id(),
            STAGING_NONCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// Write one admitted source bundle into the content-addressed store, atomically.
    fn store_source(&self, digest: &str, bundle: &[u8]) -> Result<(), BrainError> {
        let source = self.source_path(digest);
        if source.exists() {
            return Ok(());
        }
        let staged = Self::staging_path(&source);
        std::fs::write(&staged, bundle)
            .and_then(|()| std::fs::rename(&staged, &source))
            .map_err(|error| BrainError::Journal(format!("loop store write: {error}")))
    }

    /// Componentize one admitted source bundle with the pinned toolchain, atomically
    /// publishing the artifact into the content-addressed cache.
    async fn componentize(&self, digest: &str) -> Result<(), BrainError> {
        let source = self.source_path(digest);
        let component = self.component_path(digest);
        if component.exists() {
            return Ok(());
        }
        let wit = self.toolchain_dir.join("../wit/guest.wit");
        let staged = Self::staging_path(&component);
        let node: (&str, Vec<&str>) = if cfg!(windows) {
            ("cmd", vec!["/C", "node"])
        } else {
            ("node", vec![])
        };
        let mut command = tokio::process::Command::new(node.0);
        command
            .args(&node.1)
            .arg("componentize-one.mjs")
            .arg(&source)
            .arg(&wit)
            .arg(&staged)
            .arg(LOOP_TOOLCHAIN)
            .current_dir(&self.toolchain_dir);
        let output = match tokio::time::timeout(COMPONENTIZE_TIMEOUT, command.output()).await {
            Err(_elapsed) => {
                let _ = std::fs::remove_file(&staged);
                return Err(BrainError::Invalid(format!(
                    "the loop bundle did not componentize within {}s",
                    COMPONENTIZE_TIMEOUT.as_secs()
                )));
            }
            Ok(result) => result.map_err(|error| {
                BrainError::Invalid(format!("the loop toolchain is unavailable: {error}"))
            })?,
        };
        if !output.status.success() {
            let _ = std::fs::remove_file(&staged);
            return Err(BrainError::Invalid(format!(
                "the loop bundle failed to componentize under {LOOP_TOOLCHAIN}: {}",
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("no diagnostic")
            )));
        }
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
            return Err(BrainError::Invalid(format!(
                "custom loop {digest} is not present in this composition's loop store; \
                 re-create the session with the bundle"
            )));
        }
        self.load_component(digest, &component, &format!("custom loop {digest}"))
    }
}

impl AgentloopRegistry for LoophostRegistry {
    fn resolve(&self, selector: &AgentloopSelectorDoc) -> brain::Result<Arc<dyn Agentloop>> {
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
        &self,
        source_bundle_sha256: &str,
        toolchain: &str,
        bundle: &[u8],
    ) -> brain::Result<AgentloopSelectorDoc> {
        if toolchain != LOOP_TOOLCHAIN {
            return Err(BrainError::Invalid(format!(
                "loop toolchain {toolchain:?} is not supported; build against {LOOP_TOOLCHAIN:?}"
            )));
        }
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
        })
    }
}

/// Convenience for compositions: a content-addressed loop store behind one registry.
pub fn services_with_loop_store(
    store_dir: &Path,
    toolchain_dir: &Path,
) -> anyhow::Result<brain::session::BrainServices> {
    let registry = LoophostRegistry::new(store_dir, toolchain_dir)?;
    Ok(brain::session::BrainServices {
        agentloop_registry: Some(Arc::new(registry)),
        ..brain::session::BrainServices::default()
    })
}
