//! Conformance: the schemas are valid 2020-12, every example validates against the schema type
//! named by its filename (`<TypeName>.<case>.json`), and round-trips byte-for-byte (as JSON
//! values) through the generated Rust types. The tool manifest digest matches its pin.

use std::path::{Path, PathBuf};

use brain_protocol::{abi, session, tools};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn examples(dir: &str) -> Vec<(String, String, Value)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(repo_root().join("contracts/examples").join(dir)).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let type_name = name.split('.').next().unwrap().to_string();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        out.push((name, type_name, value));
    }
    assert!(!out.is_empty(), "no examples under {dir}");
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn schema_for(schema_json: &str, type_name: &str) -> jsonschema::Validator {
    let mut schema: Value = serde_json::from_str(schema_json).unwrap();
    // Point the root at the named definition; $defs stay resolvable through the root.
    schema
        .as_object_mut()
        .unwrap()
        .insert("$ref".into(), Value::String(format!("#/$defs/{type_name}")));
    schema.as_object_mut().unwrap().remove("$id");
    jsonschema::draft202012::new(&schema)
        .unwrap_or_else(|e| panic!("schema is not valid 2020-12 for {type_name}: {e}"))
}

fn round_trip<T: Serialize + DeserializeOwned>(name: &str, value: &Value) {
    let typed: T = serde_json::from_value(value.clone()).unwrap_or_else(|e| {
        panic!(
            "{name}: does not deserialise into {}: {e}",
            std::any::type_name::<T>()
        )
    });
    let back = serde_json::to_value(&typed).unwrap();
    let mut diffs = Vec::new();
    lossless(value, &back, String::new(), &mut diffs);
    assert!(
        diffs.is_empty(),
        "{name}: round trip through {} lost information:
  {}",
        std::any::type_name::<T>(),
        diffs.join(
            "
  "
        )
    );
}

/// `back` must carry everything `example` carries. Two differences are allowed, because both are
/// how the generated Rust types represent the same JSON: a `null` in the example may come back
/// absent (optional fields are `Option` and serialise as absent), and `back` may carry extra keys
/// the example omitted (schema defaults filled in).
fn lossless(example: &Value, back: &Value, path: String, diffs: &mut Vec<String>) {
    match (example, back) {
        (Value::Object(e), Value::Object(b)) => {
            for (k, ev) in e {
                match b.get(k) {
                    Some(bv) => lossless(ev, bv, format!("{path}/{k}"), diffs),
                    None if ev.is_null() => {}
                    None => diffs.push(format!("{path}/{k}: missing after round trip")),
                }
            }
        }
        (Value::Array(e), Value::Array(b)) => {
            if e.len() != b.len() {
                diffs.push(format!("{path}: array length {} -> {}", e.len(), b.len()));
            }
            for (i, (ev, bv)) in e.iter().zip(b).enumerate() {
                lossless(ev, bv, format!("{path}/{i}"), diffs);
            }
        }
        _ => {
            if example != back {
                diffs.push(format!("{path}: {example} -> {back}"));
            }
        }
    }
}

fn validate(schema_json: &str, name: &str, type_name: &str, value: &Value) {
    let validator = schema_for(schema_json, type_name);
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "{name}: schema violations:\n  {}",
        errors.join("\n  ")
    );
}

#[test]
fn schemas_are_valid_2020_12() {
    for (name, json) in [
        ("abi", brain_protocol::ABI_SCHEMA_JSON),
        ("session", brain_protocol::SESSION_SCHEMA_JSON),
    ] {
        let schema: Value = serde_json::from_str(json).unwrap();
        jsonschema::meta::validate(&schema)
            .unwrap_or_else(|e| panic!("{name} schema invalid: {e}"));
    }
}

#[test]
fn abi_examples_validate_and_round_trip() {
    for (name, type_name, value) in examples("abi") {
        validate(brain_protocol::ABI_SCHEMA_JSON, &name, &type_name, &value);
        match type_name.as_str() {
            "Request" => round_trip::<abi::Request>(&name, &value),
            "HandFrame" => round_trip::<abi::HandFrame>(&name, &value),
            "SyncManifest" => round_trip::<abi::SyncManifest>(&name, &value),
            other => panic!("{name}: no round-trip mapping for ABI type {other}; add one"),
        }
    }
}

