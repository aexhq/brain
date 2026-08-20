//! Resolution of the public definition/executor Tool contract.
//!
//! There is deliberately no builtin registry here. Official and third-party tools arrive through
//! the same ordered `ToolConfig` array, and dispatch is derived only from the sealed executor
//! descriptor. Model-visible names are data, never switches in the core.

use crate::config::{HandToolSeal, ServerToolPolicy, ToolDecl, ToolRoute};
use crate::{BrainError, Result};
use brain_protocol::abi::{
    ToolExecutable, ToolExecutableSource, ToolManifest, ToolManifestVersion, ToolSpec, ToolSpecName,
};
use brain_protocol::session::{HandToolSource, ToolConfig, ToolExecutor};
use serde::Deserialize;

/// Resolve native tools in exact declaration order. Kind discriminator strings and protocol
/// constants are checked here because generated Rust structs intentionally preserve JSON `const`
/// fields as strings/integers.
pub fn resolve(items: &[ToolConfig]) -> Result<Vec<ToolDecl>> {
    items.iter().map(resolve_one).collect()
}

fn resolve_one(tool: &ToolConfig) -> Result<ToolDecl> {
    let name = tool.definition.name.to_string();
    let input_schema = serde_json::Value::Object(tool.definition.input_schema.clone());
    let output_schema = serde_json::Value::Object(tool.definition.output_schema.clone());
    validate_schema(&name, "input", &input_schema)?;
    validate_schema(&name, "output", &output_schema)?;
    let route = match &tool.executor {
        ToolExecutor::HandToolExecutor(exec) => {
            if exec.kind != "hand" || exec.protocol != 1 {
                return Err(BrainError::Invalid(format!(
                    "tool {name}: unsupported Hand executor descriptor"
                )));
            }
            ToolRoute::Hand(HandToolSeal {
                protocol: exec.protocol,
                checksum: exec.checksum.to_string(),
                source: exec.source,
                required_env: exec
                    .required_env
                    .iter()
                    .cloned()
                    .map(String::from)
                    .collect(),
            })
        }
        ToolExecutor::AttachedToolExecutor(exec) => {
            if exec.kind != "attached" {
                return Err(BrainError::Invalid(format!(
                    "tool {name}: invalid attached executor kind"
                )));
            }
            ToolRoute::Attached {
                callback_id: exec.callback_id.to_string(),
            }
        }
        ToolExecutor::ServerToolExecutor(exec) => {
            if exec.kind != "server" {
                return Err(BrainError::Invalid(format!(
                    "tool {name}: invalid server executor kind"
                )));
            }
            ToolRoute::Server(ServerToolPolicy {
                capability: exec.capability.to_string(),
                scope: exec.scope,
                completion: exec.completion,
                effect: exec.effect,
                max_input_bytes: exec.max_input_bytes.get() as usize,
            })
        }
        ToolExecutor::IntrinsicToolExecutor(exec) => {
            if exec.kind != "intrinsic" {
                return Err(BrainError::Invalid(format!(
                    "tool {name}: invalid intrinsic executor kind"
                )));
            }
            ToolRoute::Intrinsic(exec.capability.to_string())
        }
        ToolExecutor::McpToolExecutor(exec) => {
            if exec.kind != "mcp" {
                return Err(BrainError::Invalid(format!(
                    "tool {name}: invalid MCP executor kind"
                )));
            }
            ToolRoute::Mcp {
                server: exec.server.clone(),
                remote_name: exec.remote_name.clone(),
            }
        }
    };

    Ok(ToolDecl {
        name,
        description: tool.definition.description.to_string(),
        input_schema,
        output_schema,
        route,
    })
}

fn validate_schema(name: &str, boundary: &str, schema: &serde_json::Value) -> Result<()> {
    jsonschema::draft202012::new(schema).map_err(|error| {
        BrainError::Invalid(format!("tool {name} {boundary}_schema is invalid: {error}"))
    })?;
    Ok(())
}

/// Validate one model-produced input at the sealed execution boundary. The call intent has
/// already been journaled when this is used, but invalid input never reaches an executor.
pub fn input_error(tool: &ToolDecl, input: &serde_json::Value) -> Option<String> {
    validation_error(&tool.name, "input", &tool.input_schema, input)
}

