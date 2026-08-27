use std::{collections::BTreeMap, net::IpAddr, time::Duration};

use async_trait::async_trait;
use brain_protocol::{
    ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, OperationId,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::{KernelError, ModelExecutor, model::sse::SseDecoder};

const MAX_ERROR_BYTES: usize = 16 * 1024;
const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;

pub struct RemoteModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub timeout: Duration,
}

pub struct RemoteModelClient {
    client: reqwest::Client,
    base_url: String,
    api_key: Zeroizing<String>,
}

impl RemoteModelClient {
    pub fn new(config: RemoteModelConfig) -> Result<Self, KernelError> {
        if config.base_url.trim().is_empty() || config.api_key.trim().is_empty() {
            return Err(KernelError::InvalidState(
                "model base URL and API key are required".into(),
            ));
        }
        let url = reqwest::Url::parse(&config.base_url).map_err(|error| {
            KernelError::InvalidState(format!("model base URL is invalid: {error}"))
        })?;
        let loopback_http = url.scheme() == "http"
            && url
                .host_str()
                .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback());
        if !(url.scheme() == "https" || loopback_http)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(KernelError::InvalidState(
                "model base URL must use HTTPS or literal loopback HTTP and cannot contain credentials, query, or fragment"
                    .into(),
            ));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(config.timeout.min(Duration::from_secs(10)))
            .timeout(config.timeout)
            .build()
            .map_err(|error| KernelError::Executor(error.to_string()))?;
        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_owned(),
            api_key: Zeroizing::new(config.api_key),
        })
    }

    fn body(
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: &ModelRequest,
    ) -> Value {
        let mut messages = Vec::with_capacity(request.messages.len() + 1);
        if !presentation.system.is_empty() {
            messages.push(json!({"role":"system","content":presentation.system}));
        }
        messages.extend(request.messages.iter().map(provider_message));
        let tools: Vec<Value> = presentation.tools.iter().map(|tool| json!({
            "type":"function",
            "function":{"name":tool.name,"description":tool.description,"parameters":tool.input_schema}
        })).collect();
        let mut body = json!({"model":binding.model,"stream":true,"stream_options":{"include_usage":true},"messages":messages});
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        if let Some(format) = request
            .response_format
            .as_ref()
            .or(presentation.response_format.as_ref())
        {
            body["response_format"] = format.clone();
        }
        if let Some(tokens) = request.max_output_tokens {
            body["max_completion_tokens"] = json!(tokens);
        }
        body
    }
}

fn provider_message(message: &Value) -> Value {
    let Some(role) = message.get("role").and_then(Value::as_str) else {
        return message.clone();
    };
    match role {
        "assistant" => {
            let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
                return message.clone();
            };
            let mut encoded = message.clone();
            encoded["tool_calls"] = Value::Array(
                calls
                    .iter()
                    .map(|call| {
                        let Some(call_id) = call.get("callId").and_then(Value::as_str) else {
                            return call.clone();
                        };
                        let Some(name) = call.get("name").and_then(Value::as_str) else {
                            return call.clone();
                        };
                        let input = call.get("input").cloned().unwrap_or(Value::Null);
                        json!({
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(&input).expect("JSON values serialize"),
                            }
                        })
                    })
                    .collect(),
            );
            encoded
        }
        "tool" => {
            let mut encoded = message.clone();
            if !encoded["content"].is_string() {
                encoded["content"] = Value::String(
                    serde_json::to_string(&encoded["content"]).expect("JSON values serialize"),
                );
            }
            if let Some(object) = encoded.as_object_mut() {
                object.remove("is_error");
            }
            encoded
        }
        _ => message.clone(),
    }
}

#[async_trait]
impl ModelExecutor for RemoteModelClient {
    async fn execute(
        &self,
        operation_id: &OperationId,
        request_digest: &str,
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(self.api_key.as_str())
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .header("x-idempotency-key", operation_id.as_str())
            .header("x-request-digest", request_digest)
            .json(&Self::body(binding, presentation, &request))
            .send()
            .await
            .map_err(|error| {
                KernelError::Executor(format!("model request failed before response: {error}"))
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let bytes = response
                .bytes()
                .await
                .map_err(|error| KernelError::Executor(error.to_string()))?;
            let bounded = &bytes[..bytes.len().min(MAX_ERROR_BYTES)];
            return Err(KernelError::Executor(format!(
                "model returned {status}: {}",
                String::from_utf8_lossy(bounded)
            )));
        }
        let mut decoder = SseDecoder::new(256 * 1024);
        let mut stream = response.bytes_stream();
        let mut total = 0_usize;
        let mut text = String::new();
        let mut tool_calls = BTreeMap::new();
        let mut usage = None;
        let mut finish_reason = None;
        let mut done = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                KernelError::Ambiguous(format!("model stream interrupted: {error}"))
            })?;
            total = total.saturating_add(chunk.len());
            if total > MAX_STREAM_BYTES {
                return Err(KernelError::Ambiguous(
                    "model stream exceeded 32 MiB".into(),
                ));
            }
            for data in decoder.feed(&chunk)? {
                if data == "[DONE]" {
                    done = true;
                    break;
                }
                let event: Value = serde_json::from_str(&data).map_err(|error| {
                    KernelError::Ambiguous(format!("model stream returned invalid JSON: {error}"))
                })?;
                if let Some(value) = event.get("usage").filter(|value| !value.is_null()) {
                    usage = Some(value.clone());
                    on_event(ModelStreamEvent::Usage {
                        usage: value.clone(),
                    });
                }
                if let Some(choice) = event
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|choices| choices.first())
                {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(value) = delta.get("content").and_then(Value::as_str) {
                            text.push_str(value);
                            on_event(ModelStreamEvent::TextDelta {
                                text: value.to_owned(),
                            });
                        }
                        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                            for call in calls {
                                accumulate_tool_call(&mut tool_calls, call);
                                on_event(ModelStreamEvent::ToolCallDelta { call: call.clone() });
                            }
                        }
                    }
                    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                        finish_reason = Some(reason.to_owned());
                    }
                }
            }
            if done {
                break;
            }
        }
        if !done || decoder.pending_bytes() != 0 {
            return Err(KernelError::Ambiguous(
                "model stream ended without a complete terminal marker".into(),
            ));
        }
        let tool_calls: Vec<Value> = tool_calls
            .into_values()
            .map(|call| json!({"id":call.id,"function":{"name":call.name,"arguments":call.arguments}}))
            .collect();
        Ok(ModelResult {
            response: json!({"text":text,"tool_calls":tool_calls,"finish_reason":finish_reason}),
            usage,
        })
    }
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

