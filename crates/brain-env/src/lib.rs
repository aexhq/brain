//! Native Environment adapter, worker supervision, and the Brain↔worker wire.
//!
//! Guest code runs in a separate process: `brain-env-worker` owns the Wasmtime engine and
//! everything that compiles or instantiates a Component. This crate is the server's side of
//! that boundary — it starts workers, addresses them, and speaks the wire below — so a
//! binary that links it carries no Wasm runtime.

mod client;
mod environment;
pub use environment::{BrainEnvironment, NativePolicy};
mod limits;
mod socket;
mod supervisor;
mod wire;

pub use client::{TurnBridge, WorkerClient};
pub use limits::{EnvLimits, WorkerArgs, ceiling};
pub use socket::{Listener, listen};
pub use supervisor::{LoopError, WorkerPool};
pub use wire::{
    Access, ComponentKind, HostCall, NativeEnvironment, NativeToolInput, WorkerRequest,
    WorkerResponse, Workspace, max_request_bytes, network_covers, read_frame, write_frame,
};
