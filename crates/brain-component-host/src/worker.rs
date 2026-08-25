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

use crate::{ComponentRuntime, agentloop, component_digest, environment, model, tool};

const MAX_COMPONENT_BYTES: u64 = 32 << 20;
const MAX_FRAME_BYTES: usize = 16 << 20;
const WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Serialize, Deserialize)]
pub struct ComponentSource {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerRequest {
    Agentloop {
        component: ComponentSource,
        request: agentloop::aex::agentloop::types::Activation,
    },
    Tool {
        component: ComponentSource,
        request: tool::aex::tool::types::Invocation,
    },
    Environment {
        component: ComponentSource,
        resolve: environment::aex::environment::types::ResolveRequest,
        operation: environment::aex::environment::types::Operation,
    },
    Model {
        component: ComponentSource,
        request: model::aex::model::types::Request,
    },
}

#[derive(Serialize, Deserialize)]
struct RequestFrame {
    id: u64,
    request: WorkerRequest,
}

#[derive(Serialize, Deserialize)]
struct ResponseFrame {
    id: u64,
    result: Result<Value, String>,
}

async fn execute(runtime: &ComponentRuntime, request: WorkerRequest) -> anyhow::Result<Value> {
    match request {
        WorkerRequest::Agentloop { component, request } => {
            let bytes = read_component(&component)?;
            Ok(serde_json::to_value(
                runtime.invoke_agentloop(&bytes, request).await?,
            )?)
        }
        WorkerRequest::Tool { component, request } => {
            let bytes = read_component(&component)?;
            Ok(serde_json::to_value(
                runtime.invoke_tool(&bytes, request).await?,
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
        WorkerRequest::Model { component, request } => {
            let bytes = read_component(&component)?;
            Ok(serde_json::to_value(
                runtime.exercise_model(&bytes, request).await?,
            )?)
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
    let runtime = ComponentRuntime::new()?;
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = input.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(());
        }
        if bytes > MAX_FRAME_BYTES {
            anyhow::bail!("worker request exceeds the frame bound");
        }
        let frame: RequestFrame = serde_json::from_str(&line)?;
        let response = ResponseFrame {
            id: frame.id,
            result: execute(&runtime, frame.request)
                .await
                .map_err(|error| error.to_string()),
        };
        let mut encoded = serde_json::to_vec(&response)?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            encoded = serde_json::to_vec(&ResponseFrame {
                id: response.id,
                result: Err("worker response exceeds the frame bound".into()),
            })?;
        }
        encoded.push(b'\n');
        output.write_all(&encoded).await?;
        output.flush().await?;
    }
}

pub struct WorkerPool {
    program: PathBuf,
    workers: Vec<Mutex<Worker>>,
    next_worker: AtomicUsize,
    next_request: AtomicU64,
}

impl WorkerPool {
    pub async fn new(program: impl AsRef<Path>, size: usize) -> anyhow::Result<Arc<Self>> {
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
        }))
    }

    pub async fn call(&self, request: WorkerRequest) -> anyhow::Result<Value> {
        let index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let mut worker = self.workers[index].lock().await;
        let result = tokio::time::timeout(WORKER_REQUEST_TIMEOUT, worker.call(id, request)).await;
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
    ) -> anyhow::Result<Result<Value, String>> {
        if self.child.try_wait()?.is_some() {
            anyhow::bail!("component worker exited before request {id}");
        }
        let mut encoded = serde_json::to_vec(&RequestFrame { id, request })?;
        if encoded.len() + 1 > MAX_FRAME_BYTES {
            anyhow::bail!("component worker request exceeds the frame bound");
        }
        encoded.push(b'\n');
        self.input.write_all(&encoded).await?;
        self.input.flush().await?;

        let mut line = String::new();
        let bytes = self.output.read_line(&mut line).await?;
        if bytes == 0 {
            anyhow::bail!("component worker closed before response {id}");
        }
        if bytes > MAX_FRAME_BYTES {
            anyhow::bail!("component worker response exceeds the frame bound");
        }
        let response: ResponseFrame = serde_json::from_str(&line)?;
        if response.id != id {
            anyhow::bail!("component worker response id does not match request {id}");
        }
        Ok(response.result)
    }
}
