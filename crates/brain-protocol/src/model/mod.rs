//! The provider-neutral vocabulary of a model call: the messages it sees and the
//! request and result around them.

mod call;
mod catalog;
mod message;

pub use call::*;
pub use catalog::*;
pub use message::*;
