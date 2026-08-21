//! The loop-host registry: resolves sealed selectors to runnable loops and owns the custom
//! loop store. A customer uploads the deterministic esbuild source bundle; its sealed
//! identity is (source sha256, toolchain), and this composition componentizes it once
//! server-side — cached content-addressed on disk — because componentization itself is
//! non-deterministic (design ledger A2/B3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use brain::BrainError;
use brain::agentloop::{Agentloop, AgentloopRegistry};
use brain::journal::AgentloopSelectorDoc;

use crate::WasmAgentloop;

/// The one loop toolchain this build supports: the pinned guest engine plus componentizer.
/// A different toolchain string is a different sealed identity and is refused, never guessed.
pub const LOOP_TOOLCHAIN: &str = "starlingmonkey-componentize-js-0.22.0";

/// Selector→loop resolution over a content-addressed loop store. Officials resolve to the
/// composition's prebuilt components; customs componentize at admission and load lazily.
pub struct LoophostRegistry {
    aex: Arc<dyn Agentloop>,
    store_dir: PathBuf,
    /// The directory holding `componentize-one.mjs` with the pinned componentizer installed.
    toolchain_dir: PathBuf,
    engines: Mutex<HashMap<String, Arc<WasmAgentloop>>>,
}

impl LoophostRegistry {
    pub fn new(
        aex: Arc<dyn Agentloop>,
        store_dir: impl Into<PathBuf>,
        toolchain_dir: impl Into<PathBuf>,
    ) -> std::io::Result<Self> {
        let store_dir = store_dir.into();
        std::fs::create_dir_all(store_dir.join("source"))?;
        std::fs::create_dir_all(store_dir.join("component"))?;
        Ok(Self {
            aex,
            store_dir,
            toolchain_dir: toolchain_dir.into(),
            engines: Mutex::new(HashMap::new()),
        })
    }

    fn source_path(&self, digest: &str) -> PathBuf {
        self.store_dir.join("source").join(format!("{digest}.mjs"))
    }

    fn component_path(&self, digest: &str) -> PathBuf {
        self.store_dir
            .join("component")
            .join(format!("{digest}-{LOOP_TOOLCHAIN}.wasm"))
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
        let staged = component.with_extension(format!("staging-{}", std::process::id()));
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
            .current_dir(&self.toolchain_dir);
        let output = command.output().await.map_err(|error| {
            BrainError::Invalid(format!("the loop toolchain is unavailable: {error}"))
        })?;
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

    fn custom_loop(&self, digest: &str) -> Result<Arc<WasmAgentloop>, BrainError> {
        if let Some(engine) = self.engines.lock().expect("loop engines").get(digest) {
            return Ok(engine.clone());
        }
        let component = self.component_path(digest);
        if !component.exists() {
            return Err(BrainError::Invalid(format!(
                "custom loop {digest} is not present in this composition's loop store; \
                 re-create the session with the bundle"
            )));
        }
        let engine = Arc::new(
            WasmAgentloop::from_component_file(&component).map_err(|error| {
                BrainError::Agentloop(format!("custom loop {digest} failed to load: {error}"))
            })?,
        );
        self.engines
            .lock()
            .expect("loop engines")
            .insert(digest.to_string(), engine.clone());
        Ok(engine)
    }
}

impl AgentloopRegistry for LoophostRegistry {
    fn resolve(&self, selector: &AgentloopSelectorDoc) -> brain::Result<Arc<dyn Agentloop>> {
        match selector {
            AgentloopSelectorDoc::Official { name, .. } if name == "aex" => Ok(self.aex.clone()),
            AgentloopSelectorDoc::Official { name, version } => Err(BrainError::Invalid(format!(
                "official agentloop {name}@{version} is not available in this composition"
            ))),
            AgentloopSelectorDoc::Custom {
                source_bundle_sha256,
                toolchain,
                ..
            } => {
                if toolchain != LOOP_TOOLCHAIN {
                    return Err(BrainError::Invalid(format!(
                        "loop toolchain {toolchain:?} is not supported; this composition runs {LOOP_TOOLCHAIN:?}"
                    )));
                }
                Ok(self.custom_loop(source_bundle_sha256)?)
            }
        }
    }

    fn pin_official(&self, name: &str) -> brain::Result<AgentloopSelectorDoc> {
        if name == "aex" {
            Ok(AgentloopSelectorDoc::official_aex())
        } else {
            Err(BrainError::Invalid(format!(
                "official agentloop {name:?} is not available in this composition"
            )))
        }
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
        let source = self.source_path(source_bundle_sha256);
        if !source.exists() {
            let staged = source.with_extension(format!("staging-{}", std::process::id()));
            std::fs::write(&staged, bundle)
                .and_then(|()| std::fs::rename(&staged, &source))
                .map_err(|error| BrainError::Journal(format!("loop store write: {error}")))?;
        }
        // Componentize now, so a bundle the toolchain rejects fails the create with the
        // author's diagnostic instead of failing the session's first turn. The runtime is
        // available here: admission happens inside the async create path.
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| BrainError::Journal("loop admission requires a tokio runtime".into()))?;
        tokio::task::block_in_place(|| handle.block_on(self.componentize(source_bundle_sha256)))?;
        Ok(AgentloopSelectorDoc::Custom {
            source_bundle_sha256: source_bundle_sha256.to_string(),
            source_bundle_bytes: bundle.len() as u64,
            toolchain: toolchain.to_string(),
        })
    }
}

/// Convenience for compositions: the aex loop as a wasm guest plus a custom-loop store, all
/// behind one registry.
pub fn services_with_loop_store(
    aex_component: &Path,
    store_dir: &Path,
    toolchain_dir: &Path,
) -> anyhow::Result<brain::session::BrainServices> {
    let aex: Arc<dyn Agentloop> = Arc::new(WasmAgentloop::from_component_file(aex_component)?);
    let registry = LoophostRegistry::new(aex.clone(), store_dir, toolchain_dir)?;
    Ok(brain::session::BrainServices {
        agentloop: Some(aex),
        agentloop_registry: Some(Arc::new(registry)),
        ..brain::session::BrainServices::default()
    })
}
