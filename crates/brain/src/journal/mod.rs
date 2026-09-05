//! A session's durable record: the events clients read, the journal its state folds out
//! of, and the one writer thread that puts both on disk.

mod cursor;
mod feed;
mod log;
mod record;
mod session_store;
mod store;
mod writer;

pub use cursor::event_page;
pub use feed::Feed;
pub use record::{AppendRecord, SessionRecord};
pub use session_store::LocalSessionStore;
pub use store::{CommitHandle, Folded, JournalEntry, SessionRow, SessionStore, SessionUpdate};
pub use writer::{OWNER_QUEUE_BYTES, Writer};
