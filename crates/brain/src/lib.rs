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
    AppendRecord, Feed, Folded, JournalEntry, JournalRecord, JournalStore, SessionStore,
    SessionUpdate, Writer, event_page,
};
pub use model::ModelExecutor;
pub use session::{
    CreatingSession, DEFAULT_TOOL_DEADLINE_MS, Session, SessionRuntime, empty_context, random_id,
    session_config,
};
pub use tool::ToolExecutor;
