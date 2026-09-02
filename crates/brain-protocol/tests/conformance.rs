use std::{fs, path::PathBuf};

use brain_protocol::{
    CreateSessionRequest, Decision, EnvironmentCommand, EnvironmentReceipt, EnvironmentRequest,
    EnvironmentResponse, Outcome, ToolManifest,
};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(root().join(path)).unwrap()).unwrap()
}

fn validate_definition(schema_path: &str, definition: &str, value: &Value) {
    assert!(
        definition_is_valid(schema_path, definition, value),
        "{definition} example failed validation"
    );
}

fn definition_is_valid(schema_path: &str, definition: &str, value: &Value) -> bool {
    let schema = read_json(schema_path);
    let wrapper = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": schema["$defs"],
        "$ref": format!("#/$defs/{definition}")
    });
    jsonschema::draft202012::new(&wrapper)
        .unwrap()
        .validate(value)
        .is_ok()
}

#[test]
fn contract_schemas_are_valid_draft_2020_12() {
    for path in [
        "contracts/agentloop/v1/contract.json",
        "contracts/environment/v1/schemas.json",
        "contracts/tool/v1/schemas.json",
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

    let environment_schema =
        jsonschema::draft202012::new(&read_json("contracts/environment/v1/schemas.json")).unwrap();
    for example in [
        "contracts/environment/v1/examples/invoke.json",
        "contracts/environment/v1/examples/invoke-result.json",
        "contracts/environment/v1/examples/attach.json",
        "contracts/environment/v1/examples/attach-result.json",
    ] {
        environment_schema
            .validate(&read_json(example))
            .unwrap_or_else(|error| panic!("{example}: {error}"));
    }

    let manifest = read_json("contracts/tool/v1/examples/manifest.json");
    jsonschema::draft202012::new(&read_json("contracts/tool/v1/schemas.json"))
        .unwrap()
        .validate(&manifest)
        .unwrap();

    let session = read_json("contracts/session/v1/examples/create-session.json");
    validate_definition(
        "contracts/session/v1/schemas.json",
        "CreateSessionRequest",
        &session,
    );
}

/// A manifest always describes a provisioned program: a hosting axis is not a manifest
/// field, and a manifest without a program is rejected outright.
#[test]
fn a_manifest_without_a_program_is_rejected() {
    let schema =
        jsonschema::draft202012::new(&read_json("contracts/tool/v1/schemas.json")).unwrap();
    let mut manifest = read_json("contracts/tool/v1/examples/manifest.json");
    manifest["hosting"] = serde_json::json!("client");
    assert!(schema.validate(&manifest).is_err());
    manifest.as_object_mut().unwrap().remove("hosting");
    schema.validate(&manifest).unwrap();
    manifest.as_object_mut().unwrap().remove("program");
    assert!(schema.validate(&manifest).is_err());
    let mut needs = read_json("contracts/tool/v1/examples/manifest.json");
    needs["needs"] = serde_json::json!(["../fs"]);
    assert!(schema.validate(&needs).is_err());
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
    assert_eq!(session.model.provider, "vercel-ai-gateway");
    assert_eq!(session.model.name, "openai/gpt-5-mini");
    assert_eq!(session.agentloop.identity.as_str(), "a".repeat(64));
    assert_eq!(session.tools.len(), 1);
    assert_eq!(session.tools[0].name, "read");
    assert_eq!(
        session.tools[0].environment_id.as_ref(),
        Some(&session.environments[0].environment_id)
    );
    assert_eq!(session.tools[0].needs, vec!["fs"]);
    assert!(session.tools[0].program.is_some());

    let command: EnvironmentCommand<EnvironmentRequest> =
        serde_json::from_value(read_json("contracts/environment/v1/examples/invoke.json")).unwrap();
    assert!(matches!(
        command.operation.request,
        EnvironmentRequest::Invoke { .. }
    ));
    let attach: EnvironmentCommand<EnvironmentRequest> =
        serde_json::from_value(read_json("contracts/environment/v1/examples/attach.json")).unwrap();
    assert!(matches!(
        attach.operation.request,
        EnvironmentRequest::Attach { .. }
    ));
    let response: EnvironmentResponse = serde_json::from_value(read_json(
        "contracts/environment/v1/examples/invoke-result.json",
    ))
    .unwrap();
    assert!(matches!(
        response.receipt,
        EnvironmentReceipt::Outcome {
            outcome: Outcome::Ok { .. }
        }
    ));
    let manifest: ToolManifest =
        serde_json::from_value(read_json("contracts/tool/v1/examples/manifest.json")).unwrap();
    assert_eq!(manifest.name, "bash");
    assert_eq!(manifest.binding_names, vec!["API_BASE"]);

    let decision: Decision = serde_json::from_value(
        read_json("contracts/agentloop/v1/examples/finish.json")["output"]["decision"].clone(),
    )
    .unwrap();
    assert!(matches!(decision, Decision::Finish { result: None }));
}

#[test]
fn model_selection_names_are_validated_per_provider() {
    let selection = |provider: &str, name: &str| serde_json::json!({"provider": provider, "name": name, "api_key": "k"});
    let validate = |value: &Value| {
        let schema = read_json("contracts/session/v1/schemas.json");
        let wrapper = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": schema["$defs"],
            "$ref": "#/$defs/ModelSelection"
        });
        jsonschema::draft202012::new(&wrapper)
            .unwrap()
            .validate(value)
            .is_ok()
    };
    // The contract stops naming providers: which ones a deployment admits is a
    // property of its composed registry, enforced server-side. The schema keeps
    // only the shape rules.
    assert!(validate(&selection(
        "vercel-ai-gateway",
        "openai/gpt-5-mini"
    )));
    assert!(validate(&selection("openai", "gpt-5-mini")));
    assert!(validate(&selection("anthropic", "claude-sonnet-4-5")));
    assert!(
        validate(&selection("bedrock", "some-model")),
        "an identifier-shaped provider the schema has never heard of passes; admission is the server's job"
    );
    assert!(!validate(&selection("anthropic", "claude sonnet")));
    assert!(!validate(&selection("not a provider", "model")));
    assert!(!validate(&selection("", "model")));
}

/// A client-hosted tool is answered by an application process off the serve feed: it
/// can carry no program, needs no resources, and unlike provisioned hosting it names no
/// environment — the schema enforces both directions of the environment rule.
#[test]
fn a_client_tool_binds_no_environment_and_ships_no_program() {
    let session = read_json("contracts/session/v1/examples/create-session.json");
    let mut tool = session["tools"][0].clone();
    tool["hosting"] = serde_json::json!("client");
    tool["needs"] = serde_json::json!([]);
    tool.as_object_mut().unwrap().remove("program");
    let mut request = session.clone();
    // A client tool still naming an environment is contradictory.
    request["tools"][0] = tool.clone();
    assert!(!definition_is_valid(
        "contracts/session/v1/schemas.json",
        "CreateSessionRequest",
        &request
    ));
    tool.as_object_mut().unwrap().remove("environment_id");
    request["tools"][0] = tool;
    request["environments"] = serde_json::json!([]);
    validate_definition(
        "contracts/session/v1/schemas.json",
        "CreateSessionRequest",
        &request,
    );
    // And a non-client tool without an environment stays rejected.
    let mut bare = session.clone();
    bare["tools"][0]
        .as_object_mut()
        .unwrap()
        .remove("environment_id");
    assert!(!definition_is_valid(
        "contracts/session/v1/schemas.json",
        "CreateSessionRequest",
        &bare
    ));
}
