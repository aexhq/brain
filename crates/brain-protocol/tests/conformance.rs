//! Conformance: the schemas are valid 2020-12, every example validates against the schema type
//! named by its filename (`<TypeName>.<case>.json`), and round-trips byte-for-byte (as JSON
//! values) through the generated Rust types.

use std::path::{Path, PathBuf};

use brain_protocol::{agentloop, contract, environment, session};
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
    // Point the root at the named definition; $defs stay resolvable through the root. Root-level
    // constraints (e.g. a contract-identity const block) describe the whole contract document,
    // not the referenced type, and in 2020-12 they would apply beside `$ref` — strip them.
    let root = schema.as_object_mut().unwrap();
    for key in [
        "$id",
        "type",
        "properties",
        "required",
        "additionalProperties",
    ] {
        root.remove(key);
    }
    root.insert("$ref".into(), Value::String(format!("#/$defs/{type_name}")));
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
        ("environment", contract::ENVIRONMENT_CONTRACT_SCHEMA_JSON),
        ("session", brain_protocol::SESSION_SCHEMA_JSON),
    ] {
        let schema: Value = serde_json::from_str(json).unwrap();
        jsonschema::meta::validate(&schema)
            .unwrap_or_else(|e| panic!("{name} schema invalid: {e}"));
    }
}

#[test]
fn remote_mcp_is_absent_from_the_single_current_contract() {
    let session_schema = brain_protocol::SESSION_SCHEMA_JSON;
    let environment_schema = contract::ENVIRONMENT_CONTRACT_SCHEMA_JSON;
    for removed in [
        "McpServerConfig",
        "McpProtocol",
        "RemoteMcpToolExecutor",
        "remote_mcp",
    ] {
        assert!(
            !session_schema.contains(removed),
            "removed remote MCP vocabulary reappeared in the session contract: {removed}"
        );
        assert!(
            !environment_schema.contains(removed),
            "removed remote MCP vocabulary reappeared in the Environment contract: {removed}"
        );
    }
    let legacy = serde_json::json!({
        "model": {"provider": "openai", "name": "m", "api_key": "secret"},
        "tools": {"mcp": [{"name": "old", "url": "https://example.test"}]}
    });
    assert!(
        schema_for(session_schema, "CreateSessionRequest")
            .validate(&legacy)
            .is_err()
    );
    assert!(serde_json::from_value::<session::CreateSessionRequest>(legacy).is_err());
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
            other => panic!("{name}: no round-trip mapping for session type {other}; add one"),
        }
    }
}

#[test]
fn agentloop_examples_validate_and_round_trip() {
    for (name, type_name, value) in examples("agentloop") {
        validate(
            contract::AGENTLOOP_CONTRACT_SCHEMA_JSON,
            &name,
            &type_name,
            &value,
        );
        match type_name.as_str() {
            "AgentloopSelector" => round_trip::<agentloop::AgentloopSelector>(&name, &value),
            "ActivationRequest" => round_trip::<agentloop::ActivationRequest>(&name, &value),
            "ActivationResult" => round_trip::<agentloop::ActivationResult>(&name, &value),
            "CtxOpRequest" => round_trip::<agentloop::CtxOpRequest>(&name, &value),
            "CtxOpResponse" => round_trip::<agentloop::CtxOpResponse>(&name, &value),
            other => panic!("{name}: no round-trip mapping for agentloop type {other}; add one"),
        }
    }
}

