use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::Instrument as _;

use crate::{
    AgentloopInstance, CapabilityCall, CapabilityFailure, CapabilityHandler, ComponentRuntime,
    DenyCapabilities, EnvironmentInstance, ModelInstance, agentloop, component_digest, environment,
    model, tool,
};

const MAX_COMPONENT_BYTES: u64 = 32 << 20;
const MAX_FRAME_BYTES: usize = 16 << 20;
/// How long the parent waits for one worker to make progress on a request: to take a frame,
/// and to answer with its next one. It is a liveness check on the worker process, the only
/// thing the parent cannot otherwise observe. Work the parent performs on the request's behalf
/// (a model round, a Tool dispatch, a child session's turn) happens between frames and is
/// bounded by the kernel that owns it, so it must not be measured here.
const WORKER_FRAME_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Serialize, Deserialize)]
pub struct ComponentSource {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerRequest {
    Agentloop {
        instance_id: String,
        component: ComponentSource,
        request: agentloop::aex::agentloop::types::Activation,
    },
    Tool {
        component: ComponentSource,
        request: tool::aex::tool::types::Invocation,
        grants: Vec<String>,
    },
    Environment {
        component: ComponentSource,
        resolve: environment::aex::environment::types::ResolveRequest,
        operation: environment::aex::environment::types::Operation,
    },
    EnvironmentResolve {
        instance_id: String,
        component: ComponentSource,
        request: environment::aex::environment::types::ResolveRequest,
    },
    EnvironmentSubmit {
        instance_id: String,
        binding_json: String,
        operation: environment::aex::environment::types::Operation,
    },
    EnvironmentObserve {
        instance_id: String,
        binding_json: String,
        provider_operation_id: String,
        cursor: Option<String>,
    },
    EnvironmentCancel {
        instance_id: String,
        binding_json: String,
        provider_operation_id: String,
    },
    EnvironmentAcknowledge {
        instance_id: String,
        binding_json: String,
        provider_operation_id: String,
        terminal_json: String,
    },
    EnvironmentRelease {
        instance_id: String,
        binding_json: String,
    },
    Model {
        component: ComponentSource,
        request: model::aex::model::types::Request,
    },
    ModelStart {
        instance_id: String,
        component: ComponentSource,
        request: model::aex::model::types::Request,
    },
    ModelObserve {
        instance_id: String,
        provider_operation_id: String,
        cursor: Option<String>,
    },
    ModelCancel {
        instance_id: String,
        provider_operation_id: String,
    },
    ModelAcknowledge {
        instance_id: String,
        provider_operation_id: String,
        terminal_json: String,
    },
    Release {
        world: String,
        instance_id: String,
    },
}

impl WorkerRequest {
    fn affinity(&self) -> Option<&str> {
        match self {
            Self::Agentloop { instance_id, .. }
            | Self::EnvironmentResolve { instance_id, .. }
            | Self::EnvironmentSubmit { instance_id, .. }
            | Self::EnvironmentObserve { instance_id, .. }
            | Self::EnvironmentCancel { instance_id, .. }
            | Self::EnvironmentAcknowledge { instance_id, .. }
            | Self::EnvironmentRelease { instance_id, .. }
            | Self::ModelStart { instance_id, .. }
            | Self::ModelObserve { instance_id, .. }
            | Self::ModelCancel { instance_id, .. }
            | Self::ModelAcknowledge { instance_id, .. }
            | Self::Release { instance_id, .. } => Some(instance_id),
            Self::Tool { .. } | Self::Environment { .. } | Self::Model { .. } => None,
        }
    }

