//! Stateless Responses requests preserve encrypted reasoning and compaction items.

use crate::Error;
use brain_protocol::{
    ContentBlock, Message, ModelRequest, ModelResult, ModelStreamEvent, Role, StopReason,
    ToolDefinition, Usage,
};
use serde_json::{Value, json};

const FORMAT: &str = "openai.responses.v1";

pub fn compact(request: &ModelRequest) -> Result<bool, Error> {
    match request.options.get("operation").and_then(Value::as_str) {
        None if !request.options.contains_key("operation") => Ok(false),
        Some("generate") => Ok(false),
        Some("compact") => Ok(true),
        _ => Err(Error::InvalidState(
            "unsupported Responses operation".into(),
        )),
    }
}

pub fn body(model: &str, tools: &[ToolDefinition], request: &ModelRequest) -> Result<Value, Error> {
    let mut input = Vec::new();
    for message in &request.messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Developer => "developer",
        };
        for block in &message.content {
            input.push(match block {
                ContentBlock::Text { text } => json!({"role": role, "content": text}),
                ContentBlock::Image { url } => {
                    json!({"role": role, "content": [media("input_image", "image_url", url)?]})
                }
                ContentBlock::File { url, .. } => json!({"role": role, "content": [media("input_file", "file_url", url)?]}),
                ContentBlock::Native { format, data } => {
                    if format != FORMAT || !matches!(data["type"].as_str(), Some("reasoning" | "compaction" | "message" | "function_call" | "function_call_output")) {
                        return Err(Error::InvalidState("unsupported Responses continuation item".into()));
                    }
                    validate_native_media(data)?;
                    data.clone()
                }
                ContentBlock::ToolUse { id, name, input } if message.role == Role::Assistant => json!({"type": "function_call", "call_id": id, "name": name, "arguments": input.to_string()}),
                ContentBlock::ToolResult { tool_use_id, content, is_error, media } if message.role == Role::User => {
                    let text = match content { Value::String(text) => text.clone(), value => value.to_string() };
                    let text = if *is_error { format!("ERROR: {text}") } else { text };
                    let output = if media.is_empty() { json!(text) } else {
                        let mut parts = vec![json!({"type": "input_text", "text": text})];
                        for item in media {
                            parts.push(render_media(item)?);
                        }
                        json!(parts)
                    };
                    json!({"type": "function_call_output", "call_id": tool_use_id, "output": output})
                }
                _ => return Err(Error::InvalidState("invalid role for Responses content".into())),
            });
        }
    }
    let mut body = json!({"model": model, "input": input});
    if let Some(system) = &request.system {
        body["instructions"] = json!(system);
    }
    let mut options = request.options.clone();
    options.remove("operation");
    if compact(request)? {
        if !options.is_empty()
            || request
                .response_format
                .as_ref()
                .is_some_and(|v| !v.is_null())
            || request.max_output_tokens.is_some()
        {
            return Err(Error::InvalidState(
                "compaction accepts context and instructions only".into(),
            ));
        }
        return Ok(body);
    }
    body["stream"] = json!(true);
    body["store"] = json!(false);
    body["include"] = json!(["reasoning.encrypted_content"]);
    if !tools.is_empty() {
        body["tools"] = json!(tools.iter().map(|tool| json!({"type": "function", "name": tool.name, "description": tool.description, "parameters": tool.input_schema, "strict": false})).collect::<Vec<_>>());
    }
    if let Some(tokens) = request.max_output_tokens {
        body["max_output_tokens"] = json!(tokens);
    }
    if let Some(format) = &request.response_format
        && !format.is_null()
    {
        body["text"] = json!({"format": format});
    }
    super::options(&mut body, &options, &["reasoning", "temperature", "top_p"])?;
    Ok(body)
}

fn media(kind: &str, field: &str, url: &str) -> Result<Value, Error> {
    super::validate_media(url)?;
    Ok(json!({"type": kind, field: url}))
}

fn render_media(item: &brain_protocol::Media) -> Result<Value, Error> {
    match item {
        brain_protocol::Media::Image { url } => media("input_image", "image_url", url),
        brain_protocol::Media::File { url, .. } => media("input_file", "file_url", url),
    }
}

