//! What crosses between the server and the worker process.
//!
//! A ping or an admission is one request and one answer on a connection. A turn holds
//! its connection open: the server sends the turn, the worker sends back every host
//! call the guest makes and waits for its result, and the connection ends with the
//! turn's output or its error. The server may send a cancel at any point; the worker
//! fails every pending host call with it and the guest's next call sees it.

use brain_protocol::{AgentloopId, ToolId, TurnError, TurnInput, TurnOutput};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::MAX_TURN_OUTPUT_BYTES;
#[cfg(unix)]
use crate::{MAX_PACKAGE_BYTES, MAX_TURN_INPUT_BYTES};

/// What the guest asks Brain to do. Every payload is JSON in the shapes the
/// `contracts/session/v1` types define.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostCall {
    Events { after: u64 },
    Model { request_json: String },
    Dispatch { calls_json: String },
    Emit { kind: String, payload_json: String },
    Telemetry { record_json: String },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Ping,
    Admit {
        kind: ComponentKind,
        component_base64: String,
    },
    Turn {
        digest: AgentloopId,
        environment: NativeEnvironment,
        input: Box<TurnInput>,
    },
    Tool {
        digest: ToolId,
        environment: NativeEnvironment,
        input: serde_json::Value,
        configuration: serde_json::Value,
        deadline_at_ms: u64,
    },
    /// The answer to a host call the worker sent on this connection.
    HostResult {
        id: u64,
        result: Result<String, TurnError>,
    },
    /// Stop the turn on this connection: pending host calls fail with `cancelled`, and so
    /// does the guest's next one.
    Cancel,
}

/// What one invocation is granted: computed from what it declared it needs, bounded
/// by the deployment's allow-lists, and nothing else.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NativeEnvironment {
    /// A directory that lives for this invocation, at `/scratch`.
    pub scratch: Option<Access>,
    /// The session's directory, at `/workspace`.
    pub workspace: Option<Workspace>,
    /// Origins the guest may reach over `wasi:http`, exact or `scheme://*.domain`.
    pub network_allow: Vec<String>,
    /// Files under `/secrets`, by name.
    pub secrets: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Workspace {
    pub path: String,
    pub access: Access,
}

/// Whether `grant` covers `target`, both `scheme://authority`. An exact grant covers
/// that origin; `scheme://*.domain` covers every host below `domain`, as a Content
/// Security Policy source does, and so covers a narrower family too.
pub fn network_covers(grant: &str, target: &str) -> bool {
    let (Some((grant_scheme, grant_authority)), Some((target_scheme, target_authority))) =
        (split_origin(grant), split_origin(target))
    else {
        return false;
    };
    if !grant_scheme.eq_ignore_ascii_case(target_scheme) {
        return false;
    }
    let grant_authority = grant_authority.to_ascii_lowercase();
    let target_authority = target_authority.to_ascii_lowercase();
    match grant_authority.strip_prefix("*.") {
        None => grant_authority == target_authority,
        Some(domain) => {
            let below = format!(".{domain}");
            match target_authority.strip_prefix("*.") {
                Some(target_domain) => target_domain == domain || target_domain.ends_with(&below),
                None => target_authority.ends_with(&below),
            }
        }
    }
}

fn split_origin(origin: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = origin.split_once("://")?;
    let authority = rest.trim_end_matches('/');
    (!scheme.is_empty() && !authority.is_empty() && !authority.contains('/'))
        .then_some((scheme, authority))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Agentloop,
    Tool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerResponse {
    Pong,
    Admitted {
        digest: String,
    },
    /// The guest asked for something; the server answers with `HostResult` under the
    /// same id.
    HostCall {
        id: u64,
        call: HostCall,
    },
    Turned {
        output: TurnOutput,
    },
    ToolRan {
        output: serde_json::Value,
    },
    /// The turn ran and failed, with the code the loop or the runtime gave it.
    TurnFailed {
        error: TurnError,
    },
    Error {
        code: String,
        message: String,
    },
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
        WorkerRequest::Ping | WorkerRequest::Cancel => 1_024,
        WorkerRequest::Admit { .. } => MAX_PACKAGE_BYTES * 2,
        WorkerRequest::Turn { .. }
        | WorkerRequest::Tool { .. }
        | WorkerRequest::HostResult { .. } => MAX_TURN_INPUT_BYTES + 1_024,
    }
}

pub(crate) const MAX_RESPONSE_FRAME_BYTES: usize = MAX_TURN_OUTPUT_BYTES + 1_024;
