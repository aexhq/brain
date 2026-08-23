//! OpenAI-compatible Chat Completions adapter.
//!
//! This is the dialect the benchmark drives, because `experiments/agentfeat/
//! lib/oc-fake.mjs` already speaks it for OpenCode: pointing all three runtimes
//! at one fake is what makes the head-to-head a comparison rather than three
//! separate measurements.

use super::sse::SseDecoder;
use super::{ModelRequest, ProviderEvent};
use crate::config::{OutputTokenParameter, ProviderKey, SealedPrefix};
use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::{BrainError, Result};
use serde_json::{Map, Value, json};

#[derive(Debug, Default)]
pub struct OpenAiChat;

impl OpenAiChat {
    pub fn request(body: Value, key: &ProviderKey, base_url: &str) -> Result<ModelRequest> {
        Ok(ModelRequest {
            method: "POST",
            url: format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "text/event-stream".into()),
                ("authorization".into(), format!("Bearer {}", key.expose())),
            ],
            body: serde_json::to_vec(&body)?,
        })
    }

    /// Render ONE provider-neutral message into the dialect's array elements.
    ///
    /// Split out of `body` so that the pre-rendered transcript store
    /// (`provider::render`) and the reference builder share one renderer rather
    /// than two that must be kept in step. The byte-identity test in
    /// `provider::render` would catch a divergence; this makes one impossible.
    ///
    /// One neutral message is **not** always one element: a user message
    /// carrying N `tool_result` blocks becomes N `tool` messages, plus a
    /// separate `user` message if it also carried text.
    pub fn render_one(m: &Message) -> Result<Vec<Value>> {
        let mut messages: Vec<Value> = Vec::new();
        match m.role {
            Role::Assistant => {
                let mut text = String::new();
                let mut calls = Vec::new();
                for b in &m.content {
                    match b {
                        ContentBlock::Text { text: t } => text.push_str(t),
                        ContentBlock::ToolUse { id, name, input } => calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": serde_json::to_string(input)?}
                        })),
                        ContentBlock::ToolResult { .. } => {
                            return Err(BrainError::Protocol(
                                "a tool_result block cannot appear in an assistant message".into(),
                            ));
                        }
                    }
                }
                let mut o = Map::new();
                o.insert("role".into(), json!("assistant"));
                o.insert(
                    "content".into(),
                    if text.is_empty() {
                        Value::Null
                    } else {
                        json!(text)
                    },
                );
                if !calls.is_empty() {
                    o.insert("tool_calls".into(), json!(calls));
                }
                messages.push(Value::Object(o));
            }
            Role::User => {
                // One Anthropic user message carrying N tool_result blocks
                // becomes N OpenAI `tool` messages. Text blocks in the same
                // message become a separate user message, in order.
                let mut text = String::new();
                for b in &m.content {
                    match b {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            // The dialect has no `is_error`. Dropping the
                            // signal entirely is what makes a failed
                            // fan-out read as a success, so it is carried
                            // in-band and marked, never silently lost.
                            let payload = if *is_error {
                                format!("ERROR: {content}")
                            } else {
                                content.clone()
                            };
                            messages.push(json!({
                                "role":"tool",
                                "tool_call_id": tool_use_id,
                                "content": payload
                            }));
                        }
                        ContentBlock::Text { text: t } => text.push_str(t),
                        ContentBlock::ToolUse { .. } => {
                            return Err(BrainError::Protocol(
                                "a tool_use block cannot appear in a user message".into(),
                            ));
                        }
                    }
                }
                if !text.is_empty() {
                    messages.push(json!({"role":"user","content":text}));
                }
            }
        }
        Ok(messages)
    }

    pub fn body(prefix: &SealedPrefix, history: &[Message]) -> Result<Value> {
        let mut messages: Vec<Value> = Vec::with_capacity(history.len() + 1);
        for m in history {
            messages.extend(Self::render_one(m)?);
        }

        let mut body = match prefix.rendered_base() {
            Some(Value::Object(base)) => base.clone(),
            Some(_) => {
                return Err(BrainError::Journal(
                    "stored OpenAI base segment is not an object".into(),
                ));
            }
            None => Self::render_base(prefix),
        };
        let mut rendered_messages = body
            .remove("messages")
            .and_then(|value| value.as_array().cloned())
            .ok_or_else(|| {
                BrainError::Journal("stored OpenAI base has no messages array".into())
            })?;
        rendered_messages.extend(messages);
        body.insert("messages".into(), Value::Array(rendered_messages));
        Ok(Value::Object(body))
    }

    pub fn render_base(prefix: &SealedPrefix) -> Map<String, Value> {
        let tools: Vec<Value> = prefix
            .tools
            .iter()
            .map(|t| {
                let parameters = function_parameters(&t.input_schema);
                json!({
                    "type":"function",
                    "function":{
                        "name": t.name,
                        "description": t.description,
                        "parameters": parameters,
                    }
                })
            })
            .collect();

        let mut body = Map::new();
        body.insert("model".into(), json!(prefix.model));
        body.insert("stream".into(), json!(true));
        body.insert(
            match prefix.sampling.output_token_parameter {
                OutputTokenParameter::MaxTokens => "max_tokens",
                OutputTokenParameter::MaxCompletionTokens => "max_completion_tokens",
            }
            .into(),
            json!(prefix.sampling.max_tokens),
        );
        body.insert(
            "messages".into(),
            json!([{"role":"system","content":prefix.system_prompt}]),
        );
        if !tools.is_empty() {
            body.insert("tools".into(), json!(tools));
            if prefix.tool_choice_none {
                // The closing round: the model must answer in text.
                body.insert("tool_choice".into(), json!("none"));
            }
        }
        if let Some(t) = prefix.sampling.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(reasoning_effort) = &prefix.sampling.reasoning_effort {
            body.insert("reasoning_effort".into(), json!(reasoning_effort));
        }
        if !prefix.sampling.stop_sequences.is_empty() {
            body.insert("stop".into(), json!(prefix.sampling.stop_sequences));
        }
        // Ask for usage on the final chunk. Without it several
        // OpenAI-compatible servers report nothing at all -- which is absent,
        // and absent must not become zero downstream.
        body.insert("stream_options".into(), json!({"include_usage": true}));
        if let Some(key) = prefix.prompt_cache_key() {
            body.insert("prompt_cache_key".into(), json!(key));
        }
        body
    }
}