#[test]
fn session_examples_validate_and_round_trip() {
    for (name, type_name, value) in examples("session") {
        validate(
            brain_protocol::SESSION_SCHEMA_JSON,
            &name,
            &type_name,
            &value,
        );
        match type_name.as_str() {
            "Session" => round_trip::<session::Session>(&name, &value),
            "SessionList" => round_trip::<session::SessionList>(&name, &value),
            "CreateSessionRequest" => round_trip::<session::CreateSessionRequest>(&name, &value),
            "MessageRequest" => round_trip::<session::MessageRequest>(&name, &value),
            "MessageAccepted" => round_trip::<session::MessageAccepted>(&name, &value),
            "ExternalToolCallRequest" => {
                round_trip::<session::ExternalToolCallRequest>(&name, &value)
            }
            "ExternalToolCallResponse" => {
                round_trip::<session::ExternalToolCallResponse>(&name, &value)
            }
            "Event" => round_trip::<session::Event>(&name, &value),
            "ApiErrorResponse" => round_trip::<session::ApiErrorResponse>(&name, &value),
            "Artifact" => round_trip::<session::Artifact>(&name, &value),
            "FileList" => round_trip::<session::FileList>(&name, &value),
            "PersistRequest" => round_trip::<session::PersistRequest>(&name, &value),
            other => panic!("{name}: no round-trip mapping for session type {other}; add one"),
        }
    }
}

#[test]
fn every_abi_op_has_a_request_and_a_reply_example() {
    let ex = examples("abi");
    for op in [
        "hello",
        "start",
        "poll",
        "cancel",
        "release",
        "lane_close",
        "put",
        "persist",
        "sync",
    ] {
        assert!(
            ex.iter()
                .any(|(_, t, v)| t == "Request" && v["call"]["op"] == op),
            "missing Request example for op {op}"
        );
        assert!(
            ex.iter()
                .any(|(_, t, v)| t == "HandFrame" && v["frame"]["result"]["reply"]["op"] == op),
            "missing reply example for op {op}"
        );
    }
}

#[test]
fn every_session_event_type_has_an_example() {
    let schema: Value = serde_json::from_str(brain_protocol::SESSION_SCHEMA_JSON).unwrap();
    let variants: Vec<&str> = schema["$defs"]["Event"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["properties"]["type"]["enum"][0].as_str().unwrap())
        .collect();
    let ex = examples("session");
    for t in variants {
        assert!(
            ex.iter().any(|(_, ty, v)| ty == "Event" && v["type"] == t),
            "missing Event example for {t}"
        );
    }
}

#[test]
fn tool_manifest_validates_and_digest_matches_pin() {
    let value: Value = serde_json::from_str(tools::TOOL_MANIFEST_V1_JSON).unwrap();
    validate(
        brain_protocol::ABI_SCHEMA_JSON,
        "tools/manifest.json",
        "ToolManifest",
        &value,
    );
    round_trip::<abi::ToolManifest>("tools/manifest.json", &value);

    let manifest = tools::manifest_v1();
    let names: Vec<&str> = manifest.tools.iter().map(|t| t.name.as_ref()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "manifest tools must be sorted by name");
    assert_eq!(
        names,
        ["bash", "edit", "glob", "grep", "ls", "read", "write"]
    );
    for tool in &manifest.tools {
        jsonschema::meta::validate(&Value::Object(tool.input_schema.clone()))
            .unwrap_or_else(|e| panic!("{}: input_schema invalid: {e}", *tool.name));
        jsonschema::meta::validate(&Value::Object(tool.output_schema.clone()))
            .unwrap_or_else(|e| panic!("{}: output_schema invalid: {e}", *tool.name));
    }

    let digest = tools::manifest_digest(manifest);
    assert_eq!(
        &*digest,
        tools::TOOL_MANIFEST_V1_DIGEST.trim(),
        "manifest.digest is stale: run tools/gen.sh"
    );
}

#[test]
fn call_hash_matches_every_start_example() {
    // The examples carry call_hash values computed independently (tools/make-examples.py); the
    // TypeScript package checks the same files, so three implementations must agree.
    let mut seen = 0;
    for (name, _, value) in examples("abi") {
        let req: abi::Request = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let abi::RequestCall::Start(start) = req.call {
            assert_eq!(
                tools::call_hash(&start),
                start.call_hash,
                "{name}: call_hash mismatch"
            );
            seen += 1;
        }
    }
    assert!(seen >= 2, "expected at least two start examples");
}

#[test]
fn call_hash_ignores_non_identity_fields_and_key_order() {
    let (_, _, value) = examples("abi")
        .into_iter()
        .find(|(n, _, _)| n == "Request.start-attached.json")
        .unwrap();
    let abi::RequestCall::Start(mut req) =
        serde_json::from_value::<abi::Request>(value).unwrap().call
    else {
        panic!("not a start")
    };
    let h1 = tools::call_hash(&req);
    let mut changed = req.clone();
    changed.detach = !changed.detach;
    assert_ne!(
        tools::call_hash(&changed),
        h1,
        "detach is part of the identity"
    );
    req.operation_id = "op-9999".parse().unwrap();
    req.batch_id = None;
    req.wait_ms = 0;
    req.max_bytes = 0;
    req.correlation = Default::default();
    assert_eq!(
        tools::call_hash(&req),
        h1,
        "operation_id/batch_id/wait_ms/max_bytes/correlation are not"
    );
    let mut reordered = req.clone();
    reordered.input = serde_json::json!({"command": "cargo test 2>&1 | tail -20"});
    assert_eq!(tools::call_hash(&reordered), h1);
}
