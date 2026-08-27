//! Admission and worker-client boundary for the single Agentloop extension pipeline.

mod client;
mod limits;
mod package;
mod runtime;
mod supervisor;
mod wire;

pub use limits::LoopLimits;
pub use package::{AgentloopManifest, AgentloopPackage};
pub use runtime::{AdmissionEngine, AdmittedAgentloop};
pub use supervisor::WorkerPool;
pub use wire::{WorkerRequest, WorkerResponse};

pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ACTIVATION_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ACTIVATION_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LINEAR_MEMORY_BYTES: usize = 128 * 1024 * 1024;
pub const RUNTIME_SHIM_IMPORTS: &[&str] = &["aex:agentloop/types@1.0.0"];

#[doc(hidden)]
pub async fn worker_read<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<WorkerRequest, String> {
    wire::read_frame(
        reader,
        MAX_PACKAGE_BYTES.max(MAX_ACTIVATION_INPUT_BYTES) + 1_024,
    )
    .await
}

#[doc(hidden)]
pub async fn worker_write<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    response: &WorkerResponse,
) -> Result<(), String> {
    wire::write_frame(writer, response, wire::MAX_RESPONSE_FRAME_BYTES).await
}
pub use client::WorkerClient;