    fn trace_fields(&self) -> (&str, &str, Option<&str>, Option<&str>, Option<&str>) {
        match self {
            Self::Agentloop {
                instance_id,
                component,
                ..
            } => (
                "invoke",
                "agentloop/v1",
                Some(&component.sha256),
                Some(instance_id),
                None,
            ),
            Self::Tool { component, .. } => {
                ("invoke", "tool/v1", Some(&component.sha256), None, None)
            }
            Self::Environment { component, .. } => (
                "invoke",
                "environment/v1",
                Some(&component.sha256),
                None,
                None,
            ),
            Self::EnvironmentResolve {
                instance_id,
                component,
                ..
            } => (
                "resolve",
                "environment/v1",
                Some(&component.sha256),
                Some(instance_id),
                None,
            ),
            Self::EnvironmentSubmit { instance_id, .. } => {
                ("submit", "environment/v1", None, Some(instance_id), None)
            }
            Self::EnvironmentObserve {
                instance_id,
                provider_operation_id,
                ..
            } => (
                "observe",
                "environment/v1",
                None,
                Some(instance_id),
                Some(provider_operation_id),
            ),
            Self::EnvironmentCancel {
                instance_id,
                provider_operation_id,
                ..
            } => (
                "cancel",
                "environment/v1",
                None,
                Some(instance_id),
                Some(provider_operation_id),
            ),
            Self::EnvironmentAcknowledge {
                instance_id,
                provider_operation_id,
                ..
            } => (
                "acknowledge",
                "environment/v1",
                None,
                Some(instance_id),
                Some(provider_operation_id),
            ),
            Self::EnvironmentRelease { instance_id, .. } => {
                ("release", "environment/v1", None, Some(instance_id), None)
            }
            Self::Model { component, .. } => {
                ("invoke", "model/v1", Some(&component.sha256), None, None)
            }
            Self::ModelStart {
                instance_id,
                component,
                ..
            } => (
                "start",
                "model/v1",
                Some(&component.sha256),
                Some(instance_id),
                None,
            ),
            Self::ModelObserve {
                instance_id,
                provider_operation_id,
                ..
            } => (
                "observe",
                "model/v1",
                None,
                Some(instance_id),
                Some(provider_operation_id),
            ),
            Self::ModelCancel {
                instance_id,
                provider_operation_id,
            } => (
                "cancel",
                "model/v1",
                None,
                Some(instance_id),
                Some(provider_operation_id),
            ),
            Self::ModelAcknowledge {
                instance_id,
                provider_operation_id,
                ..
            } => (
                "acknowledge",
                "model/v1",
                None,
                Some(instance_id),
                Some(provider_operation_id),
            ),
            Self::Release { world, instance_id } => {
                ("release", world, None, Some(instance_id), None)
            }
        }
    }
}

fn invocation_span(request: &WorkerRequest) -> tracing::Span {
    let (kind, world, digest, instance_id, operation_id) = request.trace_fields();
    tracing::info_span!(
        "brain.component.invoke",
        component.kind = kind,
        component.world = world,
        component.digest = digest.unwrap_or(""),
        component.instance_id = instance_id.unwrap_or(""),
        component.operation_id = operation_id.unwrap_or("")
    )
}

fn worker_env_allowed(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        matches!(
            name,
            "BRAIN_COMPONENT_CACHE_DIR"
                | "BRAIN_COMPONENT_INSTANCE_IDLE_MS"
                | "BRAIN_COMPONENT_INSTANCE_CAP"
                | "RUST_LOG"
        ) || name.starts_with("OTEL_")
    })
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
enum ParentFrame {
    Request {
        id: u64,
        request: Box<WorkerRequest>,
        #[serde(default)]
        trace_context: HashMap<String, String>,
    },
    CapabilityResult {
        id: u64,
        result: Result<Value, CapabilityFailure>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
enum WorkerFrame {
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    Capability {
        id: u64,
        call: CapabilityCall,
    },
}

struct ResidentAgentloop {
    digest: String,
    instance: AgentloopInstance,
    last_used: std::time::Instant,
}

struct ResidentEnvironment {
    digest: String,
    instance: EnvironmentInstance,
    last_used: std::time::Instant,
}

struct ResidentModel {
    digest: String,
    instance: ModelInstance,
    last_used: std::time::Instant,
}

trait ResidentEntry {
    fn last_used(&self) -> std::time::Instant;
}

impl ResidentEntry for ResidentAgentloop {
    fn last_used(&self) -> std::time::Instant {
        self.last_used
    }
}

impl ResidentEntry for ResidentEnvironment {
    fn last_used(&self) -> std::time::Instant {
        self.last_used
    }
}

impl ResidentEntry for ResidentModel {
    fn last_used(&self) -> std::time::Instant {
        self.last_used
    }
}

#[derive(Clone, Copy)]
struct ResidentPolicy {
    idle: Duration,
    cap: usize,
}

impl ResidentPolicy {
    fn from_env() -> anyhow::Result<Self> {
        let idle_ms = strict_worker_option(
            "BRAIN_COMPONENT_INSTANCE_IDLE_MS",
            300_000,
            1_000,
            3_600_000,
        )?;
        let cap = strict_worker_option("BRAIN_COMPONENT_INSTANCE_CAP", 256, 1, 4_096)?;
        Ok(Self {
            idle: Duration::from_millis(idle_ms as u64),
            cap,
        })
    }
}

fn strict_worker_option(
    name: &str,
    default: usize,
    min: usize,
    max: usize,
) -> anyhow::Result<usize> {
    let Some(raw) = std::env::var_os(name) else {
        return Ok(default);
    };
    let raw = raw
        .into_string()
        .map_err(|_| anyhow::anyhow!("{name} is not UTF-8"))?;
    let value = raw
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("{name} must be an integer"))?;
    if !(min..=max).contains(&value) {
        anyhow::bail!("{name} must be between {min} and {max}");
    }
    Ok(value)
}

fn sweep_residents<T: ResidentEntry>(map: &mut HashMap<String, T>, policy: ResidentPolicy) {
    let now = std::time::Instant::now();
    map.retain(|_, resident| now.duration_since(resident.last_used()) < policy.idle);
    while map.len() > policy.cap {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, resident)| resident.last_used())
            .map(|(id, _)| id.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
}

fn environment_instance<'a>(
    environments: &'a mut HashMap<String, ResidentEnvironment>,
    instance_id: &str,
) -> anyhow::Result<&'a mut EnvironmentInstance> {
    let resident = environments
        .get_mut(instance_id)
        .ok_or_else(|| anyhow::anyhow!("Environment instance is not resident"))?;
    resident.last_used = std::time::Instant::now();
    Ok(&mut resident.instance)
}

