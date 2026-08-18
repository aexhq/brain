//! The provider-neutral message model.
//!
//! History is stored once, in this shape, and rendered per dialect at request
//! build time. Storing the Anthropic wire shape and translating to OpenAI (or
//! vice versa) would make one dialect a second-class citizen and would put a
//! provider's schema into the journal, where it would then be frozen forever.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
        content: String,
        /// ALWAYS set on a failed tool, including a failed subagent fan-out.
        /// `runtime-subagent-facts.md` records that Pi's parallel path can drop
        /// this flag; the model then reads a failure as a success.
        is_error: bool,
    },
}

impl ContentBlock {
    pub fn text(s: impl Into<String>) -> Self {
        ContentBlock::Text { text: s.into() }
    }
    /// Approximate resident bytes of this block's owned heap data. Used only by
    /// the memory harness, never for billing -- token accounting is passed
    /// through from the provider (standing constraint, design-decisions.md §3).
    pub fn heap_bytes(&self) -> usize {
        match self {
            ContentBlock::Text { text } => text.capacity(),
            ContentBlock::ToolUse { id, name, input } => {
                id.capacity() + name.capacity() + json_bytes(input)
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => tool_use_id.capacity() + content.capacity(),
        }
    }
}

fn json_bytes(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => 8,
        serde_json::Value::String(s) => s.capacity() + 24,
        serde_json::Value::Array(a) => 24 + a.iter().map(json_bytes).sum::<usize>(),
        serde_json::Value::Object(o) => {
            48 + o
                .iter()
                .map(|(k, v)| k.capacity() + json_bytes(v))
                .sum::<usize>()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user_text(s: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            content: vec![ContentBlock::text(s)],
        }
    }
    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Message {
            role: Role::Assistant,
            content,
        }
    }
    pub fn tool_results(blocks: Vec<ContentBlock>) -> Self {
        // Anthropic requires tool_result blocks to arrive in a `user` message.
        Message {
            role: Role::User,
            content: blocks,
        }
    }
    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &serde_json::Value)> {
        self.content.iter().filter_map(|b| match b {
            ContentBlock::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
    }
    pub fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Message>()
            + self.content.capacity() * std::mem::size_of::<ContentBlock>()
            + self
                .content
                .iter()
                .map(ContentBlock::heap_bytes)
                .sum::<usize>()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    /// The provider ended the stream without a terminal reason. Distinct from
    /// EndTurn on purpose: absent is never zero.
    #[default]
    Unknown,
}

/// Provider-reported usage. Every field is `Option` because **absent is never
/// zero** -- a provider that does not report cache_read is not a provider that
/// read zero cache tokens. Five surveyed products get this wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
}

impl Usage {
    pub fn merge(&mut self, other: &Usage) {
        fn add(a: &mut Option<u64>, b: Option<u64>) {
            if let Some(b) = b {
                *a = Some(a.unwrap_or(0) + b);
            }
        }
        add(&mut self.input_tokens, other.input_tokens);
        add(&mut self.output_tokens, other.output_tokens);
        add(
            &mut self.cache_read_input_tokens,
            other.cache_read_input_tokens,
        );
        add(
            &mut self.cache_creation_input_tokens,
            other.cache_creation_input_tokens,
        );
    }
}
