use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use brain_protocol::{AgentloopIdentity, TurnError, TurnInput, TurnOutput};

#[cfg(unix)]
use crate::wire::{MAX_RESPONSE_FRAME_BYTES, max_request_bytes, read_frame, write_frame};
use crate::{HostCall, LoopError, WorkerRequest, WorkerResponse};

/// The server's side of a turn: what answers the guest's host calls, and whether the
/// turn has been cancelled.
#[async_trait]
pub trait TurnBridge: Send + Sync {
    async fn call(&self, call: HostCall) -> Result<String, TurnError>;
    fn cancelled(&self) -> bool;
}

#[derive(Clone, Debug)]
pub struct WorkerClient {
    socket: PathBuf,
}

impl WorkerClient {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub async fn ping(&self) -> Result<(), String> {
        match self.call(WorkerRequest::Ping).await? {
            WorkerResponse::Pong => Ok(()),
            response => Err(format!("unexpected worker response: {response:?}")),
        }
    }

    pub async fn admit(&self, package: &[u8]) -> Result<AgentloopIdentity, String> {
        let package_json = String::from_utf8(package.to_vec())
            .map_err(|_| "Agentloop package must be UTF-8 JSON".to_owned())?;
        match self.call(WorkerRequest::Admit { package_json }).await? {
            WorkerResponse::Admitted { digest } => Ok(digest),
            WorkerResponse::Error { code, message } => Err(format!("{code}: {message}")),
            response => Err(format!("unexpected worker response: {response:?}")),
        }
    }

    /// Runs one turn on its own connection, answering the guest's host calls through
    /// `bridge` as they arrive. `liveness` bounds how long the worker may go without a
    /// frame while the guest is computing; a bridge call in flight is the server's own
    /// time and is not counted.
    #[cfg(unix)]
    pub async fn turn(
        &self,
        session: String,
        digest: AgentloopIdentity,
        input: TurnInput,
        max_input_bytes: usize,
        bridge: &dyn TurnBridge,
        liveness: Duration,
    ) -> Result<TurnOutput, LoopError> {
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        write_frame(
            &mut stream,
            &WorkerRequest::Turn {
                digest,
                session,
                input: Box::new(input),
            },
            max_input_bytes + 1_024,
        )
        .await
        .map_err(|error| {
            if error.starts_with("worker frame exceeds") {
                "Agentloop turn input exceeds the configured limit".to_owned()
            } else {
                error
            }
        })?;
        let mut cancelled = false;
        let mut poll = tokio::time::interval(Duration::from_millis(50));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let response: WorkerResponse = tokio::select! {
                frame = tokio::time::timeout(liveness, read_frame(&mut stream, MAX_RESPONSE_FRAME_BYTES)) => {
                    match frame {
                        Ok(frame) => frame?,
                        Err(_) => return Err("brain-loop-worker stopped answering".into()),
                    }
                }
                _ = poll.tick() => {
                    if !cancelled && bridge.cancelled() {
                        cancelled = true;
                        write_frame(&mut stream, &WorkerRequest::Cancel, 1_024).await?;
                    }
                    continue;
                }
            };
            match response {
                WorkerResponse::HostCall { id, call } => {
                    let result = bridge.call(call).await;
                    if !cancelled && bridge.cancelled() {
                        cancelled = true;
                        write_frame(&mut stream, &WorkerRequest::Cancel, 1_024).await?;
                    }
                    write_frame(
                        &mut stream,
                        &WorkerRequest::HostResult { id, result },
                        MAX_RESPONSE_FRAME_BYTES,
                    )
                    .await?;
                }
                WorkerResponse::Turned { output } => return Ok(output),
                WorkerResponse::TurnFailed { error } => return Err(LoopError::Turn(error)),
                WorkerResponse::Error { code, message } => {
                    return Err(format!("{code}: {message}").into());
                }
                response => {
                    return Err(format!("unexpected worker response: {response:?}").into());
                }
            }
        }
    }

    #[cfg(not(unix))]
    pub async fn turn(
        &self,
        _session: String,
        _digest: AgentloopIdentity,
        _input: TurnInput,
        _max_input_bytes: usize,
        _bridge: &dyn TurnBridge,
        _liveness: Duration,
    ) -> Result<TurnOutput, LoopError> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }

    #[cfg(unix)]
    async fn call(&self, request: WorkerRequest) -> Result<WorkerResponse, String> {
        let max = max_request_bytes(&request);
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        write_frame(&mut stream, &request, max).await?;
        read_frame(&mut stream, MAX_RESPONSE_FRAME_BYTES).await
    }

    #[cfg(not(unix))]
    async fn call(&self, _request: WorkerRequest) -> Result<WorkerResponse, String> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }
}
