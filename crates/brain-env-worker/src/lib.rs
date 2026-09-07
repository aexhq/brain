//! The Wasmtime host: Component admission, guest execution, and the worker service that
//! answers Brain over the socket in `brain-env`.
//!
//! This crate is the only place a Wasm engine is linked. `brain-env` supervises the process
//! it produces and never compiles or instantiates a guest itself.

use brain_env::{EnvLimits, WorkerRequest, WorkerResponse};

mod runtime;
mod service;

pub use runtime::{AdmissionEngine, AdmittedAgentloop, AdmittedTool, GuestHost};
pub use service::WorkerService;

/// The interfaces a guest may import: the contract's own types and the host services.
pub const RUNTIME_SHIM_IMPORTS: &[&str] =
    &["brain:agentloop/types@0.1.0", "brain:agentloop/host@0.1.0"];
pub const CAPABILITY_IMPORTS: &[&str] = &[
    "wasi:cli/environment@0.2.9",
    "wasi:cli/exit@0.2.9",
    "wasi:cli/stderr@0.2.9",
    "wasi:cli/stdin@0.2.9",
    "wasi:cli/stdout@0.2.9",
    "wasi:cli/terminal-input@0.2.9",
    "wasi:cli/terminal-output@0.2.9",
    "wasi:cli/terminal-stderr@0.2.9",
    "wasi:cli/terminal-stdin@0.2.9",
    "wasi:cli/terminal-stdout@0.2.9",
    "wasi:clocks/monotonic-clock@0.2.9",
    "wasi:clocks/wall-clock@0.2.9",
    "wasi:filesystem/types@0.2.9",
    "wasi:filesystem/preopens@0.2.9",
    "wasi:http/types@0.2.9",
    "wasi:http/outgoing-handler@0.2.9",
    "wasi:io/error@0.2.9",
    "wasi:io/poll@0.2.9",
    "wasi:io/streams@0.2.9",
    "wasi:filesystem/types@0.2.12",
    "wasi:filesystem/preopens@0.2.12",
    "wasi:io/error@0.2.12",
    "wasi:io/poll@0.2.12",
    "wasi:io/streams@0.2.12",
    "wasi:http/types@0.2.12",
    "wasi:http/outgoing-handler@0.2.12",
];
pub const TOOL_IMPORTS: &[&str] = &["brain:tool/types@0.1.0", "brain:tool/host@0.1.0"];

/// Read one request frame from Brain, bounded by the request ceiling.
pub async fn worker_read<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    limits: &EnvLimits,
) -> Result<WorkerRequest, String> {
    brain_env::read_frame(reader, limits.max_request_frame_bytes()).await
}

/// Write one response frame to Brain, bounded by the response ceiling.
pub async fn worker_write<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    response: &WorkerResponse,
    limits: &EnvLimits,
) -> Result<(), String> {
    brain_env::write_frame(writer, response, limits.max_response_frame_bytes()).await
}
