//! The runtime on this platform runs a real component: what the Linux worker test proves
//! through the process boundary, proved in-process wherever the crate builds.

use std::sync::Arc;

use async_trait::async_trait;
use brain_env::{EnvLimits, HostCall, NativeEnvironment, NativeToolInput};
use brain_env_worker::{AdmissionEngine, GuestHost};
use brain_protocol::TurnError;

#[derive(Default)]
struct Answering {}

#[async_trait]
impl GuestHost for Answering {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::Environments { request_json } => {
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&request_json).unwrap(),
                    serde_json::json!({"operation": "list"})
                );
                Ok("[]".into())
            }
            HostCall::Model { .. } => Ok(serde_json::json!({
                "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}]},
                "stop_reason": "end_turn",
                "usage": {}
            })
            .to_string()),
            HostCall::Emit { .. } => Ok("7".into()),
            HostCall::ToolResult { .. }
            | HostCall::ToolReturned { .. }
            | HostCall::ToolFinish { .. } => Ok("7".into()),
            HostCall::Telemetry { .. } => Ok(String::new()),
        }
    }
}

fn environment() -> NativeEnvironment {
    NativeEnvironment::default()
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
    let engine = AdmissionEngine::new(limits.clone()).unwrap();
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

#[tokio::test]
async fn a_tool_component_runs_in_a_fresh_store() {
    let component = std::fs::read(
        std::env::var("BRAIN_TEST_TOOL_COMPONENT")
            .expect("BRAIN_TEST_TOOL_COMPONENT must name the built diagnostic Tool"),
    )
    .unwrap();
    let limits = EnvLimits::default();
    let engine = AdmissionEngine::new(limits.clone()).unwrap();
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
