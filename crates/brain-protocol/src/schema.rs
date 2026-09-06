//! Schema fragments the derives cannot say on their own.

use schemars::{Schema, SchemaGenerator, json_schema};

use crate::IDENTIFIER_PATTERN;

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

/// What a Tool or an Agentloop needs from its Environment: a bounded list of URIs.
/// Brain checks the shape and nothing else; the Environment honours or refuses each one.
pub(crate) fn needs(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "maxItems": 64,
        "uniqueItems": true,
        "items": { "type": "string", "format": "uri", "minLength": 1, "maxLength": 2048 }
    })
}
