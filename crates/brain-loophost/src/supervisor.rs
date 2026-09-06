#[cfg(unix)]
use std::process::Stdio;
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use brain_protocol::{AgentloopId, ToolId, TurnError, TurnInput, TurnOutput};
use tokio::sync::{Mutex, Semaphore};

use crate::{LoopLimits, NativeEnvironment, NativeToolInput, TurnBridge, WorkerClient};

/// How long the worker may go without a frame. It covers one bounded native HTTP wait;
/// runaway guest compute is stopped independently by Wasmtime fuel.
const WORKER_BACKSTOP: Duration = Duration::from_secs(125);

/// Why the pool could not run something, or why a turn it ran did not finish.
/// `Overloaded` is transient and the request was never started; `Turn` is the loop's
/// own failure with the code it or the runtime gave it; `Failed` is this side's.
#[derive(Debug)]
pub enum LoopError {
    Overloaded,
    Turn(TurnError),
    Failed(String),
}

impl std::fmt::Display for LoopError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopError::Overloaded => formatter.write_str("Loophost is at capacity"),
            LoopError::Turn(error) => write!(formatter, "{}: {}", error.code, error.message),
            LoopError::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for LoopError {}

impl From<String> for LoopError {
    fn from(message: String) -> Self {
        LoopError::Failed(message)
    }
}

impl From<&str> for LoopError {
    fn from(message: &str) -> Self {
        LoopError::Failed(message.to_owned())
    }
}

pub struct WorkerPool {
    worker_binary: PathBuf,
    socket: PathBuf,
    packages: PathBuf,
    limits: LoopLimits,
    permits: Arc<Semaphore>,
    tool_permits: Arc<Semaphore>,
    state: Mutex<WorkerState>,
}

#[derive(Default)]
struct WorkerState {
    agentloops: HashSet<AgentloopId>,
    tools: HashSet<ToolId>,
    #[cfg(unix)]
    child: Option<tokio::process::Child>,
}

impl WorkerPool {
    pub fn new(
        worker_binary: impl Into<PathBuf>,
        run_dir: impl Into<PathBuf>,
        packages: impl Into<PathBuf>,
        limits: LoopLimits,
    ) -> Self {
        let run_dir = run_dir.into();
        Self {
            worker_binary: worker_binary.into(),
            socket: run_dir.join("brain-loop-worker.sock"),
            packages: packages.into(),
            // Match the worker's execution slots exactly: an accepted connection must
            // never wait silently behind a worker-side slot and trip the liveness bound.
            permits: Arc::new(Semaphore::new(limits.concurrent_turns_per_worker.max(1))),
            tool_permits: Arc::new(Semaphore::new(limits.concurrent_turns_per_worker.max(1))),
            limits,
            state: Mutex::new(WorkerState::default()),
        }
    }

