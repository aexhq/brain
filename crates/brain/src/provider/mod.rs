//! The provider seam.
//!
//! Two responsibilities, deliberately split:
//!
//!  * `build_request` is **pure**. Given (sealed prefix, history, key) it
//!    produces the exact bytes that would go on the wire, and does no I/O. This
//!    is the function the cold-start benchmark times: "first constructable model
//!    request" is `build_request` returning `Ok` for round 0 of a session.
//!  * `stream` does the I/O and decodes to a dialect-neutral event stream.
//!
//! Adding a third dialect is one module and one match arm. Nothing outside this
//! module knows what a `tool_use` block or a `tool_calls` array looks like.

pub mod anthropic;
pub mod fake;
pub mod openai;
pub mod sse;

use crate::config::{Dialect, ProviderKey, SealedPrefix};
use crate::message::{ContentBlock, Message, StopReason, Usage};
use crate::{BrainError, Result};
use futures_util::stream::BoxStream;

/// The bytes that would go on the wire, fully formed. Building one requires no
/// network, no credentials validation and no provider round trip.
#[derive(Clone)]
pub struct ModelRequest {
    pub method: &'static str,
    pub url: String,
    /// Header names and values. The credential is in here, so this type has a
    /// redacting `Debug` for the same reason `ProviderKey` does.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ModelRequest {
    pub fn body_len(&self) -> usize {
        self.body.len()
    }
}

impl std::fmt::Debug for ModelRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Dialect-neutral streaming events.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    TextDelta {
        index: usize,
        text: String,
    },
    /// A provider-native refusal payload. It is text for diagnostics, but its refusal semantics
    /// remain distinct even when the provider also reports an ordinary stop reason.
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
    /// Usage-only provider frame. It may precede content or follow the terminal stop frame.
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

#[async_trait::async_trait]
pub trait Provider: Send + Sync + std::fmt::Debug {
    fn dialect(&self) -> Dialect;

    /// Pure. No I/O, no allocation of a client, no DNS.
    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest>;

    async fn stream(
        &self,
        req: ModelRequest,
        outbound: &crate::outbound::Outbound,
    ) -> Result<BoxStream<'static, Result<ProviderEvent>>>;
}

/// Accumulates a dialect-neutral event stream into one complete assistant
/// message.
///
/// A provider response becomes model-visible history **only after a
/// complete assistant message commits**. Partial deltas may be streamed to a
/// client but never become a journal entry, which is why this type exists
/// separately from the journal.
#[derive(Debug, Default)]
pub struct Accumulator {
    blocks: Vec<PartialBlock>,
    pub stop_reason: StopReason,
    pub usage: Usage,
    pub saw_terminal: bool,
    saw_refusal: bool,
    total_bytes: usize,
    tool_calls: usize,
}

pub const MAX_PROVIDER_ASSISTANT_BYTES: usize = 192 * 1024;
pub const MAX_PROVIDER_DELTA_BYTES: usize = 64 * 1024;
pub const MAX_PROVIDER_CONTENT_BLOCKS: usize = 64;
pub const MAX_PROVIDER_TOOL_CALLS: usize = 32;

#[derive(Debug)]
enum PartialBlock {
    Empty,
    Text {
        text: String,
        done: bool,
    },
    Tool {
        id: String,
        name: String,
        json: String,
        done: bool,
    },
}

