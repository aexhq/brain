use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Message, StopReason, Usage};

/// The model a session calls. The credential is not here: the server seals it under
/// the session id and resolves it at call time.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBinding {
    pub provider: String,
    pub name: String,
}

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub provider: String,
    #[schemars(regex(pattern = r"^\S+$"))]
    pub name: String,
    #[schemars(length(min = 1, max = 16384))]
    pub api_key: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    /// The system prompt for this call. Absent means the one the session was created
    /// with; empty means none. The session fills it in before the call is journalled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// The tools to offer on this call, by name. Absent means every tool the session was
    /// created with; each name given must be one of them. Filled in like `system`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<crate::ToolDefinition>>,
    pub messages: Vec<Message>,
    /// Absent inherits the session default; null resets it; an object sets it.
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Presentation options validated by the selected adapter; never execution authority.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options: std::collections::BTreeMap<String, serde_json::Value>,
}

fn present_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(deserializer).map(Some)
}

/// Dialect-neutral live observations; the completed model result is journaled separately.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStreamEvent {
    NativeStart {
        index: usize,
        format: String,
        data: serde_json::Value,
    },
    NativeDelta {
        index: usize,
        format: String,
        field: String,
        text: String,
    },
    TextDelta {
        index: usize,
        text: String,
    },
    /// A provider-native refusal payload. It is text for diagnostics, but its
    /// refusal semantics remain distinct even when the provider also reports an
    /// ordinary stop reason.
    RefusalDelta {
        index: usize,
        text: String,
    },
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolInputDelta {
        index: usize,
        partial_json: String,
    },
    BlockDone {
        index: usize,
    },
    /// Usage-only provider frame. It may precede content or follow the
    /// terminal stop frame.
    Usage {
        usage: Usage,
    },
    /// Terminal. Carries whatever usage the provider actually reported --
    /// every field `Option`, because absent is never zero.
    MessageDone {
        stop_reason: StopReason,
        usage: Usage,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ModelResult {
    pub message: Message,
    pub stop_reason: StopReason,
    pub usage: Usage,
}
