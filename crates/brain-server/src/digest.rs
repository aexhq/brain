//! The one place the server computes an identity from a value.
//!
//! SHA-256 over canonical JSON, so two encoders agree on the bytes. Used for idempotency
//! keys and for the configuration digest an environment binding carries; everything else
//! that looks like a digest in Brain was minted elsewhere and only travels.

use brain_protocol::Identity;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

pub fn identity_of<T: Serialize>(value: &T) -> Result<Identity, brain::Error> {
    let bytes =
        serde_jcs::to_vec(value).map_err(|error| brain::Error::InvalidState(error.to_string()))?;
    Ok(Identity::from_bytes(Sha256::digest(&bytes).into()))
}