impl Accumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, ev: ProviderEvent) -> Result<()> {
        if self.saw_terminal && !matches!(ev, ProviderEvent::Usage { .. }) {
            return Err(BrainError::Protocol(
                "provider emitted content or a second terminal event after message completion"
                    .into(),
            ));
        }
        let (index, added_bytes, starts_tool) = match &ev {
            ProviderEvent::TextDelta { index, text }
            | ProviderEvent::RefusalDelta { index, text } => (*index, text.len(), false),
            ProviderEvent::ToolUseStart { index, id, name } => {
                (*index, id.len().saturating_add(name.len()), true)
            }
            ProviderEvent::ToolInputDelta {
                index,
                partial_json,
            } => (*index, partial_json.len(), false),
            ProviderEvent::BlockDone { index } => (*index, 0, false),
            ProviderEvent::Usage { .. } | ProviderEvent::MessageDone { .. } => (0, 0, false),
        };
        if !matches!(
            ev,
            ProviderEvent::Usage { .. } | ProviderEvent::MessageDone { .. }
        ) && index >= MAX_PROVIDER_CONTENT_BLOCKS
        {
            return Err(BrainError::Protocol(format!(
                "provider content block index {index} exceeds {}",
                MAX_PROVIDER_CONTENT_BLOCKS - 1
            )));
        }
        if added_bytes > MAX_PROVIDER_DELTA_BYTES {
            return Err(BrainError::Protocol(format!(
                "provider delta exceeds {MAX_PROVIDER_DELTA_BYTES} bytes"
            )));
        }
        if self.total_bytes.saturating_add(added_bytes) > MAX_PROVIDER_ASSISTANT_BYTES {
            return Err(BrainError::Protocol(format!(
                "provider assistant content exceeds {MAX_PROVIDER_ASSISTANT_BYTES} bytes"
            )));
        }
        if starts_tool && self.tool_calls >= MAX_PROVIDER_TOOL_CALLS {
            return Err(BrainError::Protocol(format!(
                "provider returned more than {MAX_PROVIDER_TOOL_CALLS} tool calls"
            )));
        }
        match ev {
            ProviderEvent::TextDelta { index, text } => {
                self.ensure(index);
                match &mut self.blocks[index] {
                    PartialBlock::Empty => {
                        self.blocks[index] = PartialBlock::Text { text, done: false };
                    }
                    PartialBlock::Text {
                        text: value,
                        done: false,
                    } => value.push_str(&text),
                    PartialBlock::Text { done: true, .. } => {
                        return Err(BrainError::Protocol(format!(
                            "provider emitted text for completed block {index}"
                        )));
                    }
                    PartialBlock::Tool { .. } => return Err(block_type_conflict(index)),
                }
            }
            ProviderEvent::RefusalDelta { index, text } => {
                self.saw_refusal = true;
                self.ensure(index);
                match &mut self.blocks[index] {
                    PartialBlock::Empty => {
                        self.blocks[index] = PartialBlock::Text { text, done: false };
                    }
                    PartialBlock::Text {
                        text: value,
                        done: false,
                    } => value.push_str(&text),
                    PartialBlock::Text { done: true, .. } => {
                        return Err(BrainError::Protocol(format!(
                            "provider emitted refusal text for completed block {index}"
                        )));
                    }
                    PartialBlock::Tool { .. } => return Err(block_type_conflict(index)),
                }
            }
            ProviderEvent::ToolUseStart { index, id, name } => {
                self.ensure(index);
                if !matches!(self.blocks[index], PartialBlock::Empty) {
                    return Err(BrainError::Protocol(format!(
                        "provider started content block {index} more than once"
                    )));
                }
                self.blocks[index] = PartialBlock::Tool {
                    id,
                    name,
                    json: String::new(),
                    done: false,
                };
            }
            ProviderEvent::ToolInputDelta {
                index,
                partial_json,
            } => {
                self.ensure(index);
                match &mut self.blocks[index] {
                    PartialBlock::Tool {
                        json, done: false, ..
                    } => json.push_str(&partial_json),
                    PartialBlock::Tool { done: true, .. } => {
                        return Err(BrainError::Protocol(format!(
                            "provider emitted Tool JSON for completed block {index}"
                        )));
                    }
                    PartialBlock::Empty => {
                        return Err(BrainError::Protocol(format!(
                            "provider emitted Tool JSON before starting block {index}"
                        )));
                    }
                    PartialBlock::Text { .. } => return Err(block_type_conflict(index)),
                }
            }
            ProviderEvent::BlockDone { index } => {
                self.ensure(index);
                match &mut self.blocks[index] {
                    PartialBlock::Text { done, .. } | PartialBlock::Tool { done, .. } if !*done => {
                        *done = true
                    }
                    PartialBlock::Empty => {
                        return Err(BrainError::Protocol(format!(
                            "provider completed absent content block {index}"
                        )));
                    }
                    _ => {
                        return Err(BrainError::Protocol(format!(
                            "provider completed content block {index} more than once"
                        )));
                    }
                }
            }
            ProviderEvent::Usage { usage } => self.usage.merge(&usage)?,
            ProviderEvent::MessageDone { stop_reason, usage } => {
                // OpenAI emits usage in a final choices=[] chunk. Its unknown stop reason must
                // not overwrite the real finish reason from the preceding chunk.
                if stop_reason != StopReason::Unknown || self.stop_reason == StopReason::Unknown {
                    self.stop_reason = stop_reason;
                }
                self.usage.merge(&usage)?;
                self.saw_terminal = true;
            }
        }
        self.total_bytes = self.total_bytes.saturating_add(added_bytes);
        self.tool_calls += usize::from(starts_tool);
        Ok(())
    }

    fn ensure(&mut self, index: usize) {
        while self.blocks.len() <= index {
            self.blocks.push(PartialBlock::Empty);
        }
    }

    /// Finish into a message. Empty text blocks are dropped; a tool block whose
    /// accumulated JSON does not parse is surfaced as a typed protocol error
    /// rather than being coerced to `{}` -- coercing would let the model's call
    /// silently become a different call.
    pub fn finish(self) -> Result<(Message, StopReason, Usage)> {
        let mut content = Vec::with_capacity(self.blocks.len());
        for (index, b) in self.blocks.into_iter().enumerate() {
            match b {
                // OpenAI reserves index zero for text and starts Tool calls at one. A pure
                // Tool-call response therefore has exactly this one intentional empty slot.
                PartialBlock::Empty if index == 0 => {}
                PartialBlock::Empty => {
                    return Err(BrainError::Protocol(
                        "provider left a gap in its content block indexes".into(),
                    ));
                }
                PartialBlock::Text { text, .. } if text.is_empty() => {}
                PartialBlock::Text { text, .. } => content.push(ContentBlock::Text { text }),
                PartialBlock::Tool { id, name, json, .. } => {
                    let input: serde_json::Value = if json.trim().is_empty() {
                        serde_json::Value::Object(Default::default())
                    } else {
                        serde_json::from_str(&json).map_err(|e| {
                            crate::BrainError::Protocol(format!(
                                "tool_use {name} ({id}) input is not valid JSON after \
                                 {} bytes of deltas: {e}",
                                json.len()
                            ))
                        })?
                    };
                    content.push(ContentBlock::ToolUse { id, name, input });
                }
            }
        }
        let stop_reason = if self.saw_refusal {
            StopReason::Refusal
        } else {
            self.stop_reason
        };
        Ok((Message::assistant(content), stop_reason, self.usage))
    }
}

