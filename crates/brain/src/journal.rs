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

// ---------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------

/// One journaled decision. A closed enum: an unknown kind on read is a typed error, never a
/// silent passthrough (passthrough only ever hid corruption).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    /// The admitted user message. Not an SSE event (the caller knows what they sent); it is
    /// what rebuilds the user side of the model history.
    UserMessage {
        turn: String,
        content: Vec<ContentBlock>,
        /// Child creation admits its initial prompt in the same transaction as the child HEAD
        /// and parent adjacency. Such a record is also the turn-start marker; ordinary message
        /// admission keeps the separate `TurnStarted` record for backwards-compatible replay.
        #[serde(default, skip_serializing_if = "is_false")]
        starts_turn: bool,
        /// Trusted turn context forwarded to host-executed tools. It is never rendered as model
        /// input and must survive replay with the admitted message.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        metadata: HashMap<String, String>,
        /// SHA-256 of the caller's Idempotency-Key. The raw key is never persisted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idempotency_key_hash: Option<String>,
        /// SHA-256 of the canonical message content and metadata. Paired with the key hash so
        /// replay can reject reuse of one key for a different request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_hash: Option<String>,
    },
    TurnStarted {
        turn: String,
    },
    /// Exact provider request intent, committed before any billable provider I/O.
    ModelCallIntent {
        turn: String,
        logical_operation_id: String,
        attempt_id: String,
        request_digest: String,
        replacement: u32,
    },
    /// An intent that may have reached the provider but has no canonical terminal response.
    ModelCallUnknown {
        turn: String,
        logical_operation_id: String,
        attempt_id: String,
        request_digest: String,
        possibly_duplicated: bool,
    },
    /// A previously streamed provisional attempt is no longer eligible to become canonical.
    /// This record is committed atomically with the replacement intent so replay consumers can
    /// discard every delta for the superseded attempt before accepting replacement bytes.
    ModelAttemptSuperseded {
        turn: String,
        logical_operation_id: String,
        superseded_attempt_id: String,
        replacement_attempt_id: String,
        reason: String,
    },
    /// The attempt whose complete assistant response becomes canonical in the same decision.
    ModelCallCompleted {
        turn: String,
        logical_operation_id: String,
        attempt_id: String,
        request_digest: String,
    },
    /// Semantic compaction is a billable provider operation with the same durable unknown /
    /// digest-identical replacement semantics as an ordinary model round.
    CompactionIntent {
        turn: String,
        logical_operation_id: String,
        attempt_id: String,
        request_digest: String,
        replacement: u32,
    },
    CompactionUnknown {
        turn: String,
        logical_operation_id: String,
        attempt_id: String,
        request_digest: String,
        possibly_duplicated: bool,
    },
    CompactionCompleted {
        turn: String,
        logical_operation_id: String,
        attempt_id: String,
        request_digest: String,
    },
    /// A complete assistant message, full-fidelity blocks. Only complete messages are
    /// journaled -- a stream that dies mid-message journals nothing.
    Assistant {
        turn: String,
        agent: String,
        /// Winning provider attempt for this canonical complete message.
        attempt_id: String,
        content: Vec<ContentBlock>,
        stop: StopReason,
    },
    /// Raw provider usage for one round. Absent counters stay absent.
    Usage {
        turn: String,
        agent: String,
        provider: String,
        model: String,
        usage: Usage,
    },
    /// Customer-app execution routing sealed atomically with the ToolCall, before any offer.
    /// Recovery may redeliver only this exact digest to this exact application process.
    CustomerCallIntent {
        turn: String,
        call: String,
        client_id: String,
        process_id: String,
        request_digest: String,
        deadline_at_ms: u64,
    },
    /// Exact managed-Hand request envelope committed before `HandPort::submit`.
    ManagedCallIntent {
        turn: String,
        call: String,
        name: String,
        envelope: brain_protocol::hand::OperationEnvelope,
    },
    /// Opaque rooted receipt committed before Brain begins observing the operation.
    ManagedCallAccepted {
        turn: String,
        call: String,
        operation: brain_protocol::hand::OperationRef,
    },
    /// `HandPort::submit` may have reached the guest, but no rooted operation receipt can be
    /// recovered. This marker permanently revokes Brain's right to submit the intent again; the
    /// exact fenced default target is reconciled separately through `DefaultSandboxChanged`.
    ManagedCallUnknown {
        turn: String,
        call: String,
        request_digest: String,
    },
    /// Journaled BEFORE dispatch: an ambiguous outcome is recorded as possibly-run.
    ToolCall {
        turn: String,
        agent: String,
        call: String,
        name: String,
        input: serde_json::Value,
        detach: bool,
    },
    ToolResult {
        turn: String,
        agent: String,
        call: String,
        name: String,
        /// `completed | failed | cancelled | deadline_exceeded | interrupted`.
        outcome: String,
        /// What the model was shown, bounded by [`MAX_RECORD_CONTENT_BYTES`].
        content: String,
        is_error: bool,
        exit_code: Option<i64>,
        duration_ms: u64,
        truncated: bool,
    },
    /// Exact customer terminal identity committed in the same decision as its ToolResult.
    /// Result payload stays in ToolResult; this bounded projection exists for post-commit ACK.
    CustomerTerminalReceived {
        turn: String,
        call: String,
        client_id: String,
        process_id: String,
        request_digest: String,
        terminal_digest: String,
    },
    /// The exact ACK was delivered. If that delivery response was lost, recovery safely sends
    /// the same ACK again and this record remains an idempotent cleanup marker.
    CustomerTerminalAcknowledged {
        turn: String,
        call: String,
        request_digest: String,
        terminal_digest: String,
    },
    /// Exact managed terminal identity committed atomically with its ToolResult.
    ManagedTerminalReceived {
        turn: String,
        call: String,
        operation: brain_protocol::hand::OperationRef,
        terminal_digest: String,
    },
    /// The exact managed terminal ACK was accepted. Lost responses are safely replayed.
    ManagedTerminalAcknowledged {
        turn: String,
        call: String,
        request_digest: String,
        terminal_digest: String,
    },
    TurnCompleted {
        turn: String,
        stop_reason: String,
        rounds: u64,
        tool_calls: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<brain_protocol::session::TurnResult>,
    },
    TurnFailed {
        turn: String,
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
    /// A session/hand state transition worth telling clients about (`session.updated`).
    State {
        state: String,
        turn: Option<String>,
    },
    HandLost {
        turn: Option<String>,
        interrupted: Vec<String>,
        synced_ms: Option<u64>,
    },
    /// Large-object capacity was reserved atomically with HEAD before issuing any upload
    /// capability. At most one non-terminal reservation exists per session in the MVP.
    StorageUploadReserved {
        transfer_id: String,
        key: String,
        bytes: u64,
        sha256: Option<String>,
        expires_at_ms: u64,
        published_bytes: u64,
        reserved_bytes: u64,
    },
    /// The staged bytes were verified and published, but staging cleanup may still need retry.
    StorageUploadPublished {
        transfer_id: String,
        key: String,
        bytes: u64,
        published_bytes: u64,
        reserved_bytes: u64,
    },
    /// Staging cleanup completed and the capacity reservation was released.
    StorageUploadCompleted {
        transfer_id: String,
        key: String,
        bytes: u64,
        published_bytes: u64,
        reserved_bytes: u64,
    },
    /// Expired unpublished staging was deleted before its capacity reservation was released.
    StorageUploadExpired {
        transfer_id: String,
        key: String,
        bytes: u64,
        published_bytes: u64,
        reserved_bytes: u64,
    },
    StorageDeleteIntent {
        operation_id: String,
        key: String,
        bytes: u64,
        sha256: String,
        published_bytes: u64,
        reserved_bytes: u64,
    },
    StorageDeleteCompleted {
        operation_id: String,
        key: String,
        bytes: u64,
        published_bytes: u64,
        reserved_bytes: u64,
    },
    /// A public/engine sandbox file mutation is sealed before it reaches Hand. The deterministic
    /// operation identity lets an exact HTTP retry recover a lost response without repeating the
    /// effect; Hand rejects the same operation id with a different request digest.
    SandboxFileEffectIntent {
        operation_id: String,
        request_digest: String,
        action: String,
        path: String,
    },
    SandboxFileEffectCompleted {
        operation_id: String,
        request_digest: String,
        action: String,
        path: String,
        replayed: bool,
    },
    /// Durable logical state of the root tree's shared default sandbox. Brain owns this
    /// projection; Hand returns the opaque physical locator and generation.
    DefaultSandboxChanged {
        status: brain_protocol::hand::SandboxStatus,
    },
    /// Immutable content-addressed checkpoint payload chunk. Installation is a separate record in
    /// the same atomic decision, so a partial write is never visible through HEAD.
    ContextChunk {
        checkpoint_id: String,
        index: u32,
        total: u32,
        sha256: String,
        content_base64: String,
    },
    ContextInstalled {
        checkpoint_id: String,
        base_checkpoint_id: Option<String>,
        covers_through_sequence: u64,
        retained_messages: u64,
        payload_digest: String,
        base_prefix_digest: String,
        source_context_digest: String,
        token_estimate: u64,
        context_generation: u64,
        summary_kind: String,
        compactor_provider: String,
        compactor_model: String,
        retained_from_sequence: u64,
        created_at_ms: u64,
    },
    /// A loop-authored opaque entry (`contracts/agentloop/v1` `journal_append` kind `custom`).
    /// Never model input, never an SSE event; readable back through `journal_read`.
    /// `data` is typed as an object because the contract only admits objects — a journal row
    /// that fails this shape fails deserialization loudly instead of projecting a fabrication.
    LoopCustom {
        turn: String,
        data: serde_json::Map<String, serde_json::Value>,
    },
    /// A loop-authored application-visible entry; surfaces on SSE as `loop.event`.
    LoopEvent {
        turn: String,
        name: String,
        data: serde_json::Map<String, serde_json::Value>,
    },
    /// The loop's hydration floor: `data` carries its compacted working context and the next
    /// `session_start` tail begins after `covers_through_seq`.
    LoopMark {
        turn: String,
        covers_through_seq: u64,
        data: serde_json::Map<String, serde_json::Value>,
    },
    /// One durable `kv_set` batch: key to value, `null` deletes. The session's kv map is the
    /// fold of these records; caps are enforced at the op boundary before the record exists.
    LoopKvSet {
        turn: String,
        entries: serde_json::Map<String, serde_json::Value>,
    },
}

impl Record {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Record::UserMessage { .. } => "user_message",
            Record::TurnStarted { .. } => "turn_started",
            Record::ModelCallIntent { .. } => "model_call_intent",
            Record::ModelCallUnknown { .. } => "model_call_unknown",
            Record::ModelAttemptSuperseded { .. } => "model_attempt_superseded",
            Record::ModelCallCompleted { .. } => "model_call_completed",
            Record::CompactionIntent { .. } => "compaction_intent",
            Record::CompactionUnknown { .. } => "compaction_unknown",
            Record::CompactionCompleted { .. } => "compaction_completed",
            Record::Assistant { .. } => "assistant",
            Record::Usage { .. } => "usage",
            Record::CustomerCallIntent { .. } => "customer_call_intent",
            Record::ManagedCallIntent { .. } => "managed_call_intent",
            Record::ManagedCallAccepted { .. } => "managed_call_accepted",
            Record::ManagedCallUnknown { .. } => "managed_call_unknown",
            Record::ToolCall { .. } => "tool_call",
            Record::ToolResult { .. } => "tool_result",
            Record::CustomerTerminalReceived { .. } => "customer_terminal_received",
            Record::CustomerTerminalAcknowledged { .. } => "customer_terminal_acknowledged",
            Record::ManagedTerminalReceived { .. } => "managed_terminal_received",
            Record::ManagedTerminalAcknowledged { .. } => "managed_terminal_acknowledged",
            Record::TurnCompleted { .. } => "turn_completed",
            Record::TurnFailed { .. } => "turn_failed",
            Record::State { .. } => "state",
            Record::HandLost { .. } => "hand_lost",
            Record::StorageUploadReserved { .. } => "storage_upload_reserved",
            Record::StorageUploadPublished { .. } => "storage_upload_published",
            Record::StorageUploadCompleted { .. } => "storage_upload_completed",
            Record::StorageUploadExpired { .. } => "storage_upload_expired",
            Record::StorageDeleteIntent { .. } => "storage_delete_intent",
            Record::StorageDeleteCompleted { .. } => "storage_delete_completed",
            Record::SandboxFileEffectIntent { .. } => "sandbox_file_effect_intent",
            Record::SandboxFileEffectCompleted { .. } => "sandbox_file_effect_completed",
            Record::DefaultSandboxChanged { .. } => "default_sandbox_changed",
            Record::ContextChunk { .. } => "context_chunk",
            Record::ContextInstalled { .. } => "context_installed",
            Record::LoopCustom { .. } => "loop_custom",
            Record::LoopEvent { .. } => "loop_event",
            Record::LoopMark { .. } => "loop_mark",
            Record::LoopKvSet { .. } => "loop_kv_set",
        }
    }
}

/// Store-owned retention projection. It is intentionally outside [`HeadDoc`]: quotas are a
/// persistence invariant, not model/session content, and cannot be forged by a caller restoring a
/// stale fold. Adapters persist these fields beside HEAD and return them on every strong read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalRetention {
    /// Actual charged base/record bytes plus both unspent reserves.
    pub metered_bytes: u64,
    /// Terminal capacity reserved by the last durable external-effect intent.
    pub effect_reserve_bytes: u64,
    /// Create-time lifecycle capacity that ordinary traffic may never consume.
    pub lifecycle_reserve_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalRetentionLimits {
    pub session_bytes: u64,
    pub tenant_bytes: u64,
    pub tenant_sessions: u64,
}

impl Default for JournalRetentionLimits {
    fn default() -> Self {
        Self {
            session_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES,
            tenant_bytes: DEFAULT_MAX_TENANT_JOURNAL_BYTES,
            tenant_sessions: DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
        }
    }
}

