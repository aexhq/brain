use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::journal::ModelSelectorDoc;
use brain::message::{Message, StopReason, Usage};
use brain::provider::{ModelRegistry, ModelRequest, Provider, ProviderEvent};
use brain::{BrainError, Result};
use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter, ComponentSource,
    MODEL_WORLD, WorkerPool, WorkerRequest, component_digest, model,
};
use futures_util::stream::{BoxStream, StreamExt};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::Outbound;

static NEXT_MODEL_INSTANCE: AtomicU64 = AtomicU64::new(1);
static STAGING_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct ComponentProvider {
    dialect: Dialect,
    pool: Arc<WorkerPool>,
    router: Arc<CapabilityRouter>,
    component: ComponentSource,
    config: Value,
    http: Arc<ModelHttpCapabilities>,
}

impl std::fmt::Debug for ComponentProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComponentProvider")
            .field("dialect", &self.dialect)
            .field("component", &self.component.sha256)
            .finish()
    }
}

impl ComponentProvider {
    pub fn new(
        dialect: Dialect,
        pool: Arc<WorkerPool>,
        router: Arc<CapabilityRouter>,
        component: ComponentSource,
        config: Value,
        outbound: Outbound,
    ) -> Result<Self> {
        if !config.is_object() {
            return Err(BrainError::Invalid(
                "Model component config must be a JSON object".into(),
            ));
        }
        Ok(Self {
            dialect,
            pool,
            router,
            component,
            config,
            http: Arc::new(ModelHttpCapabilities::new(outbound)),
        })
    }
}

pub struct ComponentModelRegistry {
    store_dir: PathBuf,
    pool: Arc<WorkerPool>,
    router: Arc<CapabilityRouter>,
    outbound: Outbound,
    providers: StdMutex<HashMap<String, Arc<ComponentProvider>>>,
}

impl ComponentModelRegistry {
    pub fn new(
        store_dir: impl Into<PathBuf>,
        pool: Arc<WorkerPool>,
        router: Arc<CapabilityRouter>,
        outbound: Outbound,
    ) -> std::io::Result<Self> {
        let store_dir = store_dir.into();
        std::fs::create_dir_all(&store_dir)?;
        Ok(Self {
            store_dir,
            pool,
            router,
            outbound,
            providers: StdMutex::new(HashMap::new()),
        })
    }

    fn path(&self, digest: &str) -> PathBuf {
        self.store_dir.join(format!("{digest}.wasm"))
    }

    fn store(&self, digest: &str, bytes: &[u8]) -> Result<()> {
        let target = self.path(digest);
        if target.exists() {
            let existing = std::fs::read(&target)
                .map_err(|error| BrainError::Protocol(format!("Model store read: {error}")))?;
            if component_digest(&existing) != digest {
                return Err(BrainError::Protocol(format!(
                    "Model store entry {digest} has different bytes"
                )));
            }
            return Ok(());
        }
        let staged = target.with_extension(format!(
            "staging-{}-{}",
            std::process::id(),
            STAGING_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&staged, bytes)
            .and_then(|()| std::fs::rename(&staged, &target))
            .map_err(|error| BrainError::Protocol(format!("Model store write: {error}")))
    }
}

impl ModelRegistry for ComponentModelRegistry {
    fn resolve(&self, selector: &ModelSelectorDoc) -> Result<Arc<dyn Provider>> {
        if selector.world != MODEL_WORLD {
            return Err(BrainError::Invalid(format!(
                "Model world {:?} is not supported; expected {MODEL_WORLD:?}",
                selector.world
            )));
        }
        let config_json = serde_json::to_string(&selector.config)
            .map_err(|error| BrainError::Protocol(format!("Model config: {error}")))?;
        let cache_key = format!(
            "{}:{}",
            selector.component_digest,
            component_digest(config_json.as_bytes())
        );
        if let Some(provider) = self
            .providers
            .lock()
            .expect("Model providers")
            .get(&cache_key)
        {
            return Ok(provider.clone());
        }
        let path = self.path(&selector.component_digest);
        if !path.is_file() {
            return Err(BrainError::Invalid(format!(
                "Model component {} is absent from this Brain store",
                selector.component_digest
            )));
        }
        let provider = Arc::new(ComponentProvider::new(
            Dialect::OpenAiChat,
            self.pool.clone(),
            self.router.clone(),
            ComponentSource {
                path,
                sha256: selector.component_digest.clone(),
            },
            Value::Object(selector.config.clone()),
            self.outbound.clone(),
        )?);
        self.providers
            .lock()
            .expect("Model providers")
            .insert(cache_key, provider.clone());
        Ok(provider)
    }

