//! Common content and adapter-owned continuation items, preserved in order.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Developer,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        /// HTTPS URL fetched by the model provider.
        url: String,
    },
    File {
        media_type: FileMediaType,
        url: String,
    },
    Native {
        /// Versioned adapter format; incompatible adapters must reject the item.
        format: String,
        data: serde_json::Value,
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
        /// Model-visible media alongside the ordinary JSON Tool output.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<Media>,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Media {
    Image {
        url: String,
    },
    File {
        media_type: FileMediaType,
        url: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub enum FileMediaType {
    #[serde(rename = "application/pdf")]
    Pdf,
}

pub fn validate_media_url(value: &str) -> Result<(), &'static str> {
    let url = url::Url::parse(value).map_err(|_| "invalid media URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("media URL must use HTTPS without credentials");
    }
    Ok(())
}

impl Media {
    pub fn url(&self) -> &str {
        match self {
            Self::Image { url } | Self::File { url, .. } => url,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        validate_media_url(self.url())
    }
}

impl From<Media> for ContentBlock {
    fn from(media: Media) -> Self {
        match media {
            Media::Image { url } => Self::Image { url },
            Media::File { media_type, url } => Self::File { media_type, url },
        }
    }
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
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_cost_usd: Option<String>,
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
        let mut merged = self.clone();
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
        if let Some(cost) = &other.provider_cost_usd {
            match &merged.provider_cost_usd {
                Some(existing) if existing != cost => {
                    return Err("provider reported conflicting costs");
                }
                Some(_) => {}
                None => merged.provider_cost_usd = Some(cost.clone()),
            }
        }
        *self = merged;
        Ok(())
    }
}