fn model_instance<'a>(
    models: &'a mut HashMap<String, ResidentModel>,
    instance_id: &str,
) -> anyhow::Result<&'a mut ModelInstance> {
    let resident = models
        .get_mut(instance_id)
        .ok_or_else(|| anyhow::anyhow!("Model instance is not resident"))?;
    resident.last_used = std::time::Instant::now();
    Ok(&mut resident.instance)
}

async fn execute(
    runtime: &ComponentRuntime,
    agentloops: &mut HashMap<String, ResidentAgentloop>,
    environments: &mut HashMap<String, ResidentEnvironment>,
    models: &mut HashMap<String, ResidentModel>,
    policy: ResidentPolicy,
    request: WorkerRequest,
) -> anyhow::Result<Value> {
    sweep_residents(agentloops, policy);
    sweep_residents(environments, policy);
    sweep_residents(models, policy);
    match request {
        WorkerRequest::Agentloop {
            instance_id,
            component,
            request,
        } => {
            let bytes = read_component(&component)?;
            if let Some(resident) = agentloops.get_mut(&instance_id) {
                resident.last_used = std::time::Instant::now();
                if resident.digest != component.sha256 {
                    anyhow::bail!("Agentloop instance is sealed to a different component digest");
                }
                return Ok(serde_json::to_value(
                    resident.instance.activate(&request).await?,
                )?);
            }
            let mut instance = runtime
                .instantiate_agentloop_scoped(&bytes, Some(instance_id.clone()))
                .await?;
            let result = instance.activate(&request).await?;
            agentloops.insert(
                instance_id,
                ResidentAgentloop {
                    digest: component.sha256,
                    instance,
                    last_used: std::time::Instant::now(),
                },
            );
            sweep_residents(agentloops, policy);
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::Tool {
            component,
            request,
            grants,
        } => {
            let bytes = read_component(&component)?;
            Ok(serde_json::to_value(
                runtime
                    .invoke_tool_granted(&bytes, request, &grants)
                    .await?,
            )?)
        }
        WorkerRequest::Environment {
            component,
            resolve,
            operation,
        } => {
            let bytes = read_component(&component)?;
            Ok(serde_json::to_value(
                runtime
                    .exercise_environment(&bytes, resolve, operation)
                    .await?,
            )?)
        }
        WorkerRequest::EnvironmentResolve {
            instance_id,
            component,
            request,
        } => {
            let bytes = read_component(&component)?;
            if let Some(resident) = environments.get_mut(&instance_id) {
                resident.last_used = std::time::Instant::now();
                if resident.digest != component.sha256 {
                    anyhow::bail!("Environment instance is sealed to a different component digest");
                }
                return Ok(serde_json::to_value(
                    resident.instance.resolve(&request).await?,
                )?);
            }
            let mut instance = runtime
                .instantiate_environment_scoped(&bytes, Some(instance_id.clone()))
                .await?;
            let result = instance.resolve(&request).await?;
            environments.insert(
                instance_id,
                ResidentEnvironment {
                    digest: component.sha256,
                    instance,
                    last_used: std::time::Instant::now(),
                },
            );
            sweep_residents(environments, policy);
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::EnvironmentSubmit {
            instance_id,
            binding_json,
            operation,
        } => Ok(serde_json::to_value(
            environment_instance(environments, &instance_id)?
                .submit(&binding_json, &operation)
                .await?,
        )?),
        WorkerRequest::EnvironmentObserve {
            instance_id,
            binding_json,
            provider_operation_id,
            cursor,
        } => Ok(serde_json::to_value(
            environment_instance(environments, &instance_id)?
                .observe(&binding_json, &provider_operation_id, cursor.as_deref())
                .await?,
        )?),
        WorkerRequest::EnvironmentCancel {
            instance_id,
            binding_json,
            provider_operation_id,
        } => {
            environment_instance(environments, &instance_id)?
                .cancel(&binding_json, &provider_operation_id)
                .await?;
            Ok(Value::Null)
        }
        WorkerRequest::EnvironmentAcknowledge {
            instance_id,
            binding_json,
            provider_operation_id,
            terminal_json,
        } => {
            environment_instance(environments, &instance_id)?
                .acknowledge(&binding_json, &provider_operation_id, &terminal_json)
                .await?;
            Ok(Value::Null)
        }
        WorkerRequest::EnvironmentRelease {
            instance_id,
            binding_json,
        } => {
            let mut resident = environments
                .remove(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Environment instance is not resident"))?;
            resident.instance.release(&binding_json).await?;
            Ok(Value::Null)
        }
        WorkerRequest::Model { component, request } => {
            let bytes = read_component(&component)?;
            Ok(serde_json::to_value(
                runtime.exercise_model(&bytes, request).await?,
            )?)
        }
        WorkerRequest::ModelStart {
            instance_id,
            component,
            request,
        } => {
            let bytes = read_component(&component)?;
            if let Some(resident) = models.get_mut(&instance_id) {
                resident.last_used = std::time::Instant::now();
                if resident.digest != component.sha256 {
                    anyhow::bail!("Model instance is sealed to a different component digest");
                }
                return Ok(serde_json::to_value(
                    resident.instance.start(&request).await?,
                )?);
            }
            let mut instance = runtime
                .instantiate_model_scoped(&bytes, Some(instance_id.clone()))
                .await?;
            let result = instance.start(&request).await?;
            models.insert(
                instance_id,
                ResidentModel {
                    digest: component.sha256,
                    instance,
                    last_used: std::time::Instant::now(),
                },
            );
            sweep_residents(models, policy);
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::ModelObserve {
            instance_id,
            provider_operation_id,
            cursor,
        } => Ok(serde_json::to_value(
            model_instance(models, &instance_id)?
                .observe(&provider_operation_id, cursor.as_deref())
                .await?,
        )?),
        WorkerRequest::ModelCancel {
            instance_id,
            provider_operation_id,
        } => {
            model_instance(models, &instance_id)?
                .cancel(&provider_operation_id)
                .await?;
            Ok(Value::Null)
        }
        WorkerRequest::ModelAcknowledge {
            instance_id,
            provider_operation_id,
            terminal_json,
        } => {
            let mut resident = models
                .remove(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Model instance is not resident"))?;
            resident
                .instance
                .acknowledge(&provider_operation_id, &terminal_json)
                .await?;
            Ok(Value::Null)
        }
        WorkerRequest::Release { world, instance_id } => {
            match world.as_str() {
                "agentloop" => {
                    agentloops.remove(&instance_id);
                }
                "environment" => {
                    environments.remove(&instance_id);
                }
                "model" => {
                    models.remove(&instance_id);
                }
                _ => anyhow::bail!("unknown resident component world {world}"),
            }
            Ok(Value::Null)
        }
    }
}

fn read_component(source: &ComponentSource) -> anyhow::Result<Vec<u8>> {
    if source.sha256.len() != 64
        || !source
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("component digest is not lowercase SHA-256");
    }
    let metadata = std::fs::metadata(&source.path)?;
    if metadata.len() == 0 || metadata.len() > MAX_COMPONENT_BYTES {
        anyhow::bail!("component exceeds the worker byte bound");
    }
    let bytes = std::fs::read(&source.path)?;
    if component_digest(&bytes) != source.sha256 {
        anyhow::bail!("component bytes do not match the sealed digest");
    }
    Ok(bytes)
}

pub async fn run_worker() -> anyhow::Result<()> {
    let input = Arc::new(Mutex::new(BufReader::new(tokio::io::stdin())));
    let output = Arc::new(Mutex::new(tokio::io::stdout()));
    let broker = Arc::new(WorkerCapabilityBroker {
        input: input.clone(),
        output: output.clone(),
        next_id: AtomicU64::new(1),
    });
    let runtime = ComponentRuntime::with_capabilities(broker)?;
    let mut agentloops = HashMap::new();
    let mut environments = HashMap::new();
    let mut models = HashMap::new();
    let policy = ResidentPolicy::from_env()?;
    loop {
        let mut line = String::new();
        let bytes = input.lock().await.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(());
        }
        if bytes > MAX_FRAME_BYTES {
            anyhow::bail!("worker request exceeds the frame bound");
        }
        let (id, request, trace_context) = match serde_json::from_str(&line)? {
            ParentFrame::Request {
                id,
                request,
                trace_context,
            } => (id, request, trace_context),
            ParentFrame::CapabilityResult { .. } => {
                anyhow::bail!("worker received a capability result outside an invocation")
            }
        };
        let span = invocation_span(&request);
        brain_observability::set_parent_from_trace(&span, &trace_context);
        let response = WorkerFrame::Response {
            id,
            result: execute(
                &runtime,
                &mut agentloops,
                &mut environments,
                &mut models,
                policy,
                *request,
            )
            .instrument(span)
            .await
            .map_err(|error| error.to_string()),
        };
        let mut encoded = serde_json::to_vec(&response)?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            encoded = serde_json::to_vec(&WorkerFrame::Response {
                id,
                result: Err("worker response exceeds the frame bound".into()),
            })?;
        }
        encoded.push(b'\n');
        let mut output = output.lock().await;
        output.write_all(&encoded).await?;
        output.flush().await?;
    }
}

struct WorkerCapabilityBroker {
    input: Arc<Mutex<BufReader<tokio::io::Stdin>>>,
    output: Arc<Mutex<tokio::io::Stdout>>,
    next_id: AtomicU64,
}

#[async_trait::async_trait]
impl CapabilityHandler for WorkerCapabilityBroker {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut encoded =
            serde_json::to_vec(&WorkerFrame::Capability { id, call }).map_err(|error| {
                CapabilityFailure {
                    code: "worker_protocol".into(),
                    message: error.to_string(),
                    retryable: true,
                }
            })?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            return Err(CapabilityFailure {
                code: "worker_protocol".into(),
                message: "capability request exceeds the frame bound".into(),
                retryable: false,
            });
        }
        encoded.push(b'\n');
        {
            let mut output = self.output.lock().await;
            output.write_all(&encoded).await.map_err(worker_failure)?;
            output.flush().await.map_err(worker_failure)?;
        }
        let mut line = String::new();
        let bytes = self
            .input
            .lock()
            .await
            .read_line(&mut line)
            .await
            .map_err(worker_failure)?;
        if bytes == 0 || bytes > MAX_FRAME_BYTES {
            return Err(CapabilityFailure {
                code: "worker_protocol".into(),
                message: "capability response is absent or exceeds the frame bound".into(),
                retryable: true,
            });
        }
        match serde_json::from_str(&line).map_err(worker_failure)? {
            ParentFrame::CapabilityResult {
                id: response_id,
                result,
            } if response_id == id => result,
            _ => Err(CapabilityFailure {
                code: "worker_protocol".into(),
                message: format!("capability response id does not match request {id}"),
                retryable: true,
            }),
        }
    }
}

