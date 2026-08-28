use std::path::{Path, PathBuf};

use brain_protocol::{ActivationInput, ActivationOutput, AgentloopIdentity};

#[cfg(unix)]
use crate::wire::{MAX_RESPONSE_FRAME_BYTES, max_request_bytes, read_frame, write_frame};
use crate::{WorkerRequest, WorkerResponse};

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

    pub async fn activate(
        &self,
        digest: AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, String> {
        match self
            .call(WorkerRequest::Activate {
                digest,
                input: Box::new(input),
            })
            .await?
        {
            WorkerResponse::Activated { output } => Ok(output),
            WorkerResponse::Error { code, message } => Err(format!("{code}: {message}")),
            response => Err(format!("unexpected worker response: {response:?}")),
        }
    }

    #[cfg(unix)]
    async fn call(&self, request: WorkerRequest) -> Result<WorkerResponse, String> {
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|error| error.to_string())?;
        let max = max_request_bytes(&request);
        write_frame(&mut stream, &request, max).await?;
        read_frame(&mut stream, MAX_RESPONSE_FRAME_BYTES).await
    }

    #[cfg(not(unix))]
    async fn call(&self, _request: WorkerRequest) -> Result<WorkerResponse, String> {
        Err("brain-loop-worker IPC requires Unix domain sockets".into())
    }
}
