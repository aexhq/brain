use super::*;
use brain_protocol::{ContentBlock, Message};
use serde_json::{Value, json};

fn request(messages: Vec<Message>) -> ModelRequest {
    serde_json::from_value(json!({"messages": messages})).unwrap()
}

#[test]
fn signed_and_redacted_thinking_round_trip_in_order() {
    let frames = [
        json!({"type":"content_block_start", "index":0, "content_block":{"type":"thinking", "thinking":""}}),
        json!({"type":"content_block_delta", "index":0, "delta":{"type":"thinking_delta", "thinking":"consider"}}),
        json!({"type":"content_block_delta", "index":0, "delta":{"type":"signature_delta", "signature":"opaque"}}),
        json!({"type":"content_block_stop", "index":0}),
        json!({"type":"content_block_start", "index":1, "content_block":{"type":"redacted_thinking", "data":"redacted"}}),
        json!({"type":"content_block_stop", "index":1}),
        json!({"type":"content_block_start", "index":2, "content_block":{"type":"tool_use", "id":"call", "name":"read", "input":{}}}),
        json!({"type":"content_block_delta", "index":2, "delta":{"type":"input_json_delta", "partial_json":"{}"}}),
        json!({"type":"content_block_stop", "index":2}),
        json!({"type":"message_delta", "delta":{"stop_reason":"tool_use"}}),
    ];
    let mut accumulated = Accumulator::new(&crate::Limits::default());
    for frame in frames {
        for event in anthropic::decode(&frame.to_string()).unwrap() {
            accumulated.push(event).unwrap();
        }
    }
    let (message, stop, usage) = accumulated.finish().unwrap();
    assert_eq!(stop, brain_protocol::StopReason::ToolUse);
    assert_eq!(usage, brain_protocol::Usage::default());
    let restored: Message = serde_json::from_slice(&serde_json::to_vec(&message).unwrap()).unwrap();
    let call = request(vec![restored]);
    let body = anthropic::body("test", &[], &call).unwrap();
    assert_eq!(
        body["messages"][0]["content"][0],
        json!({"type":"thinking", "thinking":"consider", "signature":"opaque"})
    );
    assert_eq!(
        body["messages"][0]["content"][1],
        json!({"type":"redacted_thinking", "data":"redacted"})
    );
    assert_eq!(body["messages"][0]["content"][2]["id"], "call");
    assert!(openai::body("test", &[], &call, MaxTokensField::default()).is_err());
}

#[test]
fn chat_reasoning_survives_with_tool_calls() {
    let mut accumulated = Accumulator::new(&crate::Limits::default());
    for delta in [
        json!({"reasoning_content":"think"}),
        json!({"reasoning_content":" more"}),
        json!({"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"{}"}}]}),
    ] {
        for event in openai::decode(&json!({"choices":[{"delta":delta}]}).to_string()).unwrap() {
            accumulated.push(event).unwrap();
        }
    }
    let (message, _, _) = accumulated.finish().unwrap();
    assert_eq!(message.tool_uses().count(), 1);
    let body = openai::body(
        "test",
        &[],
        &request(vec![message]),
        MaxTokensField::default(),
    )
    .unwrap();
    assert_eq!(body["messages"][0]["reasoning_content"], "think more");
}

#[test]
fn responses_reasoning_and_compaction_preserve_native_items() {
    assert!(responses::compact_result(json!({"output":[]})).is_err());
    assert!(responses::compact_result(json!({"output":[{"type":"message"}]})).is_err());
    let reasoning =
        json!({"type":"reasoning", "id":"rs_one", "summary":[], "encrypted_content":"opaque"});
    let call = json!({"type":"function_call", "id":"fc_one", "call_id":"call", "name":"read", "arguments":"{}"});
    let mut accumulated = Accumulator::new(&crate::Limits::default());
    for (index, item) in [reasoning.clone(), call.clone()].into_iter().enumerate() {
        for event in responses::decode(
            &json!({"type":"response.output_item.done", "output_index":index,"item":item})
                .to_string(),
        )
        .unwrap()
        {
            accumulated.push(event).unwrap();
        }
    }
    let (message, _, _) = accumulated.finish().unwrap();
    let body = responses::body("test", &[], &request(vec![message])).unwrap();
    assert_eq!(body["input"][0], reasoning);
    assert_eq!(body["input"][1]["call_id"], "call");
    let retained = json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"task"}]}, {"type":"compaction","encrypted_content":"compact"}]);
    let result = responses::compact_result(json!({"output":retained})).unwrap();
    let restored = serde_json::from_slice(&serde_json::to_vec(&result.message).unwrap()).unwrap();
    assert_eq!(
        responses::body("test", &[], &request(vec![restored])).unwrap()["input"],
        retained
    );
}

#[test]
fn media_reset_options_and_native_limits_are_explicit() {
    let image = "data:image/png;base64,aGVsbG8=";
    let messages: Vec<Message> = serde_json::from_value(json!([{"role":"user", "content":[{"type":"image","url":image},{"type":"tool_result","tool_use_id":"call","content":"image","is_error":false,"media":[{"type":"image","url":image}]}]}])).unwrap();
    let body = anthropic::body("test", &[], &request(messages.clone())).unwrap();
    assert_eq!(
        body["messages"][0]["content"][0]["source"]["type"],
        "base64"
    );
    assert_eq!(
        body["messages"][0]["content"][1]["content"][1]["type"],
        "image"
    );
    assert!(
        responses::body("test", &[], &request(messages)).unwrap()["input"][1]["output"].is_array()
    );
    let reset: ModelRequest = serde_json::from_value(
        json!({"messages":[{"role":"user","content":[]}], "response_format":null}),
    )
    .unwrap();
    assert_eq!(reset.response_format, Some(Value::Null));
    assert_eq!(request(vec![]).response_format, None);
    assert!(anthropic::body("test", &[], &reset).is_ok());
    for field in [
        "model",
        "api_key",
        "base_url",
        "tools",
        "store",
        "previous_response_id",
    ] {
        let mut call = request(vec![Message::user_text("hello")]);
        call.options.insert(field.into(), json!("forged"));
        assert!(anthropic::body("test", &[], &call).is_err());
        assert!(openai::body("test", &[], &call, MaxTokensField::default()).is_err());
        assert!(responses::body("test", &[], &call).is_err());
    }
    let limits = crate::Limits {
        max_model_output_bytes: 8,
        ..Default::default()
    };
    let mut accumulated = Accumulator::new(&limits);
    assert!(
        accumulated
            .push(brain_protocol::ModelStreamEvent::NativeStart {
                index: 0,
                format: "anthropic.messages.v1".into(),
                data: json!({"type":"thinking","thinking":"too large"})
            })
            .is_err()
    );
    assert!(
        serde_json::from_value::<ContentBlock>(
            json!({"type":"native","format":"x","data":{},"credentials":"secret"})
        )
        .is_err()
    );
}

#[test]
fn chat_preserves_interleaved_media_and_text() {
    let messages = serde_json::from_value(json!([{"role":"user","content":[
        {"type":"text","text":"before"}, {"type":"image","url":"https://example.com/view.png"}, {"type":"text","text":"after"}
    ]}])).unwrap();
    let body = openai::body("test", &[], &request(messages), MaxTokensField::default()).unwrap();
    let parts = &body["messages"][0]["content"];
    assert_eq!(parts[0]["text"], "before");
    assert_eq!(parts[1]["type"], "image_url");
    assert_eq!(parts[2]["text"], "after");
}