fn worker_failure(error: impl std::fmt::Display) -> CapabilityFailure {
    CapabilityFailure {
        code: "worker_protocol".into(),
        message: error.to_string(),
        retryable: true,
    }
}

pub struct WorkerPool {
    program: PathBuf,
    workers: Vec<Mutex<Worker>>,
    next_worker: AtomicUsize,
    next_request: AtomicU64,
    capabilities: Arc<dyn CapabilityHandler>,
    frame_timeout: Duration,
}

impl WorkerPool {
    pub async fn new(program: impl AsRef<Path>, size: usize) -> anyhow::Result<Arc<Self>> {
        Self::with_capabilities(program, size, Arc::new(DenyCapabilities)).await
    }

    pub async fn with_capabilities(
        program: impl AsRef<Path>,
        size: usize,
        capabilities: Arc<dyn CapabilityHandler>,
    ) -> anyhow::Result<Arc<Self>> {
        Self::with_frame_timeout(program, size, capabilities, WORKER_FRAME_TIMEOUT).await
    }

    /// The pool with an explicit worker-liveness bound; tests drive a short one.
    pub async fn with_frame_timeout(
        program: impl AsRef<Path>,
        size: usize,
        capabilities: Arc<dyn CapabilityHandler>,
        frame_timeout: Duration,
    ) -> anyhow::Result<Arc<Self>> {
        if !(1..=64).contains(&size) {
            anyhow::bail!("component worker pool size must be between 1 and 64");
        }
        let program = program.as_ref().to_path_buf();
        let mut workers = Vec::with_capacity(size);
        for _ in 0..size {
            workers.push(Mutex::new(Worker::spawn(&program)?));
        }
        Ok(Arc::new(Self {
            program,
            workers,
            next_worker: AtomicUsize::new(0),
            next_request: AtomicU64::new(1),
            capabilities,
            frame_timeout,
        }))
    }