fn validate_native_media(item: &Value) -> Result<(), Error> {
    let parts = match item["type"].as_str() {
        Some("message") => &item["content"],
        Some("function_call_output") => &item["output"],
        _ => return Ok(()),
    };
    if let Some(parts) = parts.as_array() {
        for part in parts {
            let field = match part["type"].as_str() {
                Some("input_image") => "image_url",
                Some("input_file") => "file_url",
                Some("input_text" | "output_text" | "refusal") => continue,
                _ => {
                    return Err(Error::InvalidState(
                        "unsupported Responses native content".into(),
                    ));
                }
            };
            if !part["file_data"].is_null() || !part["file_id"].is_null() {
                return Err(Error::InvalidState(
                    "native media requires an HTTPS URL".into(),
                ));
            }
            super::validate_media(part[field].as_str().ok_or_else(|| {
                Error::InvalidState("native media requires an HTTPS URL".into())
            })?)?;
        }
    } else if !parts.is_string() {
        return Err(Error::InvalidState(
            "invalid Responses native content".into(),
        ));
    }
    Ok(())
}

pub fn decode(data: &str) -> Result<Vec<ModelStreamEvent>, Error> {
    if data == "[DONE]" {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(data).map_err(|e| Error::Ambiguous(e.to_string()))?;
    let index = value["output_index"].as_u64().unwrap_or(0) as usize;
    match value["type"].as_str() {
        Some("response.output_text.delta") => Ok(vec![ModelStreamEvent::TextDelta {
            index,
            text: string(&value, "delta")?,
        }]),
        Some("response.refusal.delta") => Ok(vec![ModelStreamEvent::RefusalDelta {
            index,
            text: string(&value, "delta")?,
        }]),
        Some("response.output_item.done") => {
            let item = &value["item"];
            let mut events = match item["type"].as_str() {
                Some("function_call") => vec![
                    ModelStreamEvent::ToolUseStart {
                        index,
                        id: string(item, "call_id")?,
                        name: string(item, "name")?,
                    },
                    ModelStreamEvent::ToolInputDelta {
                        index,
                        partial_json: string(item, "arguments")?,
                    },
                ],
                Some("message") => {
                    let parts = item["content"].as_array().ok_or_else(|| {
                        Error::Ambiguous("Responses message has no content".into())
                    })?;
                    for part in parts {
                        match part["type"].as_str() {
                            Some("output_text" | "refusal") => {}
                            _ => {
                                return Err(Error::Ambiguous(
                                    "unsupported Responses message content".into(),
                                ));
                            }
                        }
                    }
                    Vec::new()
                }
                Some("reasoning" | "compaction") => vec![ModelStreamEvent::NativeStart {
                    index,
                    format: FORMAT.into(),
                    data: item.clone(),
                }],
                _ => return Err(Error::Ambiguous("unsupported Responses output item".into())),
            };
            events.push(ModelStreamEvent::BlockDone { index });
            Ok(events)
        }
        Some("response.completed" | "response.incomplete") => {
            let response = &value["response"];
            let stop_reason = if value["type"] == "response.incomplete" {
                match response["incomplete_details"]["reason"].as_str() {
                    Some("max_output_tokens") => StopReason::MaxTokens,
                    Some("content_filter") => StopReason::Refusal,
                    _ => StopReason::Unknown,
                }
            } else if response["output"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["type"] == "function_call"))
            {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            };
            Ok(vec![ModelStreamEvent::MessageDone {
                stop_reason,
                usage: usage(&response["usage"]),
            }])
        }
        Some("error" | "response.failed") => {
            Err(Error::Ambiguous(format!("Responses failure: {value}")))
        }
        _ => Ok(Vec::new()),
    }
}

pub fn compact_result(value: Value) -> Result<ModelResult, Error> {
    let items = value["output"]
        .as_array()
        .ok_or_else(|| Error::Ambiguous("compaction returned no output items".into()))?;
    if !items.iter().any(|item| item["type"] == "compaction") {
        return Err(Error::Ambiguous(
            "compaction returned no compaction item".into(),
        ));
    }
    Ok(ModelResult {
        message: Message::assistant(
            items
                .iter()
                .map(|data| ContentBlock::Native {
                    format: FORMAT.into(),
                    data: data.clone(),
                })
                .collect(),
        ),
        stop_reason: StopReason::EndTurn,
        usage: usage(&value["usage"]),
    })
}

fn string(value: &Value, key: &str) -> Result<String, Error> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Ambiguous(format!("Responses item is missing `{key}`")))
}

fn usage(value: &Value) -> Usage {
    Usage {
        input_tokens: value["input_tokens"].as_u64(),
        output_tokens: value["output_tokens"].as_u64(),
        cache_read_input_tokens: value["input_tokens_details"]["cached_tokens"].as_u64(),
        reasoning_tokens: value["output_tokens_details"]["reasoning_tokens"].as_u64(),
        ..Usage::default()
    }
}
