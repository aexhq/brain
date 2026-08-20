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
// `render` (render-once + byte-splice request assembly) stays in the prototype until the
// slice-5 density work lands; the reference `build_request` path below is the certified one.
pub mod sse;

use crate::Result;
use crate::config::{Dialect, ProviderKey, SealedPrefix};
use crate::message::{ContentBlock, Message, StopReason, Usage};
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

    async fn stream(&self, req: ModelRequest) -> Result<BoxStream<'static, Result<ProviderEvent>>>;
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
}

#[derive(Debug)]
enum PartialBlock {
    Text(String),
    Tool {
        id: String,
        name: String,
        json: String,
    },
}

impl Accumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, ev: ProviderEvent) {
        match ev {
            ProviderEvent::TextDelta { index, text } => {
                self.ensure(index, || PartialBlock::Text(String::new()));
                if let Some(PartialBlock::Text(s)) = self.blocks.get_mut(index) {
                    s.push_str(&text);
                }
            }
            ProviderEvent::RefusalDelta { index, text } => {
                self.saw_refusal = true;
                self.ensure(index, || PartialBlock::Text(String::new()));
                if let Some(PartialBlock::Text(s)) = self.blocks.get_mut(index) {
                    s.push_str(&text);
                }
            }
            ProviderEvent::ToolUseStart { index, id, name } => {
                self.ensure(index, || PartialBlock::Text(String::new()));
                self.blocks[index] = PartialBlock::Tool {
                    id,
                    name,
                    json: String::new(),
                };
            }
            ProviderEvent::ToolInputDelta {
                index,
                partial_json,
            } => {
                self.ensure(index, || PartialBlock::Tool {
                    id: String::new(),
                    name: String::new(),
                    json: String::new(),
                });
                if let Some(PartialBlock::Tool { json, .. }) = self.blocks.get_mut(index) {
                    json.push_str(&partial_json);
                }
            }
            ProviderEvent::BlockDone { .. } => {}
            ProviderEvent::MessageDone { stop_reason, usage } => {
                // OpenAI emits usage in a final choices=[] chunk. Its unknown stop reason must
                // not overwrite the real finish reason from the preceding chunk.
                if stop_reason != StopReason::Unknown || self.stop_reason == StopReason::Unknown {
                    self.stop_reason = stop_reason;
                }
                self.usage.merge(&usage);
                self.saw_terminal = true;
            }
        }
    }

    fn ensure(&mut self, index: usize, f: impl Fn() -> PartialBlock) {
        while self.blocks.len() <= index {
            self.blocks.push(f());
        }
    }

    /// Finish into a message. Empty text blocks are dropped; a tool block whose
    /// accumulated JSON does not parse is surfaced as a typed protocol error
    /// rather than being coerced to `{}` -- coercing would let the model's call
    /// silently become a different call.
    pub fn finish(self) -> Result<(Message, StopReason, Usage)> {
        let mut content = Vec::with_capacity(self.blocks.len());
        for b in self.blocks {
            match b {
                PartialBlock::Text(t) if t.is_empty() => {}
                PartialBlock::Text(t) => content.push(ContentBlock::Text { text: t }),
                PartialBlock::Tool { id, name, json } => {
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

pub fn for_dialect(d: Dialect) -> Box<dyn Provider> {
    match d {
        Dialect::AnthropicMessages => Box::new(anthropic::Anthropic),
        Dialect::OpenAiChat => Box::new(openai::OpenAiChat),
    }
}

/// One HTTP client for the whole process.
///
/// Not an optimisation detail: a client per session would mean a TLS session
/// cache, a connection pool and a DNS resolver per session, which is a large
/// part of why process-per-session costs what it does. Brain makes the
/// same point -- pooled TLS/HTTP clients are a named reason the mux exists.
static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

pub fn http_client() -> &'static reqwest::Client {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .pool_max_idle_per_host(64)
            .build()
            .expect("static http client")
    })
}

/// Shared streaming path for every dialect: send, check status, decode SSE
/// incrementally, hand each frame to the dialect decoder.
pub(crate) async fn http_stream(
    req: ModelRequest,
    decode: fn(Option<&str>, &str) -> Result<Vec<ProviderEvent>>,
) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
    use futures_util::StreamExt;

    let mut rb = http_client().post(&req.url).body(req.body);
    for (k, v) in &req.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| crate::BrainError::Transport(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(crate::BrainError::ProviderStatus {
            status: status.as_u16(),
            // Bounded: a provider that answers an error with a megabyte of HTML
            // must not put a megabyte into our error type.
            body: body.chars().take(2048).collect(),
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
        });
        a.push(ProviderEvent::ToolInputDelta {
            index: 0,
            partial_json: "{\"path\": \"/etc/pas".into(),
        });
        a.push(ProviderEvent::MessageDone {
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
        });
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
        });
        for chunk in ["{\"pa", "th\": \"/e", "tc/hosts\"}"] {
            a.push(ProviderEvent::ToolInputDelta {
                index: 0,
                partial_json: chunk.into(),
            });
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
        });
        let (msg, stop, usage) = a.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        // Absent stays absent.
        assert_eq!(usage.cache_read_input_tokens, None);
        let uses: Vec<_> = msg.tool_uses().collect();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].1, "read");
        assert_eq!(uses[0].2["path"], "/etc/hosts");
    }
}
