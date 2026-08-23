use std::sync::Arc;

use brain::agentloop::{Agentloop, AgentloopRegistry, BuiltinAexLoop};
use brain::journal::AgentloopSelectorDoc;
use brain::session::BrainServices;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

struct TestRegistry;

impl AgentloopRegistry for TestRegistry {
    fn resolve(&self, _selector: &AgentloopSelectorDoc) -> brain::Result<Arc<dyn Agentloop>> {
        Ok(Arc::new(BuiltinAexLoop))
    }

    fn admit_custom(
        &self,
        source_bundle_sha256: &str,
        toolchain: &str,
        bundle: &[u8],
    ) -> brain::Result<AgentloopSelectorDoc> {
        Ok(AgentloopSelectorDoc {
            source_bundle_sha256: source_bundle_sha256.into(),
            source_bundle_bytes: bundle.len() as u64,
            toolchain: toolchain.into(),
        })
    }
}

pub fn registry() -> Arc<dyn AgentloopRegistry> {
    Arc::new(TestRegistry)
}

pub fn services() -> BrainServices {
    BrainServices {
        agentloop_registry: Some(registry()),
        ..BrainServices::default()
    }
}

pub fn loop_config() -> Value {
    use base64::Engine as _;
    let bundle = b"integration test loop";
    json!({
        "source_bundle_sha256": hex::encode(Sha256::digest(bundle)),
        "toolchain": "test-loop",
        "bundle_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
    })
}
