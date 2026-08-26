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
use tokio::sync::{Mutex, Semaphore, mpsc, oneshot};
use tracing::Instrument as _;

use crate::{
    AgentloopInstance, CapabilityCall, CapabilityFailure, CapabilityHandler, ComponentFailure,
    ComponentRuntime, DenyCapabilities, EnvironmentInstance, ModelInstance, agentloop,
    component_digest, environment, model, tool,
};

const MAX_COMPONENT_BYTES: u64 = 32 << 20;
const MAX_FRAME_BYTES: usize = 16 << 20;
/// How long the parent waits for one worker to make progress on a request: to take a frame,
/// and to answer with its next one. It is a liveness check on the worker process, the only
/// thing the parent cannot otherwise observe. Work the parent performs on the request's behalf
/// (a model round, a Tool dispatch, a child session's turn) happens between frames and is
/// bounded by the kernel that owns it, so it must not be measured here.
const WORKER_FRAME_TIMEOUT: Duration = Duration::from_secs(90);
/// The deepest chain of nested sessions the session contract admits: `children.max_depth` is
/// capped at 8, so a root and its descendants can park at most nine activations on one worker,
/// each waiting on the next.
const MAX_NESTED_ACTIVATIONS: usize = 9;
/// How much work one worker process carries at once. Multiplexing is what lets a parent's
/// activation and its child's run together, but nothing else bounded it: one worker was
/// measured holding 48 simultaneous activations and 1.03 GiB, where before it held one. The cap
/// must exceed the deepest chain, or it would stall the nesting it exists to allow.
pub const WORKER_REQUEST_CAP: usize = 16;
const _: () = assert!(WORKER_REQUEST_CAP > MAX_NESTED_ACTIVATIONS);

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
        heartbeat_ms: u64,
    },
    CapabilityResult {
        id: u64,
        result: Result<Value, CapabilityFailure>,
    },
}

/// One worker failure as it crosses the process boundary. A component's declared `extension-error`
/// keeps its code and its own retryability; anything else is an internal failure a retry may clear.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFailure {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default = "retryable_by_default")]
    pub retryable: bool,
}

fn retryable_by_default() -> bool {
    true
}

impl WorkerFailure {
    fn of(error: &anyhow::Error) -> Self {
        match error.downcast_ref::<ComponentFailure>() {
            Some(failure) => Self {
                message: failure.message.clone(),
                code: Some(failure.code.clone()),
                retryable: failure.retryable,
            },
            None => Self {
                message: error.to_string(),
                code: None,
                retryable: true,
            },
        }
    }

    fn into_error(self) -> anyhow::Error {
        match self.code {
            Some(code) => ComponentFailure::error(code, self.message, self.retryable),
            None => anyhow::Error::msg(self.message),
        }
    }
}

impl std::fmt::Display for WorkerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
enum WorkerFrame {
    Response {
        id: u64,
        result: Result<Value, WorkerFailure>,
    },
    Capability {
        id: u64,
        request_id: u64,
        call: CapabilityCall,
    },
    /// The worker is still there and still on this request. Reading a component of up to
    /// 32 MiB, compiling it and instantiating it all happen before a request's first real
    /// frame, and none of that is the worker falling silent.
    Progress { request_id: u64 },
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

trait Resident {
    type Instance;

    fn last_used(&self) -> std::time::Instant;
    fn touch(&mut self);
    fn instance(&mut self) -> &mut Self::Instance;
}

macro_rules! resident {
    ($resident:ty, $instance:ty) => {
        impl Resident for $resident {
            type Instance = $instance;

            fn last_used(&self) -> std::time::Instant {
                self.last_used
            }

            fn touch(&mut self) {
                self.last_used = std::time::Instant::now();
            }

            fn instance(&mut self) -> &mut Self::Instance {
                &mut self.instance
            }
        }
    };
}

resident!(ResidentAgentloop, AgentloopInstance);
resident!(ResidentEnvironment, EnvironmentInstance);
resident!(ResidentModel, ModelInstance);

/// One resident instance, behind its own lock. Requests for different instances run at the same
/// time; two requests for the same instance still serialize, because a component instance is a
/// single mutable store.
type Slot<T> = Arc<Mutex<Option<T>>>;
type Table<T> = std::sync::Mutex<HashMap<String, Slot<T>>>;

#[derive(Default)]
struct Residents {
    agentloops: Table<ResidentAgentloop>,
    environments: Table<ResidentEnvironment>,
    models: Table<ResidentModel>,
    reported: AtomicUsize,
}

impl Residents {
    fn open<T>(table: &Table<T>, instance_id: &str) -> Slot<T> {
        table
            .lock()
            .expect("resident instances")
            .entry(instance_id.to_owned())
            .or_default()
            .clone()
    }