/// OpenAI-compatible function parameters must have one object schema at the root. Zod emits a
/// root `oneOf` for a discriminated union of objects, which providers reject before generation.
/// Lower that representable case to an object-shaped superset for the model. Brain still validates
/// every generated call against the exact sealed schema before dispatch, so this compatibility
/// projection cannot broaden what an executor receives.
fn function_parameters(input: &Value) -> Value {
    let Some(root) = input.as_object() else {
        return input.clone();
    };
    if root.get("type").is_some() {
        return input.clone();
    }
    let (union_key, alternatives) = match (
        root.get("oneOf").and_then(Value::as_array),
        root.get("anyOf").and_then(Value::as_array),
    ) {
        (Some(alternatives), None) => ("oneOf", alternatives),
        (None, Some(alternatives)) => ("anyOf", alternatives),
        _ => return input.clone(),
    };
    let branches: Vec<&Map<String, Value>> = alternatives
        .iter()
        .filter_map(Value::as_object)
        .filter(|branch| branch.get("type").and_then(Value::as_str) == Some("object"))
        .collect();
    if branches.len() != alternatives.len() || branches.is_empty() {
        return input.clone();
    }

    let mut properties = Map::new();
    let mut required = branches[0]
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let all_closed = branches
        .iter()
        .all(|branch| branch.get("additionalProperties") == Some(&Value::Bool(false)));

    for branch in branches {
        if let Some(branch_properties) = branch.get("properties").and_then(Value::as_object) {
            for (name, schema) in branch_properties {
                match properties.get_mut(name) {
                    None => {
                        properties.insert(name.clone(), schema.clone());
                    }
                    Some(existing) if existing != schema => {
                        *existing = merge_property_schema(existing, schema);
                    }
                    Some(_) => {}
                }
            }
        }
        let branch_required = branch
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        required.retain(|name| branch_required.contains(&name.as_str()));
    }

    let mut lowered = root.clone();
    lowered.remove(union_key);
    lowered.insert("type".into(), json!("object"));
    lowered.insert("properties".into(), Value::Object(properties));
    if required.is_empty() {
        lowered.remove("required");
    } else {
        lowered.insert("required".into(), json!(required));
    }
    if all_closed {
        lowered.insert("additionalProperties".into(), Value::Bool(false));
    } else {
        lowered.remove("additionalProperties");
    }
    Value::Object(lowered)
}