impl JournalRetentionLimits {
    /// Validate process policy before any session is admitted. This does not imply a per-session
    /// immutable seal; hosted replicas must use one consistently pinned policy.
    pub fn validate(self) -> Result<Self> {
        if !(MIN_SESSION_JOURNAL_BYTES..=MAX_JOURNAL_BYTES).contains(&self.session_bytes) {
            return Err(BrainError::Invalid(format!(
                "{} must be between {MIN_SESSION_JOURNAL_BYTES} and {MAX_JOURNAL_BYTES}",
                JOURNAL_MAX_SESSION_BYTES_ENV
            )));
        }
        if !(self.session_bytes..=MAX_JOURNAL_BYTES).contains(&self.tenant_bytes) {
            return Err(BrainError::Invalid(format!(
                "{} must be between {} and {MAX_JOURNAL_BYTES}",
                JOURNAL_MAX_TENANT_BYTES_ENV, self.session_bytes
            )));
        }
        if !(MIN_TENANT_RETAINED_SESSIONS..=MAX_TENANT_RETAINED_SESSIONS)
            .contains(&self.tenant_sessions)
        {
            return Err(BrainError::Invalid(format!(
                "{} must be between {MIN_TENANT_RETAINED_SESSIONS} and {MAX_TENANT_RETAINED_SESSIONS}",
                JOURNAL_MAX_TENANT_SESSIONS_ENV
            )));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RetentionClass {
    ensure_effect_reserve_bytes: u64,
    consume_effect_reserve: bool,
    close_effect_reserve: bool,
    consume_lifecycle_reserve: bool,
}

fn serialized_record_charge(records: &[(u64, Record)]) -> Result<u64> {
    records.iter().try_fold(0_u64, |total, (_, record)| {
        let bytes = serde_json::to_vec(record)?.len() as u64;
        total
            .checked_add(bytes)
            .and_then(|value| value.checked_add(ESTIMATED_ITEM_ENVELOPE_BYTES as u64))
            .ok_or_else(|| BrainError::Journal("journal retention meter overflowed".into()))
    })
}

fn retention_class(records: &[(u64, Record)]) -> RetentionClass {
    use brain_protocol::hand::SandboxState;

    let mut class = RetentionClass::default();
    let mut completed_model = false;
    let mut has_tool_call = false;
    for (_, record) in records {
        match record {
            Record::ModelCallIntent { .. } => {
                // A replacement intent may reuse a partially consumed reserve, then tops it back
                // up before another external request is allowed to leave Brain.
                class.consume_effect_reserve = true;
                class.ensure_effect_reserve_bytes = JOURNAL_EFFECT_RESERVE_BYTES;
            }
            Record::CompactionIntent { .. } => {
                class.consume_effect_reserve = true;
                class.ensure_effect_reserve_bytes = class
                    .ensure_effect_reserve_bytes
                    .max(JOURNAL_TERMINAL_RESERVE_BYTES);
            }
            Record::ModelCallUnknown { .. }
            | Record::CompactionUnknown { .. }
            | Record::ManagedCallUnknown { .. }
            | Record::StorageUploadPublished { .. } => {
                class.consume_effect_reserve = true;
            }
            Record::ModelCallCompleted { .. } => {
                completed_model = true;
                class.consume_effect_reserve = true;
            }
            Record::CompactionCompleted { .. }
            | Record::ToolResult { .. }
            | Record::StorageUploadCompleted { .. }
            | Record::StorageUploadExpired { .. }
            | Record::StorageDeleteCompleted { .. }
            | Record::SandboxFileEffectCompleted { .. } => {
                class.consume_effect_reserve = true;
                class.close_effect_reserve = true;
            }
            Record::CustomerCallIntent { .. }
            | Record::ManagedCallIntent { .. }
            | Record::StorageUploadReserved { .. }
            | Record::StorageDeleteIntent { .. }
            | Record::SandboxFileEffectIntent { .. } => {
                class.consume_effect_reserve = true;
                class.ensure_effect_reserve_bytes = class
                    .ensure_effect_reserve_bytes
                    .max(JOURNAL_TERMINAL_RESERVE_BYTES);
            }
            Record::ToolCall { .. } => {
                has_tool_call = true;
                class.ensure_effect_reserve_bytes = class
                    .ensure_effect_reserve_bytes
                    .max(JOURNAL_TERMINAL_RESERVE_BYTES);
            }
            Record::DefaultSandboxChanged { status } if status.state == SandboxState::Creating => {
                class.consume_effect_reserve = true;
                class.ensure_effect_reserve_bytes = class
                    .ensure_effect_reserve_bytes
                    .max(JOURNAL_TERMINAL_RESERVE_BYTES);
            }
            Record::DefaultSandboxChanged { .. } => {
                class.consume_effect_reserve = true;
                class.close_effect_reserve = true;
            }
            Record::TurnCompleted { .. }
            | Record::TurnFailed { .. }
            | Record::CustomerTerminalAcknowledged { .. }
            | Record::ManagedTerminalAcknowledged { .. }
            | Record::HandLost { .. } => {
                class.consume_lifecycle_reserve = true;
            }
            Record::State { state, .. }
                if matches!(
                    state.as_str(),
                    "ending" | "ended" | "deleting" | "deleted" | "failed"
                ) =>
            {
                class.consume_lifecycle_reserve = true;
            }
            _ => {}
        }
    }
    if completed_model && !has_tool_call {
        class.close_effect_reserve = true;
    }
    // A terminal turn releases any residual effect reservation. Its small terminal record itself
    // may consume that reserve first; the lifecycle reserve is the final fail-safe.
    if records.iter().any(|(_, record)| {
        matches!(
            record,
            Record::TurnCompleted { .. } | Record::TurnFailed { .. }
        )
    }) {
        class.consume_effect_reserve = true;
        class.close_effect_reserve = true;
    }
    class
}

#[doc(hidden)]
pub fn initial_retention(first: &Record, session_limit: u64) -> Result<JournalRetention> {
    let record_bytes = serialized_record_charge(&[(1, first.clone())])?;
    let metered_bytes = JOURNAL_SESSION_BASE_BYTES
        .checked_add(JOURNAL_LIFECYCLE_RESERVE_BYTES)
        .and_then(|value| value.checked_add(record_bytes))
        .ok_or_else(|| BrainError::Journal("journal retention meter overflowed".into()))?;
    if metered_bytes > session_limit {
        return Err(BrainError::SessionJournalQuotaExceeded {
            requested: metered_bytes,
            limit: session_limit,
        });
    }
    Ok(JournalRetention {
        metered_bytes,
        effect_reserve_bytes: 0,
        lifecycle_reserve_bytes: JOURNAL_LIFECYCLE_RESERVE_BYTES,
    })
}

#[doc(hidden)]
pub fn project_retention(
    current: JournalRetention,
    records: &[(u64, Record)],
    session_limit: u64,
) -> Result<JournalRetention> {
    let class = retention_class(records);
    let mut next = current;
    let mut charge = serialized_record_charge(records)?;

    if class.consume_effect_reserve {
        let consumed = charge.min(next.effect_reserve_bytes);
        charge -= consumed;
        next.effect_reserve_bytes -= consumed;
    }
    if class.consume_lifecycle_reserve {
        let consumed = charge.min(next.lifecycle_reserve_bytes);
        charge -= consumed;
        next.lifecycle_reserve_bytes -= consumed;
    }
    next.metered_bytes = next
        .metered_bytes
        .checked_add(charge)
        .ok_or_else(|| BrainError::Journal("journal retention meter overflowed".into()))?;

    if class.close_effect_reserve {
        next.metered_bytes = next
            .metered_bytes
            .checked_sub(next.effect_reserve_bytes)
            .ok_or_else(|| BrainError::Journal("journal effect reserve underflowed".into()))?;
        next.effect_reserve_bytes = 0;
    }
    if class.ensure_effect_reserve_bytes > 0
        && next.effect_reserve_bytes < class.ensure_effect_reserve_bytes
    {
        let requested = class.ensure_effect_reserve_bytes - next.effect_reserve_bytes;
        next.metered_bytes = next
            .metered_bytes
            .checked_add(requested)
            .ok_or_else(|| BrainError::Journal("journal effect reserve overflowed".into()))?;
        next.effect_reserve_bytes = class.ensure_effect_reserve_bytes;
    } else if class.ensure_effect_reserve_bytes > 0
        && next.effect_reserve_bytes > class.ensure_effect_reserve_bytes
    {
        let released = next.effect_reserve_bytes - class.ensure_effect_reserve_bytes;
        next.metered_bytes = next
            .metered_bytes
            .checked_sub(released)
            .ok_or_else(|| BrainError::Journal("journal effect reserve underflowed".into()))?;
        next.effect_reserve_bytes = class.ensure_effect_reserve_bytes;
    }

    if next.metered_bytes > session_limit {
        return Err(BrainError::SessionJournalQuotaExceeded {
            requested: next.metered_bytes.saturating_sub(current.metered_bytes),
            limit: session_limit,
        });
    }
    Ok(next)
}

#[doc(hidden)]
pub fn retention_delta(current: JournalRetention, next: JournalRetention) -> Result<i64> {
    i128::from(next.metered_bytes)
        .checked_sub(i128::from(current.metered_bytes))
        .and_then(|delta| i64::try_from(delta).ok())
        .ok_or_else(|| BrainError::Journal("journal retention delta exceeds i64".into()))
}

/// A record with its journal position, as read back.
#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: u64,
    pub ts_ms: u64,
    pub record: Record,
}

/// Default bounded journal replay page. DynamoDB itself caps one Query response at 1 MiB; using
/// the same byte ceiling in every adapter keeps local/test behavior representative.
pub const DEFAULT_RECORD_PAGE_ITEMS: usize = 32;
pub const DEFAULT_RECORD_PAGE_BYTES: usize = 1024 * 1024;

pub struct RecordPageQuery<'a> {
    pub session_id: &'a str,
    /// Exclusive durable sequence cursor.
    pub after: u64,
    /// Inclusive strong high-water captured before replay starts.
    pub through_seq: u64,
    pub limit: usize,
    pub max_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct RecordPage {
    pub entries: Vec<Entry>,
    /// Exclusive cursor for the next page. `None` means replay reached the fixed high-water,
    /// including when the remaining sequence range contains only ephemeral gaps.
    pub next_after: Option<u64>,
}

#[doc(hidden)]
pub fn validate_record_page_query(query: &RecordPageQuery<'_>) -> Result<(usize, usize)> {
    let limit = query.limit.clamp(1, 100);
    if query.max_bytes < MAX_SERIALIZED_RECORD_BYTES {
        return Err(BrainError::Invalid(format!(
            "journal page byte limit {} is below the maximum record size {MAX_SERIALIZED_RECORD_BYTES}",
            query.max_bytes
        )));
    }
    Ok((limit, query.max_bytes.min(DEFAULT_RECORD_PAGE_BYTES)))
}

impl Record {
    /// The agent an activity record belongs to; `None` for session-level records.
    pub fn agent(&self) -> Option<&str> {
        match self {
            Record::Assistant { agent, .. }
            | Record::Usage { agent, .. }
            | Record::ToolCall { agent, .. }
            | Record::ToolResult { agent, .. } => Some(agent),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fold
// ---------------------------------------------------------------------------------------------

/// The model-visible history rebuilt from records. `fold` is a loop over `apply` so the cold
/// (rehydrate) and hot (in-turn append) paths cannot drift.
#[derive(Debug, Default, Clone)]
pub struct Fold {
    pub history: Vec<Message>,
    /// Consecutive tool_result records group into one user message (Anthropic requires tool
    /// results to arrive as one user message per batch); flushed by the next non-result record.
    pending_results: Vec<ContentBlock>,
    pub turns: u64,
}

impl Fold {
    /// Resumes a fold from an already-rebuilt history (in-turn compaction).
    pub fn from_history(history: Vec<Message>) -> Self {
        Fold {
            history,
            pending_results: Vec::new(),
            turns: 0,
        }
    }

    pub fn apply(&mut self, record: &Record) {
        // Subagent records (slice 8) never enter the ROOT history: a child's assistant
        // message is not the parent's, and -- load-bearing -- a child record landing
        // between two root tool results of one batch must not flush them into separate
        // user messages (providers require one user message per result batch). The
        // parent's own `task` ToolCall/ToolResult carry the parent's agent id and fold
        // normally.
        if let Some(agent) = record.agent()
            && agent != "root"
        {
            return;
        }
        match record {
            Record::UserMessage {
                content,
                starts_turn,
                ..
            } => {
                if *starts_turn {
                    self.turns += 1;
                }
                if self.pending_results.is_empty() {
                    self.history.push(Message {
                        role: Role::User,
                        content: content.clone(),
                    });
                } else {
                    // A recovered/cancelled turn may end immediately after its
                    // tool results. Merge the next real user text into that same
                    // user message so provider histories still alternate roles.
                    let mut merged = std::mem::take(&mut self.pending_results);
                    merged.extend(content.clone());
                    self.history.push(Message {
                        role: Role::User,
                        content: merged,
                    });
                }
            }
            Record::TurnStarted { .. } => self.turns += 1,
            Record::Assistant { content, .. } => {
                self.flush_results();
                self.history.push(Message {
                    role: Role::Assistant,
                    content: content.clone(),
                });
            }
            Record::ToolResult {
                call,
                content,
                is_error,
                ..
            } => {
                self.pending_results.push(ContentBlock::ToolResult {
                    tool_use_id: call.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                });
            }
            Record::Usage { .. }
            | Record::ModelCallIntent { .. }
            | Record::ModelCallUnknown { .. }
            | Record::ModelAttemptSuperseded { .. }
            | Record::ModelCallCompleted { .. }
            | Record::CompactionIntent { .. }
            | Record::CompactionUnknown { .. }
            | Record::CompactionCompleted { .. }
            | Record::CustomerCallIntent { .. }
            | Record::ManagedCallIntent { .. }
            | Record::ManagedCallAccepted { .. }
            | Record::ManagedCallUnknown { .. }
            | Record::CustomerTerminalReceived { .. }
            | Record::CustomerTerminalAcknowledged { .. }
            | Record::ManagedTerminalReceived { .. }
            | Record::ManagedTerminalAcknowledged { .. }
            | Record::ToolCall { .. }
            | Record::TurnCompleted { .. }
            | Record::TurnFailed { .. }
            | Record::State { .. }
            | Record::HandLost { .. }
            | Record::StorageUploadReserved { .. }
            | Record::StorageUploadPublished { .. }
            | Record::StorageUploadCompleted { .. }
            | Record::StorageUploadExpired { .. }
            | Record::StorageDeleteIntent { .. }
            | Record::StorageDeleteCompleted { .. }
            | Record::SandboxFileEffectIntent { .. }
            | Record::SandboxFileEffectCompleted { .. }
            | Record::DefaultSandboxChanged { .. }
            | Record::ContextChunk { .. }
            | Record::ContextInstalled { .. }
            // Loop-land state is never kernel model input; contract loops compose their own
            // provider context from marks and the journal_read projections.
            | Record::LoopCustom { .. }
            | Record::LoopEvent { .. }
            | Record::LoopMark { .. }
            | Record::LoopKvSet { .. } => {}
        }
    }

    fn flush_results(&mut self) {
        if !self.pending_results.is_empty() {
            self.history.push(Message::tool_results(std::mem::take(
                &mut self.pending_results,
            )));
        }
    }

    /// Terminal flush: called once all records are applied.
    pub fn finish(&mut self) {
        self.flush_results();
    }
}

pub fn fold(entries: &[Entry]) -> Fold {
    let mut f = Fold::default();
    for e in entries {
        f.apply(&e.record);
    }
    f.finish();
    f
}

// ---------------------------------------------------------------------------------------------
// HEAD
// ---------------------------------------------------------------------------------------------

/// Everything durable about a session that is not a record. Rewritten whole on each commit
/// (single writer under the fence), carried as one JSON attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadDoc {
    /// Trusted tenancy principal sealed by the hosting composition. This value never comes from
    /// the public create body and is used by every tenant-scoped read.
    pub tenant_id: String,
    pub root_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Immutable root-to-parent path. Roots store an empty vector; a child stores its parent's
    /// chain followed by the direct parent id. The public depth cap bounds this at eight entries.
    /// Admission decisions condition every referenced ancestor so ending any subtree fences all
    /// of its already-existing descendants without a scan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ancestor_ids: Vec<String>,
    /// Optional customer-visible child label. Roots have no child label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_name: Option<String>,
    /// Immutable pointer to the exact parent context snapshot inherited by this child. Parent
    /// history remains append-only until recursive deletion, so descendants never copy prompts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_fork: Option<ContextForkDoc>,
    pub depth: u32,
    /// Authoritative durable event high-water mark, denormalized for bounded tenant discovery.
    pub last_seq: u64,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    /// Total active-turn phase. `None` iff no turn is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_phase: Option<String>,
    /// Bounded projection of the current logical provider operation/attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_attempt: Option<ProviderAttemptDoc>,
    #[serde(default)]
    pub active_context: HashMap<String, String>,
    #[serde(default)]
    pub active_rounds: u64,
    #[serde(default)]
    pub active_tool_calls: u64,
    #[serde(default)]
    pub message_replays: Vec<MessageReplayDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextPointerDoc>,
    pub turns: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// Durable wake-up anchor for background recovery. It is present only while work or cleanup
    /// can make progress without another customer request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_due_ms: Option<u64>,
    /// Consecutive background recovery failures used only to derive bounded retry backoff.
    #[serde(default)]
    pub recovery_attempt: u32,
    /// Hashes for replaying a create request without retaining the raw idempotency key or body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_key_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_ms: Option<u64>,
    /// True once `end` ran: compute released for good, the workspace is kept.
    #[serde(default)]
    pub ended: bool,
    pub prefix: PrefixDoc,
    /// Custody blob of the BYOK key, base64. Never the plaintext.
    pub key_b64: String,
    /// Custody blob of the session-wide Hand environment map, base64. Never plaintext.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hand_secrets_b64: String,
    #[serde(default)]
    pub session_storage_bytes: u64,
    /// Bytes in an outstanding staged upload. They are not published storage, but are included
    /// in the authoritative bounded storage gauge until staging deletion is durable.
    #[serde(default)]
    pub storage_reserved_bytes: u64,
    /// Last contribution durably applied to the tenant-wide storage meter. The desired
    /// contribution is always `session_storage_bytes + storage_reserved_bytes`; retaining the
    /// prior applied value lets every adapter calculate one exact delta without a pre-read.
    #[serde(default)]
    pub tenant_metered_storage_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_upload: Option<StorageUploadReservationDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_delete: Option<StorageDeleteReservationDoc>,
    /// Bounded exact terminal identities waiting for post-commit customer-app ACK. There are at
    /// most MAX_TOOL_CALLS_PER_MODEL_ROUND entries; payload bytes are never duplicated here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_customer_acks: Vec<CustomerTerminalAckDoc>,
    /// Bounded rooted receipts waiting for post-commit managed-Hand terminal ACK.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_managed_acks: Vec<ManagedTerminalAckDoc>,
    /// Brain-owned durable projection of the root tree's shared default target. Descendants
    /// address this same target through `root_id`; they do not get a second default sandbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_sandbox: Option<brain_protocol::hand::SandboxStatus>,
    /// Present iff the session has ever committed a loop record. Its presence gates the
    /// loop-state journal fold at rehydration, so sessions that never used loop state pay no
    /// extra read. The kv map itself lives in records, never here (it can reach 512 KiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_state: Option<LoopStateDoc>,
}

/// Bounded pointer to the session's loop-authored durable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopStateDoc {
    /// Seq of the newest committed loop record.
    pub last_seq: u64,
}

/// Small mutable control projection. Production stores rewrite only this value on a decision;
/// sealed configuration is an immutable sibling item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlDoc {
    pub tenant_id: String,
    pub last_seq: u64,
    pub state: String,
    pub failure: Option<FailureDoc>,
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_phase: Option<String>,
    pub provider_attempt: Option<ProviderAttemptDoc>,
    pub active_context: HashMap<String, String>,
    pub active_rounds: u64,
    pub active_tool_calls: u64,
    pub message_replays: Vec<MessageReplayDoc>,
    pub context: Option<ContextPointerDoc>,
    pub turns: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub recovery_due_ms: Option<u64>,
    pub recovery_attempt: u32,
    pub create_key_hash: Option<String>,
    pub create_request_hash: Option<String>,
    pub last_message_ms: Option<u64>,
    pub ended: bool,
    pub session_storage_bytes: u64,
    pub storage_reserved_bytes: u64,
    #[serde(default)]
    pub tenant_metered_storage_bytes: u64,
    pub storage_upload: Option<StorageUploadReservationDoc>,
    pub storage_delete: Option<StorageDeleteReservationDoc>,
    #[serde(default)]
    pub pending_customer_acks: Vec<CustomerTerminalAckDoc>,
    #[serde(default)]
    pub pending_managed_acks: Vec<ManagedTerminalAckDoc>,
    #[serde(default)]
    pub default_sandbox: Option<brain_protocol::hand::SandboxStatus>,
    #[serde(default)]
    pub loop_state: Option<LoopStateDoc>,
}

/// Immutable create-time material, stored once as `CONFIG` and content-addressed by its digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDoc {
    pub root_id: String,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub ancestor_ids: Vec<String>,
    pub child_name: Option<String>,
    pub context_fork: Option<ContextForkDoc>,
    pub depth: u32,
    pub prefix: PrefixDoc,
    pub key_b64: String,
    pub hand_secrets_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageUploadReservationDoc {
    pub transfer_id: String,
    pub key: String,
    pub bytes: u64,
    pub sha256: Option<String>,
    pub content_type: Option<String>,
    pub overwrite: bool,
    pub previous_bytes: u64,
    pub expires_at_ms: u64,
    /// `reserved | published | completed`. Published retains its byte reservation until staging
    /// deletion succeeds; completed is a bounded replay tombstone and reserves zero bytes.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDeleteReservationDoc {
    pub operation_id: String,
    pub key: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerTerminalAckDoc {
    pub turn: String,
    pub call: String,
    pub client_id: String,
    pub process_id: String,
    pub request_digest: String,
    pub terminal_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedTerminalAckDoc {
    pub turn: String,
    pub call: String,
    pub operation: brain_protocol::hand::OperationRef,
    pub terminal_digest: String,
}

impl HeadDoc {
    /// Lifecycle to persist when an admitted turn becomes quiescent.
    ///
    /// A subtree END fence deliberately leaves the interrupted turn projection in HEAD so a new
    /// owner can reconcile its durable effects. That recovery must clear the turn without ever
    /// reopening the lifecycle. Likewise, deletion/terminal lifecycle states dominate any late
    /// turn completion.
    pub fn lifecycle_after_turn(&self) -> String {
        if self.ended {
            match self.state.as_str() {
                "ended" | "deleting" | "deleted" => self.state.clone(),
                _ => "ending".into(),
            }
        } else {
            "open".into()
        }
    }

    /// Clone only the mutable control projection. Hot-path commits must never clone the sealed
    /// CONFIG payload (Tool schemas, rendered prefix, custody blobs) merely to discard it.
    pub fn control_doc(&self) -> ControlDoc {
        ControlDoc {
            tenant_id: self.tenant_id.clone(),
            last_seq: self.last_seq,
            state: self.state.clone(),
            failure: self.failure.clone(),
            turn: self.turn.clone(),
            active_phase: self.active_phase.clone(),
            provider_attempt: self.provider_attempt.clone(),
            active_context: self.active_context.clone(),
            active_rounds: self.active_rounds,
            active_tool_calls: self.active_tool_calls,
            message_replays: self.message_replays.clone(),
            context: self.context.clone(),
            turns: self.turns,
            created_ms: self.created_ms,
            updated_ms: self.updated_ms,
            recovery_due_ms: self.recovery_due_ms,
            recovery_attempt: self.recovery_attempt,
            create_key_hash: self.create_key_hash.clone(),
            create_request_hash: self.create_request_hash.clone(),
            last_message_ms: self.last_message_ms,
            ended: self.ended,
            session_storage_bytes: self.session_storage_bytes,
            storage_reserved_bytes: self.storage_reserved_bytes,
            tenant_metered_storage_bytes: self.tenant_metered_storage_bytes,
            storage_upload: self.storage_upload.clone(),
            storage_delete: self.storage_delete.clone(),
            pending_customer_acks: self.pending_customer_acks.clone(),
            pending_managed_acks: self.pending_managed_acks.clone(),
            default_sandbox: self.default_sandbox.clone(),
            loop_state: self.loop_state,
        }
    }

    /// Clone the immutable create-time CONFIG projection. This is used only while creating a
    /// session (and in explicit integrity tooling), never by an ordinary journal commit.
    pub fn config_doc(&self) -> ConfigDoc {
        ConfigDoc {
            root_id: self.root_id.clone(),
            parent_id: self.parent_id.clone(),
            ancestor_ids: self.ancestor_ids.clone(),
            child_name: self.child_name.clone(),
            context_fork: self.context_fork.clone(),
            depth: self.depth,
            prefix: self.prefix.clone(),
            key_b64: self.key_b64.clone(),
            hand_secrets_b64: self.hand_secrets_b64.clone(),
        }
    }

    pub fn split(&self) -> (ControlDoc, ConfigDoc) {
        (self.control_doc(), self.config_doc())
    }

    pub fn join(control: ControlDoc, config: ConfigDoc) -> Self {
        Self {
            tenant_id: control.tenant_id,
            root_id: config.root_id,
            parent_id: config.parent_id,
            ancestor_ids: config.ancestor_ids,
            child_name: config.child_name,
            context_fork: config.context_fork,
            depth: config.depth,
            last_seq: control.last_seq,
            state: control.state,
            failure: control.failure,
            turn: control.turn,
            active_phase: control.active_phase,
            provider_attempt: control.provider_attempt,
            active_context: control.active_context,
            active_rounds: control.active_rounds,
            active_tool_calls: control.active_tool_calls,
            message_replays: control.message_replays,
            context: control.context,
            turns: control.turns,
            created_ms: control.created_ms,
            updated_ms: control.updated_ms,
            recovery_due_ms: control.recovery_due_ms,
            recovery_attempt: control.recovery_attempt,
            create_key_hash: control.create_key_hash,
            create_request_hash: control.create_request_hash,
            last_message_ms: control.last_message_ms,
            ended: control.ended,
            prefix: config.prefix,
            key_b64: config.key_b64,
            hand_secrets_b64: config.hand_secrets_b64,
            session_storage_bytes: control.session_storage_bytes,
            storage_reserved_bytes: control.storage_reserved_bytes,
            tenant_metered_storage_bytes: control.tenant_metered_storage_bytes,
            storage_upload: control.storage_upload,
            storage_delete: control.storage_delete,
            pending_customer_acks: control.pending_customer_acks,
            pending_managed_acks: control.pending_managed_acks,
            default_sandbox: control.default_sandbox,
            loop_state: control.loop_state,
        }
    }
}

impl HeadDoc {
    /// Return the persisted control projection with a stable due-time recovery anchor.
    ///
    /// Active turns remain due while their lease is held; a crashed owner therefore cannot hide
    /// work by claiming it. Storage upload reservations sleep until their expiry unless staging
    /// cleanup is already pending. Quiescent sessions omit the index keys entirely.
    pub fn with_recovery_projection(&self, now_ms: u64) -> Self {
        let mut projected = self.clone();
        let lease_safe_due = now_ms.saturating_add(LEASE_MS + STEAL_GRACE_MS);
        let sandbox_due = self.default_sandbox.as_ref().and_then(|sandbox| {
            use brain_protocol::hand::SandboxState;
            match sandbox.state {
                SandboxState::Creating => {
                    Some(self.recovery_due_ms.unwrap_or(0).max(lease_safe_due))
                }
                SandboxState::Running | SandboxState::Suspended => {
                    sandbox.expires_at_ms.map(|expires| expires.get())
                }
                _ => None,
            }
        });
        let upload_due = self.storage_upload.as_ref().and_then(|upload| {
            match upload.state.as_str() {
                // A reservation is scheduled work, not an active effect. Lease heartbeats must
                // never postpone its fixed expiry. A prior cleanup failure may deliberately
                // move it later by the bounded recovery backoff.
                "reserved" | "inline_reserved" => Some(if self.recovery_attempt == 0 {
                    upload.expires_at_ms
                } else {
                    self.recovery_due_ms
                        .unwrap_or(upload.expires_at_ms)
                        .max(upload.expires_at_ms)
                }),
                "published" => Some(self.recovery_due_ms.unwrap_or(0).max(lease_safe_due)),
                _ => None,
            }
        });
        let due = if matches!(self.state.as_str(), "ending" | "deleting")
            || self.turn.is_some()
            || self.active_phase.is_some()
            || self.storage_delete.is_some()
            || !self.pending_customer_acks.is_empty()
            || !self.pending_managed_acks.is_empty()
        {
            Some(self.recovery_due_ms.unwrap_or(0).max(lease_safe_due))
        } else {
            match (sandbox_due, upload_due) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            }
        };
        projected.recovery_due_ms = due;
        if due.is_none() {
            projected.recovery_attempt = 0;
        }
        projected
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureDoc {
    pub code: String,
    pub message: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAttemptDoc {
    pub logical_operation_id: String,
    pub attempt_id: String,
    pub request_digest: String,
    pub state: String,
    pub replacements_used: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageReplayDoc {
    pub key_hash: String,
    pub request_hash: String,
    pub turn_id: String,
    pub user_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPointerDoc {
    pub checkpoint_id: String,
    pub base_checkpoint_id: Option<String>,
    pub covers_through_sequence: u64,
    pub chunk_start_sequence: u64,
    pub chunk_end_sequence: u64,
    pub retained_messages: u64,
    pub payload_digest: String,
    pub base_prefix_digest: String,
    pub source_context_digest: String,
    pub token_estimate: u64,
    pub context_generation: u64,
    pub summary_kind: String,
    pub compactor_provider: String,
    pub compactor_model: String,
    pub retained_from_sequence: u64,
    pub created_at_ms: u64,
}

/// Immutable parent-context inheritance descriptor for an ordinary child session. It contains
/// only a bounded pointer/high-water and a digest of the selected model history; raw prompt or
/// summary bytes remain in the parent's append-only journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextForkDoc {
    pub source_session_id: String,
    pub source_context_generation: u64,
    pub source_through_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_context: Option<ContextPointerDoc>,
    /// `all | none | last_n`.
    pub mode: String,
    /// Present only for `last_n`; always a positive count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_n: Option<u32>,
    pub resolved_turns: u32,
    pub source_projection_digest: String,
}

/// The sealed create-time configuration, minus the credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixDoc {
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Effective immutable model request capacity and the derived conversation/compaction
    /// budgets. They are sealed at root creation and inherited verbatim by every child.
    pub context_window_tokens: u32,
    pub context_soft_tokens: u32,
    pub context_hard_tokens: u32,
    pub context_tail_tokens: u32,
    pub context_summary_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Replacement attempts permitted only for crash recovery of an unknown provider call.
    pub provider_recovery_retries: u32,
    /// Host limits copied into this immutable configuration at create time.
    pub storage_max_object_bytes: u64,
    pub storage_max_session_bytes: u64,
    pub storage_transfer_ttl_ms: u64,
    #[serde(default = "default_additional_sandbox_limit")]
    pub max_additional_sandboxes_per_root: u32,
    /// Canonical normalized session outbound ceiling. Omission at the public API is sealed as
    /// deny-all; every Tool/target policy may only narrow this value.
    #[serde(default = "default_network_ceiling")]
    pub network: serde_json::Value,
    #[serde(default = "default_child_depth")]
    pub max_child_depth: u32,
    #[serde(default = "default_direct_children")]
    pub max_direct_children: u32,
    #[serde(default = "default_descendants")]
    pub max_descendants: u32,
    /// Tenant-scoped customer application binding. Present only when `.client()` Tools are
    /// declared; one connection multiplexes every session using the same identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_client_id: Option<String>,
    #[serde(default = "default_customer_submit_retries")]
    pub customer_submit_retries: u32,
    /// Exact provider-visible immutable request object, installed at create and replayed
    /// byte-for-byte (modulo insertion of dynamic messages).
    pub rendered_base: serde_json::Value,
    pub rendered_base_digest: String,
    pub prompt_cache_key: String,
    /// Exact native Tool definitions and execution seals, in cache-visible declaration order.
    /// Bundle bytes live in internal root-scoped object custody and never appear here.
    pub tools: Vec<brain_protocol::session::ToolConfig>,
    /// Immutable managed implementation descriptors. Children inherit these descriptors but
    /// resolve their own session-scoped binding identity; the referenced bytes remain root-owned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_bundles: Vec<brain_protocol::hand::BundleDescriptor>,
    /// Trusted host policies sealed for official engine capabilities. SDK callers can name only
    /// the capability; they cannot forge these execution semantics.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub official_capabilities: HashMap<String, crate::config::ServerToolPolicy>,
    pub hand_enabled: bool,
    pub shape: String,
    pub sync_interval_seconds: u64,
    /// Names only, allowing create-time required-env validation without persisting values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hand_env_keys: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// The sealed agentloop identity (`contracts/agentloop/v1` selector semantics). Absent on
    /// sessions created before selectors existed, which sealed the official `aex` policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agentloop: Option<AgentloopSelectorDoc>,
}

/// Which agentloop a session sealed at create. Children inherit the parent's selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentloopSelectorDoc {
    Official {
        name: String,
        version: String,
    },
    Custom {
        source_bundle_sha256: String,
        source_bundle_bytes: u64,
        toolchain: String,
    },
}

impl AgentloopSelectorDoc {
    /// The identity every pre-selector session sealed implicitly.
    pub fn official_aex() -> Self {
        Self::Official {
            name: "aex".into(),
            version: "1".into(),
        }
    }
}

fn default_customer_submit_retries() -> u32 {
    1
}

fn default_child_depth() -> u32 {
    4
}

fn default_direct_children() -> u32 {
    32
}

fn default_descendants() -> u32 {
    256
}

fn default_additional_sandbox_limit() -> u32 {
    2
}

fn default_network_ceiling() -> serde_json::Value {
    serde_json::json!({"outbound": "none"})
}

/// The claim a hydrating owner holds. Every commit is conditioned on it.
#[derive(Debug, Clone)]
pub struct Lease {
    pub fence: u64,
    pub last_seq: u64,
    pub retention: JournalRetention,
}

#[derive(Debug, Clone)]
pub struct Head {
    pub session_id: String,
    pub doc: HeadDoc,
    pub fence: u64,
    pub last_seq: u64,
    pub retention: JournalRetention,
}

/// Result of the constant-size lifecycle/admission fence. `newly_fenced` identifies the one call
/// that appended the durable State record; idempotent retries return the existing projection.
#[derive(Debug, Clone)]
pub struct EndFence {
    pub head: Head,
    pub newly_fenced: bool,
}

/// A bounded, tenant-scoped session listing. `cursor` is an opaque value returned by the
/// previous page; adapters must not scan other tenants to satisfy this query.
#[derive(Debug, Clone)]
pub struct SessionListQuery<'a> {
    pub tenant_id: &'a str,
    pub state: Option<&'a str>,
    pub limit: usize,
    pub cursor: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct SessionPage {
    pub sessions: Vec<SessionSummary>,
    pub next_cursor: Option<String>,
}

/// Strongly-consistent direct-child adjacency query. This is a base-partition read, never a
/// tenant GSI, so callers may use it after an end fence as a deletion/settlement barrier.
pub struct ChildListQuery<'a> {
    pub parent_id: &'a str,
    pub limit: usize,
    pub cursor: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ChildPage {
    pub sessions: Vec<SessionSummary>,
    pub next_cursor: Option<String>,
}

/// O(1)-addressable logical additional-sandbox inventory owned by Brain. Physical locator fields
/// remain opaque inside `status.target`; terminal entries are tombstones until explicit root
/// purge, so a stale logical id can never materialize a replacement target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxInventoryDoc {
    pub root_id: String,
    pub owner_session_id: String,
    pub sandbox_id: String,
    pub operation_id: String,
    pub request_digest: String,
    pub generation_intent: String,
    pub status: brain_protocol::hand::SandboxStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub version: u64,
    pub slot_released: bool,
}

#[derive(Debug, Clone)]
pub struct SandboxReserveRequest {
    pub root_id: String,
    pub owner_session_id: String,
    pub sandbox_id: String,
    pub operation_id: String,
    pub request_digest: String,
    pub generation_intent: String,
    pub initial_status: brain_protocol::hand::SandboxStatus,
    pub now_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SandboxUpdateRequest {
    pub root_id: String,
    pub sandbox_id: String,
    pub expected_version: u64,
    pub status: brain_protocol::hand::SandboxStatus,
    pub release_slot: bool,
    pub now_ms: u64,
}

pub struct SandboxListQuery<'a> {
    pub root_id: &'a str,
    pub limit: usize,
    pub cursor: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct SandboxPage {
    pub sandboxes: Vec<SandboxInventoryDoc>,
    pub next_cursor: Option<String>,
}

/// One bounded row from the sharded due-time recovery index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryItem {
    pub session_id: String,
    pub due_ms: u64,
    pub state: String,
    pub active_phase: Option<String>,
    pub last_seq: u64,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub updated_ms: u64,
}

pub struct RecoveryQuery<'a> {
    pub shard: &'a str,
    pub due_before_ms: u64,
    pub limit: usize,
    pub cursor: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct RecoveryPage {
    pub items: Vec<RecoveryItem>,
    pub next_cursor: Option<String>,
}

/// Bounded non-content deletion job/tombstone stored outside the session partition it deletes.
/// The terminal row exists only long enough to disambiguate a lost final response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionStatusDoc {
    pub session_id: String,
    pub tenant_id: String,
    pub root_id: String,
    /// Strong parent adjacency retained outside the content partition until final deletion can
    /// remove the exact edge. Absent only for roots and legacy tombstones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Exact tenant-meter contribution retained outside the session partition while physical
    /// cleanup runs. Final HEAD removal and meter release use this value in one transaction.
    #[serde(default)]
    pub metered_storage_bytes: u64,
    /// Exact journal/identity contribution to release only when final physical purge replaces
    /// HEAD+CONFIG with this tombstone. It includes unspent recovery/effect reservations.
    #[serde(default)]
    pub metered_journal_bytes: u64,
    /// `accepted | running | succeeded`.
    pub state: String,
    pub requested_at_ms: u64,
    pub updated_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub expires_at_ms: u64,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

pub const DELETION_TOMBSTONE_TTL_MS: u64 = 24 * 60 * 60 * 1_000;

pub const RECOVERY_SHARDS: usize = 16;

pub fn recovery_shard(session_id: &str) -> String {
    let digest = Sha256::digest(session_id.as_bytes());
    format!("r{:02x}", usize::from(digest[0]) % RECOVERY_SHARDS)
}

pub fn recovery_due_key(due_ms: u64, session_id: &str) -> String {
    format!("{due_ms:020}#{session_id}")
}

/// Bounded denormalized tenant-discovery row. It contains everything needed to render a public
/// Session list item and fold billing deltas, but no provider key, secret, Tool schema or context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub session_id: String,
    pub tenant_id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub child_name: Option<String>,
    pub context_fork: Option<ContextForkDoc>,
    pub depth: u32,
    pub state: String,
    pub failure: Option<FailureDoc>,
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_phase: Option<String>,
    pub turns: u64,
    pub last_seq: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub last_message_ms: Option<u64>,
    pub provider: String,
    pub model: String,
    pub context_window_tokens: u32,
    pub shape: String,
    pub base_url: Option<String>,
    pub metadata: HashMap<String, String>,
    pub session_storage_bytes: u64,
    pub storage_reserved_bytes: u64,
}

impl SessionSummary {
    pub fn from_head(session_id: &str, doc: &HeadDoc) -> Self {
        Self {
            session_id: session_id.to_owned(),
            tenant_id: doc.tenant_id.clone(),
            root_id: doc.root_id.clone(),
            parent_id: doc.parent_id.clone(),
            child_name: doc.child_name.clone(),
            context_fork: doc.context_fork.clone(),
            depth: doc.depth,
            state: doc.state.clone(),
            failure: doc.failure.clone(),
            turn: doc.turn.clone(),
            active_phase: doc.active_phase.clone(),
            turns: doc.turns,
            last_seq: doc.last_seq,
            created_ms: doc.created_ms,
            updated_ms: doc.updated_ms,
            last_message_ms: doc.last_message_ms,
            provider: doc.prefix.provider.clone(),
            model: doc.prefix.model.clone(),
            context_window_tokens: doc.prefix.context_window_tokens,
            shape: doc.prefix.shape.clone(),
            base_url: doc.prefix.base_url.clone(),
            metadata: doc.prefix.metadata.clone(),
            session_storage_bytes: doc.session_storage_bytes,
            storage_reserved_bytes: doc.storage_reserved_bytes,
        }
    }
}

/// Sort key shared by production stores. Reversing the millisecond timestamp makes the normal
/// ascending index traversal return newest sessions first without an in-memory sort.
pub fn tenant_session_sort_key(updated_ms: u64, session_id: &str) -> String {
    format!("{:020}#{session_id}", u64::MAX - updated_ms)
}

pub fn session_id_from_list_cursor(cursor: &str) -> Result<&str> {
    let (stamp, session_id) = cursor
        .split_once('#')
        .ok_or_else(|| BrainError::Invalid("session list cursor is malformed".into()))?;
    if stamp.len() != 20
        || !stamp.bytes().all(|byte| byte.is_ascii_digit())
        || session_id.is_empty()
    {
        return Err(BrainError::Invalid(
            "session list cursor is malformed".into(),
        ));
    }
    Ok(session_id)
}

// ---------------------------------------------------------------------------------------------
// Store: the persistence seam
// ---------------------------------------------------------------------------------------------
//
// [`JournalStore`] is the adapter trait: any backend that can honour these semantics can
// carry the journal. The semantics are not negotiable --
// - `create` refuses an existing session;
// - `claim` is the ONLY operation that advances the fence; it fails (`Fenced`) while another
//   live owner holds the lease, and may steal an expired one (plus grace);
// - `commit` is atomic: all records plus the head update land or nothing does; it fails
//   `Fenced` when the owner/fence does not match OR any record seq already exists (the
//   (session, seq) key is the idempotency barrier -- a redelivered decision loses the write,
//   it never duplicates);
// - `release` with a stale fence is a silent no-op (the releaser was superseded).
//
// Built-ins: [`MemoryStore`] here (local mode: full semantics, no durability) and
// `brain_aws::DynamoJournal` (production). The shared tests in this module run against any
// store; run them against yours.

/// Key shape shared by every backend that keys records textually: zero-padded so that
/// lexicographic order is numeric order (`E#10` must not sort before `E#9`).
pub fn record_sk(seq: u64) -> String {
    format!("E#{seq:020}")
}

pub fn session_pk(session_id: &str) -> String {
    format!("S#{session_id}")
}

/// How long a lease lives without renewal, and how much longer a steal waits beyond expiry.
/// The grace absorbs clock skew between instances; the fence, not the clock, decides whether
/// a stale owner can write.
pub const LEASE_MS: u64 = 60_000;
pub const STEAL_GRACE_MS: u64 = 5_000;
pub const RECOVERY_BACKOFF_BASE_MS: u64 = 1_000;
pub const RECOVERY_BACKOFF_MAX_MS: u64 = 5 * 60_000;

#[async_trait::async_trait]
pub trait JournalStore: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn create(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        _owner: &str,
        now_ms: u64,
        tenant_storage_limit: u64,
        retention: JournalRetention,
        retention_limits: JournalRetentionLimits,
    ) -> Result<()>;
    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head>;
    /// Atomically close this session's subtree admission, supersede any live owner, append the
    /// lifecycle State record, and expose immediate recovery. This MUST be one store decision:
    /// claim-then-commit leaves a descendant-admission race between those writes.
    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<EndFence>;
    async fn get_head(&self, session_id: &str) -> Result<Head>;
    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> Result<RecordPage>;
    #[allow(clippy::too_many_arguments)]
    async fn commit(
        &self,
        session_id: &str,
        _owner: &str,
        fence: u64,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
        now_ms: u64,
        tenant_storage_delta: i64,
        tenant_storage_limit: u64,
        retention: JournalRetention,
        tenant_retention_delta: i64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<()>;
    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()>;
    /// Failure-path transition that atomically releases ownership and schedules the next bounded
    /// recovery attempt. A separate commit+release would either keep work hidden behind the old
    /// lease or expose an immediately-due row while it is still owned.
    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> Result<()>;
    /// Lightweight lease renewal for a long-running external effect. It must not advance the
    /// fence or rewrite immutable/session history.
    async fn renew(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        now_ms: u64,
        recovery_due_ms: Option<u64>,
    ) -> Result<()>;
    /// Remove append-only history while retaining the HEAD and immutable CONFIG needed to retry
    /// deletion after a process failure. This is idempotent; only `finalize_deletion` may remove
    /// the recovery anchor because it atomically releases tenant storage/journal/identity meters.
    async fn purge_history(&self, session_id: &str) -> Result<u64>;
    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> Result<()>;
    async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>>;
    /// Atomically replace the final HEAD/CONFIG recovery anchor with a small success tombstone.
    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()>;
    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> Result<SessionPage>;
    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> Result<ChildPage>;
    /// Atomically reserve one root-scoped live slot and create the logical inventory row. Exact
    /// operation/digest replay returns the existing row without consuming another slot.
    async fn reserve_sandbox(&self, request: &SandboxReserveRequest)
    -> Result<SandboxInventoryDoc>;
    async fn get_sandbox(&self, root_id: &str, sandbox_id: &str) -> Result<SandboxInventoryDoc>;
    async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> Result<SandboxPage>;
    /// Version-fenced lifecycle update. `release_slot` decrements the root live counter exactly
    /// once, only while transitioning a nonterminal row to confirmed gone/terminated.
    async fn update_sandbox(&self, request: &SandboxUpdateRequest) -> Result<SandboxInventoryDoc>;
    /// Eventually-consistent discovery only. Every candidate must still win the strongly
    /// consistent base-HEAD claim/fence before executing recovery.
    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> Result<RecoveryPage>;
    /// Administrative enumeration for local integrity audits only. Hosted request paths use
    /// `list_session_page`, whose contract requires a native tenant index.
    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>>;
}

/// The journal as the rest of the brain sees it: a store plus this instance's owner
/// identity. All fence/lease bookkeeping the caller needs rides in [`Lease`].
#[derive(Clone)]
pub struct Journal {
    store: Arc<dyn JournalStore>,
    owner: String,
    tenant_storage_limit: u64,
    retention_limits: JournalRetentionLimits,
}

impl Journal {
    pub fn new(store: Arc<dyn JournalStore>, owner: impl Into<String>) -> Self {
        Self {
            store,
            owner: owner.into(),
            tenant_storage_limit: u64::MAX,
            retention_limits: JournalRetentionLimits::default(),
        }
    }

    /// Install the process/host tenant-wide storage ceiling. Brain composition calls this once;
    /// direct store tests retain an effectively unbounded meter unless they opt in explicitly.
    pub fn with_tenant_storage_limit(mut self, limit: u64) -> Self {
        self.tenant_storage_limit = limit;
        self
    }

    pub fn with_retention_limits(mut self, limits: JournalRetentionLimits) -> Self {
        self.retention_limits = limits;
        self
    }

    /// The local-mode journal: full semantics, no durability, no dependencies.
    pub fn new_memory(owner: impl Into<String>) -> Self {
        Self::new(Arc::new(MemoryStore::default()), owner)
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The same store under a different owner identity. Exists to test (and later simulate)
    /// multi-instance fencing; production instances each construct their own `Journal`.
    pub fn cloned_as(&self, owner: impl Into<String>) -> Journal {
        Journal {
            store: self.store.clone(),
            owner: owner.into(),
            tenant_storage_limit: self.tenant_storage_limit,
            retention_limits: self.retention_limits,
        }
    }

    pub async fn create(&self, session_id: &str, doc: &HeadDoc, first: &Record) -> Result<()> {
        validate_ancestor_path(doc)?;
        let now_ms = crate::wall_ms();
        let doc = doc.with_recovery_projection(now_ms);
        validate_config_doc(&doc)?;
        validate_decision(session_id, &[(1, first.clone())], &doc)?;
        let retention = initial_retention(first, self.retention_limits.session_bytes)?;
        self.store
            .create(
                session_id,
                &doc,
                first,
                &self.owner,
                now_ms,
                self.tenant_storage_limit,
                retention,
                self.retention_limits,
            )
            .await
    }

    pub async fn claim(&self, session_id: &str) -> Result<Head> {
        self.store
            .claim(session_id, &self.owner, crate::wall_ms())
            .await
    }

    pub async fn fence_end(&self, session_id: &str) -> Result<EndFence> {
        self.store
            .fence_end(session_id, crate::wall_ms(), self.retention_limits)
            .await
    }

    pub async fn get_head(&self, session_id: &str) -> Result<Head> {
        self.store.get_head(session_id).await
    }

    pub async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>> {
        let through_seq = self.get_head(session_id).await?.last_seq;
        self.read_records_through(session_id, after, through_seq)
            .await
    }

    /// Bounded-snapshot collection helper for resident reconstruction. The caller supplies the
    /// strong HEAD high-water it already captured, so a hot journal cannot extend this replay.
    /// Public lifetime replay uses pages directly and never collects an unbounded journal.
    pub async fn read_records_through(
        &self,
        session_id: &str,
        after: u64,
        through_seq: u64,
    ) -> Result<Vec<Entry>> {
        let mut cursor = after;
        let mut entries = Vec::new();
        loop {
            let page = self
                .read_record_page(&RecordPageQuery {
                    session_id,
                    after: cursor,
                    through_seq,
                    limit: DEFAULT_RECORD_PAGE_ITEMS,
                    max_bytes: DEFAULT_RECORD_PAGE_BYTES,
                })
                .await?;
            entries.extend(page.entries);
            let Some(next) = page.next_after else {
                return Ok(entries);
            };
            cursor = next;
        }
    }

    pub async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> Result<RecordPage> {
        self.store.read_record_page(query).await
    }

    /// One decision, one durable write. `high_water` is the highest seq allocated by the
    /// session -- including ephemeral (never-journaled) event seqs -- so a rehydrated
    /// session never re-issues an id a client may already have seen.
    pub async fn commit(
        &self,
        session_id: &str,
        lease: &mut Lease,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
    ) -> Result<HeadDoc> {
        let now_ms = crate::wall_ms();
        let mut doc = doc.with_recovery_projection(now_ms);
        let desired_meter = doc
            .session_storage_bytes
            .checked_add(doc.storage_reserved_bytes)
            .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
        let tenant_storage_delta = i128::from(desired_meter)
            .checked_sub(i128::from(doc.tenant_metered_storage_bytes))
            .and_then(|delta| i64::try_from(delta).ok())
            .ok_or_else(|| BrainError::Journal("tenant storage delta exceeds i64".into()))?;
        doc.tenant_metered_storage_bytes = desired_meter;
        validate_decision(session_id, records, &doc)?;
        let next_retention = project_retention(
            lease.retention,
            records,
            self.retention_limits.session_bytes,
        )?;
        let tenant_retention_delta = retention_delta(lease.retention, next_retention)?;
        self.store
            .commit(
                session_id,
                &self.owner,
                lease.fence,
                records,
                &doc,
                high_water,
                now_ms,
                tenant_storage_delta,
                self.tenant_storage_limit,
                next_retention,
                tenant_retention_delta,
                self.retention_limits,
            )
            .await?;
        lease.last_seq = high_water;
        lease.retention = next_retention;
        Ok(doc)
    }

    pub async fn release(&self, session_id: &str, lease: &Lease) -> Result<()> {
        self.store
            .release(session_id, &self.owner, lease.fence)
            .await
    }

    pub async fn renew(
        &self,
        session_id: &str,
        lease: &Lease,
        advance_active_due: bool,
    ) -> Result<()> {
        let now_ms = crate::wall_ms();
        self.store
            .renew(
                session_id,
                &self.owner,
                lease.fence,
                now_ms,
                advance_active_due.then(|| now_ms.saturating_add(LEASE_MS + STEAL_GRACE_MS)),
            )
            .await
    }

    /// Persist bounded exponential recovery backoff after a claimed recovery failed. A busy
    /// lease owned by another process is never modified.
    pub async fn defer_recovery(&self, session_id: &str) -> Result<()> {
        let head = self.get_head(session_id).await?;
        let attempt = head.doc.recovery_attempt.saturating_add(1);
        let exponential = RECOVERY_BACKOFF_BASE_MS
            .saturating_mul(1u64 << attempt.saturating_sub(1).min(18))
            .min(RECOVERY_BACKOFF_MAX_MS);
        let mut digest = Sha256::new();
        digest.update(session_id.as_bytes());
        digest.update(attempt.to_be_bytes());
        let bytes = digest.finalize();
        let jitter = u64::from_be_bytes(bytes[..8].try_into().expect("eight digest bytes"))
            % (exponential / 4 + 1);
        let mut doc = head.doc;
        doc.recovery_attempt = attempt;
        let due_ms = crate::wall_ms()
            .saturating_add(exponential)
            .saturating_add(jitter);
        doc.recovery_due_ms = Some(due_ms);
        validate_decision(session_id, &[], &doc)?;
        self.store
            .release_and_schedule(session_id, &self.owner, head.fence, &doc, due_ms)
            .await
    }

    pub async fn purge_history(&self, session_id: &str) -> Result<u64> {
        self.store.purge_history(session_id).await
    }

    pub async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> Result<()> {
        self.store.put_deletion_status(status).await
    }

    pub async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>> {
        self.store.get_deletion_status(session_id).await
    }

    pub async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()> {
        self.store.finalize_deletion(status).await
    }

    pub async fn list_session_page(&self, query: &SessionListQuery<'_>) -> Result<SessionPage> {
        self.store.list_session_page(query).await
    }

    pub async fn list_child_page(&self, query: &ChildListQuery<'_>) -> Result<ChildPage> {
        self.store.list_child_page(query).await
    }

    pub async fn reserve_sandbox(
        &self,
        request: &SandboxReserveRequest,
    ) -> Result<SandboxInventoryDoc> {
        self.store.reserve_sandbox(request).await
    }

    pub async fn get_sandbox(
        &self,
        root_id: &str,
        sandbox_id: &str,
    ) -> Result<SandboxInventoryDoc> {
        self.store.get_sandbox(root_id, sandbox_id).await
    }

    pub async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> Result<SandboxPage> {
        self.store.list_sandbox_page(query).await
    }

    pub async fn update_sandbox(
        &self,
        request: &SandboxUpdateRequest,
    ) -> Result<SandboxInventoryDoc> {
        self.store.update_sandbox(request).await
    }

    pub async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> Result<RecoveryPage> {
        self.store.list_recovery_page(query).await
    }

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        self.store.list_sessions(limit).await
    }
}

// ---------------------------------------------------------------------------------------------
// The in-memory backend
// ---------------------------------------------------------------------------------------------

struct MemSession {
    doc: HeadDoc,
    retention: JournalRetention,
    direct_children: u32,
    descendants: u32,
    live_sandboxes: u32,
    fence: u64,
    last_seq: u64,
    owner: Option<String>,
    lease_expires_ms: u64,
    records: std::collections::BTreeMap<u64, (u64, Record)>,
}

/// The reference store: exact semantics, zero durability, zero dependencies.
#[derive(Default)]
pub struct MemoryStore {
    sessions: std::sync::Mutex<HashMap<String, MemSession>>,
    tenant_storage: std::sync::Mutex<HashMap<String, u64>>,
    /// `(metered journal bytes, retained session identities)` per tenant.
    tenant_retention: std::sync::Mutex<HashMap<String, (u64, u64)>>,
    child_links:
        std::sync::Mutex<HashMap<String, std::collections::BTreeMap<String, SessionSummary>>>,
    sandboxes:
        std::sync::Mutex<HashMap<String, std::collections::BTreeMap<String, SandboxInventoryDoc>>>,
    deletions: std::sync::Mutex<HashMap<String, DeletionStatusDoc>>,
}

#[async_trait::async_trait]
impl JournalStore for MemoryStore {
    async fn create(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        _owner: &str,
        now_ms: u64,
        tenant_storage_limit: u64,
        retention: JournalRetention,
        retention_limits: JournalRetentionLimits,
    ) -> Result<()> {
        validate_ancestor_path(doc)?;
        validate_config_doc(doc)?;
        validate_decision(session_id, &[(1, first.clone())], doc)?;
        if retention != initial_retention(first, retention_limits.session_bytes)? {
            return Err(BrainError::Journal(
                "create journal retention projection does not match the canonical charge".into(),
            ));
        }
        if doc.session_storage_bytes != 0 || doc.storage_reserved_bytes != 0 {
            return Err(BrainError::Invalid(
                "new sessions must start with zero public session storage".into(),
            ));
        }
        if doc.parent_id.is_some() && doc.tenant_metered_storage_bytes != 0 {
            return Err(BrainError::Invalid(
                "child sessions cannot reserve root-owned bundle storage".into(),
            ));
        }
        let mut map = self.sessions.lock().expect("memory journal");
        if map.contains_key(session_id) {
            return Err(BrainError::Invalid(format!(
                "session {session_id} already exists"
            )));
        }
        let mut tenant_storage = self
            .tenant_storage
            .lock()
            .expect("memory tenant storage meter");
        let used = tenant_storage.get(&doc.tenant_id).copied().unwrap_or(0);
        let next_tenant_storage = used
            .checked_add(doc.tenant_metered_storage_bytes)
            .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
        if next_tenant_storage > tenant_storage_limit {
            return Err(BrainError::TenantStorageQuotaExceeded {
                requested: doc.tenant_metered_storage_bytes,
                limit: tenant_storage_limit,
            });
        }
        let mut tenant_retention = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let (used_journal_bytes, retained_sessions) = tenant_retention
            .get(&doc.tenant_id)
            .copied()
            .unwrap_or((0, 0));
        let next_journal_bytes = used_journal_bytes
            .checked_add(retention.metered_bytes)
            .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
        if next_journal_bytes > retention_limits.tenant_bytes {
            return Err(BrainError::TenantJournalQuotaExceeded {
                requested: retention.metered_bytes,
                limit: retention_limits.tenant_bytes,
            });
        }
        let next_retained_sessions = retained_sessions.checked_add(1).ok_or_else(|| {
            BrainError::Journal("tenant retained-session meter overflowed".into())
        })?;
        if next_retained_sessions > retention_limits.tenant_sessions {
            return Err(BrainError::TenantRetainedSessionQuotaExceeded {
                limit: retention_limits.tenant_sessions,
            });
        }
        if let Some(parent_id) = &doc.parent_id {
            let parent_doc = &map
                .get(parent_id)
                .ok_or_else(|| BrainError::NoSuchSession(parent_id.clone()))?
                .doc;
            let mut expected_ancestors = parent_doc.ancestor_ids.clone();
            expected_ancestors.push(parent_id.clone());
            if doc.ancestor_ids != expected_ancestors {
                return Err(BrainError::Invalid(
                    "child ancestor path does not extend its direct parent".into(),
                ));
            }
            for ancestor_id in &doc.ancestor_ids {
                let ancestor = map
                    .get(ancestor_id)
                    .ok_or_else(|| BrainError::NoSuchSession(ancestor_id.clone()))?;
                if ancestor.doc.root_id != doc.root_id || !child_admission_open(&ancestor.doc) {
                    return Err(BrainError::Invalid(
                        "child admission is closed by an ancestor fence".into(),
                    ));
                }
            }
            let parent = map
                .get(parent_id)
                .ok_or_else(|| BrainError::NoSuchSession(parent_id.clone()))?;
            let root = map
                .get(&doc.root_id)
                .ok_or_else(|| BrainError::NoSuchSession(doc.root_id.clone()))?;
            if !child_admission_open(&parent.doc)
                || !child_admission_open(&root.doc)
                || parent.doc.root_id != doc.root_id
                || doc.depth != parent.doc.depth.saturating_add(1)
                || parent.doc.depth >= root.doc.prefix.max_child_depth
            {
                return Err(BrainError::Invalid(
                    "child admission is closed or its rooted scope is stale".into(),
                ));
            }
            if parent.direct_children >= root.doc.prefix.max_direct_children
                || root.descendants >= root.doc.prefix.max_descendants
            {
                return Err(BrainError::Overloaded);
            }
            let parent_direct_children = parent.direct_children;
            let root_descendants = root.descendants;
            if parent_id == &doc.root_id {
                let root = map.get_mut(parent_id).expect("root checked above");
                root.direct_children = root.direct_children.saturating_add(1);
                root.descendants = root.descendants.saturating_add(1);
            } else {
                map.get_mut(parent_id)
                    .expect("parent checked above")
                    .direct_children = parent_direct_children.saturating_add(1);
                map.get_mut(&doc.root_id)
                    .expect("root checked above")
                    .descendants = root_descendants.saturating_add(1);
            }
        }
        let mut records = std::collections::BTreeMap::new();
        records.insert(1, (now_ms, first.clone()));
        map.insert(
            session_id.to_string(),
            MemSession {
                doc: doc.clone(),
                retention,
                direct_children: 0,
                descendants: 0,
                live_sandboxes: 0,
                fence: 0,
                last_seq: 1,
                owner: None,
                lease_expires_ms: 0,
                records,
            },
        );
        tenant_storage.insert(doc.tenant_id.clone(), next_tenant_storage);
        tenant_retention.insert(
            doc.tenant_id.clone(),
            (next_journal_bytes, next_retained_sessions),
        );
        drop(tenant_retention);
        drop(tenant_storage);
        drop(map);
        if let Some(parent_id) = &doc.parent_id {
            self.child_links
                .lock()
                .expect("memory child links")
                .entry(parent_id.clone())
                .or_default()
                .insert(
                    session_id.to_owned(),
                    SessionSummary::from_head(session_id, doc),
                );
        }
        Ok(())
    }

    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head> {
        let mut map = self.sessions.lock().expect("memory journal");
        let s = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        let claimable = match &s.owner {
            None => true,
            Some(o) if o == owner => true,
            Some(_) => s.lease_expires_ms < now_ms.saturating_sub(STEAL_GRACE_MS),
        };
        if !claimable {
            return Err(BrainError::Fenced);
        }
        s.owner = Some(owner.to_string());
        s.lease_expires_ms = now_ms + LEASE_MS;
        s.fence += 1;
        Ok(Head {
            session_id: session_id.to_string(),
            doc: s.doc.clone(),
            fence: s.fence,
            last_seq: s.last_seq,
            retention: s.retention,
        })
    }

    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<EndFence> {
        let mut map = self.sessions.lock().expect("memory journal");
        let current = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        let head = Head {
            session_id: session_id.to_owned(),
            doc: current.doc.clone(),
            fence: current.fence,
            last_seq: current.last_seq,
            retention: current.retention,
        };
        let Some((doc, sequence, record)) = project_end_fence(&head, now_ms)? else {
            return Ok(EndFence {
                head,
                newly_fenced: false,
            });
        };
        let next_fence = head
            .fence
            .checked_add(1)
            .ok_or_else(|| BrainError::Journal("journal fence exhausted".into()))?;
        let next_retention = project_retention(
            head.retention,
            &[(sequence, record.clone())],
            retention_limits.session_bytes,
        )?;
        let delta = retention_delta(head.retention, next_retention)?;
        let mut tenant_retention = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let meter = tenant_retention.entry(doc.tenant_id.clone()).or_default();
        let next_tenant_bytes = if delta >= 0 {
            let requested = delta as u64;
            let next = meter
                .0
                .checked_add(requested)
                .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
            if next > retention_limits.tenant_bytes {
                return Err(BrainError::TenantJournalQuotaExceeded {
                    requested,
                    limit: retention_limits.tenant_bytes,
                });
            }
            next
        } else {
            meter.0.checked_sub(delta.unsigned_abs()).ok_or_else(|| {
                BrainError::Journal("tenant journal meter would become negative".into())
            })?
        };
        let current = map
            .get_mut(session_id)
            .expect("session remains under memory journal lock");
        current.doc = doc.clone();
        current.fence = next_fence;
        current.last_seq = sequence;
        current.retention = next_retention;
        current.owner = None;
        current.lease_expires_ms = 0;
        current.records.insert(sequence, (now_ms, record));
        meter.0 = next_tenant_bytes;
        if let Some(parent_id) = &doc.parent_id {
            self.child_links
                .lock()
                .expect("memory child links")
                .entry(parent_id.clone())
                .or_default()
                .insert(
                    session_id.to_owned(),
                    SessionSummary::from_head(session_id, &doc),
                );
        }
        Ok(EndFence {
            head: Head {
                session_id: session_id.to_owned(),
                doc,
                fence: next_fence,
                last_seq: sequence,
                retention: next_retention,
            },
            newly_fenced: true,
        })
    }

    async fn get_head(&self, session_id: &str) -> Result<Head> {
        let map = self.sessions.lock().expect("memory journal");
        let s = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        Ok(Head {
            session_id: session_id.to_string(),
            doc: s.doc.clone(),
            fence: s.fence,
            last_seq: s.last_seq,
            retention: s.retention,
        })
    }

    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> Result<RecordPage> {
        let (limit, max_bytes) = validate_record_page_query(query)?;
        let map = self.sessions.lock().expect("memory journal");
        let s = map
            .get(query.session_id)
            .ok_or_else(|| BrainError::NoSuchSession(query.session_id.into()))?;
        if query.after >= query.through_seq {
            return Ok(RecordPage {
                entries: Vec::new(),
                next_after: None,
            });
        }
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        let mut more = false;
        for (seq, (ts_ms, record)) in s
            .records
            .range(query.after.saturating_add(1)..=query.through_seq)
        {
            let record_bytes = serde_json::to_vec(record)?.len();
            if entries.len() >= limit || bytes.saturating_add(record_bytes) > max_bytes {
                more = true;
                break;
            }
            bytes = bytes.saturating_add(record_bytes);
            entries.push(Entry {
                seq: *seq,
                ts_ms: *ts_ms,
                record: record.clone(),
            });
        }
        let next_after = more.then(|| entries.last().expect("page limit admits one record").seq);
        Ok(RecordPage {
            entries,
            next_after,
        })
    }

    async fn commit(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
        now_ms: u64,
        tenant_storage_delta: i64,
        tenant_storage_limit: u64,
        retention: JournalRetention,
        tenant_retention_delta: i64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<()> {
        validate_decision(session_id, records, doc)?;
        let mut map = self.sessions.lock().expect("memory journal");
        if requires_ancestor_admission(records) {
            for ancestor_id in &doc.ancestor_ids {
                let ancestor = map
                    .get(ancestor_id)
                    .ok_or_else(|| BrainError::NoSuchSession(ancestor_id.clone()))?;
                if ancestor.doc.root_id != doc.root_id || !child_admission_open(&ancestor.doc) {
                    return Err(BrainError::Fenced);
                }
            }
        }
        let current = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if current.fence != fence || current.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        if records
            .iter()
            .any(|(seq, _)| current.records.contains_key(seq))
        {
            return Err(BrainError::Fenced);
        }
        let expected_retention =
            project_retention(current.retention, records, retention_limits.session_bytes)?;
        if retention != expected_retention
            || tenant_retention_delta != retention_delta(current.retention, retention)?
        {
            return Err(BrainError::Journal(
                "journal retention transition does not match the canonical charge".into(),
            ));
        }
        let mut tenant_storage = self
            .tenant_storage
            .lock()
            .expect("memory tenant storage meter");
        let used = tenant_storage.get(&doc.tenant_id).copied().unwrap_or(0);
        let next_tenant_storage = if tenant_storage_delta >= 0 {
            let requested = tenant_storage_delta as u64;
            let next = used
                .checked_add(requested)
                .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
            if next > tenant_storage_limit {
                return Err(BrainError::TenantStorageQuotaExceeded {
                    requested,
                    limit: tenant_storage_limit,
                });
            }
            next
        } else {
            let released = tenant_storage_delta.unsigned_abs();
            used.checked_sub(released).ok_or_else(|| {
                BrainError::Journal("tenant storage meter would become negative".into())
            })?
        };
        let mut tenant_retention = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let meter = tenant_retention.entry(doc.tenant_id.clone()).or_default();
        let next_tenant_journal = if tenant_retention_delta >= 0 {
            let requested = tenant_retention_delta as u64;
            let next = meter
                .0
                .checked_add(requested)
                .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
            if next > retention_limits.tenant_bytes {
                return Err(BrainError::TenantJournalQuotaExceeded {
                    requested,
                    limit: retention_limits.tenant_bytes,
                });
            }
            next
        } else {
            meter
                .0
                .checked_sub(tenant_retention_delta.unsigned_abs())
                .ok_or_else(|| {
                    BrainError::Journal("tenant journal meter would become negative".into())
                })?
        };
        let s = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        for (seq, record) in records {
            s.records.insert(*seq, (now_ms, record.clone()));
        }
        s.doc = doc.clone();
        s.retention = retention;
        s.last_seq = high_water;
        s.lease_expires_ms = now_ms + LEASE_MS; // renew; deliberately no fence bump
        tenant_storage.insert(doc.tenant_id.clone(), next_tenant_storage);
        meter.0 = next_tenant_journal;
        if let Some(parent_id) = &doc.parent_id {
            self.child_links
                .lock()
                .expect("memory child links")
                .entry(parent_id.clone())
                .or_default()
                .insert(
                    session_id.to_owned(),
                    SessionSummary::from_head(session_id, doc),
                );
        }
        Ok(())
    }

    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        if let Some(s) = map.get_mut(session_id)
            && s.fence == fence
            && s.owner.as_deref() == Some(owner)
        {
            s.owner = None;
            s.lease_expires_ms = 0;
        }
        Ok(())
    }

    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        let session = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if session.fence != fence || session.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        session.doc = doc.clone();
        session.doc.recovery_due_ms = Some(due_ms);
        session.owner = None;
        session.lease_expires_ms = 0;
        Ok(())
    }

    async fn renew(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        now_ms: u64,
        recovery_due_ms: Option<u64>,
    ) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        let session = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if session.fence != fence || session.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        session.lease_expires_ms = now_ms.saturating_add(LEASE_MS);
        if let Some(recovery_due_ms) = recovery_due_ms {
            session.doc.recovery_due_ms = Some(recovery_due_ms);
        }
        Ok(())
    }

    async fn purge_history(&self, session_id: &str) -> Result<u64> {
        let mut map = self.sessions.lock().expect("memory journal");
        let Some(session) = map.get_mut(session_id) else {
            return Ok(0);
        };
        let removed = session.records.len() as u64;
        session.records.clear();
        let sandboxes = self
            .sandboxes
            .lock()
            .expect("memory sandbox inventory")
            .remove(session_id)
            .map_or(0, |items| items.len() as u64);
        Ok(removed.saturating_add(sandboxes))
    }

    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> Result<()> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions
            .get(&status.session_id)
            .is_some_and(|existing| existing.state == "succeeded" && status.state != "succeeded")
        {
            return Ok(());
        }
        deletions.insert(status.session_id.clone(), status.clone());
        Ok(())
    }

