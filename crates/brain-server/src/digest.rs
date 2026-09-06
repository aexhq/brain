//! The one place the server computes a content address from a value.
//!
//! SHA-256 over canonical JSON, so two encoders agree on the bytes. Used to compare an
//! idempotency key's request with the one it was first used for; everything else that
//! looks like a digest in Brain was minted elsewhere and only travels.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::Digest as _;

/// The SHA-256 of some content. Equal digests mean equal content.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Sha256([u8; 32]);

impl Sha256 {
    pub fn of<T: Serialize>(value: &T) -> Result<Self, brain::Error> {
        let bytes = serde_jcs::to_vec(value)
            .map_err(|error| brain::Error::InvalidState(error.to_string()))?;
        Ok(Self(sha2::Sha256::digest(&bytes).into()))
    }

    fn from_hex(text: &str) -> Option<Self> {
        if text.len() != 64
            || !text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        hex::decode(text).ok()?.try_into().ok().map(Self)
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Sha256({self})")
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text)
            .ok_or_else(|| D::Error::custom("a digest is 64 lowercase hexadecimal characters"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_round_trips_through_hex_and_refuses_anything_else() {
        let digest = Sha256::of(&serde_json::json!({"b": 1, "a": 2})).unwrap();
        assert_eq!(
            digest,
            Sha256::of(&serde_json::json!({"a": 2, "b": 1})).unwrap()
        );
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(serde_json::from_str::<Sha256>(&json).unwrap(), digest);
        assert!(serde_json::from_str::<Sha256>("\"short\"").is_err());
        assert!(serde_json::from_str::<Sha256>(&format!("\"{}\"", "G".repeat(64))).is_err());
    }
}
