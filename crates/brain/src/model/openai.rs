//! OpenAI-compatible Chat Completions dialect: the pure codec half. The HTTP
//! transport half lives in `model::http`, shared by every dialect.

use brain_protocol::{
    ContentBlock, Message, ModelRequest, ModelStreamEvent, Role, StopReason, ToolDefinition, Usage,
};
use serde_json::{Map, Value, json};

use crate::Error;
use crate::model::MaxTokensField;

pub fn path() -> &'static str {
    "/chat/completions"
}

pub fn headers(api_key: &str) -> Vec<(String, String)> {
    vec![("authorization".into(), format!("Bearer {api_key}"))]
}

/// Render ONE provider-neutral message into the dialect's array elements.
///
/// One neutral message is **not** always one element: a user message carrying
/// N `tool_result` blocks becomes N `tool` messages, plus a separate `user`
/// message if it also carried text.
fn render_one(message: &Message) -> Result<Vec<Value>, Error> {
    let mut rendered: Vec<Value> = Vec::new();
    match message.role {
        Role::Assistant => {
            let mut text = String::new();
            let mut calls = Vec::new();
            for block in &message.content {
                match block {
                    ContentBlock::Text { text: t } => text.push_str(t),
                    ContentBlock::ToolUse { id, name, input } => calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(input).expect("JSON values serialize"),
                        }
                    })),
                    ContentBlock::ToolResult { .. } => {
                        return Err(Error::InvalidState(
                            "a tool_result block cannot appear in an assistant message".into(),
                        ));
                    }
                }
            }
            let mut object = Map::new();
            object.insert("role".into(), json!("assistant"));
            object.insert(
                "content".into(),
                if text.is_empty() {
                    Value::Null
                } else {
                    json!(text)
                },
            );
            if !calls.is_empty() {
                object.insert("tool_calls".into(), json!(calls));
            }
            rendered.push(Value::Object(object));
        }
        Role::User => {
            let mut text = String::new();
            for block in &message.content {
                match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        // The dialect has no `is_error`. Dropping the signal
                        // entirely is what lets a failed tool read as a success,
                        // so it is carried in-band and marked, never silently lost.
                        let content = stringify(content);
                        let payload = if *is_error {
                            format!("ERROR: {content}")
                        } else {
                            content
                        };
                        rendered.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": payload,
                        }));
                    }
                    ContentBlock::Text { text: t } => text.push_str(t),
                    ContentBlock::ToolUse { .. } => {
                        return Err(Error::InvalidState(
                            "a tool_use block cannot appear in a user message".into(),
                        ));
                    }
                }
            }
            if !text.is_empty() {
                rendered.push(json!({"role": "user", "content": text}));
            }
        }
    }
    Ok(rendered)
}

fn stringify(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).expect("JSON values serialize"),
    }
}

pub fn body(
    model: &str,
    tools: &[ToolDefinition],
    request: &ModelRequest,
    max_tokens_field: MaxTokensField,
) -> Result<Value, Error> {
    let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system) = request
        .system
        .as_deref()
        .filter(|system| !system.is_empty())
    {
        messages.push(json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        messages.extend(render_one(message)?);
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                }
            })
        })
        .collect();
    // Ask for usage on the final chunk. Without it several OpenAI-compatible
    // servers report nothing at all -- which is absent, and absent must not
    // become zero downstream.
    let mut body = json!({
        "model": model,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(format) = &request.response_format {
        body["response_format"] = format.clone();
    }
    if let Some(tokens) = request.max_output_tokens {
        // OpenAI deprecated `max_tokens`; most OpenAI-compatible servers only
        // know it. The provider's registry entry says which one it speaks.
        let field = match max_tokens_field {
            MaxTokensField::MaxCompletionTokens => "max_completion_tokens",
            MaxTokensField::MaxTokens => "max_tokens",
        };
        body[field] = json!(tokens);
    }
    Ok(body)
}

/// Turn one SSE frame into zero or more dialect-neutral events.
pub fn decode(data: &str) -> Result<Vec<ModelStreamEvent>, Error> {
    if data.trim() == "[DONE]" {
        return Ok(vec![]);
    }
    let value: Value = serde_json::from_str(data).map_err(|error| {
        Error::Ambiguous(format!("model stream returned invalid JSON: {error}"))
    })?;
    if let Some(error) = value.get("error") {
        return Err(Error::Ambiguous(format!("provider error frame: {error}")));
    }
    let mut out = Vec::new();

    // Usage arrives on a chunk that may have no choices at all.
    let usage = usage_of(value.get("usage"))?;
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            if let Some(text) = delta.get("content").and_then(Value::as_str)
                && !text.is_empty()
            {
                out.push(ModelStreamEvent::TextDelta {
                    index: 0,
                    text: text.to_owned(),
                });
            }
            if let Some(text) = delta.get("refusal").and_then(Value::as_str)
                && !text.is_empty()
            {
                out.push(ModelStreamEvent::RefusalDelta {
                    index: 0,
                    text: text.to_owned(),
                });
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    // `index` is what makes N parallel calls in one assistant
                    // message distinguishable. +1 so index 0 stays reserved for
                    // the text block.
                    let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
                    let function = call.get("function").unwrap_or(&Value::Null);
                    let id = call.get("id").and_then(Value::as_str);
                    let name = function.get("name").and_then(Value::as_str);
                    if let (Some(id), Some(name)) = (id, name) {
                        out.push(ModelStreamEvent::ToolUseStart {
                            index,
                            id: id.to_owned(),
                            name: name.to_owned(),
                        });
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str)
                        && !arguments.is_empty()
                    {
                        out.push(ModelStreamEvent::ToolInputDelta {
                            index,
                            partial_json: arguments.to_owned(),
                        });
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                out.push(ModelStreamEvent::MessageDone {
                    stop_reason: map_stop(reason),
                    usage: usage.clone(),
                });
            }
        }
    }

    // A usage-only chunk (stream_options.include_usage) still has to be folded.
    if usage != Usage::default()
        && !out
            .iter()
            .any(|event| matches!(event, ModelStreamEvent::MessageDone { .. }))
    {
        out.push(ModelStreamEvent::Usage { usage });
    }
    Ok(out)
}

