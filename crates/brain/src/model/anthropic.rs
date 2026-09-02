//! Anthropic Messages API dialect: the pure codec half. The HTTP transport
//! half lives in `model::http`, shared by every dialect.

use brain_protocol::{
    ContentBlock, Message, ModelRequest, ModelStreamEvent, Role, StopReason, ToolDefinition, Usage,
};
use serde_json::{Value, json};

use crate::Error;

/// The wire requires `max_tokens`; a request that did not choose one gets this.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8_192;

pub fn path() -> &'static str {
    "/messages"
}

pub fn headers(api_key: &str) -> Vec<(String, String)> {
    vec![
        ("anthropic-version".into(), "2023-06-01".into()),
        ("x-api-key".into(), api_key.to_owned()),
    ]
}

/// Render ONE provider-neutral message into the dialect's array element.
/// Anthropic is 1:1 -- tool results ride in the user message they arrived in.
fn render_one(message: &Message) -> Result<Value, Error> {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let mut blocks = Vec::with_capacity(message.content.len());
    for block in &message.content {
        blocks.push(match block {
            ContentBlock::Text { text } => json!({"type": "text", "text": text}),
            ContentBlock::ToolUse { id, name, input } => {
                if message.role != Role::Assistant {
                    return Err(Error::InvalidState(
                        "a tool_use block cannot appear in a user message".into(),
                    ));
                }
                json!({"type": "tool_use", "id": id, "name": name, "input": input})
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                if message.role != Role::User {
                    return Err(Error::InvalidState(
                        "a tool_result block cannot appear in an assistant message".into(),
                    ));
                }
                json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": stringify(content),
                    // ALWAYS present. Never omitted on a failure.
                    "is_error": is_error,
                })
            }
        });
    }
    Ok(json!({"role": role, "content": blocks}))
}

fn stringify(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).expect("JSON values serialize"),
    }
}

pub fn body(model: &str, tools: &[ToolDefinition], request: &ModelRequest) -> Result<Value, Error> {
    if request.response_format.is_some() {
        // Rejected rather than silently dropped: a loop that asked for a response
        // format is owed the format or an error, never prose that ignores it.
        return Err(Error::InvalidState(
            "response_format is not supported by the anthropic provider".into(),
        ));
    }
    let mut messages = Vec::with_capacity(request.messages.len());
    for message in &request.messages {
        messages.push(render_one(message)?);
    }
    let mut tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })
        })
        .collect();
    let mut body = json!({
        "model": model,
        "max_tokens": request.max_output_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
        "stream": true,
        "messages": messages,
    });
    // Anthropic caches only at explicit breakpoints. Put exactly one at the end
    // of the immutable prefix: the last tool when present, otherwise the system
    // block. System prompt before tools before messages is the order the cache
    // keys on.
    if tools.is_empty() {
        if !request.system.is_empty() {
            body["system"] = json!([{
                "type": "text",
                "text": request.system,
                "cache_control": {"type": "ephemeral"},
            }]);
        }
    } else {
        tools
            .last_mut()
            .and_then(Value::as_object_mut)
            .expect("tool is an object")
            .insert("cache_control".into(), json!({"type": "ephemeral"}));
        if !request.system.is_empty() {
            body["system"] = json!(request.system);
        }
        body["tools"] = Value::Array(tools);
    }
    Ok(body)
}

/// Turn one SSE frame into zero or more dialect-neutral events. The frame's
/// JSON carries its own `type`, so the SSE `event:` line is not needed.
pub fn decode(data: &str) -> Result<Vec<ModelStreamEvent>, Error> {
    let value: Value = match serde_json::from_str(data) {
        Ok(value) => value,
        Err(_) if data == "[DONE]" => return Ok(vec![]),
        Err(error) => {
            return Err(Error::Ambiguous(format!(
                "model stream returned invalid JSON: {error}"
            )));
        }
    };
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;

    Ok(match kind {
        "content_block_start" => {
            let block = value.get("content_block").unwrap_or(&Value::Null);
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => vec![ModelStreamEvent::ToolUseStart {
                    index,
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                }],
                _ => vec![],
            }
        }
        "content_block_delta" => {
            let delta = value.get("delta").unwrap_or(&Value::Null);
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => vec![ModelStreamEvent::TextDelta {
                    index,
                    text: delta
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                }],
                Some("input_json_delta") => vec![ModelStreamEvent::ToolInputDelta {
                    index,
                    partial_json: delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                }],
                _ => vec![],
            }
        }
        "content_block_stop" => vec![ModelStreamEvent::BlockDone { index }],
        "message_delta" => {
            let stop = value
                .get("delta")
                .and_then(|delta| delta.get("stop_reason"))
                .and_then(Value::as_str);
            vec![ModelStreamEvent::MessageDone {
                stop_reason: map_stop(stop),
                usage: usage_of(value.get("usage")),
            }]
        }
        "message_start" => {
            // Carries the input-token usage for the request. Emitting it as a
            // MessageDone with Unknown stop would terminate the accumulator
            // early, so it is folded as a usage-only event.
            let usage = value
                .get("message")
                .and_then(|message| message.get("usage"));
            if usage.is_some() {
                vec![ModelStreamEvent::Usage {
                    usage: usage_of(usage),
                }]
            } else {
                vec![]
            }
        }
        "error" => {
            return Err(Error::Ambiguous(format!(
                "provider error frame: {}",
                value.get("error").unwrap_or(&value)
            )));
        }
        _ => vec![],
    })
}

