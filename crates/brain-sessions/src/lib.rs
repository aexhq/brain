//! Multi-session ownership, recovery, and caller-driven Environment lifecycle.
mod environment;
mod service;
pub use environment::EnvironmentRegistry;
pub use service::{MessageAdmission, SessionResources, Sessions};
#[doc(hidden)]
pub mod locks;

mod execution;
pub use execution::{EnvironmentLoopExecutor, SessionServices, SessionToolExecutor};
