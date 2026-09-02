//! Runnable Brain composition and resources shared across sessions.

pub mod config;
pub mod environment;
pub mod idempotency;
pub mod metadata;
pub mod model_binding;
mod service;
mod session_ownership;
pub mod tool_dispatcher;

pub use config::ServerConfig;
pub use environment::{
    EnvironmentAdapter, EnvironmentDirectory, EnvironmentRegistry, HttpEnvironmentAdapter,
    InMemoryEnvironmentDirectory, SessionBindingValues,
};
pub use idempotency::IdempotencyStore;
pub use model_binding::{
    LocalModelBindingStore, ModelBindingStore, ServerModelExecutor, load_providers_file,
};
pub use service::{ServerApi, ServerResources, WorkerLoopExecutor};
pub use session_ownership::{LocalSessionOwnership, SessionOwnership};
pub use tool_dispatcher::ServerToolExecutor;
