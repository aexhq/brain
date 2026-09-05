use std::{net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use brain_protocol::{ModelBinding, ModelRequest, ModelResult, ModelStreamEvent, ToolDefinition};
use futures_util::StreamExt;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::{
    Error, ModelExecutor,
    model::{Accumulator, Dialect, MaxTokensField, anthropic, openai, sse::SseDecoder},
};

const MAX_ERROR_BYTES: usize = 16 * 1024;
const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = 256 * 1024;

pub struct RemoteModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub timeout: Duration,
    pub dialect: Dialect,
    pub max_tokens_field: MaxTokensField,
}

/// The half of a model client that does not depend on who is calling: the validated
/// endpoint and the connection pool behind it. Both are expensive to build and both are
/// the same for every session on a provider, so a process builds one per provider and
/// shares it. Building one per call discards the pool with it, which costs a TCP
/// connect and a TLS handshake on every model call.
pub struct ModelTransport {
    client: reqwest::Client,
    base_url: String,
}

pub struct RemoteModelClient {
    transport: Arc<ModelTransport>,
    api_key: Zeroizing<String>,
    dialect: Dialect,
    max_tokens_field: MaxTokensField,
}

/// The endpoint rules every provider base URL must satisfy, wherever it comes
/// from: the generated catalog (checked again at generation time), a custom
/// providers file, or an override flag.
pub fn validate_base_url(base_url: &str) -> Result<(), Error> {
    if base_url.trim().is_empty() {
        return Err(Error::InvalidState("model base URL is required".into()));
    }
    let url = reqwest::Url::parse(base_url)
        .map_err(|error| Error::InvalidState(format!("model base URL is invalid: {error}")))?;
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
        return Err(Error::InvalidState(
            "model base URL must use HTTPS or literal loopback HTTP and cannot contain credentials, query, or fragment"
                .into(),
        ));
    }
    Ok(())
}

impl ModelTransport {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, Error> {
        validate_base_url(base_url)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .build()
            .map_err(|error| Error::Executor(error.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }
}

impl RemoteModelClient {
    /// Builds a transport of its own. For a caller that makes one call, or a test.
    pub fn new(config: RemoteModelConfig) -> Result<Self, Error> {
        let transport = ModelTransport::new(&config.base_url, config.timeout)?;
        Self::bound(
            Arc::new(transport),
            config.api_key,
            config.dialect,
            config.max_tokens_field,
        )
    }

    /// Binds a credential to a transport the caller already holds. This is the shape a
    /// server uses: one transport per provider, one credential per session.
    pub fn bound(
        transport: Arc<ModelTransport>,
        api_key: String,
        dialect: Dialect,
        max_tokens_field: MaxTokensField,
    ) -> Result<Self, Error> {
        if api_key.trim().is_empty() {
            return Err(Error::InvalidState("model API key is required".into()));
        }
        Ok(Self {
            transport,
            api_key: Zeroizing::new(api_key),
            dialect,
            max_tokens_field,
        })
    }

    fn body(
        &self,
        binding: &ModelBinding,
        request: &ModelRequest,
        tools: &[ToolDefinition],
    ) -> Result<Value, Error> {
        match self.dialect {
            Dialect::OpenAiChat => {
                openai::body(&binding.model, tools, request, self.max_tokens_field)
            }
            Dialect::AnthropicMessages => anthropic::body(&binding.model, tools, request),
        }
    }

    fn decode(&self, data: &str) -> Result<Vec<ModelStreamEvent>, Error> {
        match self.dialect {
            Dialect::OpenAiChat => openai::decode(data),
            Dialect::AnthropicMessages => anthropic::decode(data),
        }
    }

