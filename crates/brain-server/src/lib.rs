//! Runnable Brain composition and resources shared across sessions.

pub mod config;
pub mod data_layout;
pub mod digest;
pub mod environment;
pub mod idempotency;
pub mod limits;
pub mod metadata;
pub mod model;
mod persistence;
mod service;
pub mod tool_dispatcher;
pub mod turns;

pub use config::ServerConfig;
pub use environment::{
    BrainEnvironment, EnvironmentAdapter, EnvironmentRegistry, HostEnvironment,
    HttpEnvironmentAdapter, NativePolicy, Services,
};
pub use idempotency::IdempotencyStore;
pub use limits::ServerLimits;
pub use model::{CredentialStore, ServerModelExecutor, load_providers_file};
pub use service::{EnvironmentLoopExecutor, ServerApi, ServerResources};
pub use tool_dispatcher::ServerToolExecutor;
pub use turns::Turns;
