//! The sealed tool manifest v1 and the two hashes both sides of the ABI compute.
//!
//! * `manifest_digest` — SHA-256 over the RFC 8785 (JCS) canonical JSON of the [`ToolManifest`].
//!   The brain seals it at session create; the hand must serve the same manifest for the life of
//!   the session (invariant I1).
//! * `call_hash` — SHA-256 over JCS of `{tool, input, lane, cwd, detach, bounds}`. Makes `start`
//!   idempotent within a generation (invariant I10).

use std::sync::LazyLock;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::abi::{Bounds, LaneRef, Sha256Hex, StartRequest, ToolManifest};

/// Raw JSON of the manifest (contracts/abi/v1/tools/manifest.json).
pub const TOOL_MANIFEST_V1_JSON: &str =
    include_str!("../../../contracts/abi/v1/tools/manifest.json");

/// Pinned digest of [`TOOL_MANIFEST_V1_JSON`] (contracts/abi/v1/tools/manifest.digest).
/// A test asserts it equals [`manifest_digest`] of the parsed manifest, so editing the manifest
/// without re-running `tools/gen.sh` fails CI — and so does an accidental edit.
pub const TOOL_MANIFEST_V1_DIGEST: &str =
    include_str!("../../../contracts/abi/v1/tools/manifest.digest");

static MANIFEST: LazyLock<ToolManifest> = LazyLock::new(|| {
    serde_json::from_str(TOOL_MANIFEST_V1_JSON).expect("embedded tool manifest is valid")
});

/// The v1 manifest, parsed once.
pub fn manifest_v1() -> &'static ToolManifest {
    &MANIFEST
}

/// SHA-256 over the JCS canonical form of `value`, lower-case hex.
pub fn jcs_sha256<T: Serialize>(value: &T) -> Result<Sha256Hex, serde_json::Error> {
    let canonical = serde_jcs::to_vec(value)?;
    let digest = Sha256::digest(&canonical);
    Ok(Sha256Hex::try_from(hex::encode(digest)).expect("hex sha256 matches the pattern"))
}

/// Digest of a tool manifest. Tools must already be sorted by name (the schema says so and the
/// digest does not re-sort: an unsorted manifest is a different manifest).
pub fn manifest_digest(manifest: &ToolManifest) -> Sha256Hex {
    jcs_sha256(manifest).expect("a ToolManifest is always serialisable")
}

#[derive(Serialize)]
struct CallHashInput<'a> {
    tool: &'a str,
    input: &'a serde_json::Value,
    lane: &'a LaneRef,
    cwd: Option<&'a str>,
    detach: bool,
    bounds: Option<&'a Bounds>,
}

/// The `call_hash` of a `start` request: SHA-256 over JCS of
/// `{"tool","input","lane","cwd","detach","bounds"}` (absent optional fields serialise as null).
pub fn call_hash(req: &StartRequest) -> Sha256Hex {
    jcs_sha256(&CallHashInput {
        tool: &req.tool,
        input: &req.input,
        lane: &req.lane,
        cwd: req.cwd.as_deref(),
        detach: req.detach,
        bounds: req.bounds.as_ref(),
    })
    .expect("a StartRequest is always serialisable")
}
