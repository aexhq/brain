use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use brain_protocol::{AgentloopId, ToolId, TurnError, TurnInput, TurnOutput};

use crate::wire::{max_request_bytes, read_frame, write_frame};
use crate::{
    ComponentKind, EnvLimits, HostCall, LoopError, NativeEnvironment, NativeToolInput,
    WorkerRequest, WorkerResponse,
};

const WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(125);
const WORKER_PROBE_INTERVAL: Duration = Duration::from_secs(5);
const WORKER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    struct Quiet;
    #[async_trait]
    impl TurnBridge for Quiet {
        async fn call(&self, _: HostCall) -> Result<String, TurnError> {
            panic!("quiet invocation made a host call")
        }
        fn cancelled(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn a_quiet_invocation_remains_live_while_its_worker_answers_health_probes() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("healthy");
        let mut listener = crate::socket::listen(&socket).unwrap();
        let worker = tokio::spawn(async move {
            let mut invocation = listener.accept().await.unwrap();
            assert!(matches!(
                read_frame::<_, WorkerRequest>(&mut invocation, 4096)
                    .await
                    .unwrap(),
                WorkerRequest::Execute { .. }
            ));
            for _ in 0..3 {
                let mut probe = listener.accept().await.unwrap();
                assert!(matches!(
                    read_frame::<_, WorkerRequest>(&mut probe, 1024)
                        .await
                        .unwrap(),
                    WorkerRequest::Ping
                ));
                write_frame(&mut probe, &WorkerResponse::Pong, 1024)
                    .await
                    .unwrap();
            }
            write_frame(
                &mut invocation,
                &WorkerResponse::Completed {
                    output: serde_json::json!("finished"),
                },
                1024,
            )
            .await
            .unwrap();
        });
        let client = WorkerClient::new(socket, &EnvLimits::default());
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            client.execute(
                ComponentKind::Tool,
                "quiet".into(),
                NativeEnvironment::default(),
                serde_json::json!({}),
                &Quiet,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(output, serde_json::json!("finished"));
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn an_unanswered_health_probe_identifies_a_stalled_worker() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("stalled");
        let mut listener = crate::socket::listen(&socket).unwrap();
        let worker = tokio::spawn(async move {
            let mut invocation = listener.accept().await.unwrap();
            assert!(matches!(
                read_frame::<_, WorkerRequest>(&mut invocation, 4096)
                    .await
                    .unwrap(),
                WorkerRequest::Execute { .. }
            ));
            let mut probe = listener.accept().await.unwrap();
            assert!(matches!(
                read_frame::<_, WorkerRequest>(&mut probe, 1024)
                    .await
                    .unwrap(),
                WorkerRequest::Ping
            ));
            std::future::pending::<()>().await;
        });
        let client = WorkerClient::new(socket, &EnvLimits::default());
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            client.execute(
                ComponentKind::Tool,
                "stalled".into(),
                NativeEnvironment::default(),
                serde_json::json!({}),
                &Quiet,
            ),
        )
        .await
        .unwrap();
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("stopped answering")
        );
        worker.abort();
    }
}

/// The server's side of a turn: what answers the guest's host calls, and whether the
/// turn has been cancelled.
#[async_trait]
pub trait TurnBridge: Send + Sync {
    async fn call(&self, call: HostCall) -> Result<String, TurnError>;
    fn cancelled(&self) -> bool;
    fn can_dispatch(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
pub struct WorkerClient {
    socket: PathBuf,
    limits: EnvLimits,
}

impl WorkerClient {
    pub fn new(socket: impl Into<PathBuf>, limits: &EnvLimits) -> Self {
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

    pub async fn tool(
        &self,
        digest: ToolId,
        environment: NativeEnvironment,
        input: NativeToolInput,
        bridge: &dyn TurnBridge,
    ) -> Result<serde_json::Value, LoopError> {
        self.execute(
            ComponentKind::Tool,
            digest.as_str().into(),
            environment,
            serde_json::to_value(input).map_err(|e| e.to_string())?,
            bridge,
        )
        .await
    }

    pub async fn turn(
        &self,
        digest: AgentloopId,
        environment: NativeEnvironment,
        input: TurnInput,
        bridge: &dyn TurnBridge,
    ) -> Result<TurnOutput, LoopError> {
        let output = self
            .execute(
                ComponentKind::Agentloop,
                digest.as_str().into(),
                environment,
                serde_json::to_value(input).map_err(|e| e.to_string())?,
                bridge,
            )
            .await?;
        serde_json::from_value(output).map_err(|e| LoopError::Failed(e.to_string()))
    }

    /// Worker health is independent of invocation progress. Cancellation uses the
    /// same connection without dropping a partially read response frame.
    async fn execute(
        &self,
        kind: ComponentKind,
        digest: String,
        environment: NativeEnvironment,
        input: serde_json::Value,
        bridge: &dyn TurnBridge,
    ) -> Result<serde_json::Value, LoopError> {
        let mut stream = crate::socket::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        write_frame(
            &mut stream,
            &WorkerRequest::Execute {
                kind,
                digest,
                environment,
                input,
                can_dispatch: bridge.can_dispatch(),
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
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut cancelled = false;
        let mut poll = tokio::time::interval(std::time::Duration::from_millis(50));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let health = async {
            loop {
                tokio::time::sleep(WORKER_PROBE_INTERVAL).await;
                if !matches!(
                    tokio::time::timeout(WORKER_PROBE_TIMEOUT, self.ping()).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        };
        tokio::pin!(health);
        loop {
            // One read future lives across the poll ticks. Dropping a half-read frame
            // would leave the stream mid-frame, and the next length prefix would be
            // whatever bytes came next.
            let mut next = std::pin::pin!(read_frame::<_, WorkerResponse>(
                &mut reader,
                self.limits.max_response_frame_bytes()
            ));
            let response = loop {
                tokio::select! {
                    frame = &mut next => break frame?,
                    () = &mut health => return Err("brain-env-worker stopped answering".into()),
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
                WorkerResponse::Completed { output } => return Ok(output),
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

    async fn call(&self, request: WorkerRequest) -> Result<WorkerResponse, String> {
        let max = max_request_bytes(&request, &self.limits);
        let response = async {
            let mut stream = crate::socket::connect(&self.socket)
                .await
                .map_err(|error| error.to_string())?;
            write_frame(&mut stream, &request, max).await?;
            read_frame(&mut stream, self.limits.max_response_frame_bytes()).await
        };
        tokio::time::timeout(WORKER_REQUEST_TIMEOUT, response)
            .await
            .map_err(|_| "brain-env-worker stopped answering".to_owned())?
    }
}
