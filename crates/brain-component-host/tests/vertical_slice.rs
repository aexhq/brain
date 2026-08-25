use std::path::PathBuf;

use brain_component_host::{ComponentRuntime, agentloop, environment, model, tool};

fn component(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("guest/dist")
        .join(format!("{name}.component.wasm"));
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