    async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions.get(session_id).is_some_and(|status| {
            status.state == "succeeded" && status.expires_at_ms <= crate::wall_ms()
        }) {
            deletions.remove(session_id);
        }
        Ok(deletions.get(session_id).cloned())
    }

    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions
            .get(&status.session_id)
            .is_some_and(|existing| existing.state == "succeeded")
        {
            return Ok(());
        }
        let mut sessions = self.sessions.lock().expect("memory journal");
        let mut meters = self
            .tenant_storage
            .lock()
            .expect("memory tenant storage meter");
        let mut retention_meters = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let next_meter = if let Some(session) = sessions.get(&status.session_id) {
            if session.doc.tenant_id != status.tenant_id
                || session.doc.tenant_metered_storage_bytes != status.metered_storage_bytes
                || session.retention.metered_bytes != status.metered_journal_bytes
            {
                return Err(BrainError::Journal(
                    "deletion status tenant meter anchor does not match HEAD".into(),
                ));
            }
            let used = meters.get(&status.tenant_id).copied().unwrap_or(0);
            Some(
                used.checked_sub(status.metered_storage_bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("tenant storage meter would become negative".into())
                    })?,
            )
        } else if status.metered_storage_bytes == 0 {
            None
        } else {
            return Err(BrainError::Journal(
                "deletion lost its metered session anchor before final release".into(),
            ));
        };
        let next_retention_meter = if sessions.contains_key(&status.session_id) {
            let (bytes, identities) = retention_meters
                .get(&status.tenant_id)
                .copied()
                .unwrap_or((0, 0));
            Some((
                bytes
                    .checked_sub(status.metered_journal_bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("tenant journal meter would become negative".into())
                    })?,
                identities.checked_sub(1).ok_or_else(|| {
                    BrainError::Journal(
                        "tenant retained-session meter would become negative".into(),
                    )
                })?,
            ))
        } else if status.metered_journal_bytes == 0 {
            None
        } else {
            return Err(BrainError::Journal(
                "deletion lost its retained-session anchor before final release".into(),
            ));
        };
        sessions.remove(&status.session_id);
        if let Some(parent_id) = &status.parent_id {
            if parent_id == &status.root_id {
                if let Some(root) = sessions.get_mut(parent_id) {
                    root.direct_children = root.direct_children.saturating_sub(1);
                    root.descendants = root.descendants.saturating_sub(1);
                }
            } else {
                if let Some(parent) = sessions.get_mut(parent_id) {
                    parent.direct_children = parent.direct_children.saturating_sub(1);
                }
                if let Some(root) = sessions.get_mut(&status.root_id) {
                    root.descendants = root.descendants.saturating_sub(1);
                }
            }
        }
        if let Some(next) = next_meter {
            meters.insert(status.tenant_id.clone(), next);
        }
        if let Some(next) = next_retention_meter {
            retention_meters.insert(status.tenant_id.clone(), next);
        }
        drop(retention_meters);
        drop(meters);
        deletions.insert(status.session_id.clone(), status.clone());
        drop(sessions);
        drop(deletions);
        let mut child_links = self.child_links.lock().expect("memory child links");
        child_links.remove(&status.session_id);
        if let Some(parent_id) = &status.parent_id
            && let Some(children) = child_links.get_mut(parent_id)
        {
            children.remove(&status.session_id);
            if children.is_empty() {
                child_links.remove(parent_id);
            }
        }
        Ok(())
    }

    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> Result<SessionPage> {
        let map = self.sessions.lock().expect("memory journal");
        let mut sessions: Vec<_> = map
            .iter()
            .filter(|(_, session)| session.doc.tenant_id == query.tenant_id)
            .filter(|(_, session)| query.state.is_none_or(|state| session.doc.state == state))
            .map(|(session_id, session)| SessionSummary::from_head(session_id, &session.doc))
            .collect();
        sessions.sort_by(|left, right| {
            right
                .updated_ms
                .cmp(&left.updated_ms)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        if let Some(cursor) = query.cursor {
            session_id_from_list_cursor(cursor)?;
            sessions.retain(|session| {
                tenant_session_sort_key(session.updated_ms, &session.session_id).as_str() > cursor
            });
        }
        let has_more = sessions.len() > query.limit;
        sessions.truncate(query.limit);
        let next_cursor = has_more.then(|| {
            let last = sessions.last().expect("a page with more rows is non-empty");
            tenant_session_sort_key(last.updated_ms, &last.session_id)
        });
        Ok(SessionPage {
            sessions,
            next_cursor,
        })
    }

    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> Result<ChildPage> {
        let links = self.child_links.lock().expect("memory child links");
        let Some(children) = links.get(query.parent_id) else {
            return Ok(ChildPage {
                sessions: Vec::new(),
                next_cursor: None,
            });
        };
        let mut rows = children
            .iter()
            .filter(|(child_id, _)| query.cursor.is_none_or(|cursor| child_id.as_str() > cursor))
            .map(|(_, summary)| summary.clone())
            .take(query.limit.clamp(1, 100) + 1)
            .collect::<Vec<_>>();
        let has_more = rows.len() > query.limit.clamp(1, 100);
        rows.truncate(query.limit.clamp(1, 100));
        let next_cursor = has_more.then(|| {
            rows.last()
                .expect("non-empty child page")
                .session_id
                .clone()
        });
        Ok(ChildPage {
            sessions: rows,
            next_cursor,
        })
    }

    async fn reserve_sandbox(
        &self,
        request: &SandboxReserveRequest,
    ) -> Result<SandboxInventoryDoc> {
        let mut sessions = self.sessions.lock().expect("memory journal");
        let root = sessions
            .get_mut(&request.root_id)
            .ok_or_else(|| BrainError::NoSuchSession(request.root_id.clone()))?;
        if root.doc.root_id != request.root_id || !child_admission_open(&root.doc) {
            return Err(BrainError::Invalid(
                "additional sandbox admission is closed for this root".into(),
            ));
        }
        let mut inventories = self.sandboxes.lock().expect("memory sandbox inventory");
        let inventory = inventories.entry(request.root_id.clone()).or_default();
        if let Some(existing) = inventory.get(&request.sandbox_id) {
            if existing.operation_id == request.operation_id
                && existing.request_digest == request.request_digest
                && existing.owner_session_id == request.owner_session_id
            {
                return Ok(existing.clone());
            }
            return Err(BrainError::IdempotencyConflict);
        }
        if root.live_sandboxes >= root.doc.prefix.max_additional_sandboxes_per_root {
            return Err(BrainError::SandboxResourceExhausted);
        }
        root.live_sandboxes = root.live_sandboxes.saturating_add(1);
        let doc = SandboxInventoryDoc {
            root_id: request.root_id.clone(),
            owner_session_id: request.owner_session_id.clone(),
            sandbox_id: request.sandbox_id.clone(),
            operation_id: request.operation_id.clone(),
            request_digest: request.request_digest.clone(),
            generation_intent: request.generation_intent.clone(),
            status: request.initial_status.clone(),
            created_at_ms: request.now_ms,
            updated_at_ms: request.now_ms,
            version: 1,
            slot_released: false,
        };
        inventory.insert(request.sandbox_id.clone(), doc.clone());
        Ok(doc)
    }

    async fn get_sandbox(&self, root_id: &str, sandbox_id: &str) -> Result<SandboxInventoryDoc> {
        self.sandboxes
            .lock()
            .expect("memory sandbox inventory")
            .get(root_id)
            .and_then(|inventory| inventory.get(sandbox_id))
            .cloned()
            .ok_or_else(|| BrainError::FileNotFound(format!("sandbox {sandbox_id}")))
    }

    async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> Result<SandboxPage> {
        let inventories = self.sandboxes.lock().expect("memory sandbox inventory");
        let Some(inventory) = inventories.get(query.root_id) else {
            return Ok(SandboxPage {
                sandboxes: Vec::new(),
                next_cursor: None,
            });
        };
        let limit = query.limit.clamp(1, 100);
        let mut rows = inventory
            .iter()
            .filter(|(sandbox_id, _)| {
                query
                    .cursor
                    .is_none_or(|cursor| sandbox_id.as_str() > cursor)
            })
            .map(|(_, sandbox)| sandbox.clone())
            .take(limit + 1)
            .collect::<Vec<_>>();
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = has_more.then(|| {
            rows.last()
                .expect("sandbox page with more rows is non-empty")
                .sandbox_id
                .clone()
        });
        Ok(SandboxPage {
            sandboxes: rows,
            next_cursor,
        })
    }

    async fn update_sandbox(&self, request: &SandboxUpdateRequest) -> Result<SandboxInventoryDoc> {
        let mut sessions = self.sessions.lock().expect("memory journal");
        let root = sessions
            .get_mut(&request.root_id)
            .ok_or_else(|| BrainError::NoSuchSession(request.root_id.clone()))?;
        let mut inventories = self.sandboxes.lock().expect("memory sandbox inventory");
        let item = inventories
            .get_mut(&request.root_id)
            .and_then(|inventory| inventory.get_mut(&request.sandbox_id))
            .ok_or_else(|| BrainError::FileNotFound(format!("sandbox {}", request.sandbox_id)))?;
        if item.version != request.expected_version {
            if serde_json::to_value(&item.status)? == serde_json::to_value(&request.status)? {
                return Ok(item.clone());
            }
            return Err(BrainError::Fenced);
        }
        if serde_json::to_value(&item.status.target)?
            != serde_json::to_value(&request.status.target)?
        {
            return Err(BrainError::Journal(
                "sandbox lifecycle update changed its sealed target".into(),
            ));
        }
        if item.slot_released && !request.release_slot {
            return Err(BrainError::SandboxGone);
        }
        if request.release_slot
            && !matches!(
                request.status.state,
                brain_protocol::hand::SandboxState::Gone
                    | brain_protocol::hand::SandboxState::Terminated
            )
        {
            return Err(BrainError::Journal(
                "sandbox slot may be released only for a confirmed terminal target".into(),
            ));
        }
        if request.release_slot && !item.slot_released {
            root.live_sandboxes = root.live_sandboxes.saturating_sub(1);
            item.slot_released = true;
        }
        item.status = request.status.clone();
        item.updated_at_ms = request.now_ms;
        item.version = item.version.saturating_add(1);
        Ok(item.clone())
    }

    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> Result<RecoveryPage> {
        let map = self.sessions.lock().expect("memory journal");
        let mut candidates =
            map.iter()
                .filter_map(|(session_id, session)| {
                    let due_ms = session.doc.recovery_due_ms?;
                    (recovery_shard(session_id) == query.shard && due_ms <= query.due_before_ms)
                        .then(|| {
                            (
                                recovery_due_key(due_ms, session_id),
                                RecoveryItem {
                                    session_id: session_id.clone(),
                                    due_ms,
                                    state: session.doc.state.clone(),
                                    active_phase: session.doc.active_phase.clone(),
                                    last_seq: session.last_seq,
                                    root_id: session.doc.root_id.clone(),
                                    parent_id: session.doc.parent_id.clone(),
                                    updated_ms: session.doc.updated_ms,
                                },
                            )
                        })
                })
                .filter(|(key, _)| query.cursor.is_none_or(|cursor| key.as_str() > cursor))
                .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        let limit = query.limit.clamp(1, 100);
        let has_more = candidates.len() > limit;
        candidates.truncate(limit);
        let next_cursor = has_more.then(|| candidates.last().expect("non-empty page").0.clone());
        Ok(RecoveryPage {
            items: candidates.into_iter().map(|(_, item)| item).collect(),
            next_cursor,
        })
    }

    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        let map = self.sessions.lock().expect("memory journal");
        Ok(map
            .iter()
            .take(limit)
            .map(|(sid, s)| Head {
                session_id: sid.clone(),
                doc: s.doc.clone(),
                fence: s.fence,
                last_seq: s.last_seq,
                retention: s.retention,
            })
            .collect())
    }
}