fn map_stop(reason: Option<&str>) -> StopReason {
    match reason {
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
fn usage_of(usage: Option<&Value>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let get = |key: &str| usage.get(key).and_then(Value::as_u64);
    Usage {
        input_tokens: get("input_tokens"),
        output_tokens: get("output_tokens"),
        cache_read_input_tokens: get("cache_read_input_tokens"),
        cache_creation_input_tokens: get("cache_creation_input_tokens"),
        reasoning_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Accumulator;
    use crate::model::sse::SseDecoder;
    use brain_protocol::ToolDefinition;

    fn decode_stream(bytes: &[u8]) -> Result<Vec<ModelStreamEvent>, Error> {
        let mut decoder = SseDecoder::new(256 * 1024);
        let mut out = Vec::new();
        for data in decoder.feed(bytes)? {
            out.extend(decode(&data)?);
        }
        Ok(out)
    }

    fn tools() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read".into(),
            description: "read".into(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
        }]
    }

    #[test]
    fn the_body_is_well_formed_and_the_cache_breakpoint_sits_on_the_last_tool() {
        let request = ModelRequest {
            system: "sys".into(),
            tools: vec!["read".into()],
            messages: vec![Message::user_text("hi")],
            response_format: None,
            max_output_tokens: None,
        };
        let body = body("claude-test", &tools(), &request).unwrap();
        assert_eq!(body["model"], "claude-test");
        assert_eq!(body["max_tokens"], DEFAULT_MAX_OUTPUT_TOKENS);
        assert_eq!(body["system"], "sys");
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn a_toolless_request_caches_the_system_block() {
        let request = ModelRequest {
            system: "sys".into(),
            tools: vec!["read".into()],
            messages: vec![Message::user_text("hi")],
            response_format: None,
            max_output_tokens: Some(64),
        };
        let request = ModelRequest {
            system: "be terse".into(),
            tools: Vec::new(),
            ..request
        };
        let body = body("claude-test", &[], &request).unwrap();
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["system"][0]["text"], "be terse");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn response_format_is_rejected_instead_of_silently_dropped() {
        let request = ModelRequest {
            system: "sys".into(),
            tools: vec!["read".into()],
            messages: vec![Message::user_text("hi")],
            response_format: Some(serde_json::json!({"type": "json_object"})),
            max_output_tokens: None,
        };
        let error = body("claude-test", &tools(), &request).unwrap_err();
        assert!(matches!(error, Error::InvalidState(_)));
        assert!(error.to_string().contains("response_format"));
    }

    #[test]
    fn tool_result_always_carries_is_error() {
        let request = ModelRequest {
            system: "sys".into(),
            tools: vec!["read".into()],
            messages: vec![Message::tool_results(vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: serde_json::json!({"stderr": "boom"}),
                is_error: true,
            }])],
            response_format: None,
            max_output_tokens: None,
        };
        let body = body("claude-test", &tools(), &request).unwrap();
        assert_eq!(body["messages"][0]["content"][0]["is_error"], true);
        assert_eq!(
            body["messages"][0]["content"][0]["content"],
            r#"{"stderr":"boom"}"#
        );
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
        let mut accumulator = Accumulator::new();
        for event in decode_stream(raw.as_bytes()).unwrap() {
            accumulator.push(event).unwrap();
        }
        assert!(accumulator.saw_terminal());
        let (message, stop, usage) = accumulator.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        assert_eq!(usage.input_tokens, Some(11));
        assert_eq!(usage.output_tokens, Some(7));
        // Never sent -> never invented.
        assert_eq!(usage.cache_read_input_tokens, None);
        assert_eq!(message.tool_uses().count(), 1);
    }

    #[test]
    fn refusal_is_typed_and_an_absent_reason_stays_unknown() {
        assert_eq!(map_stop(Some("refusal")), StopReason::Refusal);
        assert_eq!(map_stop(None), StopReason::Unknown);
    }
}
