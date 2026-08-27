use std::{fs, path::PathBuf};

use brain_protocol::{
    CreateSessionRequest, Decision, EnvironmentCommand, EnvironmentReceipt, EnvironmentRequest,
    EnvironmentResponse,
};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(root().join(path)).unwrap()).unwrap()
}

fn validate_definition(schema_path: &str, definition: &str, value: &Value) {
    let schema = read_json(schema_path);
    let wrapper = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": schema["$defs"],
        "$ref": format!("#/$defs/{definition}")
    });
    jsonschema::draft202012::new(&wrapper)
        .unwrap()
        .validate(value)
        .unwrap();
}

#[test]
fn contract_schemas_are_valid_draft_2020_12() {
    for path in [
        "contracts/agentloop/v1/contract.json",
        "contracts/environment/v1/schemas.json",
        "contracts/session/v1/schemas.json",
    ] {
        jsonschema::meta::validate(&read_json(path))
            .unwrap_or_else(|error| panic!("{path}: {error}"));
    }
}

#[test]
fn checked_in_examples_validate() {
    let agentloop = read_json("contracts/agentloop/v1/examples/finish.json");
    jsonschema::draft202012::new(&read_json("contracts/agentloop/v1/contract.json"))
        .unwrap()
        .validate(&agentloop)
        .unwrap();

    let environment = read_json("contracts/environment/v1/examples/execute.json");
    jsonschema::draft202012::new(&read_json("contracts/environment/v1/schemas.json"))
        .unwrap()
        .validate(&environment)
        .unwrap();

    let session = read_json("contracts/session/v1/examples/create-session.json");
    validate_definition(
        "contracts/session/v1/schemas.json",
        "CreateSessionRequest",
        &session,
    );
}

#[test]
fn agentloop_world_has_one_export_and_no_imports() {
    let wit = fs::read_to_string(root().join("contracts/agentloop/v1/agentloop.wit")).unwrap();
    let world = wit.split("world agentloop").nth(1).unwrap();
    assert!(!world.contains("import "));
    assert_eq!(world.matches("export step:").count(), 1);
}

#[test]
fn rust_views_round_trip_contract_examples() {
    let session: CreateSessionRequest = serde_json::from_value(read_json(
        "contracts/session/v1/examples/create-session.json",
    ))
    .unwrap();
    assert_eq!(session.model.binding_id, "model_gateway");

    let command: EnvironmentCommand<EnvironmentRequest> =
        serde_json::from_value(read_json("contracts/environment/v1/examples/execute.json"))
            .unwrap();
    assert!(matches!(
        command.operation.request,
        EnvironmentRequest::Execute { .. }
    ));
    let response: EnvironmentResponse = serde_json::from_value(read_json(
        "contracts/environment/v1/examples/execute-result.json",
    ))
    .unwrap();
    assert!(matches!(
        response.receipt,
        EnvironmentReceipt::ToolResult { .. }
    ));

    let decision: Decision = serde_json::from_value(
        read_json("contracts/agentloop/v1/examples/finish.json")["output"]["decision"].clone(),
    )
    .unwrap();
    assert!(matches!(decision, Decision::Finish { result: None }));
}
