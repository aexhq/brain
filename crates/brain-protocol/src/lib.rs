//! Brain-owned public protocols.
//!
//! JSON Schemas under `contracts/` are authoritative. [`abi`] and [`session`] are generated from
//! them; [`tools`] contains the canonical hashing shared by Brain and Hand implementations.

#[allow(clippy::all, clippy::pedantic)]
pub mod abi;
#[allow(clippy::all, clippy::pedantic)]
pub mod session;
pub mod tools;

/// The Brain↔Hand ABI major version. Major versions must match exactly.
pub const ABI_MAJOR: std::num::NonZeroU64 = std::num::NonZeroU64::new(1).unwrap();
/// The additive ABI minor version.
pub const ABI_MINOR: u64 = 0;

pub const ABI_SCHEMA_JSON: &str = include_str!("../../../contracts/abi/v1/abi.json");
pub const SESSION_SCHEMA_JSON: &str = include_str!("../../../contracts/session/v1/schemas.json");

impl abi::ProtocolVersion {
    pub const CURRENT: abi::ProtocolVersion = abi::ProtocolVersion {
        major: ABI_MAJOR,
        minor: ABI_MINOR,
    };

    #[must_use]
    pub fn compatible_with(&self, other: &abi::ProtocolVersion) -> bool {
        self.major == other.major
    }
}
