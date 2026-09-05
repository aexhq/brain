//! HTTP routing over the transport-neutral [`BrainApi`] service contract.

mod error;
mod openapi;
mod router;
mod service;

pub use error::HttpError;
pub use openapi::openapi;
pub use router::{router, router_with_bearer};
pub use service::{BrainApi, HostConnection};
