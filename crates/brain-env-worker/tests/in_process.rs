//! The runtime on this platform runs a real component: what the Linux worker test proves
//! through the process boundary, proved in-process wherever the crate builds.

use std::sync::Arc;

use async_trait::async_trait;
use brain_env::{EnvLimits, HostCall, NativeEnvironment, NativeToolInput};
use brain_env_worker::{AdmissionEngine, CAPABILITY_IMPORTS, GuestHost, RUNTIME_SHIM_IMPORTS};
use brain_protocol::{RuntimeEnvelope, TurnError, TurnInput};

#[derive(Default)]
struct Answering {
    kv: std::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>,
}

#[async_trait]
impl GuestHost for Answering {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::SetKv { key, value_json } => {
                self.kv
                    .lock()
                    .unwrap()
                    .insert(key, serde_json::from_str(&value_json).unwrap());
                Ok("7".into())
            }
            HostCall::SetTranscript { .. } => Ok("7".into()),
            HostCall::Events { after } => {
                Ok(serde_json::json!({"events": [], "next_cursor": after}).to_string())
            }
            HostCall::Model { .. } => Ok(serde_json::json!({
                "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}]},
                "stop_reason": "end_turn",
                "usage": {}
            })
            .to_string()),
            HostCall::Dispatch { .. } => Ok("[]".into()),
            HostCall::Emit { .. } => Ok("7".into()),
            HostCall::Telemetry { .. } => Ok(String::new()),
        }
    }
}

fn input(message: &str, kv: std::collections::BTreeMap<String, serde_json::Value>) -> TurnInput {
    TurnInput {
        input: message.into(),
        transcript: Vec::new(),
        kv,
        events: Vec::new(),
        configuration: serde_json::json!({}),
        system: String::new(),
        tools: Vec::new(),
        runtime: RuntimeEnvelope::at(&brain_protocol::SessionId::new("ses_test"), 1),
    }
}

fn environment() -> NativeEnvironment {
    NativeEnvironment::default()
}

#[tokio::test]
async fn python_agentloop_calls_a_brain_host_service() {
    let package = std::fs::read(
        std::env::var("BRAIN_TEST_PYTHON_AGENTLOOP").expect("build the Python Agentloop fixture"),
    )
    .unwrap();
    let limits = EnvLimits {
        max_core_instances: 1024,
        ..Default::default()
    };
    let engine = AdmissionEngine::new(
        limits.clone(),
        RUNTIME_SHIM_IMPORTS
            .iter()
            .chain(CAPABILITY_IMPORTS)
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap();
    let admitted = engine.admit(&package).unwrap();
    let services = Arc::new(Answering::default());
    let output = admitted
        .turn(
            engine.engine(),
            &limits,
            environment(),
            input("python", Default::default()),
            services.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        output.result,
        Some(serde_json::json!({"sequence": 7, "calls": 1}))
    );
    let next_kv = services.kv.lock().unwrap().clone();
    let _second = admitted
        .turn(
            engine.engine(),
            &limits,
            environment(),
            input("again", next_kv),
            services.clone(),
        )
        .await
        .unwrap();
    assert_eq!(services.kv.lock().unwrap()["calls"], 2);
}

#[tokio::test]
async fn a_packaged_python_tool_calls_its_granted_service() {
    let component = std::fs::read(
        std::env::var("BRAIN_TEST_PYTHON_TOOL").expect("build the Python Tool fixture"),
    )
    .unwrap();
    let limits = EnvLimits {
        max_core_instances: 1024,
        ..Default::default()
    };
    let engine = AdmissionEngine::new(limits.clone(), Vec::new()).unwrap();
    let admitted = engine.admit_tool(&component).unwrap();
    let services = Arc::new(Answering::default());
    let output = admitted
        .run(
            engine.engine(),
            &limits,
            environment(),
            NativeToolInput {
                input: serde_json::json!({"value": 8}),
                configuration: serde_json::json!({}),
                deadline_at_ms: u64::MAX,
            },
            services.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        output,
        serde_json::json!({"echo": {"value": 8}, "sequence": 7})
    );
}

/// A component with host imports is several core instances. Each turn gets a fresh
/// Store; durable state reaches the next one only through its input.
#[tokio::test]
async fn the_diagnostic_component_takes_two_turns_in_fresh_stores() {
    let package = std::fs::read(
        std::env::var("BRAIN_TEST_AGENTLOOP_PACKAGE")
            .expect("BRAIN_TEST_AGENTLOOP_PACKAGE must name the built diagnostic package"),
    )
    .unwrap();
    let limits = EnvLimits::default();
    let engine = AdmissionEngine::new(
        limits.clone(),
        RUNTIME_SHIM_IMPORTS
            .iter()
            .chain(CAPABILITY_IMPORTS)
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap();
    let admitted = engine.admit(&package).unwrap();
    let services = Arc::new(Answering::default());
    let first = admitted
        .turn(
            engine.engine(),
            &limits,
            environment(),
            input("hello", Default::default()),
            services.clone(),
        )
        .await
        .unwrap();
    assert_eq!(services.kv.lock().unwrap()["memory"]["turns"], 1);
    assert_eq!(
        first.result,
        Some(serde_json::json!({"turns": 1, "message": "hello"}))
    );

    let next_kv = services.kv.lock().unwrap().clone();
    let _second = admitted
        .turn(
            engine.engine(),
            &limits,
            environment(),
            input("again", next_kv),
            services.clone(),
        )
        .await
        .unwrap();
    assert_eq!(services.kv.lock().unwrap()["memory"]["turns"], 2);
}

#[tokio::test]
async fn a_tool_component_runs_in_a_fresh_store() {
    let component = std::fs::read(
        std::env::var("BRAIN_TEST_TOOL_COMPONENT")
            .expect("BRAIN_TEST_TOOL_COMPONENT must name the built diagnostic Tool"),
    )
    .unwrap();
    let limits = EnvLimits::default();
    let engine = AdmissionEngine::new(limits.clone(), Vec::new()).unwrap();
    let admitted = engine.admit_tool(&component).unwrap();
    let services = Arc::new(Answering::default());
    let output = admitted
        .run(
            engine.engine(),
            &limits,
            environment(),
            NativeToolInput {
                input: serde_json::json!({"value": 7}),
                configuration: serde_json::json!({}),
                deadline_at_ms: 1_000,
            },
            services.clone(),
        )
        .await
        .unwrap();
    assert_eq!(output, serde_json::json!({"echo": {"value": 7}}));
}
