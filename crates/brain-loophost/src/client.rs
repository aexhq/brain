use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

use async_trait::async_trait;
use brain_protocol::{AgentloopId, ToolId, TurnError, TurnInput, TurnOutput};

#[cfg(unix)]
use crate::wire::{max_request_bytes, read_frame, write_frame};
use crate::{
    ComponentKind, HostCall, LoopError, LoopLimits, NativeEnvironment, NativeToolInput,
    WorkerRequest, WorkerResponse,
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
    #[cfg_attr(not(unix), allow(dead_code))]
    limits: LoopLimits,
}

impl WorkerClient {
    pub fn new(socket: impl Into<PathBuf>, limits: &LoopLimits) -> Self {
        Self {
            socket: socket.into(),
            limits: limits.clone(),
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

    pub async fn admit(&self, package: &[u8]) -> Result<AgentloopId, String> {
        self.admit_as(package, ComponentKind::Agentloop)
            .await
            .map(AgentloopId::new)
    }

    pub async fn admit_tool(&self, component: &[u8]) -> Result<ToolId, String> {
        self.admit_as(component, ComponentKind::Tool)
            .await
            .map(ToolId::new)
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
        digest: ToolId,
        environment: NativeEnvironment,
        input: NativeToolInput,
        bridge: &dyn TurnBridge,
    ) -> Result<serde_json::Value, LoopError> {
        let liveness = self.limits.worker_liveness();
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        write_frame(
            &mut stream,
            &WorkerRequest::Tool {
                digest,
                environment,
                input: input.input,
                configuration: input.configuration,
                deadline_at_ms: input.deadline_at_ms,
            },
            self.limits.max_turn_frame_bytes(),
        )
        .await?;
        let (mut reader, mut writer) = stream.split();
        loop {
            let response = tokio::time::timeout(
                liveness,
                read_frame::<_, WorkerResponse>(
                    &mut reader,
                    self.limits.max_response_frame_bytes(),
                ),
            )
            .await
            .map_err(|_| LoopError::Failed("brain-loop-worker stopped answering".into()))??;
            match response {
                WorkerResponse::HostCall { id, call } => {
                    let result = bridge.call(call).await;
                    write_frame(
                        &mut writer,
                        &WorkerRequest::HostResult { id, result },
                        self.limits.max_response_frame_bytes(),
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
        _digest: ToolId,
        _environment: NativeEnvironment,
        _input: NativeToolInput,
        _bridge: &dyn TurnBridge,
    ) -> Result<serde_json::Value, LoopError> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }

    /// Runs one turn on its own connection, answering the guest's host calls through
    /// `bridge` as they arrive. The worker liveness bound limits how long the worker may
    /// go without a frame while the guest is computing; a bridge call in flight is the
    /// server's own time and is not counted.
    #[cfg(unix)]
    pub async fn turn(
        &self,
        digest: AgentloopId,
        environment: NativeEnvironment,
        input: TurnInput,
        bridge: &dyn TurnBridge,
    ) -> Result<TurnOutput, LoopError> {
        let liveness = self.limits.worker_liveness();
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
            self.limits.max_turn_frame_bytes(),
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
                read_frame::<_, WorkerResponse>(
                    &mut reader,
                    self.limits.max_response_frame_bytes()
                )
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
                        self.limits.max_response_frame_bytes(),
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
        _digest: AgentloopId,
        _environment: NativeEnvironment,
        _input: TurnInput,
        _bridge: &dyn TurnBridge,
    ) -> Result<TurnOutput, LoopError> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }

    #[cfg(unix)]
    async fn call(&self, request: WorkerRequest) -> Result<WorkerResponse, String> {
        let max = max_request_bytes(&request, &self.limits);
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        write_frame(&mut stream, &request, max).await?;
        read_frame(&mut stream, self.limits.max_response_frame_bytes()).await
    }

    #[cfg(not(unix))]
    async fn call(&self, _request: WorkerRequest) -> Result<WorkerResponse, String> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }
}
