//! Resolution of the public definition/executor Tool contract.
//!
//! There is deliberately no builtin registry here. Official and third-party tools arrive through
//! the same ordered `ToolConfig` array, and dispatch is derived only from the sealed executor
//! descriptor. Model-visible names are data, never switches in the core.

use crate::config::{HandToolSeal, ToolDecl, ToolRoute};
use crate::{BrainError, Result};
use brain_protocol::hand::TerminalOutcome;
use brain_protocol::session::{ToolConfig, ToolExecutor};
use serde::Deserialize;

/// Closed engine capabilities implemented by Brain's state machine over typed ports. These are
/// not host-side extension points: an SDK caller may select only these exact identifiers, and
/// model-visible Tool names never participate in dispatch.
pub fn is_direct_engine_capability(capability: &str) -> bool {
    matches!(
        capability,
        "brain.subagents" | "brain.storage" | "brain.sandbox"
    )
}

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
    // The executor union is kind-tagged at the serde layer, so a payload declaring one realm
    // can no longer deserialize as another; no per-arm kind re-checks remain.
    let route = match &tool.executor {
        ToolExecutor::AexManaged {
            bundle_digest,
            required_env,
        } => ToolRoute::Hand(HandToolSeal {
            protocol: 1,
            checksum: bundle_digest.to_string(),
            required_env: required_env.iter().cloned().map(String::from).collect(),
        }),
        ToolExecutor::CustomerApp { registration } => ToolRoute::Customer {
            registration: registration.to_string(),
        },
        ToolExecutor::Engine { capability } => ToolRoute::Intrinsic(capability.to_string()),
    };

    Ok(ToolDecl {
        name,
        description: tool
            .definition
            .description
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_default(),
        contract_digest: tool.definition.contract_digest.to_string(),
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
    if outcome.is_error || outcome.outcome != TerminalOutcome::Completed {
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

/// Apply the one Brain-owned inline terminal bound to every executor route before the result
/// decision is assembled. An effect may have completed, so oversize is converted into a small,
/// honest failed ToolResult rather than allowing the journal transaction itself to fail.
pub fn enforce_terminal_bound(
    tool_name: &str,
    outcome: crate::adapter::CallOutcome,
) -> crate::adapter::CallOutcome {
    let limit = brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES;
    let value_too_large = outcome
        .value
        .as_ref()
        .is_some_and(|value| !brain_protocol::contract::terminal_inline_fits(value));
    let terminal_too_large = outcome.terminal.as_ref().is_some_and(|terminal| {
        let projection = match terminal {
            crate::adapter::TurnTerminal::Complete { value, metadata } => {
                serde_json::json!({"value": value, "metadata": metadata})
            }
            crate::adapter::TurnTerminal::Fail { error } => {
                serde_json::json!({"error": error})
            }
        };
        !brain_protocol::contract::terminal_inline_fits(&projection)
    });
    if outcome.content.len() <= limit && !value_too_large && !terminal_too_large {
        return outcome;
    }
    let mut failed = crate::adapter::CallOutcome::failed(format!(
        "tool {tool_name} returned more than {limit} inline bytes; store large data in session storage or the sandbox and return a key/path"
    ));
    failed.duration_ms = outcome.duration_ms;
    failed.truncated = true;
    failed
}

/// Apply the complete sealed terminal post-processing pipeline. Hot dispatch and crash recovery
/// must call this same function: an executor receipt cannot become journalable or non-journalable
/// merely because Brain restarted between the effect and its canonical commit.
pub fn enforce_outcome(
    tool: Option<&ToolDecl>,
    tool_name: &str,
    outcome: crate::adapter::CallOutcome,
) -> crate::adapter::CallOutcome {
    let outcome = match tool {
        Some(tool) => enforce_output(tool, outcome),
        None => outcome,
    };
    enforce_terminal_bound(tool_name, outcome)
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

// ---------------------------------------------------------------------------------------------
// task -- a Brain-side child agent inside the parent's turn
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
                    "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "input_schema": {"type":"object"},
                    "output_schema": {"type":"object"}
                },
                "executor": {
                    "kind":"aex_managed",
                    "bundle_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "required_env":["TOKEN"]
                }
            },
            {
                "definition": {
                    "name": "delegate_anything",
                    "description": "A test intrinsic.",
                    "contract_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "input_schema": {"type":"object"},
                    "output_schema": {"type":"string"}
                },
                "executor": {"kind":"engine", "capability":"brain.subagents"}
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
        assert!(matches!(
            &decls[0].route,
            ToolRoute::Hand(seal)
                if seal.protocol == 1
                    && seal.required_env == ["TOKEN"]
                    && seal.checksum == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn schemas_are_compiled_at_create_and_values_are_checked_at_both_boundaries() {
        let item: ToolConfig = serde_json::from_value(serde_json::json!({
            "definition": {
                "name": "arbitrary_name",
                "description": "Schema gate.",
                "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
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
            "executor": {"kind": "customer_app", "registration": "schema-gate"}
        }))
        .unwrap();
        let tool = resolve(&[item]).unwrap().remove(0);
        assert!(input_error(&tool, &serde_json::json!({"value": "wrong"})).is_some());
        assert!(input_error(&tool, &serde_json::json!({"value": 1})).is_none());

        let valid = crate::adapter::CallOutcome {
            outcome: TerminalOutcome::Completed,
            value: Some(serde_json::json!({"ok": true})),
            content: "ok".into(),
            is_error: false,
            exit_code: None,
            duration_ms: 7,
            truncated: false,
            terminal: None,
        };
        assert_eq!(
            enforce_output(&tool, valid).outcome,
            TerminalOutcome::Completed
        );
        let invalid = crate::adapter::CallOutcome {
            outcome: TerminalOutcome::Completed,
            value: Some(serde_json::json!({"ok": "wrong"})),
            content: "bad".into(),
            is_error: false,
            exit_code: None,
            duration_ms: 7,
            truncated: false,
            terminal: None,
        };
        let invalid = enforce_output(&tool, invalid);
        assert_eq!(invalid.outcome, TerminalOutcome::Failed);
        assert!(invalid.content.contains("arbitrary_name output"));

        let malformed: ToolConfig = serde_json::from_value(serde_json::json!({
            "definition": {
                "name": "malformed",
                "description": "Bad schema.",
                "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "input_schema": {"type": "not-a-json-schema-type"},
                "output_schema": {"type": "object"}
            },
            "executor": {"kind": "customer_app", "registration": "malformed"}
        }))
        .unwrap();
        assert!(resolve(&[malformed]).is_err());
    }
}
