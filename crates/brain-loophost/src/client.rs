use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use brain_protocol::{AgentloopIdentity, ToolIdentity, TurnError, TurnInput, TurnOutput};

#[cfg(unix)]
use crate::wire::{MAX_RESPONSE_FRAME_BYTES, max_request_bytes, read_frame, write_frame};
use crate::{
    ComponentKind, HostCall, LoopError, NativeEnvironment, NativeToolInput, WorkerRequest,
    WorkerResponse,
};

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
        self.admit_as(package, ComponentKind::Agentloop)
            .await
            .map(AgentloopIdentity::new)
    }

    pub async fn admit_tool(&self, component: &[u8]) -> Result<ToolIdentity, String> {
        self.admit_as(component, ComponentKind::Tool)
            .await
            .map(ToolIdentity::new)
    }

    async fn admit_as(&self, package: &[u8], kind: ComponentKind) -> Result<String, String> {
        use base64::Engine as _;
        let component_base64 = base64::engine::general_purpose::STANDARD.encode(package);
        match self
            .call(WorkerRequest::Admit {
                kind,
                component_base64,
            })
            .await?
        {
            WorkerResponse::Admitted { digest } => Ok(digest),
            WorkerResponse::Error { code, message } => Err(format!("{code}: {message}")),
            response => Err(format!("unexpected worker response: {response:?}")),
        }
    }

    #[cfg(unix)]
    pub async fn tool(
        &self,
        digest: ToolIdentity,
        environment: NativeEnvironment,
        input: NativeToolInput,
        bridge: &dyn TurnBridge,
        liveness: Duration,
    ) -> Result<serde_json::Value, LoopError> {
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        write_frame(
            &mut stream,
            &WorkerRequest::Tool {
                digest,
                environment,
                call_id: input.call_id,
                input: input.input,
                configuration: input.configuration,
                deadline_at_ms: input.deadline_at_ms,
            },
            crate::MAX_TURN_INPUT_BYTES + 1_024,
        )
        .await?;
        let (mut reader, mut writer) = stream.split();
        loop {
            let response = tokio::time::timeout(
                liveness,
                read_frame::<_, WorkerResponse>(&mut reader, MAX_RESPONSE_FRAME_BYTES),
            )
            .await
            .map_err(|_| LoopError::Failed("brain-loop-worker stopped answering".into()))??;
            match response {
                WorkerResponse::HostCall { id, call } => {
                    let result = bridge.call(call).await;
                    write_frame(
                        &mut writer,
                        &WorkerRequest::HostResult { id, result },
                        MAX_RESPONSE_FRAME_BYTES,
                    )
                    .await?;
                }
                WorkerResponse::ToolRan { output } => return Ok(output),
                WorkerResponse::TurnFailed { error } => return Err(LoopError::Turn(error)),
                WorkerResponse::Error { code, message } => {
                    return Err(format!("{code}: {message}").into());
                }
                response => return Err(format!("unexpected worker response: {response:?}").into()),
            }
        }
    }

    #[cfg(not(unix))]
    pub async fn tool(
        &self,
        _digest: ToolIdentity,
        _environment: NativeEnvironment,
        _input: NativeToolInput,
        _bridge: &dyn TurnBridge,
        _liveness: Duration,
    ) -> Result<serde_json::Value, LoopError> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }

    /// Runs one turn on its own connection, answering the guest's host calls through
    /// `bridge` as they arrive. `liveness` bounds how long the worker may go without a
    /// frame while the guest is computing; a bridge call in flight is the server's own
    /// time and is not counted.
    #[cfg(unix)]
    pub async fn turn(
        &self,
        digest: AgentloopIdentity,
        environment: NativeEnvironment,
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
                environment,
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
        let (mut reader, mut writer) = stream.split();
        let mut cancelled = false;
        let mut poll = tokio::time::interval(Duration::from_millis(50));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // One read future lives across the poll ticks. Dropping a half-read frame
            // would leave the stream mid-frame, and the next length prefix would be
            // whatever bytes came next.
            let mut next = std::pin::pin!(tokio::time::timeout(
                liveness,
                read_frame::<_, WorkerResponse>(&mut reader, MAX_RESPONSE_FRAME_BYTES)
            ));
            let response = loop {
                tokio::select! {
                    frame = &mut next => match frame {
                        Ok(frame) => break frame?,
                        Err(_) => return Err("brain-loop-worker stopped answering".into()),
                    },
                    _ = poll.tick() => {
                        if !cancelled && bridge.cancelled() {
                            cancelled = true;
                            write_frame(&mut writer, &WorkerRequest::Cancel, 1_024).await?;
                        }
                    }
                }
            };
            match response {
                WorkerResponse::HostCall { id, call } => {
                    if !cancelled && bridge.cancelled() {
                        cancelled = true;
                        write_frame(&mut writer, &WorkerRequest::Cancel, 1_024).await?;
                    }
                    if cancelled {
                        continue;
                    }
                    let result = bridge.call(call).await;
                    if bridge.cancelled() {
                        cancelled = true;
                        write_frame(&mut writer, &WorkerRequest::Cancel, 1_024).await?;
                        continue;
                    }
                    write_frame(
                        &mut writer,
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
        _digest: AgentloopIdentity,
        _environment: NativeEnvironment,
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