fn accumulate_tool_call(calls: &mut BTreeMap<usize, ToolCallAccumulator>, delta: &Value) {
    let index = delta.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    let call = calls.entry(index).or_default();
    if let Some(id) = delta.get("id").and_then(Value::as_str) {
        call.id = id.to_owned();
    }
    if let Some(function) = delta.get("function") {
        if let Some(name) = function.get("name").and_then(Value::as_str) {
            call.name.push_str(name);
        }
        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
            call.arguments.push_str(arguments);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Bytes, http::HeaderMap, routing::post};
    use brain_protocol::{ModelPresentation, ToolDefinition};
    use tokio::sync::oneshot;

    #[test]
    fn assembles_fragmented_tool_calls_in_provider_order() {
        let mut calls = BTreeMap::new();
        accumulate_tool_call(
            &mut calls,
            &json!({"index":1,"id":"b","function":{"name":"read","arguments":"{\"pa"}}),
        );
        accumulate_tool_call(
            &mut calls,
            &json!({"index":0,"id":"a","function":{"name":"ls","arguments":"{}"}}),
        );
        accumulate_tool_call(
            &mut calls,
            &json!({"index":1,"function":{"arguments":"th\":\"x\"}"}}),
        );
        let calls: Vec<_> = calls.into_values().collect();
        assert_eq!(calls[0].id, "a");
        assert_eq!(calls[1].arguments, "{\"path\":\"x\"}");
    }

    #[test]
    fn encodes_portable_tool_messages_for_the_provider() {
        let body = RemoteModelClient::body(
            &ModelBinding {
                binding_id: "gateway".into(),
                model: "test/model".into(),
            },
            &ModelPresentation {
                system: String::new(),
                tools: Vec::new(),
                response_format: None,
            },
            &ModelRequest {
                messages: vec![
                    json!({
                        "role":"assistant",
                        "content":"",
                        "tool_calls":[{"callId":"call_1","name":"bash","input":{"command":"true"}}]
                    }),
                    json!({
                        "role":"tool",
                        "tool_call_id":"call_1",
                        "content":{"stdout":""},
                        "is_error":false
                    }),
                ],
                response_format: None,
                max_output_tokens: None,
            },
        );
        assert_eq!(body["messages"][0]["tool_calls"][0]["type"], "function");
        assert_eq!(
            body["messages"][0]["tool_calls"][0]["function"]["arguments"],
            r#"{"command":"true"}"#
        );
        assert_eq!(body["messages"][1]["content"], r#"{"stdout":""}"#);
        assert!(body["messages"][1].get("is_error").is_none());
    }

    #[tokio::test]
    async fn streams_a_remote_response_with_stable_operation_headers() {
        let (observed_tx, observed_rx) = oneshot::channel();
        let observed_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(observed_tx)));
        let app = Router::new().route(
            "/chat/completions",
            post(move |headers: HeaderMap, body: Bytes| {
                let observed_tx = observed_tx.clone();
                async move {
                    observed_tx
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .send((headers, body))
                        .unwrap();
                    (
                        [("content-type", "text/event-stream")],
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"total_tokens\":3}}\n\ndata: [DONE]\n\n",
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = RemoteModelClient::new(RemoteModelConfig {
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            timeout: Duration::from_secs(2),
        })
        .unwrap();
        let mut events = Vec::new();
        let result = client
            .execute(
                &OperationId::new("op_test"),
                "request-digest",
                &ModelBinding {
                    binding_id: "gateway".into(),
                    model: "test/model".into(),
                },
                &ModelPresentation {
                    system: "system".into(),
                    tools: vec![ToolDefinition {
                        name: "read".into(),
                        description: "read a file".into(),
                        input_schema: json!({"type":"object"}),
                        output_schema: None,
                    }],
                    response_format: None,
                },
                ModelRequest {
                    messages: vec![json!({"role":"user","content":"hi"})],
                    response_format: None,
                    max_output_tokens: Some(12),
                },
                &mut |event| events.push(event),
            )
            .await
            .unwrap();
        let (headers, body) = observed_rx.await.unwrap();
        assert_eq!(headers["authorization"], "Bearer test-key");
        assert_eq!(headers["x-idempotency-key"], "op_test");
        assert_eq!(headers["x-request-digest"], "request-digest");
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["messages"][0]["content"], "system");
        assert_eq!(body["max_completion_tokens"], 12);
        assert_eq!(result.response["text"], "hello");
        assert_eq!(result.usage.as_ref().unwrap()["total_tokens"], 3);
        assert_eq!(events.len(), 2);
    }
}
