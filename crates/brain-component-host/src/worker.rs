use std::collections::HashMap;
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

use crate::{
    AgentloopInstance, CapabilityCall, CapabilityFailure, CapabilityHandler, ComponentRuntime,
    DenyCapabilities, EnvironmentInstance, ModelInstance, agentloop, component_digest, environment,
    model, tool,
};

const MAX_COMPONENT_BYTES: u64 = 32 << 20;
const MAX_FRAME_BYTES: usize = 16 << 20;
const WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Serialize, Deserialize)]
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
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
enum ParentFrame {
    Request {
        id: u64,
        request: Box<WorkerRequest>,
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
}

struct ResidentEnvironment {
    digest: String,
    instance: EnvironmentInstance,
}

struct ResidentModel {
    digest: String,
    instance: ModelInstance,
}

async fn execute(
    runtime: &ComponentRuntime,
    agentloops: &mut HashMap<String, ResidentAgentloop>,
    environments: &mut HashMap<String, ResidentEnvironment>,
    models: &mut HashMap<String, ResidentModel>,
    request: WorkerRequest,
) -> anyhow::Result<Value> {
    match request {
        WorkerRequest::Agentloop {
            instance_id,
            component,
            request,
        } => {
            let bytes = read_component(&component)?;
            if let Some(resident) = agentloops.get_mut(&instance_id) {
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
                },
            );
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
                },
            );
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::EnvironmentSubmit {
            instance_id,
            binding_json,
            operation,
        } => Ok(serde_json::to_value(
            environments
                .get_mut(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Environment instance is not resident"))?
                .instance
                .submit(&binding_json, &operation)
                .await?,
        )?),
        WorkerRequest::EnvironmentObserve {
            instance_id,
            binding_json,
            provider_operation_id,
            cursor,
        } => Ok(serde_json::to_value(
            environments
                .get_mut(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Environment instance is not resident"))?
                .instance
                .observe(&binding_json, &provider_operation_id, cursor.as_deref())
                .await?,
        )?),
        WorkerRequest::EnvironmentCancel {
            instance_id,
            binding_json,
            provider_operation_id,
        } => {
            environments
                .get_mut(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Environment instance is not resident"))?
                .instance
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
            environments
                .get_mut(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Environment instance is not resident"))?
                .instance
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
                },
            );
            Ok(serde_json::to_value(result)?)
        }
        WorkerRequest::ModelObserve {
            instance_id,
            provider_operation_id,
            cursor,
        } => Ok(serde_json::to_value(
            models
                .get_mut(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Model instance is not resident"))?
                .instance
                .observe(&provider_operation_id, cursor.as_deref())
                .await?,
        )?),
        WorkerRequest::ModelCancel {
            instance_id,
            provider_operation_id,
        } => {
            models
                .get_mut(&instance_id)
                .ok_or_else(|| anyhow::anyhow!("Model instance is not resident"))?
                .instance
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
    loop {
        let mut line = String::new();
        let bytes = input.lock().await.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(());
        }
        if bytes > MAX_FRAME_BYTES {
            anyhow::bail!("worker request exceeds the frame bound");
        }
        let (id, request) = match serde_json::from_str(&line)? {
            ParentFrame::Request { id, request } => (id, request),
            ParentFrame::CapabilityResult { .. } => {
                anyhow::bail!("worker received a capability result outside an invocation")
            }
        };
        let response = WorkerFrame::Response {
            id,
            result: execute(
                &runtime,
                &mut agentloops,
                &mut environments,
                &mut models,
                *request,
            )
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
        let result = tokio::time::timeout(
            WORKER_REQUEST_TIMEOUT,
            worker.call(id, request, self.capabilities.as_ref()),
        )
        .await;
        match result {
            Ok(Ok(result)) => result.map_err(anyhow::Error::msg),
            Ok(Err(error)) => {
                *worker = Worker::spawn(&self.program)?;
                Err(error)
            }
            Err(_) => {
                *worker = Worker::spawn(&self.program)?;
                anyhow::bail!("component worker request {id} exceeded the wall-time bound")
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
        let mut child = Command::new(program)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
    ) -> anyhow::Result<Result<Value, String>> {
        if self.child.try_wait()?.is_some() {
            anyhow::bail!("component worker exited before request {id}");
        }
        let mut encoded = serde_json::to_vec(&ParentFrame::Request {
            id,
            request: Box::new(request),
        })?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            anyhow::bail!("component worker request exceeds the frame bound");
        }
        encoded.push(b'\n');
        self.input.write_all(&encoded).await?;
        self.input.flush().await?;

        loop {
            let mut line = String::new();
            let bytes = self.output.read_line(&mut line).await?;
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
                    let result = capabilities.call(call).await;
                    let mut encoded = serde_json::to_vec(&ParentFrame::CapabilityResult {
                        id: capability_id,
                        result,
                    })?;
                    if encoded.len() + 1 > MAX_FRAME_BYTES {
                        anyhow::bail!("component capability response exceeds the frame bound");
                    }
                    encoded.push(b'\n');
                    self.input.write_all(&encoded).await?;
                    self.input.flush().await?;
                }
                _ => anyhow::bail!("component worker response id does not match request {id}"),
            }
        }
    }
}