    /// A slot the caller requires to exist already; `open` would silently create one.
    fn existing<T>(table: &Table<T>, instance_id: &str, world: &str) -> anyhow::Result<Slot<T>> {
        table
            .lock()
            .expect("resident instances")
            .get(instance_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{world} instance is not resident"))
    }

    fn remove<T>(table: &Table<T>, instance_id: &str) {
        table
            .lock()
            .expect("resident instances")
            .remove(instance_id);
    }

    fn environment(&self, instance_id: &str) -> anyhow::Result<Slot<ResidentEnvironment>> {
        Self::existing(&self.environments, instance_id, "Environment")
    }

    fn model(&self, instance_id: &str) -> anyhow::Result<Slot<ResidentModel>> {
        Self::existing(&self.models, instance_id, "Model")
    }

    fn sweep(&self, policy: ResidentPolicy) {
        sweep_table(&self.agentloops, policy);
        sweep_table(&self.environments, policy);
        sweep_table(&self.models, policy);
        self.report();
    }

    /// A resident instance is a whole JavaScript runtime, and residency is bounded by count and
    /// idle time but never by bytes, so the only way to know what a worker is holding is to say
    /// so. The hosted task publishes no per-process memory metric, which is what left a stalled
    /// plane undiagnosable; this is the number that settles it.
    fn report(&self) {
        let agentloops = resident_count(&self.agentloops);
        let environments = resident_count(&self.environments);
        let models = resident_count(&self.models);
        let resident = agentloops + environments + models;
        if self.reported.swap(resident, Ordering::Relaxed) == resident {
            return;
        }
        tracing::info!(
            component.agentloops = agentloops,
            component.environments = environments,
            component.models = models,
            component.resident_bytes = resident_bytes(),
            "component worker residency"
        );
    }
}

/// The instance inside a slot the caller already proved is in the table. A slot can still be
/// empty: an instantiation that failed, or a lifecycle that already released it.
fn live<'a, T: Resident>(
    slot: &'a mut Option<T>,
    world: &str,
) -> anyhow::Result<&'a mut T::Instance> {
    slot.as_mut()
        .map(|resident| {
            resident.touch();
            resident.instance()
        })
        .ok_or_else(|| anyhow::anyhow!("{world} instance is not resident"))
}

fn resident_count<T>(table: &Table<T>) -> usize {
    table.lock().expect("resident instances").len()
}

