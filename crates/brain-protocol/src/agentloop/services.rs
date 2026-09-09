//! Payload for the Agentloop emit service.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnEmitRequest {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KvPutRequest {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub key: String,
    pub value: serde_json::Value,
}
