//! The Environments a session's Tools and Agentloop run in.
//!
//! One interface, [`EnvironmentAdapter`], and one implementation per [`Driver`]: the
//! brain env hosted in this process, the host env reached over the connection a
//! registered host holds open, and any other Environment reached over HTTP. The
//! [`EnvironmentRegistry`] resolves an entry to its adapter and journals every operation
//! before it sends it; nothing else in the server knows which kind it has.
//!
//! [`Driver`]: brain_protocol::Driver

mod adapter;
mod brain;
mod host;
mod http;
mod registry;

pub use adapter::{EnvironmentAdapter, Services};
pub use brain::{BrainEnvironment, NativePolicy};
pub use host::HostEnvironment;
pub use http::{HttpEnvironmentAdapter, validate_url};
pub use registry::EnvironmentRegistry;
