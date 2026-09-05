//! The runtime on this platform runs a real component: what the Linux worker test proves
//! through the process boundary, proved in-process wherever the crate builds.

use std::sync::Arc;

use async_trait::async_trait;
use brain_loophost::{
    AdmissionEngine, CAPABILITY_IMPORTS, GuestHost, HostCall, LoopLimits, NativeEnvironment,
    NativeToolInput, RUNTIME_SHIM_IMPORTS,
};
use brain_protocol::{RuntimeEnvelope, TurnError, TurnInput};

struct Answering;

#[async_trait]
impl GuestHost for Answering {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
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

fn input(message: &str, slots: std::collections::BTreeMap<String, serde_json::Value>) -> TurnInput {
    TurnInput {
        input: message.into(),
        transcript: Vec::new(),
        slots,
        events: Vec::new(),
        configuration: serde_json::json!({}),
        system: String::new(),
        tools: Vec::new(),
        runtime: RuntimeEnvelope::at(&brain_protocol::SessionId::new("ses_test"), 1),
    }
}

fn environment() -> NativeEnvironment {
    NativeEnvironment {
        scratch: false,
        workspace: None,
        network_allow: Vec::new(),
        secrets: Default::default(),
    }
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
    let limits = LoopLimits::default();
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
    let first = admitted
        .turn(
            engine.engine(),
            &limits,
            environment(),
            input("hello", Default::default()),
            Arc::new(Answering),
        )
        .await
        .unwrap();
    assert_eq!(first.slots["memory"]["turns"], 1);
    assert_eq!(
        first.result,
        Some(serde_json::json!({"turns": 1, "message": "hello"}))
    );

    let second = admitted
        .turn(
            engine.engine(),
            &limits,
            environment(),
            input("again", first.slots),
            Arc::new(Answering),
        )
        .await
        .unwrap();
    assert_eq!(second.slots["memory"]["turns"], 2);
}

#[tokio::test]
async fn a_tool_component_runs_in_a_fresh_store() {
    let component = std::fs::read(
        std::env::var("BRAIN_TEST_TOOL_COMPONENT")
            .expect("BRAIN_TEST_TOOL_COMPONENT must name the built diagnostic Tool"),
    )
    .unwrap();
    let limits = LoopLimits::default();
    let engine = AdmissionEngine::new(limits.clone(), Vec::new()).unwrap();
    let admitted = engine.admit_tool(&component).unwrap();
    let output = admitted
        .run(
            engine.engine(),
            &limits,
            environment(),
            NativeToolInput {
                call_id: "call_1".into(),
                input: serde_json::json!({"value": 7}),
                configuration: serde_json::json!({}),
                deadline_at_ms: 1_000,
            },
            Arc::new(Answering),
        )
        .await
        .unwrap();
    assert_eq!(output, serde_json::json!({"echo": {"value": 7}}));
}
