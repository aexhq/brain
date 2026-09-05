//! Types crossing Brain process, transport, or durable-storage boundaries.
//!
//! These types are the source of the published contracts: [`contracts`] renders them to
//! JSON Schema, and `brain-contracts` writes the result under `contracts/`.

pub mod agentloop;
pub mod codes;
pub mod contracts;
pub mod environment;
pub mod error;
pub mod execution;
pub mod host;
pub mod identity;
pub mod ids;
pub mod message;
pub mod model;
mod schema;
pub mod session;
pub mod tool;

pub use agentloop::*;
pub use environment::*;
pub use error::*;
pub use execution::*;
pub use host::*;
pub use identity::*;
pub use ids::*;
pub use message::*;
pub use model::*;
pub use session::*;
pub use tool::*;