    fn admit(
        &self,
        component_digest: &str,
        world: &str,
        component: &[u8],
        provider: &str,
        config: &serde_json::Map<String, Value>,
    ) -> Result<ModelSelectorDoc> {
        if world != MODEL_WORLD {
            return Err(BrainError::Invalid(format!(
                "Model world {world:?} is not supported; expected {MODEL_WORLD:?}"
            )));
        }
        self.store(component_digest, component)?;
        Ok(ModelSelectorDoc {
            component_digest: component_digest.into(),
            component_bytes: component.len() as u64,
            world: world.into(),
            provider: provider.into(),
            config: config.clone(),
        })
    }
}

pub async fn registry_with_component_store(
    store_dir: &Path,
    component_host: &Path,
    workers: usize,
    outbound: Outbound,
) -> anyhow::Result<Arc<dyn ModelRegistry>> {
    let router = CapabilityRouter::new(Arc::new(RejectCapabilities));
    let pool = WorkerPool::with_capabilities(component_host, workers, router.clone()).await?;
    Ok(Arc::new(ComponentModelRegistry::new(
        store_dir, pool, router, outbound,
    )?))
}

struct RejectCapabilities;

#[async_trait]
impl CapabilityHandler for RejectCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "capability_unbound".into(),
            message: format!(
                "no kernel capability handler is bound for {} instance {:?}",
                call.world, call.instance_id
            ),
            retryable: true,
        })
    }
}

