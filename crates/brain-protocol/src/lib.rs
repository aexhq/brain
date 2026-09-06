//! Types crossing Brain process, transport, or durable-storage boundaries.
//!
//! Modules are grouped by the party on the other side of the boundary: the application
//! [`client`] over the session API, the [`environment`] that runs Tools, the
//! [`agentloop`] that runs a turn, and the [`model`] provider. What every party shares
//! sits at the root: identifiers, Tools, and the closed code sets.
//!
//! These types are the source of the published contracts: [`contract`] renders them to
//! JSON Schema, and `cargo run -p brain-protocol --bin contract` writes the result under
//! this crate's `generated/contract/`.

pub mod agentloop;
pub mod client;
pub mod codes;
pub mod contract;
pub mod environment;
pub mod ids;
pub mod model;
mod schema;
pub mod tool;

pub use agentloop::*;
pub use client::*;
pub use environment::*;
pub use ids::*;
pub use model::*;
pub use tool::*;