fn block_type_conflict(index: usize) -> BrainError {
    BrainError::Protocol(format!(
        "provider changed the type of content block {index}"
    ))
}

pub fn for_dialect(d: Dialect) -> Box<dyn Provider> {
    match d {
        Dialect::AnthropicMessages => Box::new(anthropic::Anthropic),
        Dialect::OpenAiChat => Box::new(openai::OpenAiChat),
    }
}

/// Exact provider-visible immutable request segment stored at session creation. Dynamic messages
/// are appended to a clone of this object; later deployments do not re-render old session bases.
pub fn render_base(prefix: &SealedPrefix) -> serde_json::Value {
    match prefix.dialect {
        Dialect::AnthropicMessages => {
            serde_json::Value::Object(anthropic::Anthropic::render_base(prefix))
        }
        Dialect::OpenAiChat => serde_json::Value::Object(openai::OpenAiChat::render_base(prefix)),
    }
}

/// Shared streaming path for every dialect: send, check status, decode SSE
/// incrementally, hand each frame to the dialect decoder.
pub(crate) async fn http_stream(
    req: ModelRequest,
    outbound: &crate::outbound::Outbound,
    decode: fn(Option<&str>, &str) -> Result<Vec<ProviderEvent>>,
) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
    use futures_util::StreamExt;

    let url = outbound.check_url(&req.url)?;
    let mut rb = outbound.client().post(url).body(req.body);
    for (k, v) in &req.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| crate::BrainError::Transport(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let mut stream = resp.bytes_stream();
        let mut body = Vec::with_capacity(2048);
        while body.len() < 2048 {
            let Some(chunk) = stream.next().await else {
                break;
            };
            let chunk = chunk.map_err(|error| crate::BrainError::Transport(error.to_string()))?;
            let take = (2048 - body.len()).min(chunk.len());
            body.extend_from_slice(&chunk[..take]);
            if take < chunk.len() {
                break;
            }
        }
        return Err(crate::BrainError::ProviderStatus {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }

    let mut dec = sse::SseDecoder::default();
    let mut bytes = resp.bytes_stream();
    let s = async_stream::stream! {
        loop {
            match bytes.next().await {
                Some(Ok(chunk)) => match dec.feed(&chunk) {
                    Ok(frames) => {
                        for f in frames {
                            match decode(f.event.as_deref(), &f.data) {
                                Ok(evs) => { for e in evs { yield Ok(e); } }
                                Err(e) => { yield Err(e); return; }
                            }
                        }
                    }
                    Err(e) => { yield Err(e); return; }
                },
                Some(Err(e)) => { yield Err(crate::BrainError::Transport(e.to_string())); return; }
                None => {
                    if dec.pending() > 0 {
                        // Early EOF mid-frame. Reported, never swallowed: a
                        // truncated stream that looks complete is how a partial
                        // assistant message becomes history.
                        yield Err(crate::BrainError::Protocol(format!(
                            "provider stream ended with {} bytes of an incomplete SSE frame",
                            dec.pending()
                        )));
                    }
                    return;
                }
            }
        }
    };
    Ok(Box::pin(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_rejects_broken_tool_json_instead_of_coercing() {
        let mut a = Accumulator::new();
        a.push(ProviderEvent::ToolUseStart {
            index: 0,
            id: "t1".into(),
            name: "read".into(),
        })
        .unwrap();
        a.push(ProviderEvent::ToolInputDelta {
            index: 0,
            partial_json: "{\"path\": \"/etc/pas".into(),
        })
        .unwrap();
        a.push(ProviderEvent::MessageDone {
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
        })
        .unwrap();
        let err = a.finish().unwrap_err();
        assert!(
            matches!(err, crate::BrainError::Protocol(_)),
            "a truncated tool input must be a typed protocol error, got {err:?}"
        );
    }

    #[test]
    fn accumulator_joins_split_json_deltas() {
        let mut a = Accumulator::new();
        a.push(ProviderEvent::ToolUseStart {
            index: 0,
            id: "t1".into(),
            name: "read".into(),
        })
        .unwrap();
        for chunk in ["{\"pa", "th\": \"/e", "tc/hosts\"}"] {
            a.push(ProviderEvent::ToolInputDelta {
                index: 0,
                partial_json: chunk.into(),
            })
            .unwrap();
        }
        a.push(ProviderEvent::MessageDone {
            stop_reason: StopReason::ToolUse,
            usage: Usage {
                input_tokens: Some(10),
                output_tokens: Some(2),
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                reasoning_tokens: None,
            },
        })
        .unwrap();
        let (msg, stop, usage) = a.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        // Absent stays absent.
        assert_eq!(usage.cache_read_input_tokens, None);
        let uses: Vec<_> = msg.tool_uses().collect();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].1, "read");
        assert_eq!(uses[0].2["path"], "/etc/hosts");
    }

    #[test]
    fn accumulator_rejects_usage_overflow_without_wrapping() {
        let mut accumulator = Accumulator::new();
        accumulator
            .push(ProviderEvent::Usage {
                usage: Usage {
                    input_tokens: Some(u64::MAX),
                    ..Usage::default()
                },
            })
            .unwrap();
        let error = accumulator
            .push(ProviderEvent::Usage {
                usage: Usage {
                    input_tokens: Some(1),
                    ..Usage::default()
                },
            })
            .unwrap_err();
        assert!(matches!(error, BrainError::Protocol(_)));
        assert_eq!(accumulator.usage.input_tokens, Some(u64::MAX));
    }

    #[test]
    fn accumulator_rejects_type_changes_duplicates_and_post_terminal_deltas() {
        let mut type_change = Accumulator::new();
        type_change
            .push(ProviderEvent::TextDelta {
                index: 0,
                text: "text".into(),
            })
            .unwrap();
        assert!(
            type_change
                .push(ProviderEvent::ToolInputDelta {
                    index: 0,
                    partial_json: "{}".into(),
                })
                .is_err()
        );

        let mut duplicate = Accumulator::new();
        let start = || ProviderEvent::ToolUseStart {
            index: 1,
            id: "call_1".into(),
            name: "read".into(),
        };
        duplicate.push(start()).unwrap();
        assert!(duplicate.push(start()).is_err());

        let mut completed = Accumulator::new();
        completed
            .push(ProviderEvent::TextDelta {
                index: 0,
                text: "done".into(),
            })
            .unwrap();
        completed
            .push(ProviderEvent::BlockDone { index: 0 })
            .unwrap();
        assert!(
            completed
                .push(ProviderEvent::TextDelta {
                    index: 0,
                    text: "late".into(),
                })
                .is_err()
        );

        let mut terminal = Accumulator::new();
        terminal
            .push(ProviderEvent::MessageDone {
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
            .unwrap();
        assert!(
            terminal
                .push(ProviderEvent::TextDelta {
                    index: 0,
                    text: "late".into(),
                })
                .is_err()
        );
    }
}