fn merge_property_schema(left: &Value, right: &Value) -> Value {
    let (Some(left_object), Some(right_object)) = (left.as_object(), right.as_object()) else {
        return json!({"anyOf": [left, right]});
    };
    let (Some(left_const), Some(right_const)) =
        (left_object.get("const"), right_object.get("const"))
    else {
        return json!({"anyOf": [left, right]});
    };
    let mut left_without_const = left_object.clone();
    let mut right_without_const = right_object.clone();
    left_without_const.remove("const");
    right_without_const.remove("const");
    if left_without_const != right_without_const {
        return json!({"anyOf": [left, right]});
    }
    left_without_const.insert("enum".into(), json!([left_const, right_const]));
    Value::Object(left_without_const)
}

impl OpenAiChat {
    /// The full request for one round: the pure dialect codec half of a live provider. The
    /// HTTP transport half lives in `brain-providers`.
    pub fn build_request(
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        Self::request(Self::body(prefix, history)?, key, base_url)
    }
}

pub fn decode(_event: Option<&str>, data: &str) -> Result<Vec<ProviderEvent>> {
    if data.trim() == "[DONE]" {
        return Ok(vec![]);
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|e| BrainError::Protocol(format!("openai frame: {e}")))?;
    if let Some(err) = v.get("error") {
        return Err(BrainError::Protocol(format!("provider error frame: {err}")));
    }
    let mut out = Vec::new();

    // Usage arrives on a chunk that may have no choices at all.
    let usage = usage_of(v.get("usage"));
    let choices = v.get("choices").and_then(|c| c.as_array());

    if let Some(choices) = choices {
        for ch in choices {
            let delta = ch.get("delta").unwrap_or(&Value::Null);
            if let Some(t) = delta.get("content").and_then(|c| c.as_str())
                && !t.is_empty()
            {
                out.push(ProviderEvent::TextDelta {
                    index: 0,
                    text: t.to_string(),
                });
            }
            if let Some(t) = delta.get("refusal").and_then(|c| c.as_str())
                && !t.is_empty()
            {
                out.push(ProviderEvent::RefusalDelta {
                    index: 0,
                    text: t.to_string(),
                });
            }
            if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                for c in calls {
                    // `index` is what makes N parallel calls in one assistant
                    // message distinguishable. +1 so index 0 stays reserved for
                    // the text block.
                    let idx = c.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize + 1;
                    let f = c.get("function").unwrap_or(&Value::Null);
                    let id = c.get("id").and_then(|s| s.as_str());
                    let name = f.get("name").and_then(|s| s.as_str());
                    if let (Some(id), Some(name)) = (id, name) {
                        out.push(ProviderEvent::ToolUseStart {
                            index: idx,
                            id: id.to_string(),
                            name: name.to_string(),
                        });
                    }
                    if let Some(a) = f.get("arguments").and_then(|s| s.as_str())
                        && !a.is_empty()
                    {
                        out.push(ProviderEvent::ToolInputDelta {
                            index: idx,
                            partial_json: a.to_string(),
                        });
                    }
                }
            }
            if let Some(fr) = ch.get("finish_reason").and_then(|f| f.as_str()) {
                out.push(ProviderEvent::MessageDone {
                    stop_reason: map_stop(Some(fr)),
                    usage,
                });
            }
        }
    }

    // A usage-only chunk (stream_options.include_usage) still has to be folded.
    if usage != Usage::default()
        && !out
            .iter()
            .any(|e| matches!(e, ProviderEvent::MessageDone { .. }))
    {
        out.push(ProviderEvent::Usage { usage });
    }
    Ok(out)
}

fn map_stop(s: Option<&str>) -> StopReason {
    match s {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") | Some("function_call") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some("content_filter") => StopReason::Refusal,
        _ => StopReason::Unknown,
    }
}

