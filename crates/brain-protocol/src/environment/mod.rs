//! The wire between Brain and an Environment, and the outcome every call resolves to.

mod control;
mod outcome;
mod wire;

pub use control::*;
pub use outcome::*;
pub use wire::*;
