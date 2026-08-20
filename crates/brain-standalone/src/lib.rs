//! Durable, single-node adapters for the standalone Brain distribution.
//!
//! These adapters deliberately stay outside `brain` core: SQLite, local key custody, and Docker
//! are one composition of Brain's public ports, not assumptions baked into the session engine.

pub mod custody;
pub mod docker;
pub mod sqlite;

pub use custody::LocalKeyCustody;
pub use docker::{DockerConfig, DockerHandFactory};
pub use sqlite::SqliteStore;
