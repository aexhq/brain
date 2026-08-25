use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use brain::environment::{
    COMPONENT_ENVIRONMENT_WORLD, ComponentEnvironmentInvocation, ComponentEnvironmentRegistry,
};
use brain::{BrainError, Result};
use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter, ComponentSource,
    ENVIRONMENT_WORLD, WorkerPool, WorkerRequest, component_digest, environment,
};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;

const MAX_DISPATCH_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_DISPATCH_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

static STAGING_NONCE: AtomicU64 = AtomicU64::new(1);

pub struct WasmEnvironmentRegistry {
    store_dir: PathBuf,
    pool: Arc<WorkerPool>,
}

impl WasmEnvironmentRegistry {
    pub fn new(store_dir: impl Into<PathBuf>, pool: Arc<WorkerPool>) -> std::io::Result<Self> {
        let store_dir = store_dir.into();
        std::fs::create_dir_all(&store_dir)?;
        Ok(Self { store_dir, pool })
    }

    fn path(&self, digest: &str) -> PathBuf {
        self.store_dir.join(format!("{digest}.wasm"))
    }

    fn store(&self, digest: &str, bytes: &[u8]) -> Result<()> {
        let target = self.path(digest);
        if target.exists() {
            let existing = std::fs::read(&target).map_err(|error| {
                BrainError::Protocol(format!("Environment store read: {error}"))
            })?;
            if component_digest(&existing) != digest {
                return Err(BrainError::Protocol(format!(
                    "Environment store entry {digest} has different bytes"
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
            .map_err(|error| BrainError::Protocol(format!("Environment store write: {error}")))
    }
}

#[async_trait]
impl ComponentEnvironmentRegistry for WasmEnvironmentRegistry {
    fn admit(&self, digest: &str, world: &str, component: &[u8]) -> Result<()> {
        if world != COMPONENT_ENVIRONMENT_WORLD || world != ENVIRONMENT_WORLD {
            return Err(BrainError::Invalid(format!(
                "Environment world {world:?} is not supported; expected {ENVIRONMENT_WORLD:?}"
            )));
        }
        self.store(digest, component)
    }

    async fn invoke(
        &self,
        declaration: &brain_protocol::session::ComponentEnvironmentConfig,
        request: ComponentEnvironmentInvocation,
    ) -> Result<String> {
        if declaration.world != ENVIRONMENT_WORLD {
            return Err(BrainError::Journal(format!(
                "sealed Environment world {:?} is unsupported",
                declaration.world
            )));
        }
        let path = self.path(declaration.component_digest.as_str());
        if !path.is_file() {
            return Err(BrainError::Journal(format!(
                "Environment component {} is absent from this Brain store",
                declaration.component_digest.as_str()
            )));
        }
        let instance_id = format!(
            "{}:{}:{}",
            request.session_id,
            request.environment_id,
            declaration.component_digest.as_str()
        );
        let component = ComponentSource {
            path,
            sha256: declaration.component_digest.to_string(),
        };
        let resolved = self
            .pool
            .call(WorkerRequest::EnvironmentResolve {
                instance_id: instance_id.clone(),
                component,
                request: environment::aex::environment::types::ResolveRequest {
                    tenant_id: request.tenant_id,
                    session_id: request.session_id,
                    root_id: request.root_id,
                    parent_id: request.parent_id,
                    environment_id: request.environment_id,
                    config_json: serde_json::to_string(&declaration.config)?,
                    policy_json: serde_json::to_string(&request.policy)?,
                },
            })
            .await
            .map_err(environment_transport)?;
        let binding_json = required_string(&resolved, "binding_json")?.to_owned();
        let submitted = self
            .pool
            .call(WorkerRequest::EnvironmentSubmit {
                instance_id: instance_id.clone(),
                binding_json: binding_json.clone(),
                operation: environment::aex::environment::types::Operation {
                    operation_id: request.operation_id,
                    kind: "invoke".into(),
                    descriptor_json: request.descriptor_json,
                    bundle: request.bundle,
                    input_json: request.input_json,
                    deadline_at_ms: request.deadline_at_ms,
                },
            })
            .await
            .map_err(environment_transport)?;
        let provider_operation_id =
            required_string(&submitted, "provider_operation_id")?.to_owned();
        let mut cursor = None;
        loop {
            if brain::wall_ms() >= request.deadline_at_ms {
                let _ = self
                    .pool
                    .call(WorkerRequest::EnvironmentCancel {
                        instance_id: instance_id.clone(),
                        binding_json: binding_json.clone(),
                        provider_operation_id: provider_operation_id.clone(),
                    })
                    .await;
                return Err(BrainError::Transport(
                    "Environment component operation exceeded its deadline".into(),
                ));
            }
            let observed = self
                .pool
                .call(WorkerRequest::EnvironmentObserve {
                    instance_id: instance_id.clone(),
                    binding_json: binding_json.clone(),
                    provider_operation_id: provider_operation_id.clone(),
                    cursor: cursor.clone(),
                })
                .await
                .map_err(environment_transport)?;
            let observation: environment::aex::environment::types::Observation =
                serde_json::from_value(observed).map_err(|error| {
                    BrainError::Protocol(format!("Environment observation: {error}"))
                })?;
            cursor = Some(observation.cursor);
            match observation.state {
                environment::aex::environment::types::OperationState::Pending
                | environment::aex::environment::types::OperationState::Running => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                environment::aex::environment::types::OperationState::Unknown => {
                    return Err(BrainError::Environment(
                        "Environment component reported an unknown operation outcome".into(),
                    ));
                }
                environment::aex::environment::types::OperationState::Completed
                | environment::aex::environment::types::OperationState::Failed
                | environment::aex::environment::types::OperationState::Cancelled => {
                    let terminal = observation.terminal_json.ok_or_else(|| {
                        BrainError::Protocol(
                            "terminal Environment observation omitted terminal_json".into(),
                        )
                    })?;
                    self.pool
                        .call(WorkerRequest::EnvironmentAcknowledge {
                            instance_id,
                            binding_json,
                            provider_operation_id,
                            terminal_json: terminal.clone(),
                        })
                        .await
                        .map_err(environment_transport)?;
                    return Ok(terminal);
                }
            }
        }
    }

    async fn release(
        &self,
        declaration: &brain_protocol::session::ComponentEnvironmentConfig,
        request: brain::environment::ComponentEnvironmentRelease,
    ) -> Result<()> {
        let path = self.path(declaration.component_digest.as_str());
        if !path.is_file() {
            return Err(BrainError::Journal(format!(
                "Environment component {} is absent from this Brain store",
                declaration.component_digest.as_str()
            )));
        }
        let instance_id = format!(
            "{}:{}:{}",
            request.session_id,
            request.environment_id,
            declaration.component_digest.as_str()
        );
        let component = ComponentSource {
            path,
            sha256: declaration.component_digest.to_string(),
        };
        let resolved = self
            .pool
            .call(WorkerRequest::EnvironmentResolve {
                instance_id: instance_id.clone(),
                component,
                request: environment::aex::environment::types::ResolveRequest {
                    tenant_id: request.tenant_id,
                    session_id: request.session_id,
                    root_id: request.root_id,
                    parent_id: request.parent_id,
                    environment_id: request.environment_id,
                    config_json: serde_json::to_string(&declaration.config)?,
                    policy_json: serde_json::to_string(&request.policy)?,
                },
            })
            .await
            .map_err(environment_transport)?;
        let binding_json = required_string(&resolved, "binding_json")?.to_owned();
        self.pool
            .call(WorkerRequest::EnvironmentRelease {
                instance_id,
                binding_json,
            })
            .await
            .map_err(environment_transport)?;
        Ok(())
    }
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BrainError::Protocol(format!("Environment response omitted {field}")))
}

fn environment_transport(error: anyhow::Error) -> BrainError {
    BrainError::Transport(format!("Environment component: {error}"))
}

pub async fn registry_with_component_store(
    store_dir: &Path,
    component_host: &Path,
    workers: usize,
    capabilities: Arc<dyn CapabilityHandler>,
) -> anyhow::Result<Arc<dyn ComponentEnvironmentRegistry>> {
    let router = CapabilityRouter::new(capabilities);
    let pool = WorkerPool::with_capabilities(component_host, workers, router).await?;
    Ok(Arc::new(WasmEnvironmentRegistry::new(store_dir, pool)?))
}

pub struct RejectEnvironmentCapabilities;

#[async_trait]
impl CapabilityHandler for RejectEnvironmentCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "capability_unbound".into(),
            message: format!(
                "Environment host capability {} is not configured",
                call.capability
            ),
            retryable: false,
        })
    }
}

