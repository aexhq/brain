//! Anthropic Messages API adapter.

use super::sse::SseDecoder;
use super::{ModelRequest, Provider, ProviderEvent};
use crate::config::{Dialect, ProviderKey, SealedPrefix};
use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::{BrainError, Result};
use futures_util::stream::BoxStream;
use serde_json::{Map, Value, json};

#[derive(Debug, Default)]
pub struct Anthropic;

impl Anthropic {
    fn request(body: Value, key: &ProviderKey, base_url: &str) -> Result<ModelRequest> {
        Ok(ModelRequest {
            method: "POST",
            url: format!("{}/v1/messages", base_url.trim_end_matches('/')),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
                ("anthropic-version".into(), "2023-06-01".into()),
                ("x-api-key".into(), key.expose().to_string()),
            ],
            body: serde_json::to_vec(&body)?,
        })
    }

    /// Render ONE provider-neutral message into the dialect's array elements.
    ///
    /// Split out of `body` so the pre-rendered transcript store
    /// (`provider::render`) and the reference builder share one renderer rather
    /// than two that must be kept in step. Anthropic is 1:1 -- a neutral
    /// message is one element -- but the signature matches OpenAI's, which is
    /// not, so the store never has to know which dialect it is holding.
    pub fn render_one(m: &Message) -> Result<Vec<Value>> {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let mut blocks = Vec::with_capacity(m.content.len());
        for b in &m.content {
            blocks.push(match b {
                ContentBlock::Text { text } => json!({"type":"text","text":text}),
                ContentBlock::ToolUse { id, name, input } => {
                    json!({"type":"tool_use","id":id,"name":name,"input":input})
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => json!({
                    "type":"tool_result",
                    "tool_use_id":tool_use_id,
                    "content":content,
                    // ALWAYS present. Never omitted on a failure.
                    "is_error":is_error
                }),
            });
        }
        Ok(vec![json!({"role":role,"content":blocks})])
    }

    /// The pure request builder, exposed separately so the cold-start benchmark
    /// can call it without a trait object and without a client.
    pub fn body(prefix: &SealedPrefix, history: &[Message]) -> Result<Value> {
        let mut messages = Vec::with_capacity(history.len());
        for m in history {
            messages.extend(Self::render_one(m)?);
        }

        let tools: Vec<Value> = prefix
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();

        let mut body = Map::new();
        body.insert("model".into(), json!(prefix.model));
        body.insert("max_tokens".into(), json!(prefix.sampling.max_tokens));
        body.insert("stream".into(), json!(true));
        // System prompt first, then tools, then messages: the render order the
        // prompt cache keys on. Reordering these silently invalidates every cached prefix.
        body.insert("system".into(), json!(prefix.system_prompt));
        if !tools.is_empty() {
            body.insert("tools".into(), json!(tools));
        }
        body.insert("messages".into(), json!(messages));
        if let Some(t) = prefix.sampling.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if !prefix.sampling.stop_sequences.is_empty() {
            body.insert(
                "stop_sequences".into(),
                json!(prefix.sampling.stop_sequences),
            );
        }
        Ok(Value::Object(body))
    }
}

#[async_trait::async_trait]
impl Provider for Anthropic {
    fn dialect(&self) -> Dialect {
        Dialect::AnthropicMessages
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        Self::request(Self::body(prefix, history)?, key, base_url)
    }

    async fn stream(&self, req: ModelRequest) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        crate::provider::http_stream(req, decode).await
    }
}

/// Turn one SSE frame into zero or more dialect-neutral events.
pub fn decode(event: Option<&str>, data: &str) -> Result<Vec<ProviderEvent>> {
    let v: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) if data == "[DONE]" => return Ok(vec![]),
        Err(e) => return Err(BrainError::Protocol(format!("anthropic frame: {e}"))),
    };
    let ty = event
        .map(|s| s.to_string())
        .or_else(|| v.get("type").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_default();
    let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

    Ok(match ty.as_str() {
        "content_block_start" => {
            let cb = v.get("content_block").unwrap_or(&Value::Null);
            match cb.get("type").and_then(|t| t.as_str()) {
                Some("tool_use") => vec![ProviderEvent::ToolUseStart {
                    index,
                    id: cb
                        .get("id")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    name: cb
                        .get("name")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }],
                _ => vec![],
            }
        }
        "content_block_delta" => {
            let d = v.get("delta").unwrap_or(&Value::Null);
            match d.get("type").and_then(|t| t.as_str()) {
                Some("text_delta") => vec![ProviderEvent::TextDelta {
                    index,
                    text: d
                        .get("text")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }],
                Some("input_json_delta") => vec![ProviderEvent::ToolInputDelta {
                    index,
                    partial_json: d
                        .get("partial_json")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }],
                _ => vec![],
            }
        }
        "content_block_stop" => vec![ProviderEvent::BlockDone { index }],
        "message_delta" => {
            let stop = v
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|s| s.as_str());
            vec![ProviderEvent::MessageDone {
                stop_reason: map_stop(stop),
                usage: usage_of(v.get("usage")),
            }]
        }
        "message_start" => {
            // Carries the input-token usage for the request. Emitting it as a
            // MessageDone with Unknown stop would terminate the accumulator
            // early, so it is folded as a usage-only event.
            let u = v.get("message").and_then(|m| m.get("usage"));
            if u.is_some() {
                vec![ProviderEvent::MessageDone {
                    stop_reason: StopReason::Unknown,
                    usage: usage_of(u),
                }]
            } else {
                vec![]
            }
        }
        "error" => {
            return Err(BrainError::Protocol(format!(
                "provider error frame: {}",
                v.get("error").unwrap_or(&v)
            )));
        }
        _ => vec![],
    })
}