    pub async fn call(&self, request: WorkerRequest) -> anyhow::Result<Value> {
        let index = request.affinity().map_or_else(
            || self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len(),
            |affinity| {
                let digest = component_digest(affinity.as_bytes());
                usize::from_str_radix(&digest[..8], 16).expect("SHA-256 prefix is hexadecimal")
                    % self.workers.len()
            },
        );
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let mut worker = self.workers[index].lock().await;
        match worker
            .call(id, request, self.capabilities.as_ref(), self.frame_timeout)
            .await
        {
            Ok(result) => result.map_err(anyhow::Error::msg),
            Err(error) => {
                // A worker that stopped speaking the frame protocol is replaced; its resident
                // instances rehydrate on the next activation.
                *worker = Worker::spawn(&self.program)?;
                Err(error)
            }
        }
    }
}

struct Worker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Worker {
    fn spawn(program: &Path) -> anyhow::Result<Self> {
        let mut command = Command::new(program);
        command.env_clear();
        for (name, value) in std::env::vars_os() {
            if worker_env_allowed(&name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("component worker stdin is unavailable"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("component worker stdout is unavailable"))?;
        Ok(Self {
            child,
            input,
            output: BufReader::new(output),
        })
    }

    async fn call(
        &mut self,
        id: u64,
        request: WorkerRequest,
        capabilities: &dyn CapabilityHandler,
        frame_timeout: Duration,
    ) -> anyhow::Result<Result<Value, String>> {
        if self.child.try_wait()?.is_some() {
            anyhow::bail!("component worker exited before request {id}");
        }
        let mut encoded = serde_json::to_vec(&ParentFrame::Request {
            id,
            request: Box::new(request),
            trace_context: brain_observability::inject_current_trace(),
        })?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            anyhow::bail!("component worker request exceeds the frame bound");
        }
        encoded.push(b'\n');
        self.send(id, &encoded, frame_timeout).await?;

        loop {
            let mut line = String::new();
            let bytes = bounded(
                id,
                frame_timeout,
                "sent no frame",
                self.output.read_line(&mut line),
            )
            .await?;
            if bytes == 0 {
                anyhow::bail!("component worker closed before response {id}");
            }
            if bytes > MAX_FRAME_BYTES {
                anyhow::bail!("component worker response exceeds the frame bound");
            }
            match serde_json::from_str(&line)? {
                WorkerFrame::Response {
                    id: response_id,
                    result,
                } if response_id == id => return Ok(result),
                WorkerFrame::Capability {
                    id: capability_id,
                    call,
                } => {
                    // The kernel owns how long this takes and bounds it itself. The worker is
                    // parked on stdin meanwhile, so none of it is the worker falling silent.
                    let result = capabilities.call(call).await;
                    let mut encoded = serde_json::to_vec(&ParentFrame::CapabilityResult {
                        id: capability_id,
                        result,
                    })?;
                    if encoded.len() + 1 > MAX_FRAME_BYTES {
                        anyhow::bail!("component capability response exceeds the frame bound");
                    }
                    encoded.push(b'\n');
                    self.send(id, &encoded, frame_timeout).await?;
                }
                _ => anyhow::bail!("component worker response id does not match request {id}"),
            }
        }
    }

    async fn send(&mut self, id: u64, frame: &[u8], frame_timeout: Duration) -> anyhow::Result<()> {
        bounded(
            id,
            frame_timeout,
            "accepted no frame",
            self.input.write_all(frame),
        )
        .await?;
        bounded(id, frame_timeout, "accepted no frame", self.input.flush()).await
    }
}

/// One wait on the worker itself. Exceeding it means the worker stopped speaking the frame
/// protocol, which is the condition [`WORKER_FRAME_TIMEOUT`] exists to catch.
async fn bounded<T>(
    id: u64,
    frame_timeout: Duration,
    silence: &str,
    io: impl Future<Output = std::io::Result<T>>,
) -> anyhow::Result<T> {
    match tokio::time::timeout(frame_timeout, io).await {
        Ok(result) => Ok(result?),
        Err(_) => anyhow::bail!(
            "component worker {silence} for request {id} within {}s",
            frame_timeout.as_secs()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::worker_env_allowed;

    #[test]
    fn worker_only_inherits_observability_configuration() {
        assert!(worker_env_allowed("RUST_LOG".as_ref()));
        assert!(worker_env_allowed("BRAIN_COMPONENT_CACHE_DIR".as_ref()));
        assert!(worker_env_allowed(
            "BRAIN_COMPONENT_INSTANCE_IDLE_MS".as_ref()
        ));
        assert!(worker_env_allowed("BRAIN_COMPONENT_INSTANCE_CAP".as_ref()));
        assert!(worker_env_allowed("OTEL_EXPORTER_OTLP_ENDPOINT".as_ref()));
        assert!(!worker_env_allowed("AWS_SECRET_ACCESS_KEY".as_ref()));
        assert!(!worker_env_allowed("BRAIN_API_TOKEN".as_ref()));
    }
}
