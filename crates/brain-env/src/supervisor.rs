#[cfg(unix)]
use std::process::Stdio;
use std::{collections::HashSet, path::PathBuf, sync::Arc};

use brain_protocol::{AgentloopId, ToolId, TurnError, TurnInput, TurnOutput};
use tokio::sync::{Mutex, Semaphore};

use crate::{
    EnvLimits, NativeEnvironment, NativeToolInput, TurnBridge, WorkerClient, limits::ceiling,
};

/// Permits for a concurrency ceiling: zero means as many as the semaphore allows.
fn permits(limit: usize) -> usize {
    if limit == 0 {
        Semaphore::MAX_PERMITS
    } else {
        limit
    }
}

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
            LoopError::Overloaded => formatter.write_str("brain-env is at capacity"),
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
    workers: Vec<WorkerSlot>,
    next: std::sync::atomic::AtomicUsize,
    admission: Mutex<()>,
}

impl WorkerPool {
    pub fn new(
        worker_binary: impl Into<PathBuf>,
        run_dir: impl Into<PathBuf>,
        packages: impl Into<PathBuf>,
        limits: EnvLimits,
        count: std::num::NonZeroUsize,
    ) -> Self {
        let worker_binary = worker_binary.into();
        let run_dir = run_dir.into();
        let packages = packages.into();
        Self {
            workers: (0..count.get())
                .map(|index| {
                    WorkerSlot::new(
                        worker_binary.clone(),
                        run_dir.join(index.to_string()),
                        packages.clone(),
                        limits.clone(),
                    )
                })
                .collect(),
            next: std::sync::atomic::AtomicUsize::new(0),
            admission: Mutex::new(()),
        }
    }

    fn select(
        &self,
        leaf: bool,
    ) -> Result<(&WorkerSlot, tokio::sync::OwnedSemaphorePermit), LoopError> {
        let start = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for offset in 0..self.workers.len() {
            let worker = &self.workers[start.wrapping_add(offset) % self.workers.len()];
            let capacity = if leaf {
                &worker.leaf_permits
            } else {
                &worker.permits
            };
            if let Ok(permit) = capacity.clone().try_acquire_owned() {
                return Ok((worker, permit));
            }
        }
        Err(LoopError::Overloaded)
    }

    pub async fn start(&self) -> Result<(), LoopError> {
        for worker in &self.workers {
            worker.ready().await?;
        }
        Ok(())
    }

    pub async fn ready(&self) -> Result<(), LoopError> {
        if self.workers[0].permits.is_closed() {
            return Err("brain-env is shut down".into());
        }
        let mut failure = None;
        for worker in &self.workers {
            match worker.ready().await {
                Ok(()) => return Ok(()),
                Err(error) => failure = Some(error),
            }
        }
        Err(failure.expect("nonempty worker pool"))
    }

    pub async fn shutdown(&self) {
        for worker in &self.workers {
            worker.permits.close();
            worker.leaf_permits.close();
        }
        for worker in &self.workers {
            worker.stop_worker(&mut *worker.state.lock().await).await;
        }
    }

    pub async fn admit(&self, package: Vec<u8>) -> Result<AgentloopId, LoopError> {
        let _admission = self.admission.lock().await;
        let (worker, _permit) = self.select(false)?;
        worker.admit(package).await
    }

    pub async fn admit_tool(&self, component: Vec<u8>) -> Result<ToolId, LoopError> {
        let _admission = self.admission.lock().await;
        let (worker, _permit) = self.select(false)?;
        worker.admit_tool(component).await
    }

    pub async fn status(&self, digest: &AgentloopId) -> Result<bool, LoopError> {
        self.workers[0].status(digest).await
    }

    pub async fn tool_status(&self, digest: &ToolId) -> Result<bool, LoopError> {
        self.workers[0].tool_status(digest).await
    }

    pub async fn turn(
        &self,
        digest: AgentloopId,
        environment: NativeEnvironment,
        input: TurnInput,
        bridge: &dyn TurnBridge,
    ) -> Result<TurnOutput, LoopError> {
        let (worker, _permit) = self.select(!bridge.can_dispatch())?;
        worker.turn(digest, environment, input, bridge).await
    }

    pub async fn tool(
        &self,
        digest: ToolId,
        environment: NativeEnvironment,
        input: NativeToolInput,
        bridge: &dyn TurnBridge,
    ) -> Result<serde_json::Value, LoopError> {
        let (worker, _permit) = self.select(!bridge.can_dispatch())?;
        worker.tool(digest, environment, input, bridge).await
    }
}

struct WorkerSlot {
    worker_binary: PathBuf,
    socket: PathBuf,
    packages: PathBuf,
    limits: EnvLimits,
    permits: Arc<Semaphore>,
    leaf_permits: Arc<Semaphore>,
    state: Mutex<WorkerState>,
}

#[derive(Default)]
struct WorkerState {
    incarnation: u64,
    agentloops: HashSet<AgentloopId>,
    tools: HashSet<ToolId>,
    #[cfg(unix)]
    child: Option<tokio::process::Child>,
}

