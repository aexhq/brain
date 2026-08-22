use super::*;

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

/// Everything one durable CREATE decision needs, computed once by [`Journal`]: the store
/// applies it atomically and adds nothing.
pub struct CreateDecision<'a> {
    pub session_id: &'a str,
    pub doc: &'a HeadDoc,
    pub first: &'a Record,
    pub now_ms: u64,
    pub tenant_storage_limit: u64,
    pub retention: JournalRetention,
    pub retention_limits: JournalRetentionLimits,
}

/// Everything one durable COMMIT decision needs, computed once by [`Journal`]. Deltas and
/// projections are policy outputs; the store's job is the single conditional write.
pub struct CommitDecision<'a> {
    pub session_id: &'a str,
    /// The lease owner this commit is conditioned on; a superseded owner's write must fail.
    pub owner: &'a str,
    pub fence: u64,
    pub records: &'a [(u64, Record)],
    pub doc: &'a HeadDoc,
    pub high_water: u64,
    pub now_ms: u64,
    pub tenant_storage_delta: i64,
    pub tenant_storage_limit: u64,
    pub retention: JournalRetention,
    pub tenant_retention_delta: i64,
    pub retention_limits: JournalRetentionLimits,
}

#[async_trait::async_trait]
pub trait JournalStore: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn create(&self, decision: &CreateDecision<'_>) -> Result<()>;
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
    async fn commit(&self, decision: &CommitDecision<'_>) -> Result<()>;
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
            .create(&CreateDecision {
                session_id,
                doc: &doc,
                first,
                now_ms,
                tenant_storage_limit: self.tenant_storage_limit,
                retention,
                retention_limits: self.retention_limits,
            })
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
            .commit(&CommitDecision {
                session_id,
                owner: &self.owner,
                fence: lease.fence,
                records,
                doc: &doc,
                high_water,
                now_ms,
                tenant_storage_delta,
                tenant_storage_limit: self.tenant_storage_limit,
                retention: next_retention,
                tenant_retention_delta,
                retention_limits: self.retention_limits,
            })
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
