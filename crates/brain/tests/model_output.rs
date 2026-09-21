mod common;

use axum::{Router, routing::post};
use brain::{
    Limits,
    model::{Dialect, RemoteModelClient, RemoteModelConfig},
};
use brain_protocol::{Message, MessageRequest, ModelRequest};
use brain_telemetry::telemetry_channel;
use common::{NoTools, Runtime, config, scripted, temporary_directory};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn completed_provider_status_survives_invalid_json_in_the_durable_failure() {
    for (terminal, reason, arguments, expected) in [
        (
            "response.incomplete",
            "max_output_tokens",
            "{\"x\":",
            "model_output_incomplete",
        ),
        (
            "response.incomplete",
            "max_output_tokens",
            "{\"x\":1}",
            "model_output_incomplete",
        ),
        (
            "response.incomplete",
            "content_filter",
            "{\"x\":",
            "model_output_incomplete",
        ),
        (
            "response.completed",
            "",
            "{\"x\":1 \"y\":2}",
            "model_output_invalid",
        ),
        ("response.completed", "", "{\"x\":", "model_output_invalid"),
    ] {
        let item = json!({"type":"function_call", "call_id":"call_1", "name":"write", "arguments":arguments});
        let frames = [
            json!({"type":"response.output_item.done", "output_index":0, "item":item}),
            json!({"type":terminal, "response":{"output":[item], "incomplete_details":{"reason":reason},
                "usage":{"input_tokens":11,"output_tokens":6,"output_tokens_details":{"reasoning_tokens":4}}}}),
        ];
        let stream = frames
            .iter()
            .map(|frame| format!("data: {frame}\n\n"))
            .collect::<String>();
        let app = Router::new().route(
            "/responses",
            post(move || {
                let stream = stream.clone();
                async move { ([("content-type", "text/event-stream")], stream) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let model = Arc::new(
            RemoteModelClient::new(RemoteModelConfig {
                base_url: format!("http://{address}"),
                api_key: "fixture".into(),
                limits: Limits::default(),
                dialect: Dialect::OpenAiResponses,
            })
            .unwrap(),
        );
        let loop_executor = scripted(|input, services| async move {
            let messages = vec![Message::user_text(
                &input.input.as_ref().expect("user activation").message,
            )];
            services.set_transcript(messages.clone()).await?;
            services
                .model(ModelRequest {
                    system: None,
                    tools: None,
                    messages,
                    response_format: None,
                    max_output_tokens: Some(6),
                    options: Default::default(),
                })
                .await?;
            panic!("an unusable tool response must never reach dispatch");
        });
        let directory = temporary_directory("model-output");
        let (telemetry, _worker) = telemetry_channel();
        let runtime = Runtime::open(
            &directory,
            telemetry,
            8,
            120,
            loop_executor,
            model,
            Arc::new(NoTools),
        );
        let handle = runtime.create(&config(), &[]).unwrap();
        handle
            .message(MessageRequest {
                input: "write".into(),
            })
            .await
            .unwrap();
        runtime.drain();
        let events = runtime.events(handle.id(), 0, 1000).events;
        let failed = events
            .iter()
            .find(|event| event.event_type == "model_call_failed")
            .unwrap();
        assert_eq!(failed.data["code"], expected);
        assert_eq!(failed.data["ambiguous"], false);
        assert_eq!(failed.data["retryable"], false);
        assert_eq!(
            failed.data["response"]["usage"],
            json!({"input_tokens":11,"output_tokens":6,"reasoning_tokens":4})
        );
        assert_eq!(
            failed.data["response"]["stop_reason"],
            match reason {
                "max_output_tokens" => "max_tokens",
                "content_filter" => "refusal",
                _ => "tool_use",
            }
        );
        assert!(failed.data["sequence"].as_u64().unwrap() < failed.sequence);
        assert!(
            !events
                .iter()
                .any(|event| event.event_type == "tool_call_started")
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "model_call_started")
                .count(),
            1
        );
        assert!(events.last().unwrap().sequence > failed.sequence);
        drop(handle);
        drop(runtime);
        server.abort();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
