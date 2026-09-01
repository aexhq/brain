//! Admission and worker-client boundary for the single Agentloop extension pipeline.

mod client;
mod limits;
mod package;
mod runtime;
mod service;
mod supervisor;
mod wire;

pub use limits::LoopLimits;
pub use package::{AgentloopManifest, AgentloopPackage};
pub use runtime::{AdmissionEngine, AdmittedAgentloop, WarmInstances};
pub use service::WorkerService;
pub use supervisor::WorkerPool;
pub use wire::{WorkerRequest, WorkerResponse};

pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ACTIVATION_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ACTIVATION_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LINEAR_MEMORY_BYTES: usize = 128 * 1024 * 1024;

/// Activations one worker runs at once by default.
///
/// This was effectively one: the supervisor held a process-global mutex across the whole
/// activation, and its semaphore admitted three, so a fourth was refused outright. At
/// concurrency 64 only 12 of 256 turns reached the model, and a throughput measurement
/// taken against that counted the refusals as successes.
///
/// It cannot simply be unbounded either: each activation is a live Wasm instance, and 48
/// at once measured 1.03 GiB. Sixteen measured 160 turns/s against 48.6 at one, and is
/// the number the memory arithmetic supports.
pub const DEFAULT_CONCURRENT_ACTIVATIONS: usize = 16;
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