fn map_stop(s: Option<&str>) -> StopReason {
    match s {
        Some("end_turn") => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("stop_sequence") => StopReason::StopSequence,
        Some("refusal") => StopReason::Refusal,
        // A stop reason we do not recognise is Unknown, not EndTurn. Guessing
        // EndTurn would end a turn the provider did not end.
        _ => StopReason::Unknown,
    }
}

/// **Absent is never zero.** A field the provider did not send stays `None`.
fn usage_of(u: Option<&Value>) -> Usage {
    let Some(u) = u else {
        return Usage::default();
    };
    let g = |k: &str| u.get(k).and_then(|v| v.as_u64());
    Usage {
        input_tokens: g("input_tokens"),
        output_tokens: g("output_tokens"),
        cache_read_input_tokens: g("cache_read_input_tokens"),
        cache_creation_input_tokens: g("cache_creation_input_tokens"),
        reasoning_tokens: None,
    }
}

/// Test-only re-export of the decoder wired to a decoder loop, so the SSE and
/// dialect layers can be tested together without a socket.
pub fn decode_stream(bytes: &[u8]) -> Result<Vec<ProviderEvent>> {
    let mut d = SseDecoder::default();
    let mut out = Vec::new();
    for ev in d.feed(bytes)? {
        out.extend(decode(ev.event.as_deref(), &ev.data)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentDef, ToolDecl, ToolRoute};

    fn prefix() -> std::sync::Arc<SealedPrefix> {
        AgentDef::new("sys", "claude-test", Dialect::AnthropicMessages)
            .tool(ToolDecl {
                name: "read".into(),
                description: "read".into(),
                input_schema: json!({"type":"object"}),
                output_schema: json!({"type":"object"}),
                route: ToolRoute::Intrinsic("brain.test.read".into()),
            })
            .seal()
    }

    #[test]
    fn build_request_is_pure_and_well_formed() {
        let p = prefix();
        let h = vec![Message::user_text("hi")];
        let r = Anthropic
            .build_request(&p, &h, &ProviderKey::new("sk-x"), "http://127.0.0.1:1")
            .unwrap();
        assert_eq!(r.url, "http://127.0.0.1:1/v1/messages");
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["model"], "claude-test");
        assert_eq!(v["system"], "sys");
        assert_eq!(v["tools"][0]["name"], "read");
        assert_eq!(v["messages"][0]["content"][0]["text"], "hi");
        assert_eq!(v["stream"], true);
    }

    #[test]
    fn tool_result_always_carries_is_error() {
        let p = prefix();
        let h = vec![Message::tool_results(vec![ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: "boom".into(),
            is_error: true,
        }])];
        let v: Value = serde_json::from_slice(
            &Anthropic
                .build_request(&p, &h, &ProviderKey::new("k"), "http://x")
                .unwrap()
                .body,
        )
        .unwrap();
        assert_eq!(v["messages"][0]["content"][0]["is_error"], true);
    }

    #[test]
    fn decodes_a_tool_use_stream() {
        let raw = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":11}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu1\",\"name\":\"read\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"p\\\":1}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
        );
        let evs = decode_stream(raw.as_bytes()).unwrap();
        let mut acc = super::super::Accumulator::new();
        for e in evs {
            acc.push(e);
        }
        let (msg, stop, usage) = acc.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        assert_eq!(usage.input_tokens, Some(11));
        assert_eq!(usage.output_tokens, Some(7));
        // Never sent -> never invented.
        assert_eq!(usage.cache_read_input_tokens, None);
        assert_eq!(msg.tool_uses().count(), 1);
    }

    #[test]
    fn refusal_is_typed_and_an_absent_reason_stays_unknown() {
        assert_eq!(map_stop(Some("refusal")), StopReason::Refusal);
        assert_eq!(map_stop(None), StopReason::Unknown);
    }
}