#[async_trait]
impl Provider for ComponentProvider {
    fn dialect(&self) -> Dialect {
        self.dialect
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        let mut provider_options = self.config.clone();
        let options = provider_options
            .as_object_mut()
            .expect("component config validated at construction");
        options.insert("apiKey".into(), Value::String(key.expose().to_owned()));
        if !base_url.is_empty() {
            options.insert("baseUrl".into(), Value::String(base_url.to_owned()));
        }
        let generation = json!({
            "system_prompt": prefix.system_prompt,
            "max_tokens": prefix.sampling.max_tokens,
            "output_token_parameter": match prefix.sampling.output_token_parameter {
                brain::config::OutputTokenParameter::MaxTokens => "max_tokens",
                brain::config::OutputTokenParameter::MaxCompletionTokens => "max_completion_tokens",
            },
            "temperature": prefix.sampling.temperature,
            "reasoning_effort": prefix.sampling.reasoning_effort,
            "stop_sequences": prefix.sampling.stop_sequences,
            "tool_choice_none": prefix.tool_choice_none,
        });
        let request = ComponentModelRequest {
            model: prefix.model.clone(),
            messages_json: serde_json::to_string(history)?,
            tools_json: serde_json::to_string(&prefix.tools)?,
            generation_json: serde_json::to_string(&generation)?,
            provider_options_json: serde_json::to_string(&provider_options)?,
        };
        Ok(ModelRequest {
            method: "COMPONENT",
            url: format!("component://{}", self.component.sha256),
            headers: Vec::new(),
            body: serde_json::to_vec(&request)?,
        })
    }

    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        let request: ComponentModelRequest = serde_json::from_slice(&request.body)
            .map_err(|error| BrainError::Protocol(format!("component Model request: {error}")))?;
        let instance_id = format!(
            "model-{}",
            NEXT_MODEL_INSTANCE.fetch_add(1, Ordering::Relaxed)
        );
        let binding = self
            .router
            .bind("model", instance_id.clone(), self.http.clone())
            .map_err(|error| BrainError::Transport(error.to_string()))?;
        let started = self
            .pool
            .call(WorkerRequest::ModelStart {
                instance_id: instance_id.clone(),
                component: ComponentSource {
                    path: self.component.path.clone(),
                    sha256: self.component.sha256.clone(),
                },
                request: model::aex::model::types::Request {
                    operation_id: instance_id.clone(),
                    model: request.model,
                    messages_json: request.messages_json,
                    tools_json: request.tools_json,
                    response_format_json: None,
                    generation_json: request.generation_json,
                    provider_options_json: request.provider_options_json,
                    deadline_at_ms: u64::MAX,
                },
            })
            .await
            .map_err(|error| BrainError::Transport(error.to_string()))?;
        let provider_operation_id = started["provider_operation_id"]
            .as_str()
            .ok_or_else(|| {
                BrainError::Protocol("Model start returned no provider operation id".into())
            })?
            .to_owned();
        let pool = self.pool.clone();
        let stream = async_stream::stream! {
            let _binding = binding;
            let mut cleanup = ModelCleanup {
                pool: pool.clone(),
                instance_id: instance_id.clone(),
                provider_operation_id: provider_operation_id.clone(),
                armed: true,
            };
            let mut cursor = None;
            loop {
                let observed = match pool.call(WorkerRequest::ModelObserve {
                    instance_id: instance_id.clone(),
                    provider_operation_id: provider_operation_id.clone(),
                    cursor: cursor.clone(),
                }).await {
                    Ok(value) => value,
                    Err(error) => { yield Err(BrainError::Transport(error.to_string())); return; }
                };
                let observation: model::aex::model::types::Observation = match serde_json::from_value(observed) {
                    Ok(value) => value,
                    Err(error) => { yield Err(BrainError::Protocol(format!("Model observation: {error}"))); return; }
                };
                for event in observation.events {
                    match provider_event(event) {
                        Ok(Some(event)) => yield Ok(event),
                        Ok(None) => {}
                        Err(error) => { yield Err(error); return; }
                    }
                }
                cursor = observation.next_cursor;
                match observation.state {
                    model::aex::model::types::AttemptState::Completed => {
                        let terminal_json = observation.terminal_json.unwrap_or_else(|| "{}".into());
                        let stop = terminal_stop(&terminal_json);
                        yield Ok(ProviderEvent::MessageDone { stop_reason: stop, usage: Usage::default() });
                        if let Err(error) = pool.call(WorkerRequest::ModelAcknowledge {
                            instance_id: instance_id.clone(),
                            provider_operation_id: provider_operation_id.clone(),
                            terminal_json,
                        }).await {
                            yield Err(BrainError::Transport(error.to_string()));
                        }
                        cleanup.armed = false;
                        return;
                    }
                    model::aex::model::types::AttemptState::Failed
                    | model::aex::model::types::AttemptState::Cancelled
                    | model::aex::model::types::AttemptState::Unknown => {
                        yield Err(BrainError::Transport(format!("Model attempt ended {:?}", observation.state)));
                        return;
                    }
                    model::aex::model::types::AttemptState::Pending
                    | model::aex::model::types::AttemptState::Streaming => {}
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

struct ModelCleanup {
    pool: Arc<WorkerPool>,
    instance_id: String,
    provider_operation_id: String,
    armed: bool,
}

impl Drop for ModelCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let pool = self.pool.clone();
        let instance_id = self.instance_id.clone();
        let provider_operation_id = self.provider_operation_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = pool
                    .call(WorkerRequest::ModelCancel {
                        instance_id,
                        provider_operation_id,
                    })
                    .await;
            });
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ComponentModelRequest {
    model: String,
    messages_json: String,
    tools_json: String,
    generation_json: String,
    provider_options_json: String,
}

fn provider_event(event: model::aex::model::types::Event) -> Result<Option<ProviderEvent>> {
    let payload: Value = serde_json::from_str(&event.payload_json)
        .map_err(|error| BrainError::Protocol(format!("Model event payload: {error}")))?;
    let index = payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    Ok(Some(match event.kind {
        model::aex::model::types::EventKind::TextDelta => ProviderEvent::TextDelta {
            index,
            text: string_field(&payload, "text")?,
        },
        model::aex::model::types::EventKind::RefusalDelta => ProviderEvent::RefusalDelta {
            index,
            text: string_field(&payload, "text")?,
        },
        model::aex::model::types::EventKind::ReasoningDelta => return Ok(None),
        model::aex::model::types::EventKind::ToolUseStart => ProviderEvent::ToolUseStart {
            index,
            id: string_field(&payload, "id")?,
            name: string_field(&payload, "name")?,
        },
        model::aex::model::types::EventKind::ToolInputDelta => ProviderEvent::ToolInputDelta {
            index,
            partial_json: string_field(&payload, "partialJson")?,
        },
        model::aex::model::types::EventKind::Usage => ProviderEvent::Usage {
            usage: Usage {
                input_tokens: integer_field(&payload, "inputTokens"),
                output_tokens: integer_field(&payload, "outputTokens"),
                cache_read_input_tokens: integer_field(&payload, "cacheReadInputTokens"),
                cache_creation_input_tokens: integer_field(&payload, "cacheCreationInputTokens"),
                reasoning_tokens: integer_field(&payload, "reasoningTokens"),
            },
        },
    }))
}

fn terminal_stop(value: &str) -> StopReason {
    match serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| {
            value
                .get("stopReason")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
    {
        Some("end_turn") => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("stop_sequence") => StopReason::StopSequence,
        Some("refusal") => StopReason::Refusal,
        _ => StopReason::Unknown,
    }
}

fn string_field(value: &Value, name: &str) -> Result<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| BrainError::Protocol(format!("Model event has no {name}")))
}

fn integer_field(value: &Value, name: &str) -> Option<u64> {
    value.get(name).and_then(Value::as_u64)
}

struct HttpStream {
    status: u16,
    headers: Vec<(String, String)>,
    sent_head: bool,
    cursor: u64,
    pending: bytes::Bytes,
    stream: BoxStream<'static, std::result::Result<bytes::Bytes, reqwest::Error>>,
}

pub struct ModelHttpCapabilities {
    outbound: Outbound,
    streams: Mutex<HashMap<String, HttpStream>>,
    next_request: AtomicU64,
}

impl ModelHttpCapabilities {
    pub fn new(outbound: Outbound) -> Self {
        Self {
            outbound,
            streams: Mutex::new(HashMap::new()),
            next_request: AtomicU64::new(1),
        }
    }
}

#[async_trait]
impl CapabilityHandler for ModelHttpCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        match call.capability.as_str() {
            "model.http.start" => self.start(call).await,
            "model.http.read" => self.read(call.request).await,
            "model.http.cancel" => self.cancel(call.request).await,
            _ => Err(failure(
                "capability_denied",
                "unsupported Model capability",
                false,
            )),
        }
    }
}

