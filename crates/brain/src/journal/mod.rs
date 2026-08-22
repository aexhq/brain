//! The session journal: every decision is durable in one backend transaction.
//!
//! One item collection per session. `HEAD` carries ownership (lease + fence), the sealed
//! configuration and the mutable session facts; `E#<seq>` items carry the decision records.
//! The journal is also the event log: SSE replay is a derivation over these records
//! (`events::derive`), and `seq` is both the journal order and the SSE `id:`.
//!
//! Concurrency rules (each one answers a real outage class):
//! - the (session, seq) key is the idempotency barrier: a redelivered decision loses the
//!   write, it never duplicates;
//! - the fence advances on claim only, never on renew (renewing must not fence out the owner);
//! - a `Fenced` failure on commit means a newer owner exists: the local fold is stale and
//!   must be discarded, never patched.
//!
//! Persistence is a seam: [`JournalStore`]. [`MemoryStore`] is the reference backend;
//! `brain-aws` carries the DynamoDB one; custom backends implement the trait.

use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::{BrainError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

/// Bound on the tool-result content a single record may carry. Hosted DynamoDB items cap at 400 KiB;
/// this leaves generous room for the envelope and the parallel records of one decision.
pub const MAX_RECORD_CONTENT_BYTES: usize = 96 * 1024;
/// Backend-neutral bound for one serialized record, leaving DynamoDB item-envelope headroom.
pub const MAX_SERIALIZED_RECORD_BYTES: usize = 256 * 1024;
/// The complete mutable HEAD payload (control plus listing projection) is kept to the same
/// backend-neutral item ceiling. The listing alone stays deliberately small because it is
/// duplicated into tenant discovery indexes and direct-child adjacency rows.
pub const MAX_SERIALIZED_HEAD_BYTES: usize = 256 * 1024;
pub const MAX_SERIALIZED_LISTING_BYTES: usize = 64 * 1024;
/// Immutable CONFIG is a separate backend item and must obey the same neutral ceiling as HEAD.
pub const MAX_SERIALIZED_CONFIG_BYTES: usize = 256 * 1024;
/// Includes the mutable HEAD update. DynamoDB permits 100 actions; Brain keeps ample room for
/// future conditional/index items without changing provider-facing behavior.
pub const MAX_DECISION_ACTIONS: usize = 64;
/// DynamoDB transactions cap at 4 MiB. Three MiB includes conservative room for keys,
/// attribute names, expressions and SDK encoding that are not present in the JSON payloads.
pub const MAX_DECISION_SERIALIZED_BYTES: usize = 3 * 1024 * 1024;
/// Environment names for the process-wide append-only retention policy. The limits are not
/// copied into session CONFIG: every hosted replica that can claim a session must therefore pin
/// the same values for the lifetime of the deployment.
pub const JOURNAL_MAX_SESSION_BYTES_ENV: &str = "BRAIN_JOURNAL_MAX_SESSION_BYTES";
pub const JOURNAL_MAX_TENANT_BYTES_ENV: &str = "BRAIN_JOURNAL_MAX_TENANT_BYTES";
pub const JOURNAL_MAX_TENANT_SESSIONS_ENV: &str = "BRAIN_JOURNAL_MAX_TENANT_SESSIONS";
/// Default authoritative append-only retention ceilings.
pub const DEFAULT_MAX_SESSION_JOURNAL_BYTES: u64 = 128 * 1024 * 1024;
pub const DEFAULT_MAX_TENANT_JOURNAL_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_MAX_TENANT_RETAINED_SESSIONS: u64 = 4096;
/// Every retained identity pays for its bounded immutable/control projection up front. This is
/// deliberately conservative: tenant capacity cannot be bypassed with many empty sessions.
pub const JOURNAL_SESSION_BASE_BYTES: u64 = (MAX_SERIALIZED_CONFIG_BYTES
    + MAX_SERIALIZED_HEAD_BYTES
    + MAX_SERIALIZED_LISTING_BYTES
    + 3 * ESTIMATED_ITEM_ENVELOPE_BYTES) as u64;
/// Charged at create and consumed only by bounded lifecycle/recovery records. Ordinary traffic
/// cannot spend this reserve, so END/DELETE and post-terminal ACK recovery remain journalable.
pub const JOURNAL_LIFECYCLE_RESERVE_BYTES: u64 = 64 * 1024;
/// One complete post-effect terminal decision. Managed/customer/host Tools, compaction, storage
/// and sandbox operations reserve this before dispatch.
pub const JOURNAL_TERMINAL_RESERVE_BYTES: u64 = MAX_DECISION_SERIALIZED_BYTES as u64;
/// A provider completion can itself consume one maximum-size decision while durably installing
/// ToolCall intents whose later terminal batch can consume another. Reserving both before model
/// dispatch means quota exhaustion cannot strand either the provider fact or an executed Tool.
pub const JOURNAL_EFFECT_RESERVE_BYTES: u64 = 2 * JOURNAL_TERMINAL_RESERVE_BYTES;
/// A configured session ceiling must admit the largest legal create record plus lifecycle and
/// provider/Tool terminal headroom. Smaller policies could accept an identity that can never
/// safely dispatch one ordinary model round.
pub const MIN_SESSION_JOURNAL_BYTES: u64 = JOURNAL_SESSION_BASE_BYTES
    + JOURNAL_LIFECYCLE_RESERVE_BYTES
    + JOURNAL_EFFECT_RESERVE_BYTES
    + MAX_SERIALIZED_RECORD_BYTES as u64
    + ESTIMATED_ITEM_ENVELOPE_BYTES as u64;
/// Journal adapters encode atomic meter changes as signed deltas and SQLite stores the aggregate
/// in an INTEGER, so a larger policy would not be representable consistently across adapters.
pub const MAX_JOURNAL_BYTES: u64 = i64::MAX as u64;
pub const MIN_TENANT_RETAINED_SESSIONS: u64 = 1;
pub const MAX_TENANT_RETAINED_SESSIONS: u64 = 1_000_000;
/// Public message admission bound. The resulting UserMessage record remains below the item cap
/// after its journal envelope and trusted metadata are added.
pub const MAX_MESSAGE_REQUEST_BYTES: usize = brain_protocol::MAX_MESSAGE_REQUEST_BYTES;
const ESTIMATED_ITEM_ENVELOPE_BYTES: usize = 1024;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Validates one atomic append before any store adapter is entered. A provider response or Tool
/// batch therefore fails honestly in the state machine rather than discovering a cloud limit.
pub fn validate_decision(session_id: &str, records: &[(u64, Record)], doc: &HeadDoc) -> Result<()> {
    let actions = records.len().saturating_add(1);
    if actions > MAX_DECISION_ACTIONS {
        return Err(BrainError::Invalid(format!(
            "journal decision has {actions} actions; maximum is {MAX_DECISION_ACTIONS}"
        )));
    }
    let control = serde_json::to_vec(&doc.control_doc())?;
    let listing = serde_json::to_vec(&SessionSummary::from_head(session_id, doc))?;
    if listing.len() > MAX_SERIALIZED_LISTING_BYTES {
        return Err(BrainError::Invalid(format!(
            "journal listing document is {} bytes; maximum is {MAX_SERIALIZED_LISTING_BYTES}",
            listing.len()
        )));
    }
    let head_bytes = control.len().saturating_add(listing.len());
    if head_bytes > MAX_SERIALIZED_HEAD_BYTES {
        return Err(BrainError::Invalid(format!(
            "journal HEAD payload is {head_bytes} bytes; maximum is {MAX_SERIALIZED_HEAD_BYTES}"
        )));
    }
    let mut total = head_bytes.saturating_add(ESTIMATED_ITEM_ENVELOPE_BYTES);
    for (_, record) in records {
        let bytes = serde_json::to_vec(record)?;
        if bytes.len() > MAX_SERIALIZED_RECORD_BYTES {
            return Err(BrainError::Invalid(format!(
                "journal {} record is {} bytes; maximum is {MAX_SERIALIZED_RECORD_BYTES}",
                record.kind_name(),
                bytes.len()
            )));
        }
        total = total
            .saturating_add(bytes.len())
            .saturating_add(ESTIMATED_ITEM_ENVELOPE_BYTES);
    }
    if total > MAX_DECISION_SERIALIZED_BYTES {
        return Err(BrainError::Invalid(format!(
            "journal decision is approximately {total} bytes; maximum is {MAX_DECISION_SERIALIZED_BYTES}"
        )));
    }
    Ok(())
}

pub fn validate_config_doc(doc: &HeadDoc) -> Result<()> {
    let bytes = serde_json::to_vec(&doc.config_doc())?;
    if bytes.len() > MAX_SERIALIZED_CONFIG_BYTES {
        return Err(BrainError::Invalid(format!(
            "journal CONFIG payload is {} bytes; maximum is {MAX_SERIALIZED_CONFIG_BYTES}",
            bytes.len()
        )));
    }
    Ok(())
}

mod docs;
mod fold;
mod memory;
mod records;
mod store;

pub use docs::*;
pub use fold::*;
pub use memory::*;
pub use records::*;
pub use store::*;

#[cfg(test)]
#[path = "../journal_tests.rs"]
mod tests;
