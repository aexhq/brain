//! A content address minted outside Brain: the digest of an admitted agentloop package,
//! the digest of a program payload an environment holds, the digest a directory computed
//! for an environment configuration. Brain validates the shape, compares, and carries it.
//! It does not compute one here, so no boundary type can silently cost a hash.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("an identity is 64 lowercase hexadecimal characters")]
    Malformed,
}

/// The identity of some content. Equal identities mean equal content.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity([u8; 32]);

impl Identity {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(text: &str) -> Result<Self, IdentityError> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(IdentityError::Malformed);
        }
        let bytes = hex::decode(text).map_err(|_| IdentityError::Malformed)?;
        bytes
            .try_into()
            .map(Self)
            .map_err(|_| IdentityError::Malformed)
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Identity({self})")
    }
}

impl Serialize for Identity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Identity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_round_trips_through_hex() {
        let identity = Identity::from_hex(&"0123456789abcdef".repeat(4)).unwrap();
        assert_eq!(identity.to_string(), "0123456789abcdef".repeat(4));
        let json = serde_json::to_string(&identity).unwrap();
        assert_eq!(serde_json::from_str::<Identity>(&json).unwrap(), identity);
    }

    #[test]
    fn a_malformed_identity_is_refused() {
        assert!(Identity::from_hex(&"a".repeat(63)).is_err());
        assert!(Identity::from_hex(&"A".repeat(64)).is_err());
        assert!(Identity::from_hex(&"g".repeat(64)).is_err());
        assert!(serde_json::from_str::<Identity>("\"short\"").is_err());
    }
}