/// Enforce the successful structured result immediately before the caller commits it. Adapters
/// may format `content` for the model, but the independently carried value is authoritative for
/// the output contract.
pub fn enforce_output(
    tool: &ToolDecl,
    outcome: crate::adapter::CallOutcome,
) -> crate::adapter::CallOutcome {
    if outcome.is_error || outcome.outcome != "completed" {
        return outcome;
    }
    let Some(value) = outcome.value.as_ref() else {
        let mut failed = crate::adapter::CallOutcome::failed(format!(
            "tool {} completed without a structured output value",
            tool.name
        ));
        failed.duration_ms = outcome.duration_ms;
        return failed;
    };
    let Some(error) = validation_error(&tool.name, "output", &tool.output_schema, value) else {
        return outcome;
    };
    let mut failed = crate::adapter::CallOutcome::failed(error);
    failed.duration_ms = outcome.duration_ms;
    failed
}

fn validation_error(
    name: &str,
    boundary: &str,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Option<String> {
    let validator = match jsonschema::draft202012::new(schema) {
        Ok(validator) => validator,
        Err(error) => {
            return Some(format!(
                "tool {name} sealed {boundary}_schema is invalid: {error}"
            ));
        }
    };
    validator
        .iter_errors(value)
        .next()
        .map(|error| format!("tool {name} {boundary}{}: {error}", error.instance_path()))
}

/// Names of the resolved tools, for the HEAD prefix doc.
pub fn names(decls: &[ToolDecl]) -> Vec<String> {
    decls.iter().map(|d| d.name.clone()).collect()
}

/// The digest the brain seals and sends in `hello`. Any hand that cannot serve exactly this
/// manifest fails the session (`tool_manifest_mismatch`).
pub fn hand_manifest(decls: &[ToolDecl]) -> Result<ToolManifest> {
    let mut tools = Vec::new();
    for decl in decls {
        let ToolRoute::Hand(seal) = &decl.route else {
            continue;
        };
        let input_schema = decl.input_schema.as_object().cloned().ok_or_else(|| {
            BrainError::Invalid(format!("tool {} input_schema must be an object", decl.name))
        })?;
        let output_schema = decl.output_schema.as_object().cloned().ok_or_else(|| {
            BrainError::Invalid(format!(
                "tool {} output_schema must be an object",
                decl.name
            ))
        })?;
        tools.push(ToolSpec {
            name: ToolSpecName::try_from(decl.name.clone())
                .map_err(|e| BrainError::Invalid(format!("tool name: {e}")))?,
            description: decl.description.clone(),
            input_schema,
            output_schema,
            streams: None,
            executable: ToolExecutable {
                protocol: seal.protocol,
                checksum: seal.checksum.clone().try_into().map_err(|e| {
                    BrainError::Invalid(format!("tool {} checksum: {e}", decl.name))
                })?,
                source: match seal.source {
                    HandToolSource::Bundle => ToolExecutableSource::Bundle,
                    HandToolSource::Preinstalled => ToolExecutableSource::Preinstalled,
                },
                bytes: None,
                get_url: None,
                required_env: seal
                    .required_env
                    .iter()
                    .map(|name| name.parse())
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|error| {
                        BrainError::Invalid(format!(
                            "tool {} required environment name: {error}",
                            decl.name
                        ))
                    })?,
            },
        });
    }
    Ok(ToolManifest {
        version: ToolManifestVersion::X1,
        tools,
    })
}

pub fn manifest_digest(manifest: &ToolManifest) -> String {
    brain_protocol::tools::manifest_digest(manifest).to_string()
}

// ---------------------------------------------------------------------------------------------
// task -- brain-side: a self-similar child agent inside the parent's turn (slice-8 spec)
// ---------------------------------------------------------------------------------------------

/// The model-supplied input of one `task` call.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    /// Short label for events and dashboards; never sent to the child.
    pub description: String,
    /// The child's seed user message.
    pub prompt: String,
}

