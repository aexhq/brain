use std::sync::Arc;

use brain::agentloop::{Agentloop, AgentloopRegistry, SequentialAgentloop};
use brain::journal::AgentloopSelectorDoc;
use brain::session::BrainServices;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

struct TestRegistry;

impl AgentloopRegistry for TestRegistry {
    fn resolve(&self, _selector: &AgentloopSelectorDoc) -> brain::Result<Arc<dyn Agentloop>> {
        Ok(Arc::new(SequentialAgentloop))
    }

<<<<<<< HEAD
    fn admit(
        &self,
        component_digest: &str,
        world: &str,
        component: &[u8],
        config: &serde_json::Map<String, Value>,
    ) -> brain::Result<AgentloopSelectorDoc> {
        Ok(AgentloopSelectorDoc {
            component_digest: component_digest.into(),
            component_bytes: component.len() as u64,
            world: world.into(),
            config: config.clone(),
=======
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
>>>>>>> origin/main
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
<<<<<<< HEAD
    let bundle = b"integration test loop";
    json!({
        "component_digest": hex::encode(Sha256::digest(bundle)),
        "world": "aex:agentloop/agentloop@1.0.0"
    })
}

pub fn model_config() -> Value {
    let component = b"integration test model";
    json!({
        "component_digest": hex::encode(Sha256::digest(component)),
        "world": "aex:model/model@1.0.0",
        "provider": "anthropic",
        "name": "scripted",
        "api_key": "sk-fake"
    })
}

pub fn component_artifacts() -> Value {
    use base64::Engine as _;
    let loop_component = b"integration test loop";
    let model_component = b"integration test model";
    json!([
        {
            "component_digest": hex::encode(Sha256::digest(loop_component)),
            "component_base64": base64::engine::general_purpose::STANDARD.encode(loop_component),
            "bytes": loop_component.len()
        },
        {
            "component_digest": hex::encode(Sha256::digest(model_component)),
            "component_base64": base64::engine::general_purpose::STANDARD.encode(model_component),
            "bytes": model_component.len()
        }
    ])
}
=======
    use base64::Engine as _;
    let bundle = b"integration test loop";
    json!({
        "source_bundle_sha256": hex::encode(Sha256::digest(bundle)),
        "toolchain": "test-loop",
        "bundle_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
    })
}
>>>>>>> origin/main