impl WorkerSlot {
    pub fn new(
        worker_binary: impl Into<PathBuf>,
        run_dir: impl Into<PathBuf>,
        packages: impl Into<PathBuf>,
        limits: EnvLimits,
    ) -> Self {
        let run_dir = run_dir.into();
        Self {
            worker_binary: worker_binary.into(),
            socket: run_dir.join("brain-env-worker.sock"),
            packages: packages.into(),
            // Match the worker's concurrent-turn limit exactly: an accepted connection must
            // never wait silently behind the worker's own limit and trip the liveness bound.
            permits: Arc::new(Semaphore::new(permits(limits.max_concurrent_executions))),
            leaf_permits: Arc::new(Semaphore::new(permits(limits.max_concurrent_executions))),
            limits,
            state: Mutex::new(WorkerState::default()),
        }
    }

    pub async fn admit(&self, package: Vec<u8>) -> Result<AgentloopId, LoopError> {
        if package.len() > ceiling(self.limits.max_package_bytes) {
            return Err("Agentloop package exceeds the configured admission limit".into());
        }
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        let digest = WorkerClient::new(&self.socket, &self.limits)
            .admit(&package)
            .await?;
        persist_component(&self.packages, "agentloop", digest.as_str(), &package).await?;
        state.agentloops.insert(digest.clone());
        Ok(digest)
    }

    pub async fn admit_tool(&self, component: Vec<u8>) -> Result<ToolId, LoopError> {
        if component.len() > ceiling(self.limits.max_package_bytes) {
            return Err("Tool Component exceeds the configured admission limit".into());
        }
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        let digest = WorkerClient::new(&self.socket, &self.limits)
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
        let mut state = tokio::time::timeout(std::time::Duration::from_secs(5), self.state.lock())
            .await
            .map_err(|_| LoopError::Failed("worker health lock timed out".into()))?;
        self.ensure_worker(&mut state).await?;
        let client = WorkerClient::new(&self.socket, &self.limits);
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), client.ping())
            .await
            .map_err(|_| "worker health probe timed out".to_owned())
            .and_then(|result| result);
        if let Err(error) = response {
            self.stop_worker(&mut state).await;
            return Err(error.into());
        }
        Ok(())
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
        // Everything that needs the worker's identity happens under the lock; the turn
        // itself does not. Holding it across the call would serialise every session in
        // the process onto one turn at a time, whatever the permits allowed.
        let incarnation = {
            let mut state = self.state.lock().await;
            self.ensure_worker(&mut state).await?;
            if !state.agentloops.contains(&digest) {
                let package =
                    tokio::fs::read(component_path(&self.packages, "agentloop", digest.as_str()))
                        .await
                        .map_err(|_| "Agentloop digest is not admitted".to_owned())?;
                let admitted = WorkerClient::new(&self.socket, &self.limits)
                    .admit(&package)
                    .await?;
                if admitted != digest {
                    return Err("persisted Agentloop package changed digest".into());
                }
                state.agentloops.insert(digest.clone());
            }
            state.incarnation
        };
        let client = WorkerClient::new(&self.socket, &self.limits);
        let outcome = client.turn(digest, environment, input, bridge).await;
        match outcome {
            Ok(output) => {
                let output_bytes =
                    serde_json::to_vec(&output).map_err(|error| error.to_string())?;
                if output_bytes.len() > ceiling(self.limits.max_execution_output_bytes) {
                    return Err("Agentloop turn output exceeds the configured limit".into());
                }
                Ok(output)
            }
            Err(LoopError::Failed(message)) if message == "brain-env-worker stopped answering" => {
                // The guest's own compute budget fires before this, so reaching here
                // means the worker itself is not answering. Restarting it is the only
                // thing left.
                let mut state = self.state.lock().await;
                if state.incarnation == incarnation {
                    self.stop_worker(&mut state).await;
                }
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
        let incarnation = {
            let mut state = self.state.lock().await;
            self.ensure_worker(&mut state).await?;
            if !state.tools.contains(&digest) {
                let component =
                    tokio::fs::read(component_path(&self.packages, "tool", digest.as_str()))
                        .await
                        .map_err(|_| "Tool digest is not admitted".to_owned())?;
                let admitted = WorkerClient::new(&self.socket, &self.limits)
                    .admit_tool(&component)
                    .await?;
                if admitted != digest {
                    return Err("persisted Tool Component changed digest".into());
                }
                state.tools.insert(digest.clone());
            }
            state.incarnation
        };
        let outcome = WorkerClient::new(&self.socket, &self.limits)
            .tool(digest, environment, input, bridge)
            .await;
        match outcome {
            Err(LoopError::Failed(message)) if message == "brain-env-worker stopped answering" => {
                let mut state = self.state.lock().await;
                if state.incarnation == incarnation {
                    self.stop_worker(&mut state).await;
                }
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
                .args(self.limits.args())
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|error| format!("failed to start brain-env-worker: {error}"))?;
            state.child = Some(child);
            state.incarnation = state.incarnation.wrapping_add(1);
            state.agentloops.clear();
            state.tools.clear();
            let client = WorkerClient::new(&self.socket, &self.limits);
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
                        return Err(format!("brain-env-worker exited during startup: {status}"));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), ready)
                .await
                .map_err(|_| "brain-env-worker did not become ready".to_owned())??;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    async fn ensure_worker(&self, _state: &mut WorkerState) -> Result<(), String> {
        let _ = &self.worker_binary;
        Err("brain-env-worker requires a Unix server".into())
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
            "brain-env-tool-status-{}-{suffix}",
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
            EnvLimits::default(),
            std::num::NonZeroUsize::new(1).unwrap(),
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
