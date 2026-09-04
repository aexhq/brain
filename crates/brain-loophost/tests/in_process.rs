//! The runtime on this platform runs a real component: what the Linux worker test proves
//! through the process boundary, proved in-process wherever the crate builds.

use std::sync::Arc;

use brain_loophost::{
    AdmissionEngine, GuestHost, HostCall, LoopLimits, RUNTIME_SHIM_IMPORTS, WarmInstances,
};
use brain_protocol::{RuntimeEnvelope, TurnError, TurnInput};

struct Answering;

impl GuestHost for Answering {
    fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::Model { .. } => Ok(serde_json::json!({
                "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}]},
                "stop_reason": "end_turn",
                "usage": {}
            })
            .to_string()),
            HostCall::Dispatch { .. } => Ok("[]".into()),
            HostCall::Append { .. } => Ok("7".into()),
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

/// A component with host imports is several core instances; the store must admit them,
/// and a warm instance must take a second turn.
#[test]
fn the_diagnostic_component_takes_two_turns_in_process() {
    let package = std::fs::read(
        std::env::var("BRAIN_TEST_AGENTLOOP_PACKAGE")
            .expect("BRAIN_TEST_AGENTLOOP_PACKAGE must name the built diagnostic package"),
    )
    .unwrap();
    let limits = LoopLimits::default();
    let engine = AdmissionEngine::new(
        limits.clone(),
        RUNTIME_SHIM_IMPORTS.iter().map(|s| s.to_string()).collect(),
    )
    .unwrap();
    let admitted = engine.admit(&package).unwrap();
    let warm = WarmInstances::default();

    let first = admitted
        .turn(
            engine.engine(),
            &limits,
            &warm,
            "ses_local",
            input("hello", Default::default()),
            Arc::new(Answering),
        )
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
            &warm,
            "ses_local",
            input("again", first.slots),
            Arc::new(Answering),
        )
        .unwrap();
    assert_eq!(second.slots["memory"]["turns"], 2);
}
