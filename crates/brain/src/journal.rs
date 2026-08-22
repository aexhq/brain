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
        state: SessionLifecycle,
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

/// Session lifecycle vocabulary shared by `HeadDoc`, `ControlDoc`, projections and the public
/// `session.updated` transition record. Journal/wire encodings are the snake_case names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecycle {
    Open,
    Ending,
    Ended,
    Deleting,
    Deleted,
    Failed,
}

impl SessionLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionLifecycle::Open => "open",
            SessionLifecycle::Ending => "ending",
            SessionLifecycle::Ended => "ended",
            SessionLifecycle::Deleting => "deleting",
            SessionLifecycle::Deleted => "deleted",
            SessionLifecycle::Failed => "failed",
        }
    }
}

impl std::str::FromStr for SessionLifecycle {
    type Err = BrainError;

    fn from_str(value: &str) -> Result<Self> {
        Ok(match value {
            "open" => SessionLifecycle::Open,
            "ending" => SessionLifecycle::Ending,
            "ended" => SessionLifecycle::Ended,
            "deleting" => SessionLifecycle::Deleting,
            "deleted" => SessionLifecycle::Deleted,
            "failed" => SessionLifecycle::Failed,
            other => {
                return Err(BrainError::Journal(format!(
                    "unknown session lifecycle state {other:?}"
                )));
            }
        })
    }
}

/// Durable provider attempt states for the current logical model/compaction operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAttemptState {
    Intent,
    Running,
    Unknown,
    ReplacementReady,
}

impl ProviderAttemptState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderAttemptState::Intent => "intent",
            ProviderAttemptState::Running => "running",
            ProviderAttemptState::Unknown => "unknown",
            ProviderAttemptState::ReplacementReady => "replacement_ready",
        }
    }
}

/// Storage upload reservation states. Published retains its byte reservation until staging
/// deletion succeeds; completed is a bounded replay tombstone and reserves zero bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadReservationState {
    Reserved,
    InlineReserved,
    Published,
    Completed,
}

impl UploadReservationState {
    pub fn as_str(self) -> &'static str {
        match self {
            UploadReservationState::Reserved => "reserved",
            UploadReservationState::InlineReserved => "inline_reserved",
            UploadReservationState::Published => "published",
            UploadReservationState::Completed => "completed",
        }
    }
}

/// Confirmed-deletion progression; the vocabulary is the public DeletionStatus contract
/// (`contracts/session/v1/openapi.yaml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionState {
    Accepted,
    Deleting,
    Retrying,
    Blocked,
    Succeeded,
}

impl DeletionState {
    pub fn as_str(self) -> &'static str {
        match self {
            DeletionState::Accepted => "accepted",
            DeletionState::Deleting => "deleting",
            DeletionState::Retrying => "retrying",
            DeletionState::Blocked => "blocked",
            DeletionState::Succeeded => "succeeded",
        }
    }
}

/// Total active-turn phase vocabulary. `HeadDoc::active_phase` is `Some` iff a turn is active;
/// the wire and journal encodings are the snake_case names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    ReadyToBuildModelRequest,
    ReadyToContinueModel,
    ReadyToDispatchTools,
    ReadyToFinish,
    ReadyToCompact,
    ModelIntentCommitted,
    ModelRunning,
    ModelUnknown,
    CompactionIntentCommitted,
    CompactionRunning,
    CompactionUnknown,
    ManagedRunning,
    ManagedCancelling,
}

impl TurnPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnPhase::ReadyToBuildModelRequest => "ready_to_build_model_request",
            TurnPhase::ReadyToContinueModel => "ready_to_continue_model",
            TurnPhase::ReadyToDispatchTools => "ready_to_dispatch_tools",
            TurnPhase::ReadyToFinish => "ready_to_finish",
            TurnPhase::ReadyToCompact => "ready_to_compact",
            TurnPhase::ModelIntentCommitted => "model_intent_committed",
            TurnPhase::ModelRunning => "model_running",
            TurnPhase::ModelUnknown => "model_unknown",
            TurnPhase::CompactionIntentCommitted => "compaction_intent_committed",
            TurnPhase::CompactionRunning => "compaction_running",
            TurnPhase::CompactionUnknown => "compaction_unknown",
            TurnPhase::ManagedRunning => "managed_running",
            TurnPhase::ManagedCancelling => "managed_cancelling",
        }
    }
}

