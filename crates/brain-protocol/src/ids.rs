use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The shape of every name a caller mints: a tool, an environment, a binding, a code.
pub const IDENTIFIER_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
/// The shape of a content address: 64 lowercase hexadecimal characters.
pub const IDENTITY_PATTERN: &str = "^[0-9a-f]{64}$";

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
id_type!(EventId, IDENTIFIER_PATTERN);
id_type!(EnvironmentId, IDENTIFIER_PATTERN);
id_type!(AttachmentId, IDENTIFIER_PATTERN);
id_type!(AgentloopIdentity, IDENTITY_PATTERN);
