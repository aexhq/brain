use std::{fs, path::PathBuf};

use brain_protocol::{
    CreateSessionRequest, Driver, EnvironmentCommand, EnvironmentReceipt, EnvironmentRequest,
    EnvironmentResponse, Tool, TurnOutput,
};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
        "generated/contract/agentloop/v1/contract.json",
        "generated/contract/environment/v1/schemas.json",
        "generated/contract/tool/v1/schemas.json",
        "generated/contract/session/v1/schemas.json",
    ] {
        jsonschema::meta::validate(&read_json(path))
            .unwrap_or_else(|error| panic!("{path}: {error}"));
    }
}

#[test]
fn checked_in_examples_validate() {
    for (name, example) in read_json("tests/examples/agentloop/host-calls.json")
        .as_object()
        .unwrap()
    {
        validate_definition("generated/contract/session/v1/schemas.json", name, example);
    }
    let agentloop = read_json("tests/examples/agentloop/turn.json");
    jsonschema::draft202012::new(&read_json("generated/contract/agentloop/v1/contract.json"))
        .unwrap()
        .validate(&agentloop)
        .unwrap();

    let environment_schema =
        jsonschema::draft202012::new(&read_json("generated/contract/environment/v1/schemas.json"))
            .unwrap();
    for example in [
        "tests/examples/environment/setup.json",
        "tests/examples/environment/setup-result.json",
        "tests/examples/environment/execute.json",
        "tests/examples/environment/execute-result.json",
    ] {
        environment_schema
            .validate(&read_json(example))
            .unwrap_or_else(|error| panic!("{example}: {error}"));
    }

    let tool = read_json("tests/examples/tool/tool.json");
    jsonschema::draft202012::new(&read_json("generated/contract/tool/v1/schemas.json"))
        .unwrap()
        .validate(&tool)
        .unwrap();

    let session = read_json("tests/examples/session/create-session.json");
    validate_definition(
        "generated/contract/session/v1/schemas.json",
        "CreateSessionRequest",
        &session,
    );
}

/// A Tool has at least one placement; each has its own implementation and URI needs.
#[test]
fn a_tool_names_one_environment_and_its_needs_as_uris() {
    let schema =
        jsonschema::draft202012::new(&read_json("generated/contract/tool/v1/schemas.json"))
            .unwrap();
    let example = read_json("tests/examples/tool/tool.json");
    schema.validate(&example).unwrap();
    let mut without_environment = example.clone();
    without_environment
        .as_object_mut()
        .unwrap()
        .remove("placements");
    assert!(schema.validate(&without_environment).is_err());
    for deleted in ["hosting", "host_id", "binding_names", "environment_id"] {
        let mut old = example.clone();
        old[deleted] = serde_json::json!("x");
        assert!(schema.validate(&old).is_err(), "{deleted} must be refused");
    }
    let mut repeated = example.clone();
    repeated["placements"]["sandbox"]["needs"] =
        serde_json::json!(["pkg:apt/bash", "pkg:apt/bash"]);
    assert!(schema.validate(&repeated).is_err());
    let mut too_many = example.clone();
    too_many["placements"]["sandbox"]["needs"] =
        serde_json::json!((0..65).map(|i| format!("pkg:apt/p{i}")).collect::<Vec<_>>());
    assert!(schema.validate(&too_many).is_err());
    let mut host_tool = example;
    host_tool["placements"]["sandbox"]
        .as_object_mut()
        .unwrap()
        .remove("implementation");
    assert!(
        schema.validate(&host_tool).is_err(),
        "every placement has an explicit implementation"
    );
}

/// An Environment entry says how Brain reaches it beside its own configuration: the
/// driver decides which siblings are required, and the configuration is anything.
#[test]
fn an_environment_carries_its_driver_beside_its_configuration() {
    let valid = |value: Value| {
        definition_is_valid(
            "generated/contract/session/v1/schemas.json",
            "Environment",
            &value,
        )
    };
    assert!(valid(
        serde_json::json!({"name": "brain", "driver": "brain"})
    ));
    assert!(valid(serde_json::json!({
        "name": "app", "driver": "host", "host_id": "host_12345678901234567890", "configuration": {}
    })));
    assert!(valid(serde_json::json!({
        "name": "sandbox", "driver": "http", "url": "https://sandbox.example",
        "credential": "k", "configuration": {"region": "eu"}
    })));
    assert!(!valid(
        serde_json::json!({"name": "sandbox", "driver": "http"})
    ));
    assert!(!valid(serde_json::json!({"name": "app", "driver": "host"})));
    assert!(!valid(
        serde_json::json!({"name": "x", "driver": "elsewhere"})
    ));
    assert!(!valid(serde_json::json!({"driver": "brain"})));
}

#[test]
fn rust_views_round_trip_contract_examples() {
    let session: CreateSessionRequest =
        serde_json::from_value(read_json("tests/examples/session/create-session.json")).unwrap();
    assert_eq!(session.model.provider, "vercel-ai-gateway");
    assert_eq!(session.model.name, "openai/gpt-5-mini");
    assert_eq!(session.agentloop.implementation["id"], "a".repeat(64));
    assert_eq!(session.agentloop.environment.as_str(), "brain");
    assert_eq!(session.system, "Be useful.");
    assert_eq!(session.tools.len(), 1);
    assert_eq!(session.tools[0].name, "read");
    assert!(
        session.tools[0]
            .placements
            .contains_key(&session.environments[1].name)
    );
    assert_eq!(
        session.tools[0].placements.values().next().unwrap().needs,
        vec!["file:///workspace"]
    );
    assert!(
        session.tools[0]
            .placements
            .values()
            .next()
            .unwrap()
            .implementation
            .is_object()
    );
    assert!(matches!(session.environments[0].driver, Driver::Brain {}));
    assert!(matches!(
        &session.environments[1].driver,
        Driver::Http { url, credential: Some(_) } if url == "https://sandbox.example"
    ));

    let command: EnvironmentCommand =
        serde_json::from_value(read_json("tests/examples/environment/execute.json")).unwrap();
    assert!(matches!(
        command.operation.request,
        EnvironmentRequest::Execute { .. }
    ));
    let setup: EnvironmentCommand =
        serde_json::from_value(read_json("tests/examples/environment/setup.json")).unwrap();
    assert!(matches!(
        setup.operation.request,
        EnvironmentRequest::Setup { ref needs, .. } if needs.len() == 3
    ));
    let response: EnvironmentResponse =
        serde_json::from_value(read_json("tests/examples/environment/execute-result.json"))
            .unwrap();
    assert!(matches!(
        response.receipt,
        EnvironmentReceipt::Result { .. }
    ));
    let tool: Tool = serde_json::from_value(read_json("tests/examples/tool/tool.json")).unwrap();
    assert_eq!(tool.name, "bash");
    assert_eq!(tool.placements.keys().next().unwrap().as_str(), "sandbox");
    assert_eq!(tool.definition().name, "bash");

    let output: TurnOutput =
        serde_json::from_value(read_json("tests/examples/agentloop/turn.json")["output"].clone())
            .unwrap();
    assert_eq!(output.transcript.len(), 1);
    assert_eq!(output.kv["memory"]["turns"], 1);
}

#[test]
fn model_selection_names_are_validated_per_provider() {
    let selection = |provider: &str, name: &str| serde_json::json!({"provider": provider, "name": name, "api_key": "k"});
    let validate = |value: &Value| {
        let schema = read_json("generated/contract/session/v1/schemas.json");
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
