//! Runnable Brain composition and resources shared across sessions.

pub mod config;
pub mod data_layout;
pub mod digest;
pub mod environment;
pub mod idempotency;
pub mod metadata;
pub mod model_binding;
mod persistence;
pub mod resident;
mod service;
pub mod tool_dispatcher;

pub use config::ServerConfig;
pub use environment::{
    EnvironmentAdapter, EnvironmentRegistry, EnvironmentResources, HttpEnvironmentAdapter,
    SessionBindingValues,
};
pub use idempotency::IdempotencyStore;
pub use model_binding::{ModelBindingStore, ServerModelExecutor, load_providers_file};
pub use resident::ResidentHosts;
pub use service::{ServerApi, ServerResources, WorkerLoopExecutor};
pub use tool_dispatcher::ServerToolExecutor;
