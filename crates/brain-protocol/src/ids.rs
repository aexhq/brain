use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The shape of every name a caller mints: a tool, an environment, a code.
pub const IDENTIFIER_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
/// The shape of a content address: the SHA-256 of the bytes, as 64 lowercase
/// hexadecimal characters. A caller with the same bytes computes the same id.
pub const SHA256_PATTERN: &str = "^[0-9a-f]{64}$";

macro_rules! id_type {
    ($name:ident, $pattern:expr) => {
        #[derive(
            Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(#[schemars(regex(pattern = $pattern))] pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = std::convert::Infallible;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ok(Self::new(value))
            }
        }
    };
}

id_type!(SessionId, "^ses_[A-Za-z0-9]{20,32}$");
// A caller-chosen name, unique within its session.
id_type!(EnvironmentName, IDENTIFIER_PATTERN);
// The one server-minted identity outside a session: a registered host.
id_type!(HostId, "^host_[A-Za-z0-9]{20,32}$");
id_type!(AgentloopId, SHA256_PATTERN);
id_type!(ToolId, SHA256_PATTERN);
