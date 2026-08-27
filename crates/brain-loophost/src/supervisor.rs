use std::{collections::HashSet, path::PathBuf, sync::Arc};

use brain_protocol::{ActivationInput, ActivationOutput, AgentloopDigest};
use tokio::sync::{Mutex, Semaphore};

use crate::{AgentloopPackage, LoopLimits, WorkerClient};

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
    admitted: HashSet<AgentloopDigest>,
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
            permits: Arc::new(Semaphore::new(
                limits.queued_activations_per_worker.saturating_add(1),
            )),
            limits,
            state: Mutex::new(WorkerState::default()),
        }
    }

    pub async fn admit(&self, package: Vec<u8>) -> Result<AgentloopDigest, String> {
        if package.len() > self.limits.package_bytes {
            return Err("Agentloop package exceeds the configured admission limit".into());
        }
        AgentloopPackage::decode(&package)?;
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Loophost queue is full".to_owned())?;
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        let digest = WorkerClient::new(&self.socket).admit(&package).await?;
        persist_package(&self.packages, &digest, &package).await?;
        state.admitted.insert(digest.clone());
        Ok(digest)
    }

    pub async fn status(&self, digest: &AgentloopDigest) -> Result<bool, String> {
        tokio::fs::try_exists(package_path(&self.packages, digest))
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn ready(&self) -> Result<(), String> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Loophost queue is full".to_owned())?;
        let mut state = self.state.lock().await;
        self.ensure_worker(&mut state).await?;
        WorkerClient::new(&self.socket).ping().await
    }

    pub async fn activate(
        &self,
        digest: AgentloopDigest,
        input: ActivationInput,
    ) -> Result<ActivationOutput, String> {
        let input_bytes = serde_json::to_vec(&input).map_err(|error| error.to_string())?;
        if input_bytes.len() > self.limits.activation_input_bytes {
            return Err("Agentloop activation input exceeds the configured limit".into());
        }
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Loophost queue is full".to_owned())?;
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
        let client = WorkerClient::new(&self.socket);
        let call = client.activate(digest, input);
        match tokio::time::timeout(self.limits.wall_time, call).await {
            Ok(Ok(output)) => {
                let output_bytes =
                    serde_json::to_vec(&output).map_err(|error| error.to_string())?;
                if output_bytes.len() > self.limits.activation_output_bytes {
                    self.stop_worker(&mut state).await;
                    return Err("Agentloop activation output exceeds the configured limit".into());
                }
                Ok(output)
            }
            Ok(Err(error)) => Err(error),
            Err(_) => {
                self.stop_worker(&mut state).await;
                Err("Agentloop activation exceeded its wall-time limit".into())
            }
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
    digest: &AgentloopDigest,
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

fn package_path(directory: &std::path::Path, digest: &AgentloopDigest) -> PathBuf {
    directory.join(format!("{}.json", digest.as_str()))
}