impl ModelHttpCapabilities {
    async fn start(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        let request: model::aex::model::types::HttpRequest =
            serde_json::from_value(call.request)
                .map_err(|error| failure("invalid_request", &error.to_string(), false))?;
        if request.credential.is_some() {
            return Err(failure(
                "invalid_request",
                "credential handles are outside the MVP",
                false,
            ));
        }
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|error| failure("invalid_request", &error.to_string(), false))?;
        let url = self
            .outbound
            .check_url(&request.url)
            .map_err(|error| failure("network_denied", &error.to_string(), false))?;
        let mut builder = self
            .outbound
            .client()
            .request(method, url)
            .body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| failure("transport", &error.to_string(), true))?;
        let request_id = format!(
            "{}:{}",
            call.operation_id,
            self.next_request.fetch_add(1, Ordering::Relaxed)
        );
        let stream = HttpStream {
            status: response.status().as_u16(),
            headers: response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.to_string(), value.to_owned()))
                })
                .collect(),
            sent_head: false,
            cursor: 0,
            pending: bytes::Bytes::new(),
            stream: response.bytes_stream().boxed(),
        };
        self.streams.lock().await.insert(request_id.clone(), stream);
        Ok(json!({ "request_id": request_id }))
    }

    async fn read(&self, request: Value) -> std::result::Result<Value, CapabilityFailure> {
        let request_id = request
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or_else(|| failure("invalid_request", "http read has no request_id", false))?
            .to_owned();
        let max_bytes = request
            .get("max_bytes")
            .and_then(Value::as_u64)
            .filter(|value| (1..=65_536).contains(value))
            .ok_or_else(|| {
                failure(
                    "invalid_request",
                    "max_bytes must be 1 through 65536",
                    false,
                )
            })? as usize;
        let expected_cursor = request.get("cursor").and_then(Value::as_str);
        let mut stream = self
            .streams
            .lock()
            .await
            .remove(&request_id)
            .ok_or_else(|| failure("not_found", "unknown HTTP request", false))?;
        if expected_cursor.is_some_and(|cursor| cursor != stream.cursor.to_string()) {
            return Err(failure(
                "cursor_mismatch",
                "HTTP cursor does not match",
                false,
            ));
        }
        if stream.pending.is_empty() {
            match stream.stream.next().await {
                Some(Ok(bytes)) => stream.pending = bytes,
                Some(Err(error)) => return Err(failure("transport", &error.to_string(), true)),
                None => {
                    return Ok(json!({
                        "cursor": stream.cursor.to_string(),
                        "status": if stream.sent_head { Value::Null } else { json!(stream.status) },
                        "headers": if stream.sent_head { json!([]) } else { json!(stream.headers) },
                        "bytes": [],
                        "done": true,
                    }));
                }
            }
        }
        let take = max_bytes.min(stream.pending.len());
        let bytes = stream.pending.split_to(take);
        stream.cursor += 1;
        let status = if stream.sent_head {
            Value::Null
        } else {
            json!(stream.status)
        };
        let headers = if stream.sent_head {
            Vec::new()
        } else {
            stream.headers.clone()
        };
        stream.sent_head = true;
        let cursor = stream.cursor.to_string();
        self.streams.lock().await.insert(request_id, stream);
        Ok(
            json!({ "cursor": cursor, "status": status, "headers": headers, "bytes": bytes.as_ref(), "done": false }),
        )
    }

    async fn cancel(&self, request: Value) -> std::result::Result<Value, CapabilityFailure> {
        let request_id = request
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or_else(|| failure("invalid_request", "http cancel has no request_id", false))?;
        self.streams.lock().await.remove(request_id);
        Ok(Value::Null)
    }
}

