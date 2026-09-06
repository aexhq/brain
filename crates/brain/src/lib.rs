//! One durable session at a time. Which sessions exist, which run, and what happens to
//! them after a restart belongs to the host; `brain-server` is one.

pub mod agentloop;
pub mod environment;
pub mod error;
pub mod journal;
pub mod limits;
pub mod model;
pub mod session;
pub mod tool;

pub use agentloop::LoopExecutor;
pub use error::Error;
pub use journal::{
    AppendRecord, CommitHandle, Feed, Folded, JournalEntry, LocalSessionStore, SessionRecord,
    SessionStore, SessionUpdate, Writer, event_page,
};
pub use limits::Limits;
pub use model::ModelExecutor;
pub use session::{
    CreatingSession, LAST_ACTIVATION_KEY, Session, SessionRuntime, TurnServices, random_id,
    session_config,
};
pub use tool::{ToolExecutor, ToolServices};
