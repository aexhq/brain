use std::path::PathBuf;

use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, ComponentRuntime, ComponentSource,
    WorkerPool, WorkerRequest, agentloop, component_digest, environment, model, tool,
};
use serde_json::Value;
use std::sync::Arc;

fn component_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("guest/dist")
        .join(format!("{name}.component.wasm"))
}

fn component(name: &str) -> Vec<u8> {
    let path = component_path(name);
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error}; run `npm run build:components` first",
            path.display()
        )
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn four_worlds_execute_without_ambient_authority() {
    let runtime = ComponentRuntime::new().unwrap();

    let activation = runtime
        .invoke_agentloop(
            &component("agentloop"),
            agentloop::aex::agentloop::types::Activation {
                operation_id: "activation_1".into(),
                session_id: "ses_1".into(),
                kind: "message".into(),
                payload_json: r#"{"message":"hello"}"#.into(),
                config_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(activation.payload_json, r#"{"message":"hello"}"#);

    let outcome = runtime
        .invoke_tool(
            &component("tool"),
            tool::aex::tool::types::Invocation {
                metadata: tool::aex::tool::types::CallMetadata {
                    tenant_id: "tenant_1".into(),
                    session_id: "ses_1".into(),
                    turn_id: "turn_1".into(),
                    call_id: "call_1".into(),
                    tool_name: "echo".into(),
                },
                input_json: r#"{"value":"hello"}"#.into(),
                config_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert!(!outcome.is_error);
    assert_eq!(outcome.value_json, r#"{"value":"hello"}"#);

    let environment = runtime
        .exercise_environment(
            &component("environment"),
            environment::aex::environment::types::ResolveRequest {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                environment_id: "workspace".into(),
                config_json: "{}".into(),
                authority_json: "{}".into(),
            },
            environment::aex::environment::types::Operation {
                operation_id: "env_op_1".into(),
                kind: "invoke".into(),
                descriptor_json: "{}".into(),
                bundle: None,
                input_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        environment.state,
        environment::aex::environment::types::OperationState::Completed
    );

    let model = runtime
        .exercise_model(
            &component("model"),
            model::aex::model::types::Request {
                operation_id: "model_op_1".into(),
                model: "test".into(),
                messages_json: "[]".into(),
                tools_json: "[]".into(),
                response_format_json: None,
                generation_json: "{}".into(),
                provider_options_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        model.state,
        model::aex::model::types::AttemptState::Completed
    );
    assert_eq!(model.events.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bounded_worker_pool_executes_in_separate_processes() {
    let pool = WorkerPool::new(env!("CARGO_BIN_EXE_component-host"), 2)
        .await
        .unwrap();
    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let request = WorkerRequest::Tool {
        component: ComponentSource {
            path,
            sha256: component_digest(&bytes),
        },
        request: tool::aex::tool::types::Invocation {
            metadata: tool::aex::tool::types::CallMetadata {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                turn_id: "turn_1".into(),
                call_id: "call_process".into(),
                tool_name: "echo".into(),
            },
            input_json: r#"{"process":true}"#.into(),
            config_json: "{}".into(),
            deadline_at_ms: u64::MAX,
        },
        grants: Vec::new(),
    };
    let response = pool.call(request).await.unwrap();
    assert_eq!(response["value_json"], r#"{"process":true}"#);
}

struct EchoEnvironment;

#[async_trait::async_trait]
impl CapabilityHandler for EchoEnvironment {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        assert_eq!(call.capability, "tool.environment.invoke");
        assert_eq!(call.world, "tool");
        assert_eq!(call.instance_id.as_deref(), Some("call_relay"));
        Ok(Value::String(
            call.request["input_json"]
                .as_str()
                .expect("input JSON")
                .to_owned(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_relays_only_granted_component_capabilities() {
    let pool = WorkerPool::with_capabilities(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        Arc::new(EchoEnvironment),
    )
    .await
    .unwrap();
    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let request = || WorkerRequest::Tool {
        component: ComponentSource {
            path: path.clone(),
            sha256: component_digest(&bytes),
        },
        request: tool::aex::tool::types::Invocation {
            metadata: tool::aex::tool::types::CallMetadata {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                turn_id: "turn_1".into(),
                call_id: "call_relay".into(),
                tool_name: "echo".into(),
            },
            input_json: r#"{"relayed":true}"#.into(),
            config_json: r#"{"useEnvironment":true}"#.into(),
            deadline_at_ms: u64::MAX,
        },
        grants: vec!["environment".into()],
    };
    let response = pool.call(request()).await.unwrap();
    assert_eq!(response["value_json"], r#"{"relayed":true}"#);

    let denied = match request() {
        WorkerRequest::Tool {
            component, request, ..
        } => WorkerRequest::Tool {
            component,
            request,
            grants: Vec::new(),
        },
        _ => unreachable!(),
    };
    assert!(
        pool.call(denied)
            .await
            .unwrap_err()
            .to_string()
            .contains("not granted environment")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_keeps_and_releases_agentloop_instances() {
    let pool = WorkerPool::new(env!("CARGO_BIN_EXE_component-host"), 2)
        .await
        .unwrap();
    let path = component_path("agentloop");
    let bytes = std::fs::read(&path).unwrap();
    let request = || WorkerRequest::Agentloop {
        instance_id: "session-1".into(),
        component: ComponentSource {
            path: path.clone(),
            sha256: component_digest(&bytes),
        },
        request: agentloop::aex::agentloop::types::Activation {
            operation_id: "activation".into(),
            session_id: "session-1".into(),
            kind: "message".into(),
            payload_json: "{}".into(),
            config_json: r#"{"track":true}"#.into(),
            deadline_at_ms: u64::MAX,
        },
    };
    assert_eq!(
        pool.call(request()).await.unwrap()["payload_json"],
        r#"{"activations":1}"#
    );
    assert_eq!(
        pool.call(request()).await.unwrap()["payload_json"],
        r#"{"activations":2}"#
    );
    pool.call(WorkerRequest::Release {
        world: "agentloop".into(),
        instance_id: "session-1".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        pool.call(request()).await.unwrap()["payload_json"],
        r#"{"activations":1}"#
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_keeps_environment_and_model_lifecycles_resident() {
    let pool = WorkerPool::new(env!("CARGO_BIN_EXE_component-host"), 1)
        .await
        .unwrap();
    let environment_path = component_path("environment");
    let environment_bytes = std::fs::read(&environment_path).unwrap();
    let resolved = pool
        .call(WorkerRequest::EnvironmentResolve {
            instance_id: "environment-1".into(),
            component: ComponentSource {
                path: environment_path,
                sha256: component_digest(&environment_bytes),
            },
            request: environment::aex::environment::types::ResolveRequest {
                tenant_id: "tenant-1".into(),
                session_id: "session-1".into(),
                environment_id: "workspace".into(),
                config_json: "{}".into(),
                authority_json: "{}".into(),
            },
        })
        .await
        .unwrap();
    let binding = resolved["binding_json"].as_str().unwrap().to_owned();
    let submitted = pool
        .call(WorkerRequest::EnvironmentSubmit {
            instance_id: "environment-1".into(),
            binding_json: binding.clone(),
            operation: environment::aex::environment::types::Operation {
                operation_id: "operation-1".into(),
                kind: "invoke".into(),
                descriptor_json: "{}".into(),
                bundle: None,
                input_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        })
        .await
        .unwrap();
    let provider_id = submitted["provider_operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let observed = pool
        .call(WorkerRequest::EnvironmentObserve {
            instance_id: "environment-1".into(),
            binding_json: binding.clone(),
            provider_operation_id: provider_id.clone(),
            cursor: None,
        })
        .await
        .unwrap();
    let terminal = observed["terminal_json"].as_str().unwrap().to_owned();
    pool.call(WorkerRequest::EnvironmentAcknowledge {
        instance_id: "environment-1".into(),
        binding_json: binding.clone(),
        provider_operation_id: provider_id,
        terminal_json: terminal,
    })
    .await
    .unwrap();
    pool.call(WorkerRequest::EnvironmentRelease {
        instance_id: "environment-1".into(),
        binding_json: binding,
    })
    .await
    .unwrap();

    let model_path = component_path("model");
    let model_bytes = std::fs::read(&model_path).unwrap();
    let started = pool
        .call(WorkerRequest::ModelStart {
            instance_id: "model-1".into(),
            component: ComponentSource {
                path: model_path,
                sha256: component_digest(&model_bytes),
            },
            request: model::aex::model::types::Request {
                operation_id: "model-operation-1".into(),
                model: "test".into(),
                messages_json: "[]".into(),
                tools_json: "[]".into(),
                response_format_json: None,
                generation_json: "{}".into(),
                provider_options_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        })
        .await
        .unwrap();
    let provider_id = started["provider_operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let observed = pool
        .call(WorkerRequest::ModelObserve {
            instance_id: "model-1".into(),
            provider_operation_id: provider_id.clone(),
            cursor: None,
        })
        .await
        .unwrap();
    pool.call(WorkerRequest::ModelAcknowledge {
        instance_id: "model-1".into(),
        provider_operation_id: provider_id,
        terminal_json: observed["terminal_json"].as_str().unwrap().to_owned(),
    })
    .await
    .unwrap();
}