#[doc(hidden)]
pub fn child_admission_open(doc: &HeadDoc) -> bool {
    !doc.ended
        && !matches!(
            doc.state.as_str(),
            "ending" | "ended" | "deleting" | "deleted" | "failed"
        )
}

/// Build the one-record, constant-size END projection from a strong HEAD snapshot. Adapters use
/// this inside their own atomic compare-and-swap transaction. A concurrent commit either lands
/// before this snapshot and is retained, or loses the fence after this transition; there is no
/// interval in which the old owner is fenced while descendants still observe admission open.
pub fn project_end_fence(head: &Head, now_ms: u64) -> Result<Option<(HeadDoc, u64, Record)>> {
    if matches!(
        head.doc.state.as_str(),
        "ending" | "ended" | "deleting" | "deleted"
    ) {
        return Ok(None);
    }
    let sequence = head
        .last_seq
        .checked_add(1)
        .ok_or_else(|| BrainError::Journal("journal sequence exhausted".into()))?;
    let mut doc = head.doc.clone();
    doc.ended = true;
    doc.state = "ending".into();
    doc.updated_ms = now_ms;
    doc.last_seq = sequence;
    doc.recovery_attempt = 0;
    doc = doc.with_recovery_projection(now_ms);
    let record = Record::State {
        state: "ending".into(),
        turn: doc.turn.clone(),
    };
    validate_decision(&head.session_id, &[(sequence, record.clone())], &doc)?;
    Ok(Some((doc, sequence, record)))
}