fn usage_of(u: Option<&Value>) -> Usage {
    let Some(u) = u else {
        return Usage::default();
    };
    let g = |k: &str| u.get(k).and_then(|v| v.as_u64());
    Usage {
        input_tokens: g("prompt_tokens"),
        output_tokens: g("completion_tokens"),
        // OpenAI reports cache reads nested. Absent stays absent.
        cache_read_input_tokens: u
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_u64()),
        cache_creation_input_tokens: None,
        reasoning_tokens: u
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_u64),
    }
}

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
    use crate::config::Dialect;
    use crate::config::{AgentDef, GenOpts, OutputTokenParameter, ToolDecl, ToolRoute};
    use crate::provider::Accumulator;

    fn prefix() -> std::sync::Arc<SealedPrefix> {
        AgentDef::new("sys", "fake-1", Dialect::OpenAiChat)
            .tool(ToolDecl {
                name: "read".into(),
                description: "read".into(),
                contract_digest: "a".repeat(64),
                input_schema: json!({"type":"object"}),
                output_schema: json!({"type":"object"}),
                route: ToolRoute::Intrinsic("brain.test.read".into()),
            })
            .seal()
    }

    #[test]
    fn modern_gpt_profile_uses_current_completion_and_reasoning_fields() {
        let definition = AgentDef::new("sys", "gpt-5.4", Dialect::OpenAiChat).sampling(GenOpts {
            max_tokens: 8_192,
            output_token_parameter: OutputTokenParameter::MaxCompletionTokens,
            temperature: None,
            reasoning_effort: Some("high".into()),
            stop_sequences: Vec::new(),
        });
        let body = OpenAiChat::render_base(&definition.seal());
        assert_eq!(body["max_completion_tokens"], 8_192);
        assert_eq!(body["reasoning_effort"], "high");
        assert!(!body.contains_key("max_tokens"));
    }

    #[test]
    fn a_loop_echo_of_the_sealed_presentation_keeps_the_prompt_cache_key() {
        // The W2/D3 gate for this dialect: prompt_cache_key must survive a ctx-composed
        // round whose presentation matches the seal; dropping it forfeits server-side
        // prompt caching on every loop-driven request.
        let sealed = AgentDef::new("sys", "gpt-5.4", Dialect::OpenAiChat)
            .seal()
            .with_provider_base(None, Some("aex:ses_parity".into()));
        let echoed = sealed.loop_call_view(None, Some(sealed.tools.clone()), None, None, false);
        let body = OpenAiChat::render_base(&std::sync::Arc::new(echoed));
        assert_eq!(body["prompt_cache_key"], "aex:ses_parity");
        let changed = sealed.loop_call_view(Some("different".into()), None, None, None, false);
        let body = OpenAiChat::render_base(&std::sync::Arc::new(changed));
        assert!(!body.contains_key("prompt_cache_key"));
    }

    #[test]
    fn named_legacy_compatibility_profile_is_explicit() {
        let definition =
            AgentDef::new("sys", "deepseek-chat", Dialect::OpenAiChat).sampling(GenOpts {
                max_tokens: 4_096,
                output_token_parameter: OutputTokenParameter::MaxTokens,
                temperature: None,
                reasoning_effort: None,
                stop_sequences: Vec::new(),
            });
        let body = OpenAiChat::render_base(&definition.seal());
        assert_eq!(body["max_tokens"], 4_096);
        assert!(!body.contains_key("max_completion_tokens"));
    }

    #[test]
    fn parallel_tool_calls_in_one_message_stay_distinct() {
        // The exact shape oc-fake.mjs emits: ONE delta carrying all N calls.
        let raw = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[\
            {\"index\":0,\"id\":\"c0\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"i\\\":0}\"}},\
            {\"index\":1,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"i\\\":1}\"}},\
            {\"index\":2,\"id\":\"c2\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"i\\\":2}\"}},\
            {\"index\":3,\"id\":\"c3\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"i\\\":3}\"}}\
            ]},\"finish_reason\":null}]}\n\n\
            data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
        let mut acc = Accumulator::new();
        for e in decode_stream(raw.as_bytes()).unwrap() {
            acc.push(e).unwrap();
        }
        let (msg, stop, _) = acc.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        let uses: Vec<_> = msg.tool_uses().collect();
        assert_eq!(uses.len(), 4, "p90 batch size is 4; all four must survive");
        for (i, (id, name, input)) in uses.iter().enumerate() {
            assert_eq!(*id, &format!("c{i}"));
            assert_eq!(*name, "read");
            assert_eq!(input["i"], i as i64);
        }
    }

    #[test]
    fn tool_results_become_tool_role_messages_and_keep_the_error_signal() {
        let p = prefix();
        let h = vec![
            Message::user_text("go"),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "c0".into(),
                name: "read".into(),
                input: json!({}),
            }]),
            Message::tool_results(vec![ContentBlock::ToolResult {
                tool_use_id: "c0".into(),
                content: "child 3 of 4 failed".into(),
                is_error: true,
            }]),
        ];
        let v: Value = serde_json::from_slice(
            &OpenAiChat::build_request(&p, &h, &ProviderKey::new("k"), "http://x")
                .unwrap()
                .body,
        )
        .unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[2]["tool_calls"][0]["id"], "c0");
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "c0");
        assert!(
            msgs[3]["content"].as_str().unwrap().starts_with("ERROR:"),
            "the error signal must survive a dialect with no is_error field"
        );
    }

    #[test]
    fn root_object_union_is_lowered_for_openai_without_changing_the_sealed_schema() {
        let exact = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "oneOf": [
                {
                    "type": "object",
                    "properties": {"action": {"type": "string", "const": "get"}},
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "action": {"type": "string", "const": "set"},
                        "items": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["action", "items"],
                    "additionalProperties": false
                }
            ]
        });
        let prefix = AgentDef::new("sys", "fake-1", Dialect::OpenAiChat)
            .tool(ToolDecl {
                name: "todo".into(),
                description: "todo".into(),
                contract_digest: "a".repeat(64),
                input_schema: exact.clone(),
                output_schema: json!({"type":"object"}),
                route: ToolRoute::Intrinsic("brain.test.todo".into()),
            })
            .seal();

        let body = OpenAiChat::body(&prefix, &[]).unwrap();
        let parameters = &body["tools"][0]["function"]["parameters"];
        assert_eq!(parameters["type"], "object");
        assert!(parameters.get("oneOf").is_none());
        assert_eq!(
            parameters["properties"]["action"]["enum"],
            json!(["get", "set"])
        );
        assert_eq!(parameters["required"], json!(["action"]));
        assert_eq!(parameters["additionalProperties"], false);
        assert!(parameters["properties"].get("items").is_some());
        assert_eq!(prefix.tools[0].input_schema, exact);
    }

    #[test]
    fn usage_only_chunk_is_folded_and_absent_stays_absent() {
        let raw =
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}}\n\n";
        let evs = decode_stream(raw.as_bytes()).unwrap();
        match &evs[0] {
            ProviderEvent::Usage { usage } => {
                assert_eq!(usage.input_tokens, Some(9));
                assert_eq!(usage.cache_read_input_tokens, None);
            }
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn structured_refusal_survives_an_ordinary_stop_and_usage_chunk() {
        let raw = concat!(
            "data: {\"choices\":[{\"delta\":{\"refusal\":\"I cannot help with that.\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":5}}\n\n"
        );
        let mut accumulator = Accumulator::new();
        for event in decode_stream(raw.as_bytes()).unwrap() {
            accumulator.push(event).unwrap();
        }
        let (message, stop, usage) = accumulator.finish().unwrap();
        assert_eq!(stop, StopReason::Refusal);
        assert_eq!(usage.input_tokens, Some(9));
        assert!(matches!(
            &message.content[0],
            ContentBlock::Text { text } if text == "I cannot help with that."
        ));
    }

    #[test]
    fn a_real_zero_cached_tokens_survives() {
        // Truthiness checks drop a genuine zero, even though zero is data rather than absence.
        let raw = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"prompt_tokens_details\":{\"cached_tokens\":0}}}\n\n";
        let evs = decode_stream(raw.as_bytes()).unwrap();
        match &evs[0] {
            ProviderEvent::Usage { usage } => {
                assert_eq!(
                    usage.cache_read_input_tokens,
                    Some(0),
                    "a reported zero must be Some(0), never None"
                );
            }
            other => panic!("expected Usage, got {other:?}"),
        }
    }
}
