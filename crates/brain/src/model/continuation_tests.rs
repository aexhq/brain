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
    assert!(responses::body("test", &[], &call).is_err());
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
    let image = "https://example.com/view.png";
    let messages: Vec<Message> = serde_json::from_value(json!([{"role":"user", "content":[{"type":"image","url":image},{"type":"tool_result","tool_use_id":"call","content":"image","is_error":false,"media":[{"type":"image","url":image}]}]}])).unwrap();
    let body = anthropic::body("test", &[], &request(messages.clone())).unwrap();
    assert_eq!(body["messages"][0]["content"][0]["source"]["type"], "url");
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
fn responses_preserves_interleaved_media_and_text() {
    let messages = serde_json::from_value(json!([{"role":"user","content":[
        {"type":"text","text":"before"}, {"type":"image","url":"https://example.com/view.png"}, {"type":"text","text":"after"}
    ]}])).unwrap();
    let body = responses::body("test", &[], &request(messages)).unwrap();
    let parts = &body["input"];
    assert_eq!(parts[0]["content"], "before");
    assert_eq!(parts[1]["content"][0]["type"], "input_image");
    assert_eq!(parts[2]["content"], "after");
}

#[test]
fn pdf_urls_render_in_user_and_tool_content() {
    let media = json!({"type":"file", "media_type":"application/pdf", "url":"https://example.com/report.pdf"});
    let messages = serde_json::from_value(json!([{"role":"user", "content":[media.clone(), {"type":"tool_result","tool_use_id":"call","content":"report","is_error":true,"media":[media]}]}])).unwrap();
    let call = request(messages);
    let responses = responses::body("test", &[], &call).unwrap();
    assert_eq!(
        responses["input"][0]["content"][0],
        json!({"type":"input_file","file_url":"https://example.com/report.pdf"})
    );
    assert_eq!(responses["input"][1]["output"][0]["text"], "ERROR: report");
    assert_eq!(
        responses["input"][1]["output"][1],
        responses["input"][0]["content"][0]
    );
    let anthropic = anthropic::body("test", &[], &call).unwrap();
    assert_eq!(
        anthropic["messages"][0]["content"][0],
        json!({"type":"document","source":{"type":"url","url":"https://example.com/report.pdf"}})
    );
    assert_eq!(anthropic["messages"][0]["content"][1]["is_error"], true);
    assert_eq!(
        anthropic["messages"][0]["content"][1]["content"][1],
        anthropic["messages"][0]["content"][0]
    );
}

#[test]
fn media_is_url_only_in_common_and_retained_inputs() {
    for url in [
        "data:image/png;base64,aGVsbG8=",
        "http://example.com/a",
        "file:///a",
        "s3://bucket/a",
        "https://user:secret@example.com/a",
    ] {
        let call = request(vec![Message {
            role: brain_protocol::Role::User,
            content: vec![ContentBlock::Image { url: url.into() }],
        }]);
        assert!(responses::body("test", &[], &call).is_err());
        assert!(anthropic::body("test", &[], &call).is_err());
    }
    for part in [
        json!({"type":"input_file","file_data":"bytes"}),
        json!({"type":"input_image","image_url":"data:image/png;base64,aGVsbG8="}),
        json!({"type":"input_file","file_url":"https://example.com/a","file_id":"file_1"}),
    ] {
        for data in [
            json!({"type":"message","role":"user","content":[part.clone()]}),
            json!({"type":"function_call_output","call_id":"call","output":[part.clone()]}),
        ] {
            let call = request(vec![Message::assistant(vec![ContentBlock::Native {
                format: "openai.responses.v1".into(),
                data,
            }])]);
            assert!(responses::body("test", &[], &call).is_err());
        }
    }
    assert!(
        serde_json::from_value::<brain_protocol::Media>(
            json!({"type":"file","media_type":"text/plain","url":"https://example.com/a"})
        )
        .is_err()
    );
}
