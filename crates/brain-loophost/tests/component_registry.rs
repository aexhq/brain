use std::path::{Path, PathBuf};

use brain::agentloop::AgentloopRegistry as _;
use brain_component_host::{
    AGENTLOOP_WORLD, CapabilityCall, CapabilityFailure, CapabilityHandler, component_digest,
};
use brain_loophost::registry::ComponentAgentloopRegistry;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "brain-loop-registry-{}",
            brain::mint_id("test", 16)
        ));
        std::fs::create_dir_all(&path).expect("create component store");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn registry(store: &Path) -> ComponentAgentloopRegistry {
    let router =
        brain_component_host::CapabilityRouter::new(std::sync::Arc::new(RejectCapabilities));
    let pool = brain_component_host::WorkerPool::with_capabilities(
        env!("CARGO_BIN_EXE_loop-component-host"),
        2,
        router.clone(),
    )
    .await
    .expect("component workers");
    ComponentAgentloopRegistry::new(store, pool, router).expect("registry")
}

struct RejectCapabilities;

#[async_trait::async_trait]
impl CapabilityHandler for RejectCapabilities {
    async fn call(&self, _call: CapabilityCall) -> Result<serde_json::Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "unbound".into(),
            message: "unbound test capability".into(),
            retryable: false,
        })
    }
}

#[tokio::test]
async fn admission_stores_a_precompiled_component_by_digest_and_resolves_it() {
    let temp = TempDir::new();
    let registry = registry(&temp.0).await;
    let component = b"precompiled-component";
    let digest = component_digest(component);
    let config = serde_json::Map::from_iter([("mode".into(), "test".into())]);

    let selector = registry
        .admit(&digest, AGENTLOOP_WORLD, component, &config)
        .expect("admit component");

    assert_eq!(selector.component_digest, digest);
    assert_eq!(selector.component_bytes, component.len() as u64);
    assert_eq!(selector.world, AGENTLOOP_WORLD);
    assert_eq!(selector.config, config);
    assert_eq!(
        std::fs::read(temp.0.join(format!("{digest}.wasm"))).expect("stored component"),
        component
    );
    registry
        .resolve(&selector)
        .expect("resolve stored component");
}

#[tokio::test]
async fn admission_rejects_foreign_worlds_and_reuses_existing_digest_entries() {
    let temp = TempDir::new();
    let registry = registry(&temp.0).await;
    let first = b"first";
    let digest = component_digest(first);

    let foreign = registry.admit(
        &digest,
        "vendor:agentloop/agentloop@2.0.0",
        first,
        &serde_json::Map::new(),
    );
    assert!(foreign.is_err(), "foreign worlds must fail admission");

    registry
        .admit(&digest, AGENTLOOP_WORLD, first, &serde_json::Map::new())
        .expect("first admission");
    registry
        .admit(
            &digest,
            AGENTLOOP_WORLD,
            b"different",
            &serde_json::Map::new(),
        )
        .expect("verified digest admission reuses the stored component");
    assert_eq!(
        std::fs::read(temp.0.join(format!("{digest}.wasm"))).expect("stored component"),
        first,
        "an existing content-addressed entry is never overwritten"
    );
}

#[tokio::test]
async fn services_require_a_positive_bounded_worker_count() {
    let temp = TempDir::new();
    let result = brain_loophost::registry::services_with_component_store(
        &temp.0,
        Path::new(env!("CARGO_BIN_EXE_loop-component-host")),
        0,
    )
    .await;
    assert!(result.is_err(), "zero workers must fail fast");
}
