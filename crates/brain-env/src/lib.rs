//! Native Environment adapter, Component admission, and worker process pool.

mod client;
mod environment;
pub use environment::{BrainEnvironment, NativePolicy};
mod limits;
mod runtime;
mod service;
mod supervisor;
mod wire;

pub use client::{TurnBridge, WorkerClient};
pub use limits::{EnvLimits, WorkerArgs};
pub use runtime::{AdmissionEngine, AdmittedAgentloop, AdmittedTool, GuestHost, NativeToolInput};
pub use service::WorkerService;
pub use supervisor::{LoopError, WorkerPool};
pub use wire::{
    Access, ComponentKind, HostCall, NativeEnvironment, WorkerRequest, WorkerResponse, Workspace,
    network_covers,
};

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

#[doc(hidden)]
pub async fn worker_read<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    limits: &EnvLimits,
) -> Result<WorkerRequest, String> {
    wire::read_frame(reader, limits.max_request_frame_bytes()).await
}

#[doc(hidden)]
pub async fn worker_write<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    response: &WorkerResponse,
    limits: &EnvLimits,
) -> Result<(), String> {
    wire::write_frame(writer, response, limits.max_response_frame_bytes()).await
}