fn failure(code: &str, message: &str, retryable: bool) -> CapabilityFailure {
    CapabilityFailure {
        code: code.into(),
        message: message.chars().take(4096).collect(),
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn model_events_preserve_absent_usage_and_tool_indexes() {
        let usage = provider_event(model::aex::model::types::Event {
            cursor: "1".into(),
            kind: model::aex::model::types::EventKind::Usage,
            payload_json: r#"{"inputTokens":11}"#.into(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            usage,
            ProviderEvent::Usage {
                usage: Usage {
                    input_tokens: Some(11),
                    ..Usage::default()
                }
            }
        );

        let tool = provider_event(model::aex::model::types::Event {
            cursor: "2".into(),
            kind: model::aex::model::types::EventKind::ToolUseStart,
            payload_json: r#"{"index":3,"id":"call","name":"read"}"#.into(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            tool,
            ProviderEvent::ToolUseStart {
                index: 3,
                id: "call".into(),
                name: "read".into(),
            }
        );
    }

    #[test]
    fn unknown_terminal_never_becomes_end_turn() {
        assert_eq!(
            terminal_stop(r#"{"stopReason":"new_reason"}"#),
            StopReason::Unknown
        );
        assert_eq!(
            terminal_stop(r#"{"stopReason":"end_turn"}"#),
            StopReason::EndTurn
        );
    }

    #[tokio::test]
    async fn model_http_capability_streams_bounded_chunks_with_one_response_head() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nX-Test: yes\r\n\r\ndata: one\n\nxx",
                )
                .await
                .unwrap();
        });
        let handler = ModelHttpCapabilities::new(Outbound::new(true));
        let started = handler
            .call(CapabilityCall {
                world: "model".into(),
                instance_id: Some("model-1".into()),
                capability: "model.http.start".into(),
                operation_id: "operation-1".into(),
                request: serde_json::to_value(model::aex::model::types::HttpRequest {
                    method: "POST".into(),
                    url: format!("http://{address}/v1/messages"),
                    headers: Vec::new(),
                    body: Vec::new(),
                    credential: None,
                    deadline_at_ms: u64::MAX,
                })
                .unwrap(),
            })
            .await
            .unwrap();
        let request_id = started["request_id"].as_str().unwrap();
        let mut cursor = None;
        let mut body = Vec::new();
        let mut heads = 0;
        loop {
            let chunk = handler
                .read(json!({ "request_id": request_id, "cursor": cursor, "max_bytes": 4 }))
                .await
                .unwrap();
            if !chunk["status"].is_null() {
                heads += 1;
                assert_eq!(chunk["status"], 200);
            }
            body.extend(
                chunk["bytes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|byte| byte.as_u64().unwrap() as u8),
            );
            cursor = chunk["cursor"].as_str().map(str::to_owned);
            if chunk["done"] == true {
                break;
            }
        }
        assert_eq!(heads, 1);
        assert_eq!(body, b"data: one\n\nxx");
    }
}
