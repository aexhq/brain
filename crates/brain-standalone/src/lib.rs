//! Durable, single-node adapters for the standalone Brain distribution.
//!
//! These adapters deliberately stay outside `brain` core: SQLite, local key custody/storage, and
//! an explicit unsandboxed local Environment are one composition of Brain's public ports.

pub mod compose;
pub mod custody;
pub mod local_environment;
pub mod sqlite;
pub mod storage;

pub use compose::{DurableLocalParts, durable_local_parts};
pub use custody::LocalKeyCustody;
pub use local_environment::LocalEnvironment;
pub use sqlite::SqliteStore;
pub use storage::LocalSessionStorage;
