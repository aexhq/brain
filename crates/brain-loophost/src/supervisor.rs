use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use brain_protocol::{AgentloopIdentity, TurnError, TurnInput, TurnOutput};
use tokio::sync::{Mutex, Semaphore};

use crate::{AgentloopPackage, LoopLimits, TurnBridge, WorkerClient};

/// How long the worker may go without a frame while the guest is computing before the
/// supervisor gives up on it. Strictly more than the guest's own compute budget, which
/// stops a runaway guest first and costs one instance; this stops a worker that is not
/// answering at all and costs every warm instance in it.
const WORKER_BACKSTOP: Duration = Duration::from_secs(1);

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
            LoopError::Overloaded => formatter.write_str("Loophost queue is full"),
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
    state: Mutex<WorkerState>,
}

#[derive(Default)]
struct WorkerState {
    admitted: HashSet<AgentloopIdentity>,
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
            // Admission, not execution: what runs at once is bounded inside the worker,
            // where the instances actually live. This is how many callers may be in
            // flight or waiting for one of those slots before the pool refuses.
            permits: Arc::new(Semaphore::new(
                limits
                    .concurrent_turns_per_worker
                    .saturating_add(limits.queued_turns_per_worker)
                    .max(1),
            )),
            limits,
            state: Mutex::new(WorkerState::default()),
        }
    }

    pub async fn admit(&self, package: Vec<u8>) -> Result<AgentloopIdentity, LoopError> {
        if package.len() > self.limits.package_bytes {
            return Err("Agentloop package exceeds the configured admission limit".into());
        }
        AgentloopPackage::decode(&package)?;
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LoopError::Overloaded)?;
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        let digest = WorkerClient::new(&self.socket).admit(&package).await?;
        persist_package(&self.packages, &digest, &package).await?;
        state.admitted.insert(digest.clone());
        Ok(digest)
    }

    pub async fn status(&self, digest: &AgentloopIdentity) -> Result<bool, LoopError> {
        tokio::fs::try_exists(package_path(&self.packages, digest))
            .await
            .map_err(|error| LoopError::Failed(error.to_string()))
    }

    pub async fn ready(&self) -> Result<(), LoopError> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| LoopError::Overloaded)?;
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        Ok(WorkerClient::new(&self.socket).ping().await?)
    }

    /// Runs one turn. The bridge answers the guest's host calls for as long as the turn
    /// runs; a worker that stops answering between them is restarted.
    pub async fn turn(
        &self,
        session: String,
        digest: AgentloopIdentity,
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
            if !state.admitted.contains(&digest) {
                let package = tokio::fs::read(package_path(&self.packages, &digest))
                    .await
                    .map_err(|_| "Agentloop digest is not admitted".to_owned())?;
                let admitted = WorkerClient::new(&self.socket).admit(&package).await?;
                if admitted != digest {
                    return Err("persisted Agentloop package changed digest".into());
                }
                state.admitted.insert(digest.clone());
            }
        }
        let client = WorkerClient::new(&self.socket);
        let outcome = client
            .turn(
                session,
                digest,
                input,
                self.limits.turn_input_bytes,
                bridge,
                self.limits.wall_time + WORKER_BACKSTOP,
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
            }
            let _ = tokio::fs::remove_file(&self.socket).await;
            let child = tokio::process::Command::new(&self.worker_binary)
                .arg(&self.socket)
                .kill_on_drop(true)
                .spawn()
                .map_err(|error| format!("failed to start brain-loop-worker: {error}"))?;
            state.child = Some(child);
            state.admitted.clear();
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
        state.admitted.clear();
        let _ = tokio::fs::remove_file(&self.socket).await;
    }

    #[cfg(not(unix))]
    async fn stop_worker(&self, state: &mut WorkerState) {
        state.admitted.clear();
    }
}

async fn persist_package(
    directory: &std::path::Path,
    digest: &AgentloopIdentity,
    package: &[u8],
) -> Result<(), String> {
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|error| error.to_string())?;
    let target = package_path(directory, digest);
    let temporary = directory.join(format!(".{}.tmp", digest.as_str()));
    tokio::fs::write(&temporary, package)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::rename(&temporary, &target)
        .await
        .map_err(|error| error.to_string())
}

fn package_path(directory: &std::path::Path, digest: &AgentloopIdentity) -> PathBuf {
    directory.join(format!("{}.json", digest.as_str()))
}
