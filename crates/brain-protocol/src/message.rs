//! The provider-neutral message model.
//!
//! History is stored once, in this shape, and rendered per dialect at request
//! build time. Storing a provider's wire shape and translating to the others
//! would make one dialect a second-class citizen and would put that provider's
//! schema into the journal, where it would then be frozen forever.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        /// ALWAYS set on a failed tool. Omitting the flag on a failure lets the
        /// model read that failure as a success.
        is_error: bool,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            content: vec![ContentBlock::text(text)],
        }
    }

    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Message {
            role: Role::Assistant,
            content,
        }
    }

    /// Tool results ride in a `user` message: that is where the Anthropic
    /// dialect requires them, and the OpenAI dialect splits them back out into
    /// `tool` role messages at render time.
    pub fn tool_results(blocks: Vec<ContentBlock>) -> Self {
        Message {
            role: Role::User,
            content: blocks,
        }
    }

    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &serde_json::Value)> {
        self.content.iter().filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Refusal,
    /// The provider ended the stream without a terminal reason. Distinct from
    /// EndTurn on purpose: absent is never zero.
    #[default]
    Unknown,
}

/// Provider-reported usage. Every field is `Option` because **absent is never
/// zero** -- a provider that does not report cache reads is not a provider that
/// read zero cache tokens.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    /// Folds another usage report in, field by field. Fails on overflow rather
    /// than wrapping: a wrapped token count is a billing lie.
    pub fn merge(&mut self, other: &Usage) -> Result<(), &'static str> {
        fn add(a: &mut Option<u64>, b: Option<u64>) -> Result<(), &'static str> {
            if let Some(b) = b {
                *a = Some(
                    a.unwrap_or(0)
                        .checked_add(b)
                        .ok_or("provider usage overflowed u64")?,
                );
            }
            Ok(())
        }
        let mut merged = *self;
        add(&mut merged.input_tokens, other.input_tokens)?;
        add(&mut merged.output_tokens, other.output_tokens)?;
        add(
            &mut merged.cache_read_input_tokens,
            other.cache_read_input_tokens,
        )?;
        add(
            &mut merged.cache_creation_input_tokens,
            other.cache_creation_input_tokens,
        )?;
        add(&mut merged.reasoning_tokens, other.reasoning_tokens)?;
        *self = merged;
        Ok(())
    }
}
