//! What a value *is*, as something you can compare.
//!
//! Brain has to answer three questions about content: is this the same sealed
//! artifact, is this retry the same effect, and have I already answered this request.
//! All three were spelled "digest", so every one of them was a hash call at the point
//! of use, and the cost of taking a hash was invisible.
//!
//! [`Identity`] answers those questions instead. It compares, prints and travels; it
//! does not hand back its bytes, because a caller that wanted the bytes wanted to
//! compare. And it composes: an identity built from identities that were computed
//! once, when their values were sealed, never re-reads the values themselves.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::{Digest as _, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("value cannot be encoded as canonical JSON: {0}")]
    CanonicalJson(#[from] serde_json::Error),
}

/// The identity of some content. Equal identities mean equal content.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity([u8; 32]);

impl Identity {
    /// The identity of a value that another implementation will compute for itself —
    /// an Environment checking a dispatch it was handed, a host verifying the sealed
    /// presentation. Canonical JSON first, so two encoders agree.
    ///
    /// This is the expensive constructor: canonicalising sorts every object key and
    /// re-serialises. Reach for it once, when a value is sealed, and carry the result.
    pub fn of<T: Serialize>(value: &T) -> Result<Self, IdentityError> {
        Ok(Self::of_bytes(&canonical_json(value)?))
    }

    /// The identity of bytes that are already settled — a canonical encoding produced
    /// earlier, or a payload exactly as it was written.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// The identity of several things taken together, from identities they already
    /// carry. This is what keeps a per-decision identity off the size of the
    /// conversation: the parts were each sealed once, and combining them reads 32
    /// bytes apiece rather than the values behind them.
    pub fn over(parts: &[Identity]) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"aex-brain-identity-v1\0");
        for part in parts {
            hash.update(part.0);
        }
        Self(hash.finalize().into())
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
        let bytes = hex::decode(&text).map_err(D::Error::custom)?;
        Ok(Self(bytes.try_into().map_err(|_| {
            D::Error::custom("an identity is 32 bytes of lowercase hex")
        })?))
    }
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, IdentityError> {
    Ok(serde_jcs::to_vec(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ignores_object_key_order() {
        let left = serde_json::json!({"b": 2, "a": 1});
        let right = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(Identity::of(&left).unwrap(), Identity::of(&right).unwrap());
    }

    #[test]
    fn different_content_has_a_different_identity() {
        assert_ne!(
            Identity::of(&serde_json::json!({"a": 1})).unwrap(),
            Identity::of(&serde_json::json!({"a": 2})).unwrap()
        );
    }

    #[test]
    fn combining_is_ordered_and_not_concatenation() {
        let first = Identity::of(&"first").unwrap();
        let second = Identity::of(&"second").unwrap();
        assert_ne!(
            Identity::over(&[first, second]),
            Identity::over(&[second, first])
        );
        // Two parts must not collide with one part covering the same bytes, or a
        // dispatch could borrow the identity of an unrelated pair.
        assert_ne!(
            Identity::over(&[first, second]),
            Identity::of_bytes(&[first.0, second.0].concat())
        );
    }

    #[test]
    fn an_identity_survives_the_wire_as_hex() {
        let identity = Identity::of(&serde_json::json!({"a": 1})).unwrap();
        let text = serde_json::to_string(&identity).unwrap();
        assert_eq!(text.len(), 66, "64 hex characters and two quotes");
        assert_eq!(
            serde_json::from_str::<Identity>(&text).unwrap(),
            identity,
            "an identity read back from the wire is the same identity"
        );
    }

    #[test]
    fn text_that_is_not_an_identity_is_rejected() {
        for text in ["\"\"", "\"nothex\"", "\"ab\"", "\"[]\""] {
            assert!(serde_json::from_str::<Identity>(text).is_err(), "{text}");
        }
    }
}
