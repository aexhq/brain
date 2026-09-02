//! One durable session at a time. Which sessions exist, which run, and what happens to
//! them after a restart belongs to the host; `brain-server` is one.

pub mod agentloop;
pub mod error;
pub mod journal;
pub mod model;
pub mod session;
pub mod tool;

pub use agentloop::LoopExecutor;
pub use error::Error;
pub use journal::{
    AppendRecord, JournalRecord, JournalStore, ObservedJournal, SegmentJournal, SessionUpdate,
    event_page, interrupt_unfinished_turns,
};
pub use model::ModelExecutor;
pub use session::{
    CreatingSession, DEFAULT_TOOL_DEADLINE_MS, Session, SessionConfig, empty_context, sealed_config,
};
pub use tool::ToolExecutor;
