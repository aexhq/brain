use brain_protocol::{ActivationInput, ActivationOutput, AgentloopIdentity};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::MAX_ACTIVATION_OUTPUT_BYTES;
#[cfg(unix)]
use crate::{MAX_ACTIVATION_INPUT_BYTES, MAX_PACKAGE_BYTES};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Ping,
    Admit {
        package_json: String,
    },
    Activate {
        digest: AgentloopIdentity,
        input: Box<ActivationInput>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerResponse {
    Pong,
    Admitted { digest: AgentloopIdentity },
    Activated { output: ActivationOutput },
    Error { code: String, message: String },
}

pub(crate) async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
    max_bytes: usize,
) -> Result<(), String> {
    let payload = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if payload.len() > max_bytes {
        return Err(format!("worker frame exceeds {max_bytes} bytes"));
    }
    let length =
        u32::try_from(payload.len()).map_err(|_| "worker frame length overflow".to_owned())?;
    writer
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|error| error.to_string())?;
    writer
        .write_all(&payload)
        .await
        .map_err(|error| error.to_string())?;
    writer.flush().await.map_err(|error| error.to_string())
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<T, String> {
    let length = reader.read_u32().await.map_err(|error| error.to_string())? as usize;
    if length > max_bytes {
        return Err(format!("worker frame exceeds {max_bytes} bytes"));
    }
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&payload).map_err(|error| error.to_string())
}

#[cfg(unix)]
pub(crate) fn max_request_bytes(request: &WorkerRequest) -> usize {
    match request {
        WorkerRequest::Ping => 1_024,
        WorkerRequest::Admit { .. } => MAX_PACKAGE_BYTES + 1_024,
        WorkerRequest::Activate { .. } => MAX_ACTIVATION_INPUT_BYTES + 1_024,
    }
}

pub(crate) const MAX_RESPONSE_FRAME_BYTES: usize = MAX_ACTIVATION_OUTPUT_BYTES + 1_024;
