//! Admission and worker-client boundary for the single Agentloop extension pipeline.

mod client;
mod limits;
mod runtime;
mod service;
mod supervisor;
mod wire;

pub use client::{TurnBridge, WorkerClient};
pub use limits::LoopLimits;
pub use runtime::{AdmissionEngine, AdmittedAgentloop, AdmittedTool, GuestHost, NativeToolInput};
pub use service::WorkerService;
pub use supervisor::{LoopError, NativePolicy, WorkerPool};
pub use wire::{ComponentKind, HostCall, NativeEnvironment, WorkerRequest, WorkerResponse};

pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TURN_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TURN_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LINEAR_MEMORY_BYTES: usize = 128 * 1024 * 1024;

/// Turns one worker runs at once by default.
///
/// A turn holds a fresh Wasm Store and instance for as long as it runs. Eight instances
/// of the default 128 MiB ceiling bound guest linear memory to 1 GiB.
pub const DEFAULT_CONCURRENT_TURNS: usize = 8;

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
) -> Result<WorkerRequest, String> {
    wire::read_frame(reader, MAX_PACKAGE_BYTES.max(MAX_TURN_INPUT_BYTES) + 1_024).await
}

#[doc(hidden)]
pub async fn worker_write<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    response: &WorkerResponse,
) -> Result<(), String> {
    wire::write_frame(writer, response, wire::MAX_RESPONSE_FRAME_BYTES).await
}
