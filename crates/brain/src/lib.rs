//! Minimal durable session kernel. Runtime composition belongs to `brain-server`.

pub mod agentloop;
pub mod context;
pub mod error;
pub mod journal;
pub mod model;
pub mod operation;
pub mod session;
pub mod tool;

pub use agentloop::LoopExecutor;
pub use error::KernelError;
pub use journal::{
    AppendRecord, DEFAULT_IDEMPOTENCY_RETENTION, JournalRecord, JournalStore, SegmentJournal,
    SessionUpdate,
};
pub use model::ModelExecutor;
pub use session::{CreatingSession, Kernel, KernelConfig, SessionHandle};
pub use tool::ToolExecutor;
