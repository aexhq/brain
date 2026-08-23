use super::*;

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
    /// Exact managed-Environment request envelope committed before `EnvironmentPort::submit`.
    ManagedCallIntent {
        turn: String,
        call: String,
        name: String,
        envelope: brain_protocol::environment::OperationEnvelope,
    },
    /// Opaque rooted receipt committed before Brain begins observing the operation.
    ManagedCallAccepted {
        turn: String,
        call: String,
        operation: brain_protocol::environment::OperationRef,
    },
    /// `EnvironmentPort::submit` may have reached the guest, but no rooted operation receipt can be
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
        outcome: brain_protocol::session::ToolOutcome,
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
        operation: brain_protocol::environment::OperationRef,
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
        stop_reason: TurnStopReason,
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
    /// A session/environment state transition worth telling clients about (`session.updated`).
    State {
        state: SessionLifecycle,
        turn: Option<String>,
    },
    EnvironmentLost {
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
    /// A public/engine sandbox file mutation is sealed before it reaches Environment. The deterministic
    /// operation identity lets an exact HTTP retry recover a lost response without repeating the
    /// effect; Environment rejects the same operation id with a different request digest.
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
    /// projection; Environment returns the opaque physical locator and generation.
    DefaultSandboxChanged {
        status: brain_protocol::environment::SandboxStatus,
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
            Record::EnvironmentLost { .. } => "environment_lost",
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

pub(super) fn serialized_record_charge(records: &[(u64, Record)]) -> Result<u64> {
    records.iter().try_fold(0_u64, |total, (_, record)| {
        let bytes = serde_json::to_vec(record)?.len() as u64;
        total
            .checked_add(bytes)
            .and_then(|value| value.checked_add(ESTIMATED_ITEM_ENVELOPE_BYTES as u64))
            .ok_or_else(|| BrainError::Journal("journal retention meter overflowed".into()))
    })
}

fn retention_class(records: &[(u64, Record)]) -> RetentionClass {
    use brain_protocol::environment::SandboxState;

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
            | Record::EnvironmentLost { .. } => {
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