fn map_stop(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::Refusal,
        // A stop reason we do not recognise is Unknown, not EndTurn. Guessing
        // EndTurn would end a turn the provider did not end.
        _ => StopReason::Unknown,
    }
}

/// **Absent is never zero.** A field the provider did not send stays `None`.
fn usage_of(usage: Option<&Value>) -> Result<Usage, Error> {
    let Some(usage) = usage else {
        return Ok(Usage::default());
    };
    let get = |key: &str| usage.get(key).and_then(Value::as_u64);
    Ok(Usage {
        input_tokens: get("prompt_tokens"),
        output_tokens: get("completion_tokens"),
        cache_read_input_tokens: usage
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_u64),
        cache_creation_input_tokens: None,
        reasoning_tokens: usage
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_u64),
        provider_cost_usd: provider_cost_of(usage)?,
    })
}

fn provider_cost_of(usage: &Value) -> Result<Option<String>, Error> {
    let Some(cost) = usage.get("gateway_cost") else {
        return Ok(None);
    };
    let cost = match cost {
        Value::Number(cost) => cost.to_string(),
        Value::String(cost) => cost.trim().to_owned(),
        _ => {
            return Err(Error::Ambiguous(
                "provider returned an invalid gateway_cost".into(),
            ));
        }
    };
    if cost.starts_with('-') || serde_json::from_str::<serde_json::Number>(&cost).is_err() {
        return Err(Error::Ambiguous(
            "provider returned an invalid gateway_cost".into(),
        ));
    }
    Ok(Some(cost))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Accumulator;
    use crate::model::sse::SseDecoder;
    #[test]
    fn the_output_token_cap_lands_in_the_field_the_provider_speaks() {
        let request = ModelRequest {
            system: Some("sys".into()),
            tools: Some(vec!["read".into()]),
            messages: vec![Message::user_text("hi")],
            response_format: None,
            max_output_tokens: Some(64),
        };
        let modern = body("m", &tools(), &request, MaxTokensField::MaxCompletionTokens).unwrap();
        assert_eq!(modern["max_completion_tokens"], 64);
        assert!(modern.get("max_tokens").is_none());
        let compatible = body("m", &tools(), &request, MaxTokensField::MaxTokens).unwrap();
        assert_eq!(compatible["max_tokens"], 64);
        assert!(
            compatible.get("max_completion_tokens").is_none(),
            "an OpenAI-compatible server must not receive the field it does not know"
        );
    }

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
    fn tool_results_become_tool_role_messages_and_keep_the_error_signal() {
        let request = ModelRequest {
            system: Some("sys".into()),
            tools: Some(vec!["read".into()]),
            messages: vec![
                Message::user_text("go"),
                Message::assistant(vec![ContentBlock::ToolUse {
                    id: "c0".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                }]),
                Message::tool_results(vec![ContentBlock::ToolResult {
                    tool_use_id: "c0".into(),
                    content: serde_json::json!("child 3 of 4 failed"),
                    is_error: true,
                }]),
            ],
            response_format: None,
            max_output_tokens: None,
        };
        let body = body("test/model", &tools(), &request, MaxTokensField::default()).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "c0");
        assert_eq!(messages[2]["tool_calls"][0]["type"], "function");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "c0");
        assert!(
            messages[3]["content"]
                .as_str()
                .unwrap()
                .starts_with("ERROR:"),
            "the error signal must survive a dialect with no is_error field"
        );
    }

    #[test]
    fn structured_tool_result_content_is_stringified() {
        let request = ModelRequest {
            system: Some("sys".into()),
            tools: Some(vec!["read".into()]),
            messages: vec![Message::tool_results(vec![ContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: serde_json::json!({"stdout": ""}),
                is_error: false,
            }])],
            response_format: None,
            max_output_tokens: None,
        };
        let body = body("test/model", &tools(), &request, MaxTokensField::default()).unwrap();
        assert_eq!(body["messages"][1]["content"], r#"{"stdout":""}"#);
    }

    #[test]
    fn parallel_tool_calls_in_one_message_stay_distinct() {
        let raw = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
            {\"index\":0,\"id\":\"c0\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"i\\\":0}\"}},\
            {\"index\":1,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"i\\\":1}\"}}\
            ]},\"finish_reason\":null}]}\n\n\
            data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
        let mut accumulator = Accumulator::new();
        for event in decode_stream(raw.as_bytes()).unwrap() {
            accumulator.push(event).unwrap();
        }
        let (message, stop, _) = accumulator.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        let uses: Vec<_> = message.tool_uses().collect();
        assert_eq!(uses.len(), 2);
        for (i, (id, name, input)) in uses.iter().enumerate() {
            assert_eq!(*id, format!("c{i}"));
            assert_eq!(*name, "read");
            assert_eq!(input["i"], i as i64);
        }
    }

    #[test]
    fn usage_only_chunk_is_folded_and_absent_stays_absent() {
        let raw =
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}}\n\n";
        let events = decode_stream(raw.as_bytes()).unwrap();
        match &events[0] {
            ModelStreamEvent::Usage { usage } => {
                assert_eq!(usage.input_tokens, Some(9));
                assert_eq!(usage.cache_read_input_tokens, None);
            }
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn gateway_cost_is_preserved_as_an_exact_decimal_string() {
        for (value, expected) in [("0.00012925", "0.00012925"), ("\"2.925E-05\"", "2.925E-05")] {
            let raw =
                format!("data: {{\"choices\":[],\"usage\":{{\"gateway_cost\":{value}}}}}\n\n");
            let events = decode_stream(raw.as_bytes()).unwrap();
            match &events[0] {
                ModelStreamEvent::Usage { usage } => {
                    assert_eq!(usage.provider_cost_usd.as_deref(), Some(expected));
                }
                other => panic!("expected Usage, got {other:?}"),
            }
        }
    }

    #[test]
    fn invalid_gateway_cost_fails_the_model_call() {
        let raw = "data: {\"choices\":[],\"usage\":{\"gateway_cost\":\"free\"}}\n\n";
        assert!(decode_stream(raw.as_bytes()).is_err());
    }

    #[test]
    fn a_real_zero_cached_tokens_survives() {
        let raw = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"prompt_tokens_details\":{\"cached_tokens\":0}}}\n\n";
        let events = decode_stream(raw.as_bytes()).unwrap();
        match &events[0] {
            ModelStreamEvent::Usage { usage } => {
                assert_eq!(
                    usage.cache_read_input_tokens,
                    Some(0),
                    "a reported zero must be Some(0), never None"
                );
            }
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn the_trailing_usage_chunk_does_not_clobber_the_finish_reason() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut accumulator = Accumulator::new();
        for event in decode_stream(raw.as_bytes()).unwrap() {
            accumulator.push(event).unwrap();
        }
        let (message, stop, usage) = accumulator.finish().unwrap();
        assert_eq!(stop, StopReason::EndTurn);
        assert_eq!(usage.input_tokens, Some(9));
        assert!(matches!(
            &message.content[0],
            ContentBlock::Text { text } if text == "hi"
        ));
    }

    #[test]
    fn structured_refusal_survives_an_ordinary_stop() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"refusal\":\"I cannot help with that.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let mut accumulator = Accumulator::new();
        for event in decode_stream(raw.as_bytes()).unwrap() {
            accumulator.push(event).unwrap();
        }
        let (_, stop, _) = accumulator.finish().unwrap();
        assert_eq!(stop, StopReason::Refusal);
    }
}
