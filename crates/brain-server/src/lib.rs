//! Runnable Brain composition and resources shared across sessions.

pub mod config;
pub mod data_layout;
pub mod digest;
pub mod environment;
pub mod executions;
pub mod idempotency;
pub mod limits;
pub mod metadata;
pub mod model;
mod persistence;
mod service;

pub use brain_sessions::{EnvironmentLoopExecutor, SessionToolExecutor};
pub use config::ServerConfig;
pub use environment::{
    BrainEnvironment, EnvironmentAdapter, EnvironmentRegistry, HostEnvironment,
    HttpEnvironmentAdapter, NativePolicy, Services,
};
pub use executions::Executions;
pub use idempotency::IdempotencyStore;
pub use limits::ServerLimits;
pub use model::{CredentialStore, ServerModelExecutor, load_providers_file};
pub use service::{ServerApi, ServerResources};