#[test]
fn every_ctx_op_has_request_and_result_examples() {
    let schema: Value = serde_json::from_str(contract::AGENTLOOP_CONTRACT_SCHEMA_JSON).unwrap();
    let ops: Vec<&str> = schema["properties"]["contract"]["properties"]["ops"]["const"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let ex = examples("agentloop");
    for op in ops {
        assert!(
            ex.iter()
                .any(|(_, ty, v)| ty == "CtxOpRequest" && v["op"]["op"] == op),
            "missing CtxOpRequest example for {op}"
        );
        assert!(
            ex.iter()
                .any(|(_, ty, v)| ty == "CtxOpResponse" && v["result"]["op"] == op),
            "missing CtxOpResponse result example for {op}"
        );
    }
}

#[test]
fn contract_digest_files_stay_pinned_to_the_schemas() {
    assert_eq!(
        format!("{}", *contract::environment_contract_digest()),
        contract::ENVIRONMENT_CONTRACT_DIGEST.trim(),
        "environment contract.digest drifted; run tools/generate-protocol.py environment"
    );
    assert_eq!(
        format!("{}", *contract::agentloop_contract_digest()),
        contract::AGENTLOOP_CONTRACT_DIGEST.trim(),
        "agentloop contract.digest drifted; run tools/generate-protocol.py agentloop"
    );
}

#[test]
fn ctx_op_request_digest_covers_effect_fields_and_excludes_op_id() {
    let base: agentloop::CtxOpRequest = serde_json::from_value(serde_json::json!({
        "op_id": "op-k-0001",
        "activation_id": "act-0001",
        "op": { "op": "kv_get", "keys": ["a"] }
    }))
    .unwrap();
    let first = contract::ctx_op_request_digest(&base);

    let renamed: agentloop::CtxOpRequest = serde_json::from_value(serde_json::json!({
        "op_id": "op-k-0002",
        "activation_id": "act-0001",
        "op": { "op": "kv_get", "keys": ["a"] }
    }))
    .unwrap();
    assert_eq!(
        first,
        contract::ctx_op_request_digest(&renamed),
        "op_id must not participate in the request digest"
    );

    let different: agentloop::CtxOpRequest = serde_json::from_value(serde_json::json!({
        "op_id": "op-k-0001",
        "activation_id": "act-0001",
        "op": { "op": "kv_get", "keys": ["b"] }
    }))
    .unwrap();
    assert_ne!(
        first,
        contract::ctx_op_request_digest(&different),
        "op payload must participate in the request digest"
    );
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
fn credential_debug_is_redacted_without_changing_wire_serialization() {
    const API_SECRET: &str = "sentinel-provider-api-key";
    const ENV_SECRET: &str = "sentinel-session-secret";
    const BUNDLE_SECRET: &str = "c2VudGluZWwtYnVuZGxlLWJ5dGVz";
    const URL_SECRET: &str = "sentinel-presigned-signature";
    const HEADER_SECRET: &str = "sentinel-transfer-header";
    const CAPABILITY_SECRET: &str = "secret-capability-sentinel";

    let create_value = serde_json::json!({
        "model": {"provider": "openai", "name": "test-model", "api_key": API_SECRET},
        "secrets": {"TOKEN": ENV_SECRET},
        "agentloop": {
            "source_bundle_sha256": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
            "toolchain": "loop-toolchain-1",
            "bundle_base64": "eA==",
        },
    });
    let create: session::CreateSessionRequest =
        serde_json::from_value(create_value.clone()).unwrap();
    let debug = format!("{create:?}");
    assert!(!debug.contains(API_SECRET));
    assert!(!debug.contains(ENV_SECRET));
    let serialized = serde_json::to_value(&create).unwrap();
    assert_eq!(serialized["model"]["api_key"], API_SECRET);
    assert_eq!(serialized["secrets"]["TOKEN"], ENV_SECRET);

    let layer: session::ToolArtifactLayer = serde_json::from_value(serde_json::json!({
        "bytes": 21,
        "checksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "content_base64": BUNDLE_SECRET,
        "media_type": "application/javascript+esm",
    }))
    .unwrap();
    assert!(!format!("{layer:?}").contains(BUNDLE_SECRET));
    assert_eq!(
        serde_json::to_value(&layer).unwrap()["content_base64"],
        BUNDLE_SECRET
    );

    let prepare_value = serde_json::json!({
        "session_id": "session-1",
        "root_id": "root-1",
        "bindings": [],
        "bundles": [{
            "bundle_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "url": format!("https://objects.example.test/bundle?X-Amz-Signature={URL_SECRET}"),
            "headers": {"Authorization": HEADER_SECRET},
            "expires_at_ms": 123456,
            "max_bytes": 4096,
        }],
        "network": {"kind": "none"},
        "resources": {"timeout_ms": 1000, "max_output_bytes": 4096},
        "secret_capability": {
            "capability_ref": CAPABILITY_SECRET,
            "expires_at_ms": 123456,
            "env_names": ["TOKEN"],
        },
    });
    let prepare: environment::PrepareSessionRequest =
        serde_json::from_value(prepare_value.clone()).unwrap();
    let prepare_debug = format!("{prepare:?}");
    for secret in [URL_SECRET, HEADER_SECRET, CAPABILITY_SECRET] {
        assert!(!prepare_debug.contains(secret), "Debug leaked {secret}");
    }
    assert_eq!(serde_json::to_value(&prepare).unwrap(), prepare_value);

    let transfer_value = serde_json::json!({
        "kind": "object",
        "object": {
            "object_id": "object-1",
            "bytes": 12,
            "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        },
        "fetch": {
            "transfer_id": "transfer-1",
            "object_id": "object-1",
            "method": "GET",
            "url": format!("https://objects.example.test/input?X-Amz-Signature={URL_SECRET}"),
            "headers": {"x-transfer-token": HEADER_SECRET},
            "expires_at_ms": 123456,
            "max_bytes": 12,
        },
    });
    let transfer: environment::SandboxFileWriteSource =
        serde_json::from_value(transfer_value.clone()).unwrap();
    let transfer_debug = format!("{transfer:?}");
    assert!(!transfer_debug.contains(URL_SECRET));
    assert!(!transfer_debug.contains(HEADER_SECRET));
    assert_eq!(serde_json::to_value(&transfer).unwrap(), transfer_value);
}

#[test]
fn execution_and_stdin_digests_omit_only_the_self_digest() {
    let zero = "0000000000000000000000000000000000000000000000000000000000000000";
    let one = "1111111111111111111111111111111111111111111111111111111111111111";
    let execution_value = serde_json::json!({
        "target": {
            "kind": "additional",
            "session_id": "session-1",
            "root_id": "root-1",
            "binding_ref": "binding-1",
            "sandbox_id": "sandbox-1",
        },
        "expected_generation": "generation-1",
        "execution_id": "execution-1",
        "request_digest": zero,
        "input": {"command": "printf hello", "interactive": false},
        "resources": {"timeout_ms": 1000, "max_output_bytes": 4096},
        "network": {"kind": "none"},
    });
    let mut execution: environment::SandboxExecutionRequest =
        serde_json::from_value(execution_value).unwrap();
    let first = contract::sandbox_execution_request_digest(&execution);
    execution.request_digest = one.parse().unwrap();
    assert_eq!(
        first,
        contract::sandbox_execution_request_digest(&execution)
    );
    execution.input.command = "printf changed".parse().unwrap();
    assert_ne!(
        first,
        contract::sandbox_execution_request_digest(&execution)
    );

    let stdin_value = serde_json::json!({
        "operation_id": "stdin-1",
        "request_digest": zero,
        "target": {
            "kind": "additional",
            "session_id": "session-1",
            "root_id": "root-1",
            "binding_ref": "binding-1",
            "sandbox_id": "sandbox-1",
        },
        "expected_generation": "generation-1",
        "execution_id": "execution-1",
        "text": "hello",
        "eof": false,
    });
    let mut stdin: environment::WriteStdinRequest = serde_json::from_value(stdin_value).unwrap();
    let first = contract::write_stdin_request_digest(&stdin);
    stdin.request_digest = one.parse().unwrap();
    assert_eq!(first, contract::write_stdin_request_digest(&stdin));
    stdin.text = "hello\n".parse().unwrap();
    assert_ne!(first, contract::write_stdin_request_digest(&stdin));
    stdin.text = "hello".parse().unwrap();
    stdin.eof = true;
    assert_ne!(first, contract::write_stdin_request_digest(&stdin));
}

#[test]
fn file_effect_digests_refresh_transport_authority_without_changing_effect_identity() {
    let zero = "0000000000000000000000000000000000000000000000000000000000000000";
    let sha = "1111111111111111111111111111111111111111111111111111111111111111";
    let target = serde_json::json!({
        "kind": "environment",
        "session_id": "session-1",
        "root_id": "root-1",
        "binding_ref": "binding-1",
    });
    let object = serde_json::json!({
        "object_id": "object-1",
        "bytes": 5,
        "sha256": sha,
        "media_type": "text/plain",
    });
    let authority = serde_json::json!({
        "transfer_id": "download-1",
        "object_id": "object-1",
        "method": "GET",
        "url": "https://objects.example/first",
        "headers": {"authorization": "first"},
        "expires_at_ms": 1000,
        "max_bytes": 5,
    });

    let mut write: environment::SandboxFileWriteRequest =
        serde_json::from_value(serde_json::json!({
            "operation_id": "write-1",
            "request_digest": zero,
            "target": target,
            "expected_generation": "generation-1",
            "path": "/workspace/file.txt",
            "source": {"kind": "object", "object": object, "fetch": authority},
            "overwrite": false,
        }))
        .unwrap();
    let write_digest = contract::sandbox_file_write_request_digest(&write);
    let write_value = serde_json::to_value(&write).unwrap();
    let mut refreshed_write = write_value.clone();
    let fetch = refreshed_write["source"]["fetch"].as_object_mut().unwrap();
    fetch.insert("transfer_id".into(), serde_json::json!("download-2"));
    fetch.insert(
        "url".into(),
        serde_json::json!("https://objects.example/second"),
    );
    fetch.insert(
        "headers".into(),
        serde_json::json!({"authorization": "second"}),
    );
    fetch.insert("expires_at_ms".into(), serde_json::json!(2000));
    write = serde_json::from_value(refreshed_write).unwrap();
    assert_eq!(
        write_digest,
        contract::sandbox_file_write_request_digest(&write),
        "a refreshed download capability must replay the same immutable object write"
    );
    write.path = "/workspace/other.txt".parse().unwrap();
    assert_ne!(
        write_digest,
        contract::sandbox_file_write_request_digest(&write)
    );

    let mut import: environment::SandboxCopyRequest = serde_json::from_value(serde_json::json!({
        "operation_id": "copy-import-1",
        "request_digest": zero,
        "target": target,
        "expected_generation": "generation-1",
        "path": "/workspace/import.txt",
        "object": object,
        "transfer": authority,
        "direction": "import",
        "overwrite": false,
    }))
    .unwrap();
    let import_digest = contract::sandbox_copy_request_digest(&import);
    import.transfer.transfer_id = "download-2".parse().unwrap();
    import.transfer.url = "https://objects.example/second".parse().unwrap();
    import.transfer.headers = [("authorization".to_owned(), "second".parse().unwrap())]
        .into_iter()
        .collect();
    import.transfer.expires_at_ms = std::num::NonZeroU64::new(2000).unwrap();
    assert_eq!(
        import_digest,
        contract::sandbox_copy_request_digest(&import)
    );
    import.path = "/workspace/other.txt".parse().unwrap();
    assert_ne!(
        import_digest,
        contract::sandbox_copy_request_digest(&import)
    );

    let mut export: environment::SandboxCopyRequest = serde_json::from_value(serde_json::json!({
        "operation_id": "copy-export-1",
        "request_digest": zero,
        "target": target,
        "expected_generation": "generation-1",
        "path": "/workspace/export.txt",
        "object": null,
        "transfer": {
            "transfer_id": "upload-1",
            "object_id": "pending-1",
            "method": "PUT",
            "url": "https://objects.example/first",
            "headers": {"authorization": "first"},
            "expires_at_ms": 1000,
            "max_bytes": 5
        },
        "direction": "export",
        "overwrite": false,
    }))
    .unwrap();
    let export_digest = contract::sandbox_copy_request_digest(&export);
    export.transfer.url = "https://objects.example/second".parse().unwrap();
    export.transfer.headers = [("authorization".to_owned(), "second".parse().unwrap())]
        .into_iter()
        .collect();
    export.transfer.expires_at_ms = std::num::NonZeroU64::new(2000).unwrap();
    assert_eq!(
        export_digest,
        contract::sandbox_copy_request_digest(&export)
    );
    export.transfer.transfer_id = "upload-2".parse().unwrap();
    assert_ne!(
        export_digest,
        contract::sandbox_copy_request_digest(&export)
    );
}
