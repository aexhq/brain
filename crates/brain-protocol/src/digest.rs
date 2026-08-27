use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::{JournalId, OperationId};

#[derive(Debug, thiserror::Error)]
pub enum DigestError {
    #[error("value cannot be encoded as canonical JSON: {0}")]
    CanonicalJson(#[from] serde_json::Error),
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, DigestError> {
    Ok(serde_jcs::to_vec(value)?)
}

pub fn request_digest<T: Serialize>(value: &T) -> Result<String, DigestError> {
    Ok(hex::encode(Sha256::digest(canonical_json(value)?)))
}

pub fn operation_id(journal_id: &JournalId, logical_position: u64) -> OperationId {
    let mut hash = Sha256::new();
    hash.update(b"aex-brain-operation-v1\0");
    hash.update(journal_id.as_str().as_bytes());
    hash.update(logical_position.to_be_bytes());
    OperationId::new(format!("op_{}", hex::encode(&hash.finalize()[..16])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_identity_is_stable_and_positioned() {
        let journal = JournalId::new("jrn_example");
        assert_eq!(operation_id(&journal, 7), operation_id(&journal, 7));
        assert_ne!(operation_id(&journal, 7), operation_id(&journal, 8));
    }

    #[test]
    fn canonical_digest_ignores_object_key_order() {
        let left = serde_json::json!({"b": 2, "a": 1});
        let right = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(
            request_digest(&left).unwrap(),
            request_digest(&right).unwrap()
        );
    }
}
