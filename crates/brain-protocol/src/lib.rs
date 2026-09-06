//! Types crossing Brain process, transport, or durable-storage boundaries.
//!
//! These types are the source of the published contracts: [`contract`] renders them to
//! JSON Schema, and `cargo run -p brain-protocol --bin contract` writes the result under
//! this crate's `generated/contract/`.

pub mod agentloop;
pub mod codes;
pub mod contract;
pub mod environment;
pub mod error;
pub mod execution;
pub mod host;
pub mod ids;
pub mod message;
pub mod model;
mod schema;
pub mod session;
pub mod tool;
pub mod turn;

pub use agentloop::*;
pub use environment::*;
pub use error::*;
pub use execution::*;
pub use host::*;
pub use ids::*;
pub use message::*;
pub use model::*;
pub use session::*;
pub use tool::*;
pub use turn::*;