impl std::str::FromStr for TurnPhase {
    type Err = BrainError;

    fn from_str(value: &str) -> Result<Self> {
        Ok(match value {
            "ready_to_build_model_request" => TurnPhase::ReadyToBuildModelRequest,
            "ready_to_continue_model" => TurnPhase::ReadyToContinueModel,
            "ready_to_dispatch_tools" => TurnPhase::ReadyToDispatchTools,
            "ready_to_finish" => TurnPhase::ReadyToFinish,
            "ready_to_compact" => TurnPhase::ReadyToCompact,
            "model_intent_committed" => TurnPhase::ModelIntentCommitted,
            "model_running" => TurnPhase::ModelRunning,
            "model_unknown" => TurnPhase::ModelUnknown,
            "compaction_intent_committed" => TurnPhase::CompactionIntentCommitted,
            "compaction_running" => TurnPhase::CompactionRunning,
            "compaction_unknown" => TurnPhase::CompactionUnknown,
            "managed_running" => TurnPhase::ManagedRunning,
            "managed_cancelling" => TurnPhase::ManagedCancelling,
            other => {
                return Err(BrainError::Journal(format!(
                    "unknown active-turn phase {other:?}"
                )));
            }
        })
    }
}

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
    pub state: SessionLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    /// Total active-turn phase. `None` iff no turn is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_phase: Option<TurnPhase>,
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
    pub state: SessionLifecycle,
    pub failure: Option<FailureDoc>,
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_phase: Option<TurnPhase>,
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
    pub state: UploadReservationState,
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
    pub fn lifecycle_after_turn(&self) -> SessionLifecycle {
        if self.ended {
            match self.state {
                SessionLifecycle::Ended
                | SessionLifecycle::Deleting
                | SessionLifecycle::Deleted => self.state,
                _ => SessionLifecycle::Ending,
            }
        } else {
            SessionLifecycle::Open
        }
    }

    /// Clone only the mutable control projection. Hot-path commits must never clone the sealed
    /// CONFIG payload (Tool schemas, rendered prefix, custody blobs) merely to discard it.
    pub fn control_doc(&self) -> ControlDoc {
        ControlDoc {
            tenant_id: self.tenant_id.clone(),
            last_seq: self.last_seq,
            state: self.state,
            failure: self.failure.clone(),
            turn: self.turn.clone(),
            active_phase: self.active_phase,
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
    pub state: ProviderAttemptState,
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
    pub state: Option<SessionLifecycle>,
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
    pub state: SessionLifecycle,
    pub active_phase: Option<TurnPhase>,
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
    /// Confirmed-deletion progression.
    pub state: DeletionState,
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
    pub state: SessionLifecycle,
    pub failure: Option<FailureDoc>,
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_phase: Option<TurnPhase>,
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
            state: doc.state,
            failure: doc.failure.clone(),
            turn: doc.turn.clone(),
            active_phase: doc.active_phase,
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
        if deletions.get(&status.session_id).is_some_and(|existing| {
            existing.state == DeletionState::Succeeded && status.state != DeletionState::Succeeded
        }) {
            return Ok(());
        }
        deletions.insert(status.session_id.clone(), status.clone());
        Ok(())
    }

    async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions.get(session_id).is_some_and(|status| {
            status.state == DeletionState::Succeeded && status.expires_at_ms <= crate::wall_ms()
        }) {
            deletions.remove(session_id);
        }
        Ok(deletions.get(session_id).cloned())
    }

    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions
            .get(&status.session_id)
            .is_some_and(|existing| existing.state == DeletionState::Succeeded)
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
                                    state: session.doc.state,
                                    active_phase: session.doc.active_phase,
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
    doc.state = SessionLifecycle::Ending;
    doc.updated_ms = now_ms;
    doc.last_seq = sequence;
    doc.recovery_attempt = 0;
    doc = doc.with_recovery_projection(now_ms);
    let record = Record::State {
        state: SessionLifecycle::Ending,
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
#[path = "journal_tests.rs"]
mod tests;