#[derive(Clone)]
pub struct HttpEnvironmentCapabilities {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    bearer: Option<String>,
    timeout: Duration,
}

#[derive(Serialize)]
struct DispatchRequest<'a> {
    operation_id: &'a str,
    action: &'a str,
    request: &'a Value,
    deadline_at_ms: &'a str,
}

impl HttpEnvironmentCapabilities {
    pub fn new(
        endpoint: impl Into<String>,
        bearer: Option<String>,
        timeout: Duration,
    ) -> Result<Self> {
        let endpoint = endpoint.into().parse::<reqwest::Url>().map_err(|error| {
            BrainError::Invalid(format!("Environment dispatch URL is invalid: {error}"))
        })?;
        if endpoint.scheme() != "http"
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(BrainError::Invalid(
                "Environment dispatch must be an http:// loopback URL without credentials, query, or fragment"
                    .into(),
            ));
        }
        let loopback = endpoint
            .host_str()
            .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !loopback {
            return Err(BrainError::Invalid(
                "Environment dispatch host must be a literal loopback address".into(),
            ));
        }
        if timeout.is_zero() {
            return Err(BrainError::Invalid(
                "Environment dispatch timeout must be positive".into(),
            ));
        }
        if bearer.as_ref().is_some_and(|token| {
            reqwest::header::HeaderValue::try_from(format!("Bearer {token}")).is_err()
        }) {
            return Err(BrainError::Invalid(
                "Environment dispatch token is not a valid HTTP bearer value".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .pool_max_idle_per_host(16)
            .build()
            .map_err(|error| {
                BrainError::Invalid(format!("Environment dispatch client: {error}"))
            })?;
        Ok(Self {
            client,
            endpoint,
            bearer,
            timeout,
        })
    }
}

impl std::fmt::Debug for HttpEnvironmentCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpEnvironmentCapabilities")
            .field("endpoint", &self.endpoint)
            .field("bearer", &self.bearer.as_ref().map(|_| "<redacted>"))
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[async_trait]
impl CapabilityHandler for HttpEnvironmentCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        if call.capability != "environment.dispatch" || call.world != ENVIRONMENT_WORLD {
            return Err(CapabilityFailure {
                code: "capability_unbound".into(),
                message: format!(
                    "Environment host capability {} is not configured",
                    call.capability
                ),
                retryable: false,
            });
        }
        let action = call
            .request
            .get("action")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 64)
            .ok_or_else(|| {
                capability_failure(
                    "invalid_request",
                    "Environment dispatch action is invalid",
                    false,
                )
            })?;
        let request = call.request.get("request").ok_or_else(|| {
            capability_failure(
                "invalid_request",
                "Environment dispatch request is missing",
                false,
            )
        })?;
        let deadline_at_ms = call
            .request
            .get("deadline_at_ms")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                capability_failure(
                    "invalid_request",
                    "Environment dispatch deadline is invalid",
                    false,
                )
            })?;
        let remaining_ms = deadline_at_ms.saturating_sub(brain::wall_ms());
        if remaining_ms == 0 {
            return Err(capability_failure(
                "deadline_exceeded",
                "Environment dispatch deadline elapsed",
                true,
            ));
        }
        let body = serde_json::to_vec(&DispatchRequest {
            operation_id: &call.operation_id,
            action,
            request,
            deadline_at_ms: call
                .request
                .get("deadline_at_ms")
                .and_then(Value::as_str)
                .expect("validated deadline"),
        })
        .map_err(|error| {
            capability_failure(
                "invalid_request",
                &format!("Environment dispatch request: {error}"),
                false,
            )
        })?;
        if body.len() > MAX_DISPATCH_REQUEST_BYTES {
            return Err(capability_failure(
                "request_too_large",
                "Environment dispatch request exceeds 8388608 bytes",
                false,
            ));
        }
        let mut request_builder = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(bearer) = &self.bearer {
            request_builder = request_builder.bearer_auth(bearer);
        }
        let timeout = self.timeout.min(Duration::from_millis(remaining_ms));
        let response = tokio::time::timeout(timeout, request_builder.send())
            .await
            .map_err(|_| {
                capability_failure("deadline_exceeded", "Environment dispatch timed out", true)
            })?
            .map_err(|_| {
                capability_failure(
                    "dispatch_unavailable",
                    "Environment dispatch is unavailable",
                    true,
                )
            })?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|bytes| bytes > MAX_DISPATCH_RESPONSE_BYTES as u64)
        {
            return Err(capability_failure(
                "response_too_large",
                "Environment dispatch response exceeds 2097152 bytes",
                false,
            ));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                capability_failure(
                    "dispatch_unavailable",
                    "Environment dispatch response is unavailable",
                    true,
                )
            })?;
            if body.len().saturating_add(chunk.len()) > MAX_DISPATCH_RESPONSE_BYTES {
                return Err(capability_failure(
                    "response_too_large",
                    "Environment dispatch response exceeds 2097152 bytes",
                    false,
                ));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(capability_failure(
                "dispatch_failed",
                &format!("Environment dispatch returned {status}"),
                status.is_server_error(),
            ));
        }
        serde_json::from_slice(&body).map_err(|_| {
            capability_failure(
                "invalid_response",
                "Environment dispatch returned invalid JSON",
                false,
            )
        })
    }
}

