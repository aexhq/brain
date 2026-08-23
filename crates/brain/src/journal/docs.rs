use super::*;

// ---------------------------------------------------------------------------------------------
// HEAD
// ---------------------------------------------------------------------------------------------

/// Turn-level stop reasons as journaled. One value wider than the public contract: a recovery
/// that interrupts a turn journals `Interrupted`, which the public wire reports as `error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStopReason {
    EndTurn,
    Refusal,
    MaxRounds,
    Cancelled,
    Interrupted,
    Error,
}

impl std::str::FromStr for TurnStopReason {
    type Err = BrainError;

    fn from_str(value: &str) -> Result<Self> {
        Ok(match value {
            "end_turn" => TurnStopReason::EndTurn,
            "refusal" => TurnStopReason::Refusal,
            "max_rounds" => TurnStopReason::MaxRounds,
            "cancelled" => TurnStopReason::Cancelled,
            "interrupted" => TurnStopReason::Interrupted,
            "error" => TurnStopReason::Error,
            other => {
                return Err(BrainError::Journal(format!(
                    "unknown turn stop reason {other:?}"
                )));
            }
        })
    }
}

impl TurnStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnStopReason::EndTurn => "end_turn",
            TurnStopReason::Refusal => "refusal",
            TurnStopReason::MaxRounds => "max_rounds",
            TurnStopReason::Cancelled => "cancelled",
            TurnStopReason::Interrupted => "interrupted",
            TurnStopReason::Error => "error",
        }
    }

    /// The public `turn.completed` narrowing: the wire has no `interrupted`.
    pub fn to_public(self) -> brain_protocol::session::StopReason {
        use brain_protocol::session::StopReason;
        match self {
            TurnStopReason::EndTurn => StopReason::EndTurn,
            TurnStopReason::Refusal => StopReason::Refusal,
            TurnStopReason::MaxRounds => StopReason::MaxRounds,
            TurnStopReason::Cancelled => StopReason::Cancelled,
            TurnStopReason::Interrupted | TurnStopReason::Error => StopReason::Error,
        }
    }
}

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
    /// The exhaustive destructure makes a forgotten new field a compile error, not silently
    /// dropped durable state.
    pub fn control_doc(&self) -> ControlDoc {
        let Self {
            tenant_id,
            last_seq,
            state,
            failure,
            turn,
            active_phase,
            provider_attempt,
            active_context,
            active_rounds,
            active_tool_calls,
            message_replays,
            context,
            turns,
            created_ms,
            updated_ms,
            recovery_due_ms,
            recovery_attempt,
            create_key_hash,
            create_request_hash,
            last_message_ms,
            ended,
            session_storage_bytes,
            storage_reserved_bytes,
            tenant_metered_storage_bytes,
            storage_upload,
            storage_delete,
            pending_customer_acks,
            pending_managed_acks,
            default_sandbox,
            loop_state,
            root_id: _,
            parent_id: _,
            ancestor_ids: _,
            child_name: _,
            context_fork: _,
            depth: _,
            prefix: _,
            key_b64: _,
            hand_secrets_b64: _,
        } = self;
        ControlDoc {
            tenant_id: tenant_id.clone(),
            last_seq: *last_seq,
            state: *state,
            failure: failure.clone(),
            turn: turn.clone(),
            active_phase: *active_phase,
            provider_attempt: provider_attempt.clone(),
            active_context: active_context.clone(),
            active_rounds: *active_rounds,
            active_tool_calls: *active_tool_calls,
            message_replays: message_replays.clone(),
            context: context.clone(),
            turns: *turns,
            created_ms: *created_ms,
            updated_ms: *updated_ms,
            recovery_due_ms: *recovery_due_ms,
            recovery_attempt: *recovery_attempt,
            create_key_hash: create_key_hash.clone(),
            create_request_hash: create_request_hash.clone(),
            last_message_ms: *last_message_ms,
            ended: *ended,
            session_storage_bytes: *session_storage_bytes,
            storage_reserved_bytes: *storage_reserved_bytes,
            tenant_metered_storage_bytes: *tenant_metered_storage_bytes,
            storage_upload: storage_upload.clone(),
            storage_delete: storage_delete.clone(),
            pending_customer_acks: pending_customer_acks.clone(),
            pending_managed_acks: pending_managed_acks.clone(),
            default_sandbox: default_sandbox.clone(),
            loop_state: *loop_state,
        }
    }

    /// Clone the immutable create-time CONFIG projection. This is used only while creating a
    /// session (and in explicit integrity tooling), never by an ordinary journal commit.
    pub fn config_doc(&self) -> ConfigDoc {
        let Self {
            root_id,
            parent_id,
            ancestor_ids,
            child_name,
            context_fork,
            depth,
            prefix,
            key_b64,
            hand_secrets_b64,
            tenant_id: _,
            last_seq: _,
            state: _,
            failure: _,
            turn: _,
            active_phase: _,
            provider_attempt: _,
            active_context: _,
            active_rounds: _,
            active_tool_calls: _,
            message_replays: _,
            context: _,
            turns: _,
            created_ms: _,
            updated_ms: _,
            recovery_due_ms: _,
            recovery_attempt: _,
            create_key_hash: _,
            create_request_hash: _,
            last_message_ms: _,
            ended: _,
            session_storage_bytes: _,
            storage_reserved_bytes: _,
            tenant_metered_storage_bytes: _,
            storage_upload: _,
            storage_delete: _,
            pending_customer_acks: _,
            pending_managed_acks: _,
            default_sandbox: _,
            loop_state: _,
        } = self;
        ConfigDoc {
            root_id: root_id.clone(),
            parent_id: parent_id.clone(),
            ancestor_ids: ancestor_ids.clone(),
            child_name: child_name.clone(),
            context_fork: context_fork.clone(),
            depth: *depth,
            prefix: prefix.clone(),
            key_b64: key_b64.clone(),
            hand_secrets_b64: hand_secrets_b64.clone(),
        }
    }

    pub fn split(&self) -> (ControlDoc, ConfigDoc) {
        (self.control_doc(), self.config_doc())
    }

    pub fn join(control: ControlDoc, config: ConfigDoc) -> Self {
        let ControlDoc {
            tenant_id,
            last_seq,
            state,
            failure,
            turn,
            active_phase,
            provider_attempt,
            active_context,
            active_rounds,
            active_tool_calls,
            message_replays,
            context,
            turns,
            created_ms,
            updated_ms,
            recovery_due_ms,
            recovery_attempt,
            create_key_hash,
            create_request_hash,
            last_message_ms,
            ended,
            session_storage_bytes,
            storage_reserved_bytes,
            tenant_metered_storage_bytes,
            storage_upload,
            storage_delete,
            pending_customer_acks,
            pending_managed_acks,
            default_sandbox,
            loop_state,
        } = control;
        let ConfigDoc {
            root_id,
            parent_id,
            ancestor_ids,
            child_name,
            context_fork,
            depth,
            prefix,
            key_b64,
            hand_secrets_b64,
        } = config;
        Self {
            tenant_id,
            last_seq,
            state,
            failure,
            turn,
            active_phase,
            provider_attempt,
            active_context,
            active_rounds,
            active_tool_calls,
            message_replays,
            context,
            turns,
            created_ms,
            updated_ms,
            recovery_due_ms,
            recovery_attempt,
            create_key_hash,
            create_request_hash,
            last_message_ms,
            ended,
            session_storage_bytes,
            storage_reserved_bytes,
            tenant_metered_storage_bytes,
            storage_upload,
            storage_delete,
            pending_customer_acks,
            pending_managed_acks,
            default_sandbox,
            loop_state,
            root_id,
            parent_id,
            ancestor_ids,
            child_name,
            context_fork,
            depth,
            prefix,
            key_b64,
            hand_secrets_b64,
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
    /// The sealed agent loop identity (`contracts/agentloop/v1` selector semantics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agentloop: Option<AgentloopSelectorDoc>,
}

/// Which agent loop a session sealed at create. Children inherit the parent's selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopSelectorDoc {
    pub source_bundle_sha256: String,
    pub source_bundle_bytes: u64,
    pub toolchain: String,
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
