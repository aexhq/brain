//! Schema fragments the derives cannot say on their own: constants, cross-field rules,
//! and the policy shapes Brain carries without reading.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::{IDENTIFIER_PATTERN, RESOURCE_NAME_PATTERN};

pub(crate) fn identifier(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "pattern": IDENTIFIER_PATTERN })
}

/// A JSON object of any shape: a tool's input or output schema.
pub(crate) fn json_object(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "object" })
}

pub(crate) fn environment_contract(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "const": crate::ENVIRONMENT_CONTRACT })
}

pub(crate) fn agentloop_contract(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "const": crate::AGENTLOOP_CONTRACT_VERSION })
}

/// Request headers an `http` program sends: bounded names and values.
pub(crate) fn http_headers(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "propertyNames": { "maxLength": 256 },
        "additionalProperties": { "type": "string", "maxLength": 16384 }
    })
}

/// A session summary always carries its share key on the wire: the serving layer mints
/// it before the summary leaves the process. The field defaults in Rust only so that a
/// summary can exist before that point.
pub(crate) fn share_key_always_present(schema: &mut Schema) {
    if let Some(serde_json::Value::Array(required)) = schema.get_mut("required") {
        required.push(serde_json::json!("share_key"));
    }
    if let Some(share_key) = schema.pointer_mut("/properties/share_key") {
        share_key
            .as_object_mut()
            .map(|field| field.remove("default"));
    }
}

/// Binding values by name, as they travel to an environment. Plaintext on that wire
/// only: the journal never holds them.
pub(crate) struct BindingValues;

impl JsonSchema for BindingValues {
    fn schema_name() -> Cow<'static, str> {
        "BindingValues".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "propertyNames": { "type": "string", "pattern": IDENTIFIER_PATTERN },
            "additionalProperties": { "type": "string", "maxLength": 32768 }
        })
    }
}

/// The published shape of [`crate::Resources`]. The contract fixes the policy block of
/// each named resource; vendor resources are namespaced and opaque. Brain compares
/// names only and never reads the blocks, so the shapes are the contract's word rather
/// than Rust types.
pub(crate) struct ResourcePolicies;

impl JsonSchema for ResourcePolicies {
    fn schema_name() -> Cow<'static, str> {
        "Resources".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let identifier = json_schema!({ "type": "string", "pattern": IDENTIFIER_PATTERN });
        // A vendor resource is the namespaced form of a resource name, with the
        // namespace no longer optional.
        let vendor = RESOURCE_NAME_PATTERN
            .replacen("(:", ":", 1)
            .replacen(")?$", "$", 1);
        json_schema!({
            "type": "object",
            "properties": {
                "fs": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["root"],
                    "properties": { "root": { "type": "string", "minLength": 1, "maxLength": 4096 } }
                },
                "process": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "timeout_ms_max": { "type": "integer", "minimum": 1 },
                        "output_bytes_max": { "type": "integer", "minimum": 1 }
                    }
                },
                "net": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["allow"],
                    "properties": {
                        "allow": {
                            "type": "array",
                            "maxItems": 256,
                            "items": { "type": "string", "minLength": 1, "maxLength": 256 }
                        }
                    }
                },
                "dom": { "type": "object", "additionalProperties": false, "properties": {} },
                "secrets": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["names"],
                    "properties": {
                        "names": { "type": "array", "maxItems": 64, "uniqueItems": true, "items": identifier }
                    }
                }
            },
            "patternProperties": { vendor: { "type": "object" } },
            "additionalProperties": false
        })
    }
}

/// The hosting rule of a bound tool, which no single field can say: a client-hosted
/// tool carries no program, needs nothing, and names no environment; every other tool
/// names one.
pub(crate) fn bound_tool_rules(schema: &mut Schema) {
    let client = serde_json::json!({
        "required": ["hosting"],
        "properties": { "hosting": { "const": "client" } }
    });
    schema.insert(
        "allOf".into(),
        serde_json::json!([
            {
                "if": client,
                "then": {
                    "allOf": [
                        { "not": { "required": ["program"] } },
                        { "not": { "required": ["environment_id"] } },
                        { "properties": { "needs": { "maxItems": 0 } } }
                    ]
                }
            },
            { "if": { "not": client }, "then": { "required": ["environment_id"] } }
        ]),
    );
}
