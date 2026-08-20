use serde_json::{Value, json};

/// Build the same native Hand descriptor a client builds from the published ABI manifest.
pub fn hand_tool(name: &str) -> Value {
    let spec = brain_protocol::tools::manifest_v1()
        .tools
        .iter()
        .find(|spec| *spec.name == name)
        .unwrap_or_else(|| panic!("missing test Hand tool {name}"));
    json!({
        "definition": {
            "name": spec.name,
            "description": spec.description,
            "input_schema": spec.input_schema,
            "output_schema": spec.output_schema
        },
        "executor": {
            "kind": "hand",
            "protocol": spec.executable.protocol,
            "checksum": spec.executable.checksum,
            "source": spec.executable.source,
            "required_env": spec.executable.required_env
        }
    })
}

/// Native descriptor for Brain's deliberately selected subagent intrinsic.
#[allow(dead_code)]
pub fn subagents_tool() -> Value {
    json!({
        "definition": {
            "name": "task",
            "description": "Delegate a bounded task to a child agent in this session.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "description": {"type": "string", "minLength": 1},
                    "prompt": {"type": "string", "minLength": 1}
                },
                "required": ["description", "prompt"],
                "additionalProperties": false
            },
            "output_schema": {
                "type": "object",
                "properties": {
                    "agent_id": {"type": "string"},
                    "outcome": {"enum": ["completed", "failed", "cancelled"]},
                    "summary": {"type": "string"}
                },
                "required": ["agent_id", "outcome", "summary"],
                "additionalProperties": false
            }
        },
        "executor": {"kind": "intrinsic", "capability": "brain.subagents.v1"}
    })
}