    pub async fn admit(&self, package: Vec<u8>) -> Result<AgentloopId, LoopError> {
        if package.len() > self.limits.package_bytes {
            return Err("Agentloop package exceeds the configured admission limit".into());
        }
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LoopError::Overloaded)?;
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        let digest = WorkerClient::new(&self.socket).admit(&package).await?;
        persist_component(&self.packages, "agentloop", digest.as_str(), &package).await?;
        state.agentloops.insert(digest.clone());
        Ok(digest)
    }

    pub async fn admit_tool(&self, component: Vec<u8>) -> Result<ToolId, LoopError> {
        if component.len() > self.limits.package_bytes {
            return Err("Tool Component exceeds the configured admission limit".into());
        }
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LoopError::Overloaded)?;
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        let digest = WorkerClient::new(&self.socket)
            .admit_tool(&component)
            .await?;
        persist_component(&self.packages, "tool", digest.as_str(), &component).await?;
        state.tools.insert(digest.clone());
        Ok(digest)
    }

    pub async fn status(&self, digest: &AgentloopId) -> Result<bool, LoopError> {
        tokio::fs::try_exists(component_path(&self.packages, "agentloop", digest.as_str()))
            .await
            .map_err(|error| LoopError::Failed(error.to_string()))
    }

    pub async fn tool_status(&self, digest: &ToolId) -> Result<bool, LoopError> {
        tokio::fs::try_exists(component_path(&self.packages, "tool", digest.as_str()))
            .await
            .map_err(|error| LoopError::Failed(error.to_string()))
    }

    pub async fn ready(&self) -> Result<(), LoopError> {
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        Ok(WorkerClient::new(&self.socket).ping().await?)
    }

    /// Runs one turn with exactly the grants in `environment`. The bridge answers the
    /// guest's host calls for as long as the turn runs; a worker that stops answering
    /// between them is restarted.
    pub async fn turn(
        &self,
        digest: AgentloopId,
        environment: NativeEnvironment,
        input: TurnInput,
        bridge: &dyn TurnBridge,
    ) -> Result<TurnOutput, LoopError> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LoopError::Overloaded)?;
        // Everything that needs the worker's identity happens under the lock; the turn
        // itself does not. Holding it across the call would serialise every session in
        // the process onto one turn at a time, whatever the permits allowed.
        {
            let mut state = self.state.lock().await;
            self.ensure_worker(&mut state).await?;
            if !state.agentloops.contains(&digest) {
                let package =
                    tokio::fs::read(component_path(&self.packages, "agentloop", digest.as_str()))
                        .await
                        .map_err(|_| "Agentloop digest is not admitted".to_owned())?;
                let admitted = WorkerClient::new(&self.socket).admit(&package).await?;
                if admitted != digest {
                    return Err("persisted Agentloop package changed digest".into());
                }
                state.agentloops.insert(digest.clone());
            }
        }
        let client = WorkerClient::new(&self.socket);
        let outcome = client
            .turn(
                digest,
                environment,
                input,
                self.limits.turn_input_bytes,
                bridge,
                WORKER_BACKSTOP,
            )
            .await;
        match outcome {
            Ok(output) => {
                let output_bytes =
                    serde_json::to_vec(&output).map_err(|error| error.to_string())?;
                if output_bytes.len() > self.limits.turn_output_bytes {
                    return Err("Agentloop turn output exceeds the configured limit".into());
                }
                Ok(output)
            }
            Err(LoopError::Failed(message)) if message == "brain-loop-worker stopped answering" => {
                // The guest's own compute budget fires before this, so reaching here
                // means the worker itself is not answering. Restarting it is the only
                // thing left.
                let mut state = self.state.lock().await;
                self.stop_worker(&mut state).await;
                Err(LoopError::Failed(message))
            }
            Err(error) => Err(error),
        }
    }

    pub async fn tool(
        &self,
        digest: ToolId,
        environment: NativeEnvironment,
        input: NativeToolInput,
        bridge: &dyn TurnBridge,
    ) -> Result<serde_json::Value, LoopError> {
        let _permit = self
            .tool_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LoopError::Overloaded)?;
        {
            let mut state = self.state.lock().await;
            self.ensure_worker(&mut state).await?;
            if !state.tools.contains(&digest) {
                let component =
                    tokio::fs::read(component_path(&self.packages, "tool", digest.as_str()))
                        .await
                        .map_err(|_| "Tool digest is not admitted".to_owned())?;
                let admitted = WorkerClient::new(&self.socket)
                    .admit_tool(&component)
                    .await?;
                if admitted != digest {
                    return Err("persisted Tool Component changed digest".into());
                }
                state.tools.insert(digest.clone());
            }
        }
        let outcome = WorkerClient::new(&self.socket)
            .tool(digest, environment, input, bridge, WORKER_BACKSTOP)
            .await;
        match outcome {
            Err(LoopError::Failed(message)) if message == "brain-loop-worker stopped answering" => {
                let mut state = self.state.lock().await;
                self.stop_worker(&mut state).await;
                Err(LoopError::Failed(message))
            }
            outcome => outcome,
        }
    }

    #[cfg(unix)]
    async fn ensure_worker(&self, state: &mut WorkerState) -> Result<(), String> {
        let exited = match state.child.as_mut() {
            Some(child) => child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some(),
            None => true,
        };
        if exited {
            self.stop_worker(state).await;
            if let Some(parent) = self.socket.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| error.to_string())?;
                secure_worker_directory(parent).await?;
            }
            let _ = tokio::fs::remove_file(&self.socket).await;
            let child = tokio::process::Command::new(&self.worker_binary)
                .arg(&self.socket)
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|error| format!("failed to start brain-loop-worker: {error}"))?;
            state.child = Some(child);
            state.agentloops.clear();
            state.tools.clear();
            let client = WorkerClient::new(&self.socket);
            let ready = async {
                loop {
                    if client.ping().await.is_ok() {
                        return Ok(());
                    }
                    if let Some(status) = state
                        .child
                        .as_mut()
                        .expect("worker child exists")
                        .try_wait()
                        .map_err(|error| error.to_string())?
                    {
                        return Err(format!("brain-loop-worker exited during startup: {status}"));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), ready)
                .await
                .map_err(|_| "brain-loop-worker did not become ready".to_owned())??;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    async fn ensure_worker(&self, _state: &mut WorkerState) -> Result<(), String> {
        let _ = &self.worker_binary;
        Err("brain-loop-worker requires a Unix server".into())
    }

    #[cfg(unix)]
    async fn stop_worker(&self, state: &mut WorkerState) {
        if let Some(mut child) = state.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        state.agentloops.clear();
        state.tools.clear();
        let _ = tokio::fs::remove_file(&self.socket).await;
    }

    #[cfg(not(unix))]
    async fn stop_worker(&self, state: &mut WorkerState) {
        state.agentloops.clear();
        state.tools.clear();
    }
}

#[cfg(unix)]
async fn secure_worker_directory(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .await
        .map_err(|error| format!("failed to restrict worker socket directory: {error}"))
}

async fn persist_component(
    directory: &std::path::Path,
    kind: &str,
    digest: &str,
    package: &[u8],
) -> Result<(), String> {
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|error| error.to_string())?;
    let target = component_path(directory, kind, digest);
    let temporary = directory.join(format!(".{kind}-{digest}.tmp"));
    tokio::fs::write(&temporary, package)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::File::open(&temporary)
        .await
        .map_err(|error| error.to_string())?
        .sync_all()
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::rename(&temporary, &target)
        .await
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    for path in [Some(directory), directory.parent()].into_iter().flatten() {
        tokio::fs::File::open(path)
            .await
            .map_err(|error| error.to_string())?
            .sync_all()
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn component_path(directory: &std::path::Path, kind: &str, digest: &str) -> PathBuf {
    directory.join(format!("{kind}-{digest}.wasm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_status_reads_the_admitted_component_store() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "brain-loophost-tool-status-{}-{suffix}",
            std::process::id()
        ));
        let components = root.join("components");
        tokio::fs::create_dir_all(&components).await.unwrap();
        let admitted = ToolId::new("b".repeat(64));
        tokio::fs::write(
            component_path(&components, "tool", admitted.as_str()),
            b"component",
        )
        .await
        .unwrap();
        let pool = WorkerPool::new(
            "worker",
            root.join("run"),
            &components,
            LoopLimits::default(),
        );
        assert!(pool.tool_status(&admitted).await.unwrap());
        assert!(
            !pool
                .tool_status(&ToolId::new("c".repeat(64)))
                .await
                .unwrap()
        );
        tokio::fs::remove_dir_all(&root).await.unwrap();
    }
}