/// This process's resident set, as the kernel reports it. Linux only, which is where the
/// hosted plane runs; elsewhere the field is absent rather than wrong.
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// Evict idle instances and hold the table to its cap. A slot that is locked is serving a request
/// right now, so it is neither idle nor evictable; an emptied slot is dropped so a failed
/// instantiation leaves nothing behind. The slot locks are only ever tried, never awaited, while
/// the table lock is held: a request that already holds a slot may still take the table.
fn sweep_table<T: Resident>(table: &Table<T>, policy: ResidentPolicy) {
    let now = std::time::Instant::now();
    let mut table = table.lock().expect("resident instances");
    table.retain(|_, slot| match slot.try_lock() {
        Ok(guard) => guard
            .as_ref()
            .is_some_and(|resident| now.duration_since(resident.last_used()) < policy.idle),
        Err(_) => true,
    });
    while table.len() > policy.cap {
        let Some(oldest) = table
            .iter()
            .filter_map(|(id, slot)| {
                let guard = slot.try_lock().ok()?;
                let last_used = guard.as_ref()?.last_used();
                Some((last_used, id.clone()))
            })
            .min_by_key(|(last_used, _)| *last_used)
            .map(|(_, id)| id)
        else {
            break;
        };
        table.remove(&oldest);
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

async fn execute(
    runtime: &ComponentRuntime,
    residents: &Residents,
    policy: ResidentPolicy,
    request: WorkerRequest,
) -> anyhow::Result<Value> {
    residents.sweep(policy);
    match request {
        WorkerRequest::Agentloop {
            instance_id,
            component,
            request,
        } => {
            let bytes = read_component(&component)?;
            let slot = Residents::open(&residents.agentloops, &instance_id);
            let mut resident = slot.lock().await;
            if let Some(resident) = resident.as_mut() {
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
            *resident = Some(ResidentAgentloop {
                digest: component.sha256,
                instance,
                last_used: std::time::Instant::now(),
            });
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
            let slot = Residents::open(&residents.environments, &instance_id);
            let mut resident = slot.lock().await;
            if let Some(resident) = resident.as_mut() {
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
            *resident = Some(ResidentEnvironment {
                digest: component.sha256,
                instance,
                last_used: std::time::Instant::now(),
            });
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::EnvironmentSubmit {
            instance_id,
            binding_json,
            operation,
        } => {
            let slot = residents.environment(&instance_id)?;
            let mut resident = slot.lock().await;
            Ok(serde_json::to_value(
                live(&mut resident, "Environment")?
                    .submit(&binding_json, &operation)
                    .await?,
            )?)
        }
        WorkerRequest::EnvironmentObserve {
            instance_id,
            binding_json,
            provider_operation_id,
            cursor,
        } => {
            let slot = residents.environment(&instance_id)?;
            let mut resident = slot.lock().await;
            Ok(serde_json::to_value(
                live(&mut resident, "Environment")?
                    .observe(&binding_json, &provider_operation_id, cursor.as_deref())
                    .await?,
            )?)
        }
        WorkerRequest::EnvironmentCancel {
            instance_id,
            binding_json,
            provider_operation_id,
        } => {
            let slot = residents.environment(&instance_id)?;
            let mut resident = slot.lock().await;
            live(&mut resident, "Environment")?
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
            let slot = residents.environment(&instance_id)?;
            let mut resident = slot.lock().await;
            live(&mut resident, "Environment")?
                .acknowledge(&binding_json, &provider_operation_id, &terminal_json)
                .await?;
            Ok(Value::Null)
        }
        WorkerRequest::EnvironmentRelease {
            instance_id,
            binding_json,
        } => {
            let slot = residents.environment(&instance_id)?;
            let mut resident = slot.lock().await;
            live(&mut resident, "Environment")?
                .release(&binding_json)
                .await?;
            *resident = None;
            Residents::remove(&residents.environments, &instance_id);
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
            let slot = Residents::open(&residents.models, &instance_id);
            let mut resident = slot.lock().await;
            if let Some(resident) = resident.as_mut() {
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
            *resident = Some(ResidentModel {
                digest: component.sha256,
                instance,
                last_used: std::time::Instant::now(),
            });
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::ModelObserve {
            instance_id,
            provider_operation_id,
            cursor,
        } => {
            let slot = residents.model(&instance_id)?;
            let mut resident = slot.lock().await;
            Ok(serde_json::to_value(
                live(&mut resident, "Model")?
                    .observe(&provider_operation_id, cursor.as_deref())
                    .await?,
            )?)
        }
        WorkerRequest::ModelCancel {
            instance_id,
            provider_operation_id,
        } => {
            let slot = residents.model(&instance_id)?;
            let mut resident = slot.lock().await;
            live(&mut resident, "Model")?
                .cancel(&provider_operation_id)
                .await?;
            Ok(Value::Null)
        }
        WorkerRequest::ModelAcknowledge {
            instance_id,
            provider_operation_id,
            terminal_json,
        } => {
            let slot = residents.model(&instance_id)?;
            let mut resident = slot.lock().await;
            live(&mut resident, "Model")?
                .acknowledge(&provider_operation_id, &terminal_json)
                .await?;
            *resident = None;
            Residents::remove(&residents.models, &instance_id);
            Ok(Value::Null)
        }
        WorkerRequest::Release { world, instance_id } => {
            match world.as_str() {
                crate::AGENTLOOP_COMPONENT => {
                    Residents::remove(&residents.agentloops, &instance_id);
                }
                crate::ENVIRONMENT_COMPONENT => {
                    Residents::remove(&residents.environments, &instance_id);
                }
                crate::MODEL_COMPONENT => {
                    Residents::remove(&residents.models, &instance_id);
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

// The request a guest's capability call belongs to, so the frame the worker emits can name it
// and the parent can bound each request on its own progress.
tokio::task_local! {
    static SERVING_REQUEST: u64;
}

pub async fn run_worker() -> anyhow::Result<()> {
    let mut input = BufReader::new(tokio::io::stdin());
    let output = Arc::new(Mutex::new(tokio::io::stdout()));
    let broker = Arc::new(WorkerCapabilityBroker {
        output: output.clone(),
        awaiting: std::sync::Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
    });
    let runtime = ComponentRuntime::with_capabilities(broker.clone())?;
    let residents = Arc::new(Residents::default());
    let policy = ResidentPolicy::from_env()?;
    loop {
        let mut line = String::new();
        let bytes = input.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(());
        }
        if bytes > MAX_FRAME_BYTES {
            anyhow::bail!("worker request exceeds the frame bound");
        }
        match serde_json::from_str(&line)? {
            ParentFrame::Request {
                id,
                request,
                trace_context,
                heartbeat_ms,
            } => {
                let span = invocation_span(&request);
                brain_observability::set_parent_from_trace(&span, &trace_context);
                let runtime = runtime.clone();
                let residents = residents.clone();
                let output = output.clone();
                let heartbeat = tokio::spawn(beat(output.clone(), id, heartbeat_ms));
                // One task per request: a request parked in a capability leaves the worker free
                // to serve the next one, which is what lets a subagent's child run while its
                // parent waits on it.
                tokio::spawn(SERVING_REQUEST.scope(id, async move {
                    let result = execute(&runtime, &residents, policy, *request)
                        .instrument(span)
                        .await
                        .map_err(|error| WorkerFailure::of(&error));
                    let mut encoded =
                        match serde_json::to_vec(&WorkerFrame::Response { id, result }) {
                            Ok(encoded) if encoded.len() < MAX_FRAME_BYTES => encoded,
                            _ => serde_json::to_vec(&WorkerFrame::Response {
                                id,
                                result: Err(WorkerFailure {
                                    message: "worker response exceeds the frame bound".into(),
                                    code: None,
                                    retryable: true,
                                }),
                            })
                            .expect("a bounded error response always encodes"),
                        };
                    encoded.push(b'\n');
                    heartbeat.abort();
                    let mut output = output.lock().await;
                    let _ = output.write_all(&encoded).await;
                    let _ = output.flush().await;
                }));
            }
            ParentFrame::CapabilityResult { id, result } => broker.settle(id, result),
        }
    }
}

/// Say the worker is still on this request until the caller aborts this.
async fn beat(output: Arc<Mutex<tokio::io::Stdout>>, request_id: u64, every_ms: u64) {
    let every = Duration::from_millis(every_ms.max(1));
    let mut encoded = serde_json::to_vec(&WorkerFrame::Progress { request_id })
        .expect("a progress frame encodes");
    encoded.push(b'\n');
    loop {
        tokio::time::sleep(every).await;
        let mut output = output.lock().await;
        if output.write_all(&encoded).await.is_err() || output.flush().await.is_err() {
            return;
        }
    }
}

struct WorkerCapabilityBroker {
    output: Arc<Mutex<tokio::io::Stdout>>,
    awaiting: std::sync::Mutex<HashMap<u64, oneshot::Sender<Result<Value, CapabilityFailure>>>>,
    next_id: AtomicU64,
}

impl WorkerCapabilityBroker {
    fn settle(&self, id: u64, result: Result<Value, CapabilityFailure>) {
        if let Some(reply) = self
            .awaiting
            .lock()
            .expect("capability waiters")
            .remove(&id)
        {
            let _ = reply.send(result);
        }
    }
}

#[async_trait::async_trait]
impl CapabilityHandler for WorkerCapabilityBroker {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request_id = SERVING_REQUEST.try_with(|request| *request).unwrap_or(0);
        let mut encoded = serde_json::to_vec(&WorkerFrame::Capability {
            id,
            request_id,
            call,
        })
        .map_err(|error| CapabilityFailure {
            code: "worker_protocol".into(),
            message: error.to_string(),
            retryable: true,
        })?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            return Err(CapabilityFailure {
                code: "worker_protocol".into(),
                message: "capability request exceeds the frame bound".into(),
                retryable: false,
            });
        }
        encoded.push(b'\n');
        let (reply, response) = oneshot::channel();
        self.awaiting
            .lock()
            .expect("capability waiters")
            .insert(id, reply);
        {
            let mut output = self.output.lock().await;
            output.write_all(&encoded).await.map_err(worker_failure)?;
            output.flush().await.map_err(worker_failure)?;
        }
        response.await.map_err(|_| CapabilityFailure {
            code: "worker_protocol".into(),
            message: "the parent closed before answering this capability".into(),
            retryable: true,
        })?
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
    workers: Vec<Mutex<Arc<Worker>>>,
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
            workers.push(Mutex::new(Worker::spawn(&program, capabilities.clone())?));
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
        let worker = self.workers[index].lock().await.clone();
        match worker.call(id, request, self.frame_timeout).await {
            Ok(result) => result.map_err(|failure| failure.into_error()),
            Err(error) => {
                // A worker that stopped speaking the frame protocol is replaced, and every other
                // request riding it fails with it rather than waiting on a process that is gone.
                let mut slot = self.workers[index].lock().await;
                if Arc::ptr_eq(&slot, &worker) {
                    *slot = Worker::spawn(&self.program, self.capabilities.clone())?;
                }
                Err(error)
            }
        }
    }
}

/// What a request learns about its own progress. The kernel owns how long a capability takes
/// and bounds it itself, so a request with one outstanding is not a worker falling silent.
enum RequestFrame {
    Alive,
    CapabilityStarted,
    CapabilityFinished,
    Done(Result<Value, WorkerFailure>),
}

/// One worker process. Requests are multiplexed over its pipes by id, so a request parked in a
/// capability holds nothing that another request needs.
struct Worker {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    inflight: std::sync::Mutex<Option<HashMap<u64, mpsc::UnboundedSender<RequestFrame>>>>,
    carrying: Semaphore,
}

impl Worker {
    fn spawn(
        program: &Path,
        capabilities: Arc<dyn CapabilityHandler>,
    ) -> anyhow::Result<Arc<Self>> {
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("component worker stdin is unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("component worker stdout is unavailable"))?;
        let worker = Arc::new(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            inflight: std::sync::Mutex::new(Some(HashMap::new())),
            carrying: Semaphore::new(WORKER_REQUEST_CAP),
        });
        // The reader holds a weak handle so the pool dropping the worker still closes the pipes
        // and ends this task.
        let reading = Arc::downgrade(&worker);
        tokio::spawn(read_frames(reading, BufReader::new(stdout), capabilities));
        Ok(worker)
    }

    async fn call(
        &self,
        id: u64,
        request: WorkerRequest,
        frame_timeout: Duration,
    ) -> anyhow::Result<Result<Value, WorkerFailure>> {
        // Waiting here is waiting for room in one process, not for another request to finish
        // with it: a chain of nested activations is shorter than the cap, so it always fits.
        let _room = self
            .carrying
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("component worker closed before request {id}"))?;
        if self.child.lock().await.try_wait()?.is_some() {
            anyhow::bail!("component worker exited before request {id}");
        }
        let mut encoded = serde_json::to_vec(&ParentFrame::Request {
            id,
            request: Box::new(request),
            trace_context: brain_observability::inject_current_trace(),
            // Three beats inside the bound: one lost to a scheduling hiccup is not silence.
            heartbeat_ms: (frame_timeout.as_millis() as u64 / 3).max(1),
        })?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            anyhow::bail!("component worker request exceeds the frame bound");
        }
        encoded.push(b'\n');
        let (sender, mut frames) = mpsc::unbounded_channel();
        self.register(id, sender)?;
        let outcome = self
            .exchange(id, &encoded, &mut frames, frame_timeout)
            .await;
        self.forget(id);
        outcome
    }

    fn register(&self, id: u64, sender: mpsc::UnboundedSender<RequestFrame>) -> anyhow::Result<()> {
        let mut inflight = self.inflight.lock().expect("in-flight requests");
        let Some(inflight) = inflight.as_mut() else {
            anyhow::bail!("component worker closed before request {id}");
        };
        inflight.insert(id, sender);
        Ok(())
    }

    fn forget(&self, id: u64) {
        if let Some(inflight) = self.inflight.lock().expect("in-flight requests").as_mut() {
            inflight.remove(&id);
        }
    }

    async fn exchange(
        &self,
        id: u64,
        encoded: &[u8],
        frames: &mut mpsc::UnboundedReceiver<RequestFrame>,
        frame_timeout: Duration,
    ) -> anyhow::Result<Result<Value, WorkerFailure>> {
        {
            let mut stdin = self.stdin.lock().await;
            bounded(
                id,
                frame_timeout,
                "accepted no frame",
                stdin.write_all(encoded),
            )
            .await?;
            bounded(id, frame_timeout, "accepted no frame", stdin.flush()).await?;
        }
        let mut serving = 0usize;
        loop {
            let frame = if serving == 0 {
                match tokio::time::timeout(frame_timeout, frames.recv()).await {
                    Ok(frame) => frame,
                    Err(_) => anyhow::bail!(
                        "component worker sent no frame for request {id} within {}s",
                        frame_timeout.as_secs()
                    ),
                }
            } else {
                frames.recv().await
            };
            match frame {
                Some(RequestFrame::Alive) => {}
                Some(RequestFrame::CapabilityStarted) => serving += 1,
                Some(RequestFrame::CapabilityFinished) => serving = serving.saturating_sub(1),
                Some(RequestFrame::Done(result)) => return Ok(result),
                None => anyhow::bail!("component worker closed before response {id}"),
            }
        }
    }

    fn deliver(&self, id: u64, frame: RequestFrame) {
        if let Some(inflight) = self.inflight.lock().expect("in-flight requests").as_ref()
            && let Some(sender) = inflight.get(&id)
        {
            let _ = sender.send(frame);
        }
    }

    /// The process is gone: every request riding it ends now, and no new one may join.
    fn close(&self) {
        let _ = self.inflight.lock().expect("in-flight requests").take();
        self.carrying.close();
    }

    async fn send(&self, frame: &ParentFrame) -> anyhow::Result<()> {
        let mut encoded = serde_json::to_vec(frame)?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            anyhow::bail!("component capability response exceeds the frame bound");
        }
        encoded.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&encoded).await?;
        stdin.flush().await?;
        Ok(())
    }
}

/// One capability the parent owes the worker, for as long as it owes it.
struct Serving(Arc<Worker>, u64);

impl Serving {
    fn start(worker: Arc<Worker>, request_id: u64) -> Self {
        worker.deliver(request_id, RequestFrame::CapabilityStarted);
        Self(worker, request_id)
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.0.deliver(self.1, RequestFrame::CapabilityFinished);
    }
}

/// Route every frame the worker emits to the request it names. Capability work runs in its own
/// task so servicing one request never delays another's frames.
async fn read_frames(
    worker: std::sync::Weak<Worker>,
    mut output: BufReader<ChildStdout>,
    capabilities: Arc<dyn CapabilityHandler>,
) {
    loop {
        let mut line = String::new();
        let bytes = match output.read_line(&mut line).await {
            Ok(bytes) => bytes,
            Err(_) => break,
        };
        if bytes == 0 || bytes > MAX_FRAME_BYTES {
            break;
        }
        let Some(worker) = worker.upgrade() else {
            return;
        };
        match serde_json::from_str(&line) {
            Ok(WorkerFrame::Response { id, result }) => {
                worker.deliver(id, RequestFrame::Done(result));
            }
            Ok(WorkerFrame::Progress { request_id }) => {
                worker.deliver(request_id, RequestFrame::Alive);
            }
            Ok(WorkerFrame::Capability {
                id,
                request_id,
                call,
            }) => {
                // The kernel owns how long this takes and bounds it itself, so the request
                // stops spending its own bound the moment this frame lands and starts again
                // only when the debt is paid — including if paying it panics, which would
                // otherwise leave the request waiting on nothing.
                let serving = Serving::start(worker.clone(), request_id);
                let capabilities = capabilities.clone();
                tokio::spawn(async move {
                    let result = capabilities.call(call).await;
                    let _ = worker
                        .send(&ParentFrame::CapabilityResult { id, result })
                        .await;
                    drop(serving);
                });
            }
            Err(_) => break,
        }
    }
    if let Some(worker) = worker.upgrade() {
        worker.close();
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
