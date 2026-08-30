use serde::{Deserialize, Serialize};

use crate::message::{Message, StopReason, Usage};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBinding {
    pub binding_id: String,
    pub model: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    pub provider: String,
    pub name: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

/// Dialect-neutral streaming events. Journaled verbatim in `model_result`, so
/// the journal never learns a provider's frame shapes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStreamEvent {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelResult {
    pub message: Message,
    pub stop_reason: StopReason,
    pub usage: Usage,
}