/// The per-call metadata the dispatcher carries; also what error text the model sees when a
/// tool cannot run at all.
pub fn undeclared(name: &str) -> String {
    format!("tool {name} is not declared in this session's sealed tool set")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_tools_resolve_to_an_empty_set() {
        assert!(resolve(&[]).unwrap().is_empty());
    }

    #[test]
    fn arbitrary_names_resolve_from_executor_descriptors_in_exact_order() {
        let items: Vec<ToolConfig> = serde_json::from_value(serde_json::json!([
            {
                "definition": {
                    "name": "run_anything",
                    "description": "A test Hand tool.",
                    "input_schema": {"type":"object"},
                    "output_schema": {"type":"object"}
                },
                "executor": {
                    "kind":"hand", "protocol":1,
                    "checksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "source":"bundle", "required_env":["TOKEN"]
                }
            },
            {
                "definition": {
                    "name": "delegate_anything",
                    "description": "A test intrinsic.",
                    "input_schema": {"type":"object"},
                    "output_schema": {"type":"string"}
                },
                "executor": {"kind":"intrinsic", "capability":"brain.subagents"}
            }
        ]))
        .unwrap();
        let decls = resolve(&items).unwrap();
        assert_eq!(names(&decls), ["run_anything", "delegate_anything"]);
        assert!(matches!(decls[0].route, ToolRoute::Hand(_)));
        assert!(matches!(
            &decls[1].route,
            ToolRoute::Intrinsic(capability) if capability == "brain.subagents"
        ));
        let manifest = hand_manifest(&decls).unwrap();
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(&*manifest.tools[0].name, "run_anything");
        assert_eq!(
            manifest.tools[0].executable.required_env[0].as_str(),
            "TOKEN"
        );
    }

    #[test]
    fn manifest_digest_matches_the_pin() {
        let m = brain_protocol::tools::manifest_v1();
        assert_eq!(
            *brain_protocol::tools::manifest_digest(m),
            manifest_digest(m)
        );
    }

    #[test]
    fn schemas_are_compiled_at_create_and_values_are_checked_at_both_boundaries() {
        let item: ToolConfig = serde_json::from_value(serde_json::json!({
            "definition": {
                "name": "arbitrary_name",
                "description": "Schema gate.",
                "input_schema": {
                    "type": "object",
                    "properties": {"value": {"type": "integer"}},
                    "required": ["value"]
                },
                "output_schema": {
                    "type": "object",
                    "properties": {"ok": {"type": "boolean"}},
                    "required": ["ok"]
                }
            },
            "executor": {"kind": "attached", "callback_id": "schema-gate"}
        }))
        .unwrap();
        let tool = resolve(&[item]).unwrap().remove(0);
        assert!(input_error(&tool, &serde_json::json!({"value": "wrong"})).is_some());
        assert!(input_error(&tool, &serde_json::json!({"value": 1})).is_none());

        let valid = crate::adapter::CallOutcome {
            outcome: "completed".into(),
            value: Some(serde_json::json!({"ok": true})),
            content: "ok".into(),
            is_error: false,
            exit_code: None,
            duration_ms: 7,
            truncated: false,
            terminal: None,
        };
        assert_eq!(enforce_output(&tool, valid).outcome, "completed");
        let invalid = crate::adapter::CallOutcome {
            outcome: "completed".into(),
            value: Some(serde_json::json!({"ok": "wrong"})),
            content: "bad".into(),
            is_error: false,
            exit_code: None,
            duration_ms: 7,
            truncated: false,
            terminal: None,
        };
        let invalid = enforce_output(&tool, invalid);
        assert_eq!(invalid.outcome, "failed");
        assert!(invalid.content.contains("arbitrary_name output"));

        let malformed: ToolConfig = serde_json::from_value(serde_json::json!({
            "definition": {
                "name": "malformed",
                "description": "Bad schema.",
                "input_schema": {"type": "not-a-json-schema-type"},
                "output_schema": {"type": "object"}
            },
            "executor": {"kind": "attached", "callback_id": "malformed"}
        }))
        .unwrap();
        assert!(resolve(&[malformed]).is_err());
    }
}