    fn request(&self, body: &Value) -> reqwest::RequestBuilder {
        let (path, headers) = match self.dialect {
            Dialect::OpenAiChat => (openai::path(), openai::headers(self.api_key.as_str())),
            Dialect::AnthropicMessages => {
                (anthropic::path(), anthropic::headers(self.api_key.as_str()))
            }
        };
        let mut request = self
            .transport
            .client
            .post(format!("{}{path}", self.transport.base_url))
            .header("content-type", "application/json")
            .header("accept", "text/event-stream");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        request.json(body)
    }
}

#[async_trait]
impl ModelExecutor for RemoteModelClient {
    async fn execute(
        &self,
        binding: &ModelBinding,
        request: ModelRequest,
        tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error> {
        let body = self.body(binding, &request, tools)?;
        let mut response = self.request(&body).send().await.map_err(|error| {
            Error::Ambiguous(format!("model request outcome is unknown: {error}"))
        })?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let retry_after_ms = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok())
                .and_then(|seconds| seconds.checked_mul(1_000));
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| Error::Ambiguous(error.to_string()))?
            {
                let count = chunk.len().min(MAX_ERROR_BYTES - bytes.len());
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.len() == MAX_ERROR_BYTES {
                    break;
                }
            }
            return Err(Error::ProviderStatus {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
                retry_after_ms,
            });
        }
        let mut decoder = SseDecoder::new(MAX_FRAME_BYTES);
        let mut stream = response.bytes_stream();
        let mut total = 0_usize;
        let mut accumulator = Accumulator::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|error| Error::Ambiguous(format!("model stream interrupted: {error}")))?;
            total = total.saturating_add(chunk.len());
            if total > MAX_STREAM_BYTES {
                return Err(Error::Ambiguous("model stream exceeded 32 MiB".into()));
            }
            for data in decoder.feed(&chunk)? {
                for event in self.decode(&data)? {
                    on_event(event.clone());
                    accumulator.push(event)?;
                }
            }
        }
        if !accumulator.saw_terminal() || decoder.pending_bytes() != 0 {
            return Err(Error::Ambiguous(
                "model stream ended without a terminal event".into(),
            ));
        }
        let (message, stop_reason, usage) = accumulator.finish()?;
        Ok(ModelResult {
            message,
            stop_reason,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use brain_protocol::{Message, StopReason};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

    fn binding() -> ModelBinding {
        ModelBinding {
            binding_id: "gateway".into(),
            model: "test/model".into(),
        }
    }

    async fn serve(app: Router) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        address
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
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1}}\n\ndata: [DONE]\n\n",
                    )
                }
            }),
        );
        let address = serve(app).await;
        let client = RemoteModelClient::new(RemoteModelConfig {
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            timeout: Duration::from_secs(2),
            dialect: Dialect::OpenAiChat,
            max_tokens_field: MaxTokensField::default(),
        })
        .unwrap();
        let mut events = Vec::new();
        let result = client
            .execute(
                &binding(),
                ModelRequest {
                    system: Some("system".into()),
                    tools: Some(vec!["read".into()]),
                    messages: vec![Message::user_text("hi")],
                    response_format: None,
                    max_output_tokens: Some(12),
                },
                &[ToolDefinition {
                    name: "read".into(),
                    description: "read a file".into(),
                    input_schema: json!({"type":"object"}),
                    output_schema: None,
                }],
                &mut |event| events.push(event),
            )
            .await
            .unwrap();
        let (headers, body) = observed_rx.await.unwrap();
        assert_eq!(headers["authorization"], "Bearer test-key");
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["messages"][0]["content"], "system");
        assert_eq!(body["max_completion_tokens"], 12);
        assert!(matches!(
            &result.message.content[0],
            brain_protocol::ContentBlock::Text { text } if text == "hello"
        ));
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, Some(2));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ModelStreamEvent::TextDelta { .. }
                        | ModelStreamEvent::MessageDone { .. }
                        | ModelStreamEvent::Usage { .. }
                ))
                .count(),
            3,
            "the journaled stream carries the delta, the terminal, and the trailing usage"
        );
    }

    #[tokio::test]
    async fn speaks_the_anthropic_dialect_when_bound_to_it() {
        let (observed_tx, observed_rx) = oneshot::channel();
        let observed_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(observed_tx)));
        let app = Router::new().route(
            "/messages",
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
                        concat!(
                            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":4}}}\n\n",
                            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
                            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
                        ),
                    )
                }
            }),
        );
        let address = serve(app).await;
        let client = RemoteModelClient::new(RemoteModelConfig {
            base_url: format!("http://{address}"),
            api_key: "sk-ant".into(),
            timeout: Duration::from_secs(2),
            dialect: Dialect::AnthropicMessages,
            max_tokens_field: MaxTokensField::default(),
        })
        .unwrap();
        let result = client
            .execute(
                &ModelBinding {
                    binding_id: "direct".into(),
                    model: "claude-test".into(),
                },
                ModelRequest {
                    system: Some("system".into()),
                    tools: None,
                    messages: vec![Message::user_text("hi")],
                    response_format: None,
                    max_output_tokens: None,
                },
                &[],
                &mut |_| {},
            )
            .await
            .unwrap();
        let (headers, body) = observed_rx.await.unwrap();
        assert_eq!(headers["x-api-key"], "sk-ant");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert!(headers.get("authorization").is_none());
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["system"][0]["text"], "system");
        assert_eq!(body["max_tokens"], 8_192);
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, Some(4));
        assert_eq!(result.usage.output_tokens, Some(1));
    }

    #[tokio::test]
    async fn a_provider_error_is_returned_without_retry() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let attempts = counted.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("content-type", "text/plain"), ("retry-after", "7")],
                        "rate limited".to_owned(),
                    )
                }
            }),
        );
        let address = serve(app).await;
        let client = RemoteModelClient::new(RemoteModelConfig {
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            timeout: Duration::from_secs(2),
            dialect: Dialect::OpenAiChat,
            max_tokens_field: MaxTokensField::default(),
        })
        .unwrap();
        let request = ModelRequest {
            system: None,
            tools: None,
            messages: vec![Message::user_text("hi")],
            response_format: None,
            max_output_tokens: None,
        };
        let error = client
            .execute(&binding(), request.clone(), &[], &mut |_| {})
            .await
            .unwrap_err();
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(matches!(
            error,
            Error::ProviderStatus {
                status: 429,
                retry_after_ms: Some(7_000),
                ..
            }
        ));

        let denied = Router::new().route(
            "/chat/completions",
            post(|| async {
                let chunks = futures_util::stream::once(async {
                    Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; MAX_ERROR_BYTES + 1]))
                })
                .chain(futures_util::stream::pending());
                (
                    StatusCode::BAD_REQUEST,
                    axum::body::Body::from_stream(chunks),
                )
            }),
        );
        let address = serve(denied).await;
        let client = RemoteModelClient::new(RemoteModelConfig {
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            timeout: Duration::from_secs(2),
            dialect: Dialect::OpenAiChat,
            max_tokens_field: MaxTokensField::default(),
        })
        .unwrap();
        let error = client
            .execute(&binding(), request, &[], &mut |_| {})
            .await
            .unwrap_err();
        assert!(
            matches!(&error, Error::ProviderStatus { status: 400, body, .. } if body.len() == MAX_ERROR_BYTES),
            "a deterministic 4xx must surface as a typed status, got {error:?}"
        );
    }

    #[tokio::test]
    async fn a_stream_without_a_terminal_event_is_ambiguous() {
        let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                (
                    [("content-type", "text/event-stream")],
                    "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
                )
            }),
        );
        let address = serve(app).await;
        let client = RemoteModelClient::new(RemoteModelConfig {
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            timeout: Duration::from_secs(2),
            dialect: Dialect::OpenAiChat,
            max_tokens_field: MaxTokensField::default(),
        })
        .unwrap();
        let error = client
            .execute(
                &binding(),
                ModelRequest {
                    system: None,
                    tools: None,
                    messages: vec![Message::user_text("hi")],
                    response_format: None,
                    max_output_tokens: None,
                },
                &[],
                &mut |_| {},
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Ambiguous(_)));
    }
}
