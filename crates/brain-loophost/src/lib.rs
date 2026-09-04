//! Admission and worker-client boundary for the single Agentloop extension pipeline.

mod client;
mod limits;
mod package;
mod runtime;
mod service;
mod supervisor;
mod wire;

pub use client::{TurnBridge, WorkerClient};
pub use limits::LoopLimits;
pub use package::{AgentloopManifest, AgentloopPackage};
pub use runtime::{AdmissionEngine, AdmittedAgentloop, GuestHost, WarmInstances};
pub use service::WorkerService;
pub use supervisor::{LoopError, WorkerPool};
pub use wire::{HostCall, WorkerRequest, WorkerResponse};

pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TURN_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TURN_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LINEAR_MEMORY_BYTES: usize = 128 * 1024 * 1024;

/// Turns one worker runs at once by default.
///
/// A turn holds a live Wasm instance and a blocking thread for as long as it runs, model
/// calls and parked tool calls included, so this bounds worker memory and threads
/// together. Sixty-four instances of the default 128 MiB ceiling is the worst case the
/// arithmetic allows; a real loop's heap is far smaller than its ceiling.
pub const DEFAULT_CONCURRENT_TURNS: usize = 64;

/// The interfaces a guest may import: the contract's own types and the host services.
pub const RUNTIME_SHIM_IMPORTS: &[&str] =
    &["aex:agentloop/types@2.0.0", "aex:agentloop/host@2.0.0"];

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