/// A decision that starts a new turn must atomically observe every immutable ancestor fence.
/// Recovery/terminal commits deliberately do not use this predicate: an ancestor ending while a
/// child effect is already in flight must not prevent the child from recording its exact outcome.
pub fn requires_ancestor_admission(records: &[(u64, Record)]) -> bool {
    records
        .iter()
        .any(|(_, record)| matches!(record, Record::TurnStarted { .. }))
}

pub fn validate_ancestor_path(doc: &HeadDoc) -> Result<()> {
    match &doc.parent_id {
        None if doc.depth == 0 && doc.ancestor_ids.is_empty() && !doc.root_id.is_empty() => Ok(()),
        Some(parent_id)
            if doc.depth as usize == doc.ancestor_ids.len()
                && doc
                    .ancestor_ids
                    .last()
                    .is_some_and(|value| value == parent_id)
                && doc
                    .ancestor_ids
                    .first()
                    .is_some_and(|value| value == &doc.root_id)
                && doc.ancestor_ids.len() <= 8 =>
        {
            Ok(())
        }
        _ => Err(BrainError::Invalid(
            "session ancestor path does not match root, parent and depth".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(turn: &str, text: &str) -> Record {
        Record::UserMessage {
            turn: turn.into(),
            content: vec![ContentBlock::text(text)],
            starts_turn: false,
            metadata: HashMap::new(),
            idempotency_key_hash: None,
            request_hash: None,
        }
    }
    fn assistant(turn: &str, blocks: Vec<ContentBlock>) -> Record {
        Record::Assistant {
            turn: turn.into(),
            agent: "root".into(),
            attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
            content: blocks,
            stop: StopReason::EndTurn,
        }
    }
    fn result(call: &str, content: &str, is_error: bool) -> Record {
        Record::ToolResult {
            turn: "t1".into(),
            agent: "root".into(),
            call: call.into(),
            name: "bash".into(),
            outcome: if is_error { "failed" } else { "completed" }.into(),
            content: content.into(),
            is_error,
            exit_code: Some(if is_error { 1 } else { 0 }),
            duration_ms: 5,
            truncated: false,
        }
    }
    fn entries(records: Vec<Record>) -> Vec<Entry> {
        records
            .into_iter()
            .enumerate()
            .map(|(i, record)| Entry {
                seq: i as u64 + 1,
                ts_ms: 0,
                record,
            })
            .collect()
    }

    async fn create_memory_store(
        store: &MemoryStore,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        owner: &str,
        now_ms: u64,
    ) -> Result<()> {
        let limits = JournalRetentionLimits::default();
        let retention = initial_retention(first, limits.session_bytes)?;
        store
            .create(
                session_id,
                doc,
                first,
                owner,
                now_ms,
                u64::MAX,
                retention,
                limits,
            )
            .await
    }

    #[test]
    fn pre_idempotency_user_records_remain_readable() {
        let record: Record = serde_json::from_value(serde_json::json!({
            "kind": "user_message",
            "turn": "trn_old",
            "content": [{"type": "text", "text": "hello"}],
            "metadata": {}
        }))
        .unwrap();
        let Record::UserMessage {
            idempotency_key_hash,
            request_hash,
            ..
        } = record
        else {
            panic!("expected user message");
        };
        assert!(idempotency_key_hash.is_none());
        assert!(request_hash.is_none());
    }

    #[test]
    fn fold_rebuilds_the_conversation_and_groups_consecutive_tool_results() {
        let f = fold(&entries(vec![
            user("t1", "build it"),
            Record::TurnStarted { turn: "t1".into() },
            assistant(
                "t1",
                vec![
                    ContentBlock::text("running"),
                    ContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "bash".into(),
                        input: serde_json::json!({"command":"a"}),
                    },
                    ContentBlock::ToolUse {
                        id: "c2".into(),
                        name: "bash".into(),
                        input: serde_json::json!({"command":"b"}),
                    },
                ],
            ),
            result("c1", "ok-a", false),
            result("c2", "boom", true),
            assistant("t1", vec![ContentBlock::text("done")]),
        ]));
        assert_eq!(
            f.history.len(),
            4,
            "user, assistant, ONE grouped results message, assistant"
        );
        assert_eq!(f.history[2].role, Role::User);
        assert_eq!(f.history[2].content.len(), 2, "both results in one message");
        assert!(matches!(
            &f.history[2].content[1],
            ContentBlock::ToolResult { is_error: true, .. }
        ));
        assert_eq!(f.turns, 1);
    }

    #[test]
    fn fold_flushes_trailing_results_at_finish() {
        // A crash after committing results but before the next assistant message must still
        // rebuild a history the provider will accept.
        let f = fold(&entries(vec![
            user("t1", "x"),
            assistant(
                "t1",
                vec![ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                }],
            ),
            result("c1", "out", false),
        ]));
        assert_eq!(f.history.len(), 3);
        assert_eq!(f.history[2].role, Role::User);
    }

    #[test]
    fn subagent_records_never_split_or_pollute_root_history() {
        let mut child_assistant = assistant("t1", vec![ContentBlock::text("child")]);
        if let Record::Assistant { agent, .. } = &mut child_assistant {
            *agent = "agt_child".into();
        }
        let mut child_result = result("child-call", "child-out", false);
        if let Record::ToolResult { agent, .. } = &mut child_result {
            *agent = "agt_child".into();
        }
        let f = fold(&entries(vec![
            user("t1", "go"),
            assistant(
                "t1",
                vec![
                    ContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "task".into(),
                        input: serde_json::json!({}),
                    },
                    ContentBlock::ToolUse {
                        id: "c2".into(),
                        name: "task".into(),
                        input: serde_json::json!({}),
                    },
                ],
            ),
            result("c1", "one", false),
            child_assistant,
            child_result,
            result("c2", "two", false),
            assistant("t1", vec![ContentBlock::text("done")]),
        ]));
        assert_eq!(f.history.len(), 4);
        assert_eq!(f.history[2].content.len(), 2);
        assert!(f.history.iter().all(|message| {
            message
                .content
                .iter()
                .all(|block| !matches!(block, ContentBlock::Text { text } if text == "child"))
        }));
    }

    #[test]
    fn next_user_text_merges_with_an_interrupted_tool_result() {
        let f = fold(&entries(vec![
            user("t1", "start"),
            assistant(
                "t1",
                vec![ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "task".into(),
                    input: serde_json::json!({}),
                }],
            ),
            result("c1", "subagent interrupted", true),
            Record::TurnCompleted {
                turn: "t1".into(),
                stop_reason: "interrupted".into(),
                rounds: 1,
                tool_calls: 1,
                result: None,
            },
            user("t2", "continue"),
        ]));
        assert_eq!(f.history.len(), 3);
        assert_eq!(f.history[2].role, Role::User);
        assert!(matches!(
            &f.history[2].content[..],
            [ContentBlock::ToolResult { is_error: true, .. }, ContentBlock::Text { text }]
                if text == "continue"
        ));
    }

    #[test]
    fn fold_is_a_loop_over_apply() {
        // F1 (donor property): batch fold == incremental apply, at every prefix.
        let all = entries(vec![
            user("t1", "a"),
            assistant("t1", vec![ContentBlock::text("b")]),
            user("t2", "c"),
            result("c9", "r", false),
            assistant("t2", vec![ContentBlock::text("d")]),
        ]);
        for split in 0..=all.len() {
            let mut inc = Fold::default();
            for e in &all[..split] {
                inc.apply(&e.record);
            }
            inc.finish();
            let batch = fold(&all[..split]);
            assert_eq!(batch.history, inc.history, "split {split}");
        }
    }

    #[test]
    fn checkpoint_records_do_not_mutate_the_raw_audit_fold() {
        let f = fold(&entries(vec![
            user("t1", "one"),
            assistant("t1", vec![ContentBlock::text("1")]),
            user("t2", "two"),
            assistant("t2", vec![ContentBlock::text("2")]),
            Record::ContextInstalled {
                checkpoint_id: "ctx_1".into(),
                base_checkpoint_id: None,
                covers_through_sequence: 4,
                retained_messages: 2,
                payload_digest: "a".repeat(64),
                base_prefix_digest: "b".repeat(64),
                source_context_digest: "c".repeat(64),
                token_estimate: 4,
                context_generation: 1,
                summary_kind: "semantic".into(),
                compactor_provider: "fake".into(),
                compactor_model: "fake".into(),
                retained_from_sequence: 1,
                created_at_ms: 0,
            },
            user("t3", "three"),
        ]));
        assert_eq!(
            f.history.len(),
            5,
            "raw fold remains an audit reconstruction"
        );
        assert_eq!(f.history[0], Message::user_text("one"));
    }

    #[test]
    fn unknown_record_kind_is_a_typed_error_not_a_passthrough() {
        let bad = r#"{"kind":"totally_new","x":1}"#;
        assert!(serde_json::from_str::<Record>(bad).is_err());
    }

    #[test]
    fn record_sks_sort_numerically() {
        assert!(record_sk(9) < record_sk(10));
        assert!(record_sk(999) < record_sk(1000));
    }

    #[test]
    fn head_doc_round_trips() {
        let doc = HeadDoc {
            loop_state: None,
            tenant_id: "local".into(),
            root_id: "ses_test".into(),
            parent_id: None,
            ancestor_ids: Vec::new(),
            child_name: None,
            context_fork: None,
            depth: 0,
            last_seq: 1,
            state: "open".into(),
            failure: None,
            turn: None,
            active_phase: None,
            provider_attempt: None,
            active_context: HashMap::new(),
            active_rounds: 0,
            active_tool_calls: 0,
            message_replays: vec![],
            context: None,
            turns: 0,
            created_ms: 1,
            updated_ms: 2,
            recovery_due_ms: None,
            recovery_attempt: 0,
            create_key_hash: None,
            create_request_hash: None,
            last_message_ms: None,
            ended: false,
            prefix: PrefixDoc {
                agentloop: None,
                system_prompt: Some("sp".into()),
                provider: "anthropic".into(),
                model: "claude".into(),
                base_url: None,
                max_output_tokens: Some(4096),
                context_window_tokens: 32 * 1024,
                context_soft_tokens: 18 * 1024,
                context_hard_tokens: 22 * 1024,
                context_tail_tokens: 4 * 1024,
                context_summary_tokens: 4 * 1024,
                temperature: None,
                reasoning_effort: None,
                provider_recovery_retries: 1,
                storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
                storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
                storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
                max_child_depth: 4,
                max_direct_children: 32,
                max_descendants: 256,
                max_additional_sandboxes_per_root: 2,
                network: serde_json::json!({"outbound":"none"}),
                customer_client_id: None,
                customer_submit_retries: 1,
                rendered_base: serde_json::json!({}),
                rendered_base_digest: String::new(),
                prompt_cache_key: String::new(),
                tools: vec![],
                managed_bundles: vec![],
                official_capabilities: HashMap::new(),
                hand_enabled: true,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                hand_env_keys: vec![],
                metadata: HashMap::new(),
            },
            key_b64: "AAAA".into(),
            hand_secrets_b64: String::new(),
            session_storage_bytes: 0,
            storage_reserved_bytes: 0,
            tenant_metered_storage_bytes: 0,
            storage_upload: None,
            storage_delete: None,
            pending_customer_acks: vec![],
            pending_managed_acks: vec![],
            default_sandbox: None,
        };
        let s = serde_json::to_string(&doc).unwrap();
        let back: HeadDoc = serde_json::from_str(&s).unwrap();
        assert_eq!(back.prefix.model, "claude");
        assert_eq!(back.state, "open");
    }

    fn head_doc() -> HeadDoc {
        HeadDoc {
            loop_state: None,
            tenant_id: "local".into(),
            root_id: "ses_test".into(),
            parent_id: None,
            ancestor_ids: Vec::new(),
            child_name: None,
            context_fork: None,
            depth: 0,
            last_seq: 1,
            state: "open".into(),
            failure: None,
            turn: None,
            active_phase: None,
            provider_attempt: None,
            active_context: HashMap::new(),
            active_rounds: 0,
            active_tool_calls: 0,
            message_replays: vec![],
            context: None,
            turns: 0,
            created_ms: 1,
            updated_ms: 1,
            recovery_due_ms: None,
            recovery_attempt: 0,
            create_key_hash: None,
            create_request_hash: None,
            last_message_ms: None,
            ended: false,
            prefix: PrefixDoc {
                agentloop: None,
                system_prompt: None,
                provider: "anthropic".into(),
                model: "m".into(),
                base_url: None,
                max_output_tokens: None,
                context_window_tokens: 32 * 1024,
                context_soft_tokens: 18 * 1024,
                context_hard_tokens: 22 * 1024,
                context_tail_tokens: 4 * 1024,
                context_summary_tokens: 4 * 1024,
                temperature: None,
                reasoning_effort: None,
                provider_recovery_retries: 1,
                storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
                storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
                storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
                max_child_depth: 4,
                max_direct_children: 32,
                max_descendants: 256,
                max_additional_sandboxes_per_root: 2,
                network: serde_json::json!({"outbound":"none"}),
                customer_client_id: None,
                customer_submit_retries: 1,
                rendered_base: serde_json::json!({}),
                rendered_base_digest: String::new(),
                prompt_cache_key: String::new(),
                tools: vec![],
                managed_bundles: vec![],
                official_capabilities: HashMap::new(),
                hand_enabled: false,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                hand_env_keys: vec![],
                metadata: HashMap::new(),
            },
            key_b64: String::new(),
            hand_secrets_b64: String::new(),
            session_storage_bytes: 0,
            storage_reserved_bytes: 0,
            tenant_metered_storage_bytes: 0,
            storage_upload: None,
            storage_delete: None,
            pending_customer_acks: vec![],
            pending_managed_acks: vec![],
            default_sandbox: None,
        }
    }

    #[test]
    fn decision_limits_reject_oversized_items_and_aggregate_batches_before_store_io() {
        let doc = head_doc();
        let oversized = Record::Assistant {
            turn: "turn".into(),
            agent: "root".into(),
            attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
            content: vec![ContentBlock::text("x".repeat(MAX_SERIALIZED_RECORD_BYTES))],
            stop: StopReason::EndTurn,
        };
        let error = validate_decision("ses_limits", &[(2, oversized)], &doc).unwrap_err();
        assert!(error.to_string().contains("assistant record"));

        let near_max_results = (0..40)
            .map(|index| {
                (
                    index + 2,
                    Record::ToolResult {
                        turn: "turn".into(),
                        agent: "root".into(),
                        call: format!("call_{index}"),
                        name: "tool".into(),
                        outcome: "completed".into(),
                        content: "x".repeat(MAX_RECORD_CONTENT_BYTES),
                        is_error: false,
                        exit_code: None,
                        duration_ms: 1,
                        truncated: false,
                    },
                )
            })
            .collect::<Vec<_>>();
        let error = validate_decision("ses_limits", &near_max_results, &doc).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("journal decision is approximately")
        );

        let too_many = (0..MAX_DECISION_ACTIONS)
            .map(|index| {
                (
                    index as u64 + 2,
                    Record::State {
                        state: "open".into(),
                        turn: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        let error = validate_decision("ses_limits", &too_many, &doc).unwrap_err();
        assert!(error.to_string().contains("actions"));

        let mut oversized_listing = doc;
        oversized_listing
            .prefix
            .metadata
            .insert("large".into(), "x".repeat(MAX_SERIALIZED_LISTING_BYTES));
        let error = validate_decision("ses_limits", &[], &oversized_listing).unwrap_err();
        assert!(error.to_string().contains("listing document"));
    }

    #[tokio::test]
    async fn memory_journal_full_lifecycle() {
        let j = Journal::new_memory("brain-a");
        let doc = head_doc();
        j.create(
            "ses_m",
            &doc,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            j.create(
                "ses_m",
                &doc,
                &Record::State {
                    state: "open".into(),
                    turn: None
                }
            )
            .await,
            Err(BrainError::Invalid(_))
        ));

        let head = j.claim("ses_m").await.unwrap();
        assert_eq!(
            head.fence, 1,
            "create is unowned; the first claim establishes fence 1"
        );
        assert_eq!(head.last_seq, 1);

        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        let rec = (2u64, Record::TurnStarted { turn: "t1".into() });
        j.commit("ses_m", &mut lease, std::slice::from_ref(&rec), &doc, 3)
            .await
            .unwrap();
        assert_eq!(
            lease.last_seq, 3,
            "high water persisted, ephemeral seq included"
        );

        let entries = j.read_records("ses_m", 0).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].seq, 2);

        // Re-committing the same seq is a superseded write, exactly like DynamoDB.
        assert!(matches!(
            j.commit("ses_m", &mut lease, std::slice::from_ref(&rec), &doc, 4)
                .await,
            Err(BrainError::Fenced)
        ));

        let deletion_head = j.get_head("ses_m").await.unwrap();
        assert_eq!(j.purge_history("ses_m").await.unwrap(), 2);
        j.finalize_deletion(&DeletionStatusDoc {
            session_id: "ses_m".into(),
            tenant_id: deletion_head.doc.tenant_id.clone(),
            root_id: deletion_head.doc.root_id.clone(),
            parent_id: deletion_head.doc.parent_id.clone(),
            metered_storage_bytes: deletion_head.doc.tenant_metered_storage_bytes,
            metered_journal_bytes: deletion_head.retention.metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: i64::MAX as u64,
            attempts: 1,
            last_error: None,
        })
        .await
        .unwrap();
        assert!(matches!(
            j.get_head("ses_m").await,
            Err(BrainError::NoSuchSession(_))
        ));
    }

    #[tokio::test]
    async fn record_pages_are_bounded_and_stop_at_a_fixed_high_water() {
        let journal = Journal::new_memory("brain-page");
        let doc = head_doc();
        journal
            .create(
                "ses_page",
                &doc,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();
        let head = journal.claim("ses_page").await.unwrap();
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        let records = (2..=7)
            .map(|seq| {
                (
                    seq,
                    Record::State {
                        state: "open".into(),
                        turn: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        // 8 and 9 model live-only provisional sequence gaps.
        journal
            .commit("ses_page", &mut lease, &records, &doc, 9)
            .await
            .unwrap();

        let mut cursor = 0;
        let mut seen = Vec::new();
        loop {
            let page = journal
                .read_record_page(&RecordPageQuery {
                    session_id: "ses_page",
                    after: cursor,
                    through_seq: 9,
                    limit: 2,
                    max_bytes: DEFAULT_RECORD_PAGE_BYTES,
                })
                .await
                .unwrap();
            assert!(page.entries.len() <= 2);
            seen.extend(page.entries.iter().map(|entry| entry.seq));
            let Some(next) = page.next_after else {
                break;
            };
            cursor = next;
        }
        assert_eq!(seen, (1..=7).collect::<Vec<_>>());

        journal
            .commit(
                "ses_page",
                &mut lease,
                &[(10, Record::TurnStarted { turn: "t2".into() })],
                &doc,
                10,
            )
            .await
            .unwrap();
        let fixed = journal
            .read_record_page(&RecordPageQuery {
                session_id: "ses_page",
                after: 7,
                through_seq: 9,
                limit: 2,
                max_bytes: DEFAULT_RECORD_PAGE_BYTES,
            })
            .await
            .unwrap();
        assert!(fixed.entries.is_empty());
        assert!(fixed.next_after.is_none());
    }

    #[tokio::test]
    async fn memory_journal_fences_out_a_stale_owner() {
        let a = Journal::new_memory("brain-a");
        let b = a.cloned_as("brain-b");
        let doc = head_doc();
        a.create(
            "ses_f",
            &doc,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
        let head_a = a.claim("ses_f").await.unwrap();
        let mut lease_a = Lease {
            fence: head_a.fence,
            last_seq: head_a.last_seq,
            retention: head_a.retention,
        };

        // B cannot steal while A's lease is live...
        assert!(matches!(b.claim("ses_f").await, Err(BrainError::Fenced)));

        // ...but after A releases, B claims with a HIGHER fence, and A's writes are dead.
        a.release("ses_f", &lease_a).await.unwrap();
        let head_b = b.claim("ses_f").await.unwrap();
        assert!(head_b.fence > head_a.fence);
        let rec = (2u64, Record::TurnStarted { turn: "t".into() });
        assert!(matches!(
            a.commit("ses_f", &mut lease_a, std::slice::from_ref(&rec), &doc, 2)
                .await,
            Err(BrainError::Fenced)
        ));
        let mut lease_b = Lease {
            fence: head_b.fence,
            last_seq: head_b.last_seq,
            retention: head_b.retention,
        };
        b.commit("ses_f", &mut lease_b, std::slice::from_ref(&rec), &doc, 2)
            .await
            .unwrap();
    }

    fn sandbox_reservation(index: usize) -> SandboxReserveRequest {
        let root_id = "ses_sandbox_root".to_string();
        let sandbox_id = format!("sbx_{index:02}");
        SandboxReserveRequest {
            root_id: root_id.clone(),
            owner_session_id: root_id.clone(),
            sandbox_id: sandbox_id.clone(),
            operation_id: format!("op_{index:02}"),
            request_digest: format!("{index:064x}"),
            generation_intent: format!("gen_{index:02}"),
            initial_status: serde_json::from_value(serde_json::json!({
                "target": {
                    "kind": "additional",
                    "session_id": root_id,
                    "root_id": "ses_sandbox_root",
                    "binding_ref": format!("bnd_{index:02}"),
                    "sandbox_id": sandbox_id,
                },
                "state": "creating",
                "expires_at_ms": null,
            }))
            .unwrap(),
            now_ms: index as u64 + 1,
        }
    }

    #[tokio::test]
    async fn sandbox_inventory_reserves_cap_atomically_and_keeps_terminal_tombstones() {
        let store = Arc::new(MemoryStore::default());
        let mut root = head_doc();
        root.root_id = "ses_sandbox_root".into();
        root.prefix.max_additional_sandboxes_per_root = 2;
        create_memory_store(
            &store,
            "ses_sandbox_root",
            &root,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner",
            0,
        )
        .await
        .unwrap();

        let mut tasks = Vec::new();
        for index in 0..8 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store.reserve_sandbox(&sandbox_reservation(index)).await
            }));
        }
        let mut created = Vec::new();
        let mut exhausted = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(item) => created.push(item),
                Err(BrainError::SandboxResourceExhausted) => exhausted += 1,
                Err(error) => panic!("unexpected reservation result: {error}"),
            }
        }
        assert_eq!(created.len(), 2);
        assert_eq!(exhausted, 6);

        let replay_request = sandbox_reservation(
            created[0]
                .sandbox_id
                .strip_prefix("sbx_")
                .unwrap()
                .parse()
                .unwrap(),
        );
        let replay = store.reserve_sandbox(&replay_request).await.unwrap();
        assert_eq!(replay.sandbox_id, created[0].sandbox_id);

        let mut terminal: brain_protocol::hand::SandboxStatus =
            serde_json::from_value(serde_json::to_value(&created[0].status).unwrap()).unwrap();
        terminal.state = brain_protocol::hand::SandboxState::Terminated;
        let tombstone = store
            .update_sandbox(&SandboxUpdateRequest {
                root_id: created[0].root_id.clone(),
                sandbox_id: created[0].sandbox_id.clone(),
                expected_version: created[0].version,
                status: terminal,
                release_slot: true,
                now_ms: 100,
            })
            .await
            .unwrap();
        assert!(tombstone.slot_released);
        assert_eq!(
            store
                .get_sandbox(&tombstone.root_id, &tombstone.sandbox_id)
                .await
                .unwrap()
                .status
                .state,
            brain_protocol::hand::SandboxState::Terminated
        );
        store
            .reserve_sandbox(&sandbox_reservation(9))
            .await
            .expect("confirmed termination releases exactly one slot");
        assert!(matches!(
            store
                .update_sandbox(&SandboxUpdateRequest {
                    root_id: tombstone.root_id.clone(),
                    sandbox_id: tombstone.sandbox_id.clone(),
                    expected_version: tombstone.version,
                    status: sandbox_reservation(0).initial_status,
                    release_slot: false,
                    now_ms: 101,
                })
                .await,
            Err(BrainError::SandboxGone)
        ));
    }

    #[tokio::test]
    async fn lease_heartbeat_prevents_recovery_steal_until_it_stops() {
        let store = MemoryStore::default();
        let mut doc = head_doc();
        doc.state = "open".into();
        doc.turn = Some("trn_heartbeat".into());
        doc.active_phase = Some("model_running".into());
        create_memory_store(
            &store,
            "ses_heartbeat",
            &doc,
            &Record::State {
                state: "open".into(),
                turn: doc.turn.clone(),
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        let claimed = store
            .claim("ses_heartbeat", "owner-a", 1)
            .await
            .expect("first owner claims the unowned create");
        assert_eq!(claimed.fence, 1);
        store
            .renew("ses_heartbeat", "owner-a", 1, 50_000, Some(115_000))
            .await
            .unwrap();
        assert!(matches!(
            store.claim("ses_heartbeat", "owner-b", 70_000).await,
            Err(BrainError::Fenced)
        ));
        let recovered = store
            .claim("ses_heartbeat", "owner-b", 116_000)
            .await
            .expect("stopped heartbeat becomes stealable after lease plus grace");
        assert_eq!(recovered.fence, 2);
    }

    #[tokio::test]
    async fn lease_renewal_preserves_scheduled_upload_expiry_and_idle_due_absence() {
        let store = MemoryStore::default();
        let mut reserved = head_doc();
        reserved.storage_upload = Some(StorageUploadReservationDoc {
            transfer_id: "xfer_fixed".into(),
            key: "large.bin".into(),
            bytes: 10,
            sha256: Some("00".repeat(32)),
            content_type: None,
            overwrite: false,
            previous_bytes: 0,
            expires_at_ms: 900_000,
            state: "reserved".into(),
        });
        // Create itself must not smuggle a pre-existing public storage reservation. The test is
        // only exercising the independently durable expiry anchor carried by the reservation.
        reserved.storage_reserved_bytes = 0;
        reserved.recovery_due_ms = Some(900_000);
        create_memory_store(
            &store,
            "ses_reserved",
            &reserved,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        store
            .claim("ses_reserved", "owner-a", 1)
            .await
            .expect("first owner claims the unowned create");
        store
            .renew("ses_reserved", "owner-a", 1, 100_000, None)
            .await
            .unwrap();
        assert_eq!(
            store
                .get_head("ses_reserved")
                .await
                .unwrap()
                .doc
                .recovery_due_ms,
            Some(900_000),
            "lease-only heartbeat must not postpone or replace the fixed upload expiry"
        );

        let idle = head_doc().with_recovery_projection(100_000);
        create_memory_store(
            &store,
            "ses_idle",
            &idle,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        store
            .claim("ses_idle", "owner-a", 1)
            .await
            .expect("first owner claims the unowned create");
        store
            .renew("ses_idle", "owner-a", 1, 100_000, None)
            .await
            .unwrap();
        assert_eq!(
            store
                .get_head("ses_idle")
                .await
                .unwrap()
                .doc
                .recovery_due_ms,
            None,
            "quiescent lease renewal must not create recovery work"
        );
    }

    #[test]
    fn unacknowledged_customer_terminal_keeps_a_quiescent_session_recoverable() {
        let mut doc = head_doc();
        doc.pending_customer_acks.push(CustomerTerminalAckDoc {
            turn: "trn_customer".into(),
            call: "op_customer".into(),
            client_id: "app".into(),
            process_id: "process:test".into(),
            request_digest: "a".repeat(64),
            terminal_digest: "b".repeat(64),
        });
        let projected = doc.with_recovery_projection(100_000);
        assert_eq!(
            projected.recovery_due_ms,
            Some(100_000 + LEASE_MS + STEAL_GRACE_MS)
        );
        let mut acknowledged = projected;
        acknowledged.pending_customer_acks.clear();
        assert_eq!(
            acknowledged
                .with_recovery_projection(200_000)
                .recovery_due_ms,
            None
        );
    }

    #[test]
    fn accepted_end_remains_due_until_the_subtree_reaches_ended() {
        let mut doc = head_doc();
        doc.state = "ending".into();
        doc.ended = true;
        let projected = doc.with_recovery_projection(100_000);
        assert_eq!(
            projected.recovery_due_ms,
            Some(100_000 + LEASE_MS + STEAL_GRACE_MS)
        );

        let mut ended = projected;
        ended.state = "ended".into();
        assert_eq!(
            ended.with_recovery_projection(200_000).recovery_due_ms,
            None,
            "a fully converged end must leave no recovery anchor"
        );
    }

    #[tokio::test]
    async fn successful_quiescent_commit_returns_the_canonical_cleared_projection() {
        let store = Arc::new(MemoryStore::default());
        let journal = Journal::new(store, "owner-a");
        let mut active = head_doc();
        active.state = "open".into();
        active.turn = Some("trn_done".into());
        active.active_phase = Some("model_running".into());
        journal
            .create(
                "ses_projection",
                &active,
                &Record::State {
                    state: "open".into(),
                    turn: active.turn.clone(),
                },
            )
            .await
            .unwrap();
        let head = journal.claim("ses_projection").await.unwrap();
        let mut doc = head.doc;
        doc.state = "open".into();
        doc.turn = None;
        doc.active_phase = None;
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        let persisted = journal
            .commit("ses_projection", &mut lease, &[], &doc, head.last_seq)
            .await
            .unwrap();
        assert_eq!(persisted.recovery_due_ms, None);
        journal
            .renew("ses_projection", &lease, false)
            .await
            .unwrap();
        assert_eq!(
            journal
                .get_head("ses_projection")
                .await
                .unwrap()
                .doc
                .recovery_due_ms,
            None,
            "a later heartbeat cannot resurrect the completed recovery row"
        );
    }

    #[tokio::test]
    async fn final_deletion_atomically_replaces_content_anchor_with_bounded_tombstone() {
        let store = MemoryStore::default();
        let doc = head_doc();
        create_memory_store(
            &store,
            "ses_deleted",
            &doc,
            &Record::State {
                state: "deleting".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        let terminal = DeletionStatusDoc {
            session_id: "ses_deleted".into(),
            tenant_id: doc.tenant_id.clone(),
            root_id: "ses_deleted".into(),
            parent_id: None,
            metered_storage_bytes: 0,
            metered_journal_bytes: store
                .get_head("ses_deleted")
                .await
                .unwrap()
                .retention
                .metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: u64::MAX,
            attempts: 1,
            last_error: None,
        };
        store.finalize_deletion(&terminal).await.unwrap();
        assert!(matches!(
            store.get_head("ses_deleted").await,
            Err(BrainError::NoSuchSession(_))
        ));
        assert_eq!(
            store
                .get_deletion_status("ses_deleted")
                .await
                .unwrap()
                .unwrap(),
            terminal
        );
        let mut stale = terminal.clone();
        stale.state = "retrying".into();
        stale.completed_at_ms = None;
        store.put_deletion_status(&stale).await.unwrap();
        assert_eq!(
            store
                .get_deletion_status("ses_deleted")
                .await
                .unwrap()
                .unwrap()
                .state,
            "succeeded"
        );
    }

    #[tokio::test]
    async fn tenant_storage_meter_is_shared_atomic_and_released_once() {
        let store = Arc::new(MemoryStore::default());
        let left_journal = Journal::new(store.clone(), "owner-a").with_tenant_storage_limit(10);
        let right_journal = left_journal.cloned_as("owner-b");
        let mut left = head_doc();
        left.tenant_id = "tenant-meter".into();
        left.root_id = "ses_meter_left".into();
        let mut right = head_doc();
        right.tenant_id = "tenant-meter".into();
        right.root_id = "ses_meter_right".into();
        for (journal, id, doc) in [
            (&left_journal, "ses_meter_left", &left),
            (&right_journal, "ses_meter_right", &right),
        ] {
            journal
                .create(
                    id,
                    doc,
                    &Record::State {
                        state: "open".into(),
                        turn: None,
                    },
                )
                .await
                .unwrap();
        }

        let left_head = left_journal.claim("ses_meter_left").await.unwrap();
        let mut left_lease = Lease {
            fence: left_head.fence,
            last_seq: left_head.last_seq,
            retention: left_head.retention,
        };
        let mut left_doc = left_head.doc;
        left_doc.storage_reserved_bytes = 6;
        left_doc = left_journal
            .commit(
                "ses_meter_left",
                &mut left_lease,
                &[],
                &left_doc,
                left_head.last_seq,
            )
            .await
            .unwrap();

        let right_head = right_journal.claim("ses_meter_right").await.unwrap();
        let mut right_lease = Lease {
            fence: right_head.fence,
            last_seq: right_head.last_seq,
            retention: right_head.retention,
        };
        let mut rejected_doc = right_head.doc.clone();
        rejected_doc.storage_reserved_bytes = 5;
        assert!(matches!(
            right_journal
                .commit(
                    "ses_meter_right",
                    &mut right_lease,
                    &[],
                    &rejected_doc,
                    right_head.last_seq,
                )
                .await,
            Err(BrainError::TenantStorageQuotaExceeded {
                requested: 5,
                limit: 10
            })
        ));
        assert_eq!(
            right_journal
                .get_head("ses_meter_right")
                .await
                .unwrap()
                .doc
                .tenant_metered_storage_bytes,
            0,
            "a rejected decision leaves the authoritative session contribution unchanged"
        );

        let mut right_doc = right_head.doc;
        right_doc.storage_reserved_bytes = 4;
        right_doc = right_journal
            .commit(
                "ses_meter_right",
                &mut right_lease,
                &[],
                &right_doc,
                right_head.last_seq,
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .tenant_storage
                .lock()
                .expect("tenant meter")
                .get("tenant-meter"),
            Some(&10)
        );

        // Reserve -> publish is a gauge transfer, not a second charge.
        left_doc.session_storage_bytes = 6;
        left_doc.storage_reserved_bytes = 0;
        let left_last_seq = left_lease.last_seq;
        left_doc = left_journal
            .commit(
                "ses_meter_left",
                &mut left_lease,
                &[],
                &left_doc,
                left_last_seq,
            )
            .await
            .unwrap();
        assert_eq!(left_doc.tenant_metered_storage_bytes, 6);
        assert_eq!(
            store
                .tenant_storage
                .lock()
                .expect("tenant meter")
                .get("tenant-meter"),
            Some(&10)
        );

        let status = DeletionStatusDoc {
            session_id: "ses_meter_left".into(),
            tenant_id: "tenant-meter".into(),
            root_id: "ses_meter_left".into(),
            parent_id: None,
            metered_storage_bytes: 6,
            metered_journal_bytes: left_lease.retention.metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: u64::MAX,
            attempts: 1,
            last_error: None,
        };
        store.finalize_deletion(&status).await.unwrap();
        store.finalize_deletion(&status).await.unwrap();
        assert_eq!(
            store
                .tenant_storage
                .lock()
                .expect("tenant meter")
                .get("tenant-meter"),
            Some(&4),
            "lost final response cannot release tenant capacity twice"
        );

        // The surviving root can immediately consume the released capacity.
        right_doc.storage_reserved_bytes = 10;
        let right_last_seq = right_lease.last_seq;
        let right_doc = right_journal
            .commit(
                "ses_meter_right",
                &mut right_lease,
                &[],
                &right_doc,
                right_last_seq,
            )
            .await
            .unwrap();
        assert_eq!(right_doc.tenant_metered_storage_bytes, 10);
    }

    #[tokio::test]
    async fn root_bundle_bytes_reserve_tenant_capacity_at_create_and_release_once() {
        let store = Arc::new(MemoryStore::default());
        let journal = Journal::new(store.clone(), "owner-a").with_tenant_storage_limit(10);
        let mut first = head_doc();
        first.tenant_id = "tenant-bundles".into();
        first.root_id = "ses_bundle_first".into();
        first.tenant_metered_storage_bytes = 6;
        journal
            .create(
                "ses_bundle_first",
                &first,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(first.session_storage_bytes, 0);
        assert_eq!(first.storage_reserved_bytes, 0);
        assert_eq!(
            store
                .tenant_storage
                .lock()
                .expect("tenant meter")
                .get("tenant-bundles"),
            Some(&6)
        );

        let mut rejected = head_doc();
        rejected.tenant_id = "tenant-bundles".into();
        rejected.root_id = "ses_bundle_rejected".into();
        rejected.tenant_metered_storage_bytes = 5;
        assert!(matches!(
            journal
                .create(
                    "ses_bundle_rejected",
                    &rejected,
                    &Record::State {
                        state: "open".into(),
                        turn: None,
                    },
                )
                .await,
            Err(BrainError::TenantStorageQuotaExceeded {
                requested: 5,
                limit: 10
            })
        ));
        assert!(matches!(
            journal.get_head("ses_bundle_rejected").await,
            Err(BrainError::NoSuchSession(_))
        ));

        let status = DeletionStatusDoc {
            session_id: "ses_bundle_first".into(),
            tenant_id: "tenant-bundles".into(),
            root_id: "ses_bundle_first".into(),
            parent_id: None,
            metered_storage_bytes: 6,
            metered_journal_bytes: store
                .get_head("ses_bundle_first")
                .await
                .unwrap()
                .retention
                .metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: u64::MAX,
            attempts: 1,
            last_error: None,
        };
        store.finalize_deletion(&status).await.unwrap();
        store.finalize_deletion(&status).await.unwrap();
        assert_eq!(
            store
                .tenant_storage
                .lock()
                .expect("tenant meter")
                .get("tenant-bundles"),
            Some(&0)
        );
        journal
            .create(
                "ses_bundle_rejected",
                &rejected,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn final_child_deletion_removes_the_strong_parent_adjacency() {
        let store = MemoryStore::default();
        let mut parent = head_doc();
        parent.root_id = "ses_parent".into();
        create_memory_store(
            &store,
            "ses_parent",
            &parent,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        let mut child = head_doc();
        child.root_id = "ses_parent".into();
        child.parent_id = Some("ses_parent".into());
        child.ancestor_ids = vec!["ses_parent".into()];
        child.depth = 1;
        create_memory_store(
            &store,
            "ses_child",
            &child,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        assert_eq!(
            store
                .list_child_page(&ChildListQuery {
                    parent_id: "ses_parent",
                    limit: 100,
                    cursor: None,
                })
                .await
                .unwrap()
                .sessions
                .len(),
            1
        );
        store
            .finalize_deletion(&DeletionStatusDoc {
                session_id: "ses_child".into(),
                tenant_id: "local".into(),
                root_id: "ses_parent".into(),
                parent_id: Some("ses_parent".into()),
                metered_storage_bytes: 0,
                metered_journal_bytes: store
                    .get_head("ses_child")
                    .await
                    .unwrap()
                    .retention
                    .metered_bytes,
                state: "succeeded".into(),
                requested_at_ms: 1,
                updated_at_ms: 2,
                completed_at_ms: Some(2),
                expires_at_ms: u64::MAX,
                attempts: 1,
                last_error: None,
            })
            .await
            .unwrap();
        assert!(
            store
                .list_child_page(&ChildListQuery {
                    parent_id: "ses_parent",
                    limit: 100,
                    cursor: None,
                })
                .await
                .unwrap()
                .sessions
                .is_empty()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn child_admission_is_atomic_at_the_direct_limit_and_releases_once() {
        let store = Arc::new(MemoryStore::default());
        let mut root = head_doc();
        root.root_id = "ses_quota_root".into();
        root.prefix.max_direct_children = 3;
        root.prefix.max_descendants = 3;
        create_memory_store(
            &store,
            "ses_quota_root",
            &root,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();

        let mut tasks = Vec::new();
        for index in 0..16 {
            let store = Arc::clone(&store);
            let mut child = root.clone();
            child.root_id = "ses_quota_root".into();
            child.parent_id = Some("ses_quota_root".into());
            child.ancestor_ids = vec!["ses_quota_root".into()];
            child.depth = 1;
            let child_id = format!("ses_quota_child_{index:02}");
            tasks.push(tokio::spawn(async move {
                let result = create_memory_store(
                    &store,
                    &child_id,
                    &child,
                    &Record::State {
                        state: "open".into(),
                        turn: None,
                    },
                    "owner-a",
                    0,
                )
                .await;
                (child_id, result)
            }));
        }
        let mut admitted = Vec::new();
        let mut rejected = 0;
        for task in tasks {
            let (child_id, result) = task.await.unwrap();
            match result {
                Ok(()) => admitted.push(child_id),
                Err(BrainError::Overloaded) => rejected += 1,
                Err(error) => panic!("unexpected child admission error: {error}"),
            }
        }
        assert_eq!(admitted.len(), 3);
        assert_eq!(rejected, 13);
        {
            let sessions = store.sessions.lock().expect("memory journal");
            let root = sessions.get("ses_quota_root").unwrap();
            assert_eq!(root.direct_children, 3);
            assert_eq!(root.descendants, 3);
        }

        let released = admitted.pop().unwrap();
        let released_journal_bytes = store
            .get_head(&released)
            .await
            .unwrap()
            .retention
            .metered_bytes;
        let terminal = DeletionStatusDoc {
            session_id: released,
            tenant_id: "local".into(),
            root_id: "ses_quota_root".into(),
            parent_id: Some("ses_quota_root".into()),
            metered_storage_bytes: 0,
            metered_journal_bytes: released_journal_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: u64::MAX,
            attempts: 1,
            last_error: None,
        };
        store.finalize_deletion(&terminal).await.unwrap();
        store
            .finalize_deletion(&terminal)
            .await
            .expect("lost-response retry is idempotent");
        {
            let sessions = store.sessions.lock().expect("memory journal");
            let root = sessions.get("ses_quota_root").unwrap();
            assert_eq!(root.direct_children, 2);
            assert_eq!(root.descendants, 2);
        }

        let mut replacement = root.clone();
        replacement.parent_id = Some("ses_quota_root".into());
        replacement.ancestor_ids = vec!["ses_quota_root".into()];
        replacement.depth = 1;
        create_memory_store(
            &store,
            "ses_quota_replacement",
            &replacement,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            3,
        )
        .await
        .unwrap();

        let claimed = store.claim("ses_quota_root", "owner-a", 4).await.unwrap();
        let mut fenced = claimed.doc;
        fenced.ended = true;
        fenced.state = "ending".into();
        store
            .commit(
                "ses_quota_root",
                "owner-a",
                claimed.fence,
                &[],
                &fenced,
                claimed.last_seq,
                4,
                0,
                u64::MAX,
                claimed.retention,
                0,
                JournalRetentionLimits::default(),
            )
            .await
            .unwrap();
        let mut late = root;
        late.parent_id = Some("ses_quota_root".into());
        late.ancestor_ids = vec!["ses_quota_root".into()];
        late.depth = 1;
        assert!(matches!(
            create_memory_store(
                &store,
                "ses_after_end_fence",
                &late,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
                "owner-b",
                5,
            )
            .await,
            Err(BrainError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn descendant_limit_is_shared_across_breadth_and_depth() {
        let store = MemoryStore::default();
        let mut root = head_doc();
        root.root_id = "ses_desc_root".into();
        root.prefix.max_direct_children = 8;
        root.prefix.max_descendants = 2;
        create_memory_store(
            &store,
            "ses_desc_root",
            &root,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
        let mut child = root.clone();
        child.parent_id = Some("ses_desc_root".into());
        child.ancestor_ids = vec!["ses_desc_root".into()];
        child.depth = 1;
        create_memory_store(
            &store,
            "ses_desc_child",
            &child,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            1,
        )
        .await
        .unwrap();
        let mut grandchild = root.clone();
        grandchild.parent_id = Some("ses_desc_child".into());
        grandchild.ancestor_ids = vec!["ses_desc_root".into(), "ses_desc_child".into()];
        grandchild.depth = 2;
        create_memory_store(
            &store,
            "ses_desc_grandchild",
            &grandchild,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            2,
        )
        .await
        .unwrap();
        let mut sibling = child;
        sibling.parent_id = Some("ses_desc_root".into());
        sibling.ancestor_ids = vec!["ses_desc_root".into()];
        assert!(matches!(
            create_memory_store(
                &store,
                "ses_desc_sibling",
                &sibling,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
                "owner-a",
                3,
            )
            .await,
            Err(BrainError::Overloaded)
        ));
    }

    #[tokio::test]
    async fn ancestor_fence_atomically_rejects_a_deep_turn_and_new_descendant() {
        let journal = Journal::new_memory("brain-ancestor-race");
        let mut root = head_doc();
        root.root_id = "ses_root".into();
        root.prefix.max_child_depth = 8;
        journal
            .create(
                "ses_root",
                &root,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();

        let mut child = root.clone();
        child.root_id = "ses_root".into();
        child.parent_id = Some("ses_root".into());
        child.ancestor_ids = vec!["ses_root".into()];
        child.depth = 1;
        journal
            .create(
                "ses_child",
                &child,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();

        let mut grandchild = child.clone();
        grandchild.parent_id = Some("ses_child".into());
        grandchild.ancestor_ids = vec!["ses_root".into(), "ses_child".into()];
        grandchild.depth = 2;
        journal
            .create(
                "ses_grandchild",
                &grandchild,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();

        // The deep actor wins its own lease before the root fence, exactly like a concurrent
        // follow-up on another replica. The later decision must still condition the root path.
        let grandchild_head = journal.claim("ses_grandchild").await.unwrap();
        let mut root_head = journal.claim("ses_root").await.unwrap();
        root_head.doc.ended = true;
        root_head.doc.state = "ending".into();
        let mut root_lease = Lease {
            fence: root_head.fence,
            last_seq: root_head.last_seq,
            retention: root_head.retention,
        };
        journal
            .commit(
                "ses_root",
                &mut root_lease,
                &[(
                    2,
                    Record::State {
                        state: "ending".into(),
                        turn: None,
                    },
                )],
                &root_head.doc,
                2,
            )
            .await
            .unwrap();

        let mut grandchild_doc = grandchild_head.doc.clone();
        grandchild_doc.state = "open".into();
        grandchild_doc.turn = Some("trn_after_fence".into());
        let mut grandchild_lease = Lease {
            fence: grandchild_head.fence,
            last_seq: grandchild_head.last_seq,
            retention: grandchild_head.retention,
        };
        assert!(matches!(
            journal
                .commit(
                    "ses_grandchild",
                    &mut grandchild_lease,
                    &[(
                        2,
                        Record::TurnStarted {
                            turn: "trn_after_fence".into(),
                        },
                    )],
                    &grandchild_doc,
                    2,
                )
                .await,
            Err(BrainError::Fenced)
        ));

        let mut great_grandchild = grandchild.clone();
        great_grandchild.parent_id = Some("ses_grandchild".into());
        great_grandchild.ancestor_ids = vec![
            "ses_root".into(),
            "ses_child".into(),
            "ses_grandchild".into(),
        ];
        great_grandchild.depth = 3;
        assert!(matches!(
            journal
                .create(
                    "ses_great_grandchild",
                    &great_grandchild,
                    &Record::State {
                        state: "open".into(),
                        turn: None,
                    },
                )
                .await,
            Err(BrainError::Invalid(_))
        ));
    }

    fn model_intent(turn: &str) -> Record {
        Record::ModelCallIntent {
            turn: turn.into(),
            logical_operation_id: format!("model:{turn}:1"),
            attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
            request_digest: "a".repeat(64),
            replacement: 0,
        }
    }

    fn sandbox_status(state: &str) -> brain_protocol::hand::SandboxStatus {
        serde_json::from_value(serde_json::json!({
            "state": state,
            "target": {
                "binding_ref": "binding_default",
                "kind": "default",
                "root_id": "ses_retention",
                "session_id": "ses_retention"
            },
            "expires_at_ms": null
        }))
        .unwrap()
    }

    #[test]
    fn retention_policy_validation_is_ordered_and_adapter_representable() {
        assert!(JournalRetentionLimits::default().validate().is_ok());
        for limits in [
            JournalRetentionLimits {
                session_bytes: MIN_SESSION_JOURNAL_BYTES - 1,
                ..JournalRetentionLimits::default()
            },
            JournalRetentionLimits {
                session_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES,
                tenant_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES - 1,
                tenant_sessions: DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
            },
            JournalRetentionLimits {
                tenant_sessions: 0,
                ..JournalRetentionLimits::default()
            },
            JournalRetentionLimits {
                session_bytes: MAX_JOURNAL_BYTES + 1,
                tenant_bytes: MAX_JOURNAL_BYTES + 1,
                tenant_sessions: DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
            },
        ] {
            assert!(matches!(limits.validate(), Err(BrainError::Invalid(_))));
        }
        assert!(
            JournalRetentionLimits {
                session_bytes: MIN_SESSION_JOURNAL_BYTES,
                tenant_bytes: MIN_SESSION_JOURNAL_BYTES,
                tenant_sessions: MIN_TENANT_RETAINED_SESSIONS,
            }
            .validate()
            .is_ok()
        );
        assert!(
            JournalRetentionLimits {
                session_bytes: MAX_JOURNAL_BYTES,
                tenant_bytes: MAX_JOURNAL_BYTES,
                tenant_sessions: MAX_TENANT_RETAINED_SESSIONS,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn every_effect_class_reserves_before_dispatch_and_recovery_does_not_duplicate_it() {
        let first = Record::State {
            state: "open".into(),
            turn: None,
        };
        let initial = initial_retention(&first, u64::MAX).unwrap();
        let single_terminal_intents = vec![
            Record::CompactionIntent {
                turn: "trn_effects".into(),
                logical_operation_id: "compact:trn_effects:1".into(),
                attempt_id: "att_compaction".into(),
                request_digest: "a".repeat(64),
                replacement: 0,
            },
            Record::CustomerCallIntent {
                turn: "trn_effects".into(),
                call: "op_customer".into(),
                client_id: "app".into(),
                process_id: "process:test".into(),
                request_digest: "b".repeat(64),
                deadline_at_ms: 10_000,
            },
            Record::ToolCall {
                turn: "trn_effects".into(),
                agent: "root".into(),
                call: "op_managed".into(),
                name: "managed".into(),
                input: serde_json::json!({"value": true}),
                detach: false,
            },
            Record::StorageUploadReserved {
                transfer_id: "transfer_effects".into(),
                key: "out.bin".into(),
                bytes: 1,
                sha256: Some("c".repeat(64)),
                expires_at_ms: 10_000,
                published_bytes: 0,
                reserved_bytes: 1,
            },
            Record::StorageDeleteIntent {
                operation_id: "delete_effects".into(),
                key: "old.bin".into(),
                bytes: 1,
                sha256: "d".repeat(64),
                published_bytes: 1,
                reserved_bytes: 0,
            },
            Record::DefaultSandboxChanged {
                status: sandbox_status("creating"),
            },
        ];
        for intent in single_terminal_intents {
            let projected = project_retention(initial, &[(2, intent)], u64::MAX).unwrap();
            assert_eq!(
                projected.effect_reserve_bytes,
                JOURNAL_TERMINAL_RESERVE_BYTES
            );
        }

        let provider =
            project_retention(initial, &[(2, model_intent("trn_retry"))], u64::MAX).unwrap();
        assert_eq!(provider.effect_reserve_bytes, JOURNAL_EFFECT_RESERVE_BYTES);
        let recovery = vec![
            (
                3,
                Record::ModelCallUnknown {
                    turn: "trn_retry".into(),
                    logical_operation_id: "model:trn_retry:1".into(),
                    attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                    request_digest: "a".repeat(64),
                    possibly_duplicated: true,
                },
            ),
            (
                4,
                Record::ModelAttemptSuperseded {
                    turn: "trn_retry".into(),
                    logical_operation_id: "model:trn_retry:1".into(),
                    superseded_attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                    replacement_attempt_id: "att_bbbbbbbbbbbbbbbbbbbb".into(),
                    reason: "unknown".into(),
                },
            ),
            (
                5,
                Record::ModelCallIntent {
                    turn: "trn_retry".into(),
                    logical_operation_id: "model:trn_retry:1".into(),
                    attempt_id: "att_bbbbbbbbbbbbbbbbbbbb".into(),
                    request_digest: "a".repeat(64),
                    replacement: 1,
                },
            ),
        ];
        let recovered = project_retention(provider, &recovery, u64::MAX).unwrap();
        assert_eq!(recovered.effect_reserve_bytes, JOURNAL_EFFECT_RESERVE_BYTES);
        assert_eq!(
            recovered.metered_bytes - provider.metered_bytes,
            serialized_record_charge(&recovery).unwrap(),
            "replacement recovery charges only its durable records and restores, rather than duplicates, the reserve"
        );

        let reserved = project_retention(
            initial,
            &[(
                2,
                Record::StorageUploadReserved {
                    transfer_id: "transfer_adopt".into(),
                    key: "adopt.bin".into(),
                    bytes: 1,
                    sha256: Some("e".repeat(64)),
                    expires_at_ms: 10_000,
                    published_bytes: 0,
                    reserved_bytes: 1,
                },
            )],
            u64::MAX,
        )
        .unwrap();
        let published_record = Record::StorageUploadPublished {
            transfer_id: "transfer_adopt".into(),
            key: "adopt.bin".into(),
            bytes: 1,
            published_bytes: 1,
            reserved_bytes: 1,
        };
        let published =
            project_retention(reserved, &[(3, published_record.clone())], u64::MAX).unwrap();
        assert!(published.effect_reserve_bytes < JOURNAL_TERMINAL_RESERVE_BYTES);
        let republished = project_retention(published, &[(4, published_record)], u64::MAX).unwrap();
        assert_eq!(
            republished.metered_bytes, published.metered_bytes,
            "replayed adoption consumes already-reserved capacity without adding a second reserve"
        );
    }

    #[test]
    fn effect_retention_reserves_provider_completion_and_tool_terminal_decisions() {
        assert_eq!(
            JOURNAL_EFFECT_RESERVE_BYTES,
            2 * MAX_DECISION_SERIALIZED_BYTES as u64
        );
        let first = Record::State {
            state: "open".into(),
            turn: None,
        };
        let initial = initial_retention(&first, u64::MAX).unwrap();
        let intent = model_intent("trn_retention");
        let after_intent = project_retention(initial, &[(2, intent)], u64::MAX).unwrap();
        assert_eq!(
            after_intent.effect_reserve_bytes,
            JOURNAL_EFFECT_RESERVE_BYTES
        );

        let completion = vec![
            (
                3,
                Record::ModelCallCompleted {
                    turn: "trn_retention".into(),
                    logical_operation_id: "model:trn_retention:1".into(),
                    attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                    request_digest: "a".repeat(64),
                },
            ),
            (
                4,
                Record::ToolCall {
                    turn: "trn_retention".into(),
                    agent: "root".into(),
                    call: "op_retention".into(),
                    name: "managed".into(),
                    input: serde_json::json!({"value": "x"}),
                    detach: false,
                },
            ),
        ];
        let after_completion = project_retention(after_intent, &completion, u64::MAX).unwrap();
        assert_eq!(
            after_completion.effect_reserve_bytes, JOURNAL_TERMINAL_RESERVE_BYTES,
            "the provider terminal releases its half of the reserve but retains one complete Tool-terminal decision"
        );

        let terminal = Record::ToolResult {
            turn: "trn_retention".into(),
            agent: "root".into(),
            call: "op_retention".into(),
            name: "managed".into(),
            outcome: "completed".into(),
            content: "x".repeat(MAX_RECORD_CONTENT_BYTES),
            is_error: false,
            exit_code: Some(0),
            duration_ms: 1,
            truncated: false,
        };
        let after_terminal =
            project_retention(after_completion, &[(5, terminal)], u64::MAX).unwrap();
        assert_eq!(after_terminal.effect_reserve_bytes, 0);
        assert!(after_terminal.metered_bytes < after_completion.metered_bytes);
    }

    #[tokio::test]
    async fn retained_identity_quota_is_shared_by_roots_and_children_and_released_once() {
        let store = Arc::new(MemoryStore::default());
        let limits = JournalRetentionLimits {
            session_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES,
            tenant_bytes: DEFAULT_MAX_TENANT_JOURNAL_BYTES,
            tenant_sessions: 2,
        };
        let journal = Journal::new(store.clone(), "owner-retention").with_retention_limits(limits);
        let first = Record::State {
            state: "open".into(),
            turn: None,
        };
        let mut root = head_doc();
        root.tenant_id = "tenant-retention-identities".into();
        root.root_id = "ses_retained_root".into();
        journal
            .create("ses_retained_root", &root, &first)
            .await
            .unwrap();

        let mut child = root.clone();
        child.parent_id = Some(root.root_id.clone());
        child.ancestor_ids = vec![root.root_id.clone()];
        child.depth = 1;
        journal
            .create("ses_retained_child", &child, &first)
            .await
            .unwrap();
        assert_eq!(
            store
                .tenant_retention
                .lock()
                .expect("retention meter")
                .get(&root.tenant_id)
                .map(|meter| meter.1),
            Some(2)
        );

        let mut rejected = head_doc();
        rejected.tenant_id = root.tenant_id.clone();
        rejected.root_id = "ses_retained_rejected".into();
        assert!(matches!(
            journal
                .create("ses_retained_rejected", &rejected, &first)
                .await,
            Err(BrainError::TenantRetainedSessionQuotaExceeded { limit: 2 })
        ));

        let child_head = journal.get_head("ses_retained_child").await.unwrap();
        let terminal = DeletionStatusDoc {
            session_id: "ses_retained_child".into(),
            tenant_id: root.tenant_id.clone(),
            root_id: root.root_id.clone(),
            parent_id: Some(root.root_id.clone()),
            metered_storage_bytes: 0,
            metered_journal_bytes: child_head.retention.metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: u64::MAX,
            attempts: 1,
            last_error: None,
        };
        store.finalize_deletion(&terminal).await.unwrap();
        store.finalize_deletion(&terminal).await.unwrap();
        assert_eq!(
            store
                .tenant_retention
                .lock()
                .expect("retention meter")
                .get(&root.tenant_id)
                .map(|meter| meter.1),
            Some(1),
            "lost final response cannot release a retained identity twice"
        );
        journal
            .create("ses_retained_rejected", &rejected, &first)
            .await
            .expect("physical final deletion immediately frees one identity");
    }

    #[tokio::test]
    async fn journal_quota_rejection_is_atomic_and_end_uses_precharged_lifecycle_capacity() {
        let first = Record::State {
            state: "open".into(),
            turn: None,
        };
        let initial = initial_retention(&first, u64::MAX).unwrap();
        let user = user(
            "trn_session_limit",
            "ordinary append must not consume lifecycle capacity",
        );
        let ordinary_charge = serialized_record_charge(&[(2, user.clone())]).unwrap();
        let limits = JournalRetentionLimits {
            session_bytes: initial
                .metered_bytes
                .saturating_add(ordinary_charge)
                .saturating_sub(1),
            tenant_bytes: u64::MAX,
            tenant_sessions: 8,
        };
        let store = Arc::new(MemoryStore::default());
        let journal =
            Journal::new(store.clone(), "owner-session-limit").with_retention_limits(limits);
        let mut doc = head_doc();
        doc.root_id = "ses_session_limit".into();
        journal
            .create("ses_session_limit", &doc, &first)
            .await
            .unwrap();
        let head = journal.claim("ses_session_limit").await.unwrap();
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        assert!(matches!(
            journal
                .commit("ses_session_limit", &mut lease, &[(2, user)], &head.doc, 2,)
                .await,
            Err(BrainError::SessionJournalQuotaExceeded { .. })
        ));
        let persisted = journal.get_head("ses_session_limit").await.unwrap();
        assert_eq!(persisted.last_seq, 1);
        assert_eq!(persisted.retention, initial);
        assert_eq!(lease.last_seq, 1);
        assert_eq!(lease.retention, initial);

        let fenced = journal
            .fence_end("ses_session_limit")
            .await
            .expect("END consumes its create-time reserve even at the ordinary append ceiling");
        assert!(fenced.newly_fenced);
        assert_eq!(fenced.head.doc.state, "ending");
        assert_eq!(fenced.head.retention.metered_bytes, initial.metered_bytes);
        assert!(fenced.head.retention.lifecycle_reserve_bytes < initial.lifecycle_reserve_bytes);
    }

    #[tokio::test]
    async fn effect_terminal_commits_without_new_quota_after_intent_reservation() {
        let first = Record::State {
            state: "open".into(),
            turn: None,
        };
        let initial = initial_retention(&first, u64::MAX).unwrap();
        let intent = model_intent("trn_effect_reserve");
        let after_intent = project_retention(initial, &[(2, intent.clone())], u64::MAX).unwrap();
        let limits = JournalRetentionLimits {
            session_bytes: after_intent.metered_bytes,
            tenant_bytes: after_intent.metered_bytes,
            tenant_sessions: 1,
        };
        let store = Arc::new(MemoryStore::default());
        let journal = Journal::new(store, "owner-effect-reserve").with_retention_limits(limits);
        let mut doc = head_doc();
        doc.root_id = "ses_effect_reserve".into();
        journal
            .create("ses_effect_reserve", &doc, &first)
            .await
            .unwrap();
        let head = journal.claim("ses_effect_reserve").await.unwrap();
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        journal
            .commit(
                "ses_effect_reserve",
                &mut lease,
                &[(2, intent)],
                &head.doc,
                2,
            )
            .await
            .expect("intent atomically reserves every later terminal byte");
        assert_eq!(lease.retention, after_intent);

        let completed = vec![
            (
                3,
                Record::ModelCallCompleted {
                    turn: "trn_effect_reserve".into(),
                    logical_operation_id: "model:trn_effect_reserve:1".into(),
                    attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                    request_digest: "a".repeat(64),
                },
            ),
            (
                4,
                Record::ToolCall {
                    turn: "trn_effect_reserve".into(),
                    agent: "root".into(),
                    call: "op_effect_reserve".into(),
                    name: "managed".into(),
                    input: serde_json::json!({"value": true}),
                    detach: false,
                },
            ),
        ];
        journal
            .commit("ses_effect_reserve", &mut lease, &completed, &head.doc, 4)
            .await
            .expect("provider terminal consumes only the already-reserved capacity");
        let tool_result = Record::ToolResult {
            turn: "trn_effect_reserve".into(),
            agent: "root".into(),
            call: "op_effect_reserve".into(),
            name: "managed".into(),
            outcome: "completed".into(),
            content: "terminal".into(),
            is_error: false,
            exit_code: Some(0),
            duration_ms: 1,
            truncated: false,
        };
        journal
            .commit(
                "ses_effect_reserve",
                &mut lease,
                &[(5, tool_result)],
                &head.doc,
                5,
            )
            .await
            .expect("executed Tool terminal stays journalable at the tenant/session ceiling");
        assert_eq!(lease.retention.effect_reserve_bytes, 0);
    }

    #[tokio::test]
    async fn recovery_cursor_reaches_later_due_rows_while_early_rows_remain_due() {
        let store = MemoryStore::default();
        let shard = "r00";
        let mut ids = Vec::new();
        for candidate in 0..20_000u32 {
            let id = format!("ses_cursor_{candidate:08}");
            if recovery_shard(&id) == shard {
                ids.push(id);
                if ids.len() == 40 {
                    break;
                }
            }
        }
        assert_eq!(ids.len(), 40);
        for (index, id) in ids.iter().enumerate() {
            let mut doc = head_doc();
            doc.root_id = id.clone();
            doc.state = "open".into();
            doc.turn = Some(format!("trn_cursor_{index:08}"));
            doc.active_phase = Some("model_running".into());
            doc.recovery_due_ms = Some(1);
            create_memory_store(
                &store,
                id,
                &doc,
                &Record::State {
                    state: "open".into(),
                    turn: doc.turn.clone(),
                },
                "owner-a",
                0,
            )
            .await
            .unwrap();
        }
        let first = store
            .list_recovery_page(&RecoveryQuery {
                shard,
                due_before_ms: 1,
                limit: 32,
                cursor: None,
            })
            .await
            .unwrap();
        assert_eq!(first.items.len(), 32);
        let cursor = first.next_cursor.expect("more due rows");
        let second = store
            .list_recovery_page(&RecoveryQuery {
                shard,
                due_before_ms: 1,
                limit: 32,
                cursor: Some(&cursor),
            })
            .await
            .unwrap();
        assert_eq!(second.items.len(), 8);
        assert!(second.next_cursor.is_none());
        assert!(
            first
                .items
                .iter()
                .all(|item| item.due_ms == 1 && item.state == "open")
        );
    }
}
