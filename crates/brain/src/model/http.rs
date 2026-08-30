use std::{net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use brain_protocol::{
    Identity, ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent,
    OperationId,
};
use futures_util::StreamExt;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::{
    KernelError, ModelExecutor,
    model::{Accumulator, Dialect, MaxTokensField, anthropic, openai, sse::SseDecoder},
};

const MAX_ERROR_BYTES: usize = 16 * 1024;
const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = 256 * 1024;

// Live retry policy: clean provider failures -- a complete 408/429/5xx error
// response before anything streamed -- retry in place with bounded backoff.
// Ambiguous losses (transport errors, mid-stream death) are never retried here:
// the request may have billed, and only the journal's ambiguous path is honest
// about that. Never retried either: deterministic 4xx (auth, validation,
// context overflow) and quota exhaustion, which fail fast.
const PROVIDER_LIVE_RETRIES: u32 = 3;
/// Full-jitter exponential backoff: `rand(0..=min(cap, base << attempt))`.
const PROVIDER_RETRY_BASE_MS: u64 = 1_000;
const PROVIDER_RETRY_CAP_MS: u64 = 30_000;
/// A provider-sent `Retry-After` is honored but never beyond this.
const PROVIDER_RETRY_AFTER_CAP_MS: u64 = 60_000;

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
pub fn validate_base_url(base_url: &str) -> Result<(), KernelError> {
    if base_url.trim().is_empty() {
        return Err(KernelError::InvalidState(
            "model base URL is required".into(),
        ));
    }
    let url = reqwest::Url::parse(base_url).map_err(|error| {
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
    Ok(())
}

impl ModelTransport {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, KernelError> {
        validate_base_url(base_url)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .timeout(timeout)
            .build()
            .map_err(|error| KernelError::Executor(error.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }
}

impl RemoteModelClient {
    /// Builds a transport of its own. For a caller that makes one call, or a test.
    pub fn new(config: RemoteModelConfig) -> Result<Self, KernelError> {
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
    ) -> Result<Self, KernelError> {
        if api_key.trim().is_empty() {
            return Err(KernelError::InvalidState(
                "model API key is required".into(),
            ));
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
        presentation: &ModelPresentation,
        request: &ModelRequest,
    ) -> Result<Value, KernelError> {
        match self.dialect {
            Dialect::OpenAiChat => {
                openai::body(&binding.model, presentation, request, self.max_tokens_field)
            }
            Dialect::AnthropicMessages => anthropic::body(&binding.model, presentation, request),
        }
    }

    fn decode(&self, data: &str) -> Result<Vec<ModelStreamEvent>, KernelError> {
        match self.dialect {
            Dialect::OpenAiChat => openai::decode(data),
            Dialect::AnthropicMessages => anthropic::decode(data),
        }
    }

    fn request(
        &self,
        operation_id: &OperationId,
        request_identity: &Identity,
        body: &Value,
    ) -> reqwest::RequestBuilder {
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
            .header("accept", "text/event-stream")
            .header("x-idempotency-key", operation_id.as_str())
            .header("x-request-identity", request_identity.to_string());
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
        operation_id: &OperationId,
        request_identity: &Identity,
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        let body = self.body(binding, presentation, &request)?;
        let mut attempt = 0;
        let response = loop {
            let response = self
                .request(operation_id, request_identity, &body)
                .send()
                .await
                .map_err(|error| {
                    KernelError::Executor(format!("model request failed before response: {error}"))
                })?;
            if response.status().is_success() {
                break response;
            }
            let status = response.status().as_u16();
            let retry_after_ms = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok())
                .and_then(|seconds| seconds.checked_mul(1_000));
            let bytes = response
                .bytes()
                .await
                .map_err(|error| KernelError::Executor(error.to_string()))?;
            let bounded = &bytes[..bytes.len().min(MAX_ERROR_BYTES)];
            let error = KernelError::ProviderStatus {
                status,
                body: String::from_utf8_lossy(bounded).into_owned(),
                retry_after_ms,
            };
            match live_retry_delay(&error, attempt) {
                Some(delay) if attempt < PROVIDER_LIVE_RETRIES => {
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                _ => return Err(error),
            }
        };
        let mut decoder = SseDecoder::new(MAX_FRAME_BYTES);
        let mut stream = response.bytes_stream();
        let mut total = 0_usize;
        let mut accumulator = Accumulator::new();
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
                for event in self.decode(&data)? {
                    on_event(event.clone());
                    accumulator.push(event)?;
                }
            }
        }
        if !accumulator.saw_terminal() || decoder.pending_bytes() != 0 {
            return Err(KernelError::Ambiguous(
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

/// The pause before live-retry number `attempt` (0-based) of a clean failure, or `None`
/// when the failure class must not be retried in place.
fn live_retry_delay(error: &KernelError, attempt: u32) -> Option<Duration> {
    let KernelError::ProviderStatus {
        status,
        body,
        retry_after_ms,
    } = error
    else {
        return None;
    };
    if !matches!(status, 408 | 429) && *status < 500 {
        return None;
    }
    // OpenAI reports exhausted quota as a 429; waiting will not refill an account.
    if *status == 429 && body.contains("insufficient_quota") {
        return None;
    }
    let ms = match retry_after_ms {
        Some(requested) => (*requested).min(PROVIDER_RETRY_AFTER_CAP_MS),
        None => {
            use rand::Rng;
            let ceiling = PROVIDER_RETRY_BASE_MS
                .checked_shl(attempt.min(16))
                .unwrap_or(u64::MAX)
                .min(PROVIDER_RETRY_CAP_MS);
            rand::rng().random_range(0..=ceiling)
        }
    };
    Some(Duration::from_millis(ms))
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    fn status(status: u16, body: &str, retry_after_ms: Option<u64>) -> KernelError {
        KernelError::ProviderStatus {
            status,
            body: body.into(),
            retry_after_ms,
        }
    }

    #[test]
    fn clean_failures_retry_and_deterministic_ones_do_not() {
        assert!(live_retry_delay(&status(429, "rate limited", None), 0).is_some());
        assert!(live_retry_delay(&status(408, "timeout", None), 0).is_some());
        assert!(live_retry_delay(&status(500, "oops", None), 0).is_some());
        assert!(live_retry_delay(&status(529, "overloaded", None), 2).is_some());
        assert!(live_retry_delay(&status(400, "bad request", None), 0).is_none());
        assert!(live_retry_delay(&status(401, "bad key", None), 0).is_none());
        assert!(live_retry_delay(&status(404, "no model", None), 0).is_none());
        assert!(
            live_retry_delay(&KernelError::Ambiguous("mid-stream loss".into()), 0).is_none(),
            "ambiguous losses are never retried in place"
        );
    }

    #[test]
    fn quota_exhaustion_fails_fast() {
        let quota = status(
            429,
            r#"{"error":{"code":"insufficient_quota","message":"..."}}"#,
            Some(1_000),
        );
        assert!(live_retry_delay(&quota, 0).is_none());
    }

    #[test]
    fn retry_after_is_honored_and_capped() {
        let asked = live_retry_delay(&status(429, "slow down", Some(2_000)), 0).unwrap();
        assert_eq!(asked.as_millis(), 2_000);
        let capped = live_retry_delay(&status(429, "slow down", Some(600_000)), 0).unwrap();
        assert_eq!(capped.as_millis(), PROVIDER_RETRY_AFTER_CAP_MS as u128);
    }

    #[test]
    fn backoff_stays_inside_the_jitter_ceiling() {
        for attempt in 0..6 {
            let delay = live_retry_delay(&status(503, "unavailable", None), attempt).unwrap();
            let ceiling = PROVIDER_RETRY_BASE_MS
                .checked_shl(attempt)
                .unwrap_or(u64::MAX)
                .min(PROVIDER_RETRY_CAP_MS);
            assert!(delay.as_millis() <= ceiling as u128);
        }
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
    use brain_protocol::{Message, ModelPresentation, StopReason, ToolDefinition};
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
                &OperationId::new("op_test"),
                &Identity::of(&"request").unwrap(),
                &binding(),
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
                    messages: vec![Message::user_text("hi")],
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
        assert_eq!(
            headers["x-request-identity"],
            Identity::of(&"request").unwrap().to_string()
        );
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
                &OperationId::new("op_test"),
                &Identity::of(&"request").unwrap(),
                &ModelBinding {
                    binding_id: "direct".into(),
                    model: "claude-test".into(),
                },
                &ModelPresentation {
                    system: "system".into(),
                    tools: Vec::new(),
                    response_format: None,
                },
                ModelRequest {
                    messages: vec![Message::user_text("hi")],
                    response_format: None,
                    max_output_tokens: None,
                },
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
    async fn a_clean_429_is_retried_and_a_400_is_not() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let attempts = counted.clone();
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [
                                ("content-type", "text/plain"),
                                ("retry-after", "0"),
                            ],
                            "rate limited".to_owned(),
                        )
                    } else {
                        (
                            StatusCode::OK,
                            [
                                ("content-type", "text/event-stream"),
                                ("retry-after", "0"),
                            ],
                            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_owned(),
                        )
                    }
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
            messages: vec![Message::user_text("hi")],
            response_format: None,
            max_output_tokens: None,
        };
        let presentation = ModelPresentation {
            system: String::new(),
            tools: Vec::new(),
            response_format: None,
        };
        let result = client
            .execute(
                &OperationId::new("op_retry"),
                &Identity::of(&"request").unwrap(),
                &binding(),
                &presentation,
                request.clone(),
                &mut |_| {},
            )
            .await
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(result.stop_reason, StopReason::EndTurn);

        let denied = Router::new().route(
            "/chat/completions",
            post(|| async { (StatusCode::BAD_REQUEST, "bad request") }),
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
            .execute(
                &OperationId::new("op_denied"),
                &Identity::of(&"request").unwrap(),
                &binding(),
                &presentation,
                request,
                &mut |_| {},
            )
            .await
            .unwrap_err();
        assert!(
            matches!(error, KernelError::ProviderStatus { status: 400, .. }),
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
                &OperationId::new("op_partial"),
                &Identity::of(&"request").unwrap(),
                &binding(),
                &ModelPresentation {
                    system: String::new(),
                    tools: Vec::new(),
                    response_format: None,
                },
                ModelRequest {
                    messages: vec![Message::user_text("hi")],
                    response_format: None,
                    max_output_tokens: None,
                },
                &mut |_| {},
            )
            .await
            .unwrap_err();
        assert!(matches!(error, KernelError::Ambiguous(_)));
    }
}