fn capability_failure(code: &str, message: &str, retryable: bool) -> CapabilityFailure {
    CapabilityFailure {
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn dispatch_endpoint_is_literal_loopback_http() {
        for endpoint in [
            "https://127.0.0.1:8080/environment",
            "http://localhost:8080/environment",
            "http://10.0.0.1:8080/environment",
            "http://user:secret@127.0.0.1:8080/environment",
        ] {
            assert!(
                HttpEnvironmentCapabilities::new(endpoint, None, Duration::from_secs(1)).is_err()
            );
        }
        assert!(
            HttpEnvironmentCapabilities::new(
                "http://[::1]:8080/environment",
                None,
                Duration::from_secs(1)
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn dispatch_forwards_the_generic_operation_and_redacts_failures() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            loop {
                let mut chunk = [0_u8; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                received.extend_from_slice(&chunk[..count]);
                if count == 0 || received.windows(4).any(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&received);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap();
                    let header_end = received
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .unwrap()
                        + 4;
                    while received.len() < header_end + length {
                        let count = socket.read(&mut chunk).await.unwrap();
                        received.extend_from_slice(&chunk[..count]);
                    }
                    let body: Value =
                        serde_json::from_slice(&received[header_end..header_end + length]).unwrap();
                    assert_eq!(body["action"], "submit");
                    assert_eq!(body["operation_id"], "op-1");
                    assert_eq!(body["request"]["value"], 42);
                    let response = br#"{"accepted":true}"#;
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n", response.len()).as_bytes()).await.unwrap();
                    socket.write_all(response).await.unwrap();
                    return;
                }
            }
        });
        let handler = HttpEnvironmentCapabilities::new(
            format!("http://{address}/environment"),
            Some("secret".into()),
            Duration::from_secs(2),
        )
        .unwrap();
        let response = handler
            .call(CapabilityCall {
                world: ENVIRONMENT_WORLD.into(),
                instance_id: Some("instance".into()),
                capability: "environment.dispatch".into(),
                operation_id: "op-1".into(),
                request: serde_json::json!({
                    "action": "submit",
                    "request": {"value": 42},
                    "deadline_at_ms": (brain::wall_ms() + 2_000).to_string(),
                }),
            })
            .await
            .unwrap();
        assert_eq!(response, serde_json::json!({"accepted": true}));
        server.await.unwrap();
    }
}
