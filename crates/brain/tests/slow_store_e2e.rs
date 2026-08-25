//! The journal contract must hold on a slow store, not only on the in-memory double.
//!
//! The 2026-08-22 kernel-overhead spike found recovery/retry machinery firing routinely
//! against DynamoDB-class latency that never fires in fast-store tests (216 model calls
//! for 200 turns), and a store-side misclassification that only concurrency + latency
//! exposed. This lane drives concurrent sessions through the real kernel over a journal
//! whose every operation pays an injected delay, and asserts the invariants that latency
//! is known to threaten: every turn completes, and the provider is called exactly once
//! per turn — no spurious replacement attempts, no duplicate rounds, no stolen leases.

use brain::adapter::DisabledToolExecutor;
use brain::config::Dialect;
use brain::journal::{
    ChildListQuery, ChildPage, CommitDecision, CreateDecision, DeletionStatusDoc, EndFence, Head,
    HeadDoc, Journal, JournalRetentionLimits, JournalStore, MemoryStore, RecordPage,
    RecordPageQuery, RecoveryPage, RecoveryQuery, SessionListQuery, SessionPage,
};
use brain::provider::Provider;
use brain::provider::fake::{FakeMode, FakeProvider};
use brain::session::{Brain, BrainConfig};
use brain_protocol::session::{CreateSessionRequest, MessageRequestContent};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

mod support;

/// Every store operation pays the injected delay; mutations pay double. 15 ms per op is
/// DynamoDB-class once a decision makes two or three of them, while keeping the whole
/// lane under CI budgets.
struct SlowStore {
    inner: MemoryStore,
    read_delay: Duration,
    write_delay: Duration,
}

impl SlowStore {
    fn new() -> Self {
        Self {
            inner: MemoryStore::default(),
            read_delay: Duration::from_millis(8),
            write_delay: Duration::from_millis(15),
        }
    }
    async fn read(&self) {
        tokio::time::sleep(self.read_delay).await;
    }
    async fn write(&self) {
        tokio::time::sleep(self.write_delay).await;
    }
}

#[async_trait::async_trait]
impl JournalStore for SlowStore {
    async fn create(&self, decision: &CreateDecision<'_>) -> brain::Result<()> {
        self.write().await;
        self.inner.create(decision).await
    }
    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> brain::Result<Head> {
        self.write().await;
        self.inner.claim(session_id, owner, now_ms).await
    }
    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> brain::Result<EndFence> {
        self.write().await;
        self.inner
            .fence_end(session_id, now_ms, retention_limits)
            .await
    }
    async fn get_head(&self, session_id: &str) -> brain::Result<Head> {
        self.read().await;
        self.inner.get_head(session_id).await
    }
    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> brain::Result<RecordPage> {
        self.read().await;
        self.inner.read_record_page(query).await
    }
    async fn commit(&self, decision: &CommitDecision<'_>) -> brain::Result<()> {
        self.write().await;
        self.inner.commit(decision).await
    }
    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> brain::Result<()> {
        self.write().await;
        self.inner.release(session_id, owner, fence).await
    }
    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> brain::Result<()> {
        self.write().await;
        self.inner
            .release_and_schedule(session_id, owner, fence, doc, due_ms)
            .await
    }
    async fn renew(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        now_ms: u64,
        recovery_due_ms: Option<u64>,
    ) -> brain::Result<()> {
        self.write().await;
        self.inner
            .renew(session_id, owner, fence, now_ms, recovery_due_ms)
            .await
    }
    async fn purge_history(&self, session_id: &str) -> brain::Result<u64> {
        self.write().await;
        self.inner.purge_history(session_id).await
    }
    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> brain::Result<()> {
        self.write().await;
        self.inner.put_deletion_status(status).await
    }
    async fn get_deletion_status(
        &self,
        session_id: &str,
    ) -> brain::Result<Option<DeletionStatusDoc>> {
        self.read().await;
        self.inner.get_deletion_status(session_id).await
    }
    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> brain::Result<()> {
        self.write().await;
        self.inner.finalize_deletion(status).await
    }
    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> brain::Result<SessionPage> {
        self.read().await;
        self.inner.list_session_page(query).await
    }
    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> brain::Result<ChildPage> {
        self.read().await;
        self.inner.list_child_page(query).await
    }
    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> brain::Result<RecoveryPage> {
        self.read().await;
        self.inner.list_recovery_page(query).await
    }
    async fn list_sessions(&self, limit: usize) -> brain::Result<Vec<Head>> {
        self.read().await;
        self.inner.list_sessions(limit).await
    }
}

fn create_request() -> CreateSessionRequest {
    serde_json::from_value(json!({
<<<<<<< HEAD
        "model": support::model_config(),
        "component_artifacts": support::component_artifacts(),
=======
        "model": {
            "provider": "anthropic",
            "name": "scripted",
            "api_key": "sk-fake"
        },
>>>>>>> origin/main
        "agentloop": support::loop_config()
    }))
    .expect("typed create request")
}

async fn wait_turn_finished(brain: &Arc<Brain>, session_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let session = brain.get(session_id).await.expect("session status");
        if session.current_turn.is_none() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "turn did not finish for {session_id} on the slow store"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_turns_on_a_slow_store_complete_without_spurious_retries() {
    const SESSIONS: usize = 6;
    const TURNS: usize = 3;

    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.set_mode(FakeMode::Policy {
        tool_rounds: 0,
        parallel: 1,
        tool: "bash".into(),
        text_bytes: 256,
    });
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            max_concurrent_model_rounds: 64,
            max_concurrent_turns: 64,
            idle_discard: Duration::from_secs(60),
            ..BrainConfig::default()
        },
        Journal::new(Arc::new(SlowStore::new()), "slow-lane"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        support::services(),
        Arc::new(move |_| provider.clone() as Arc<dyn Provider>),
    );

    let mut ids = Vec::new();
    for i in 0..SESSIONS {
        let session = brain
            .create_session(create_request(), Some(&format!("slow-lane-{i}")))
            .await
            .expect("create session on slow store");
        ids.push(session.id.to_string());
    }

    let mut tasks = tokio::task::JoinSet::new();
    for id in &ids {
        let brain = brain.clone();
        let id = id.clone();
        tasks.spawn(async move {
            for turn in 0..TURNS {
                brain
                    .message(
                        &id,
                        MessageRequestContent::String(
                            format!("turn {turn}").parse().expect("message"),
                        ),
                    )
                    .await
                    .expect("admit message on slow store");
                wait_turn_finished(&brain, &id).await;
            }
            id
        });
    }
    while let Some(done) = tasks.join_next().await {
        done.expect("session task");
    }

    // The load-bearing assertion: latency alone must not trigger replacement attempts,
    // duplicate rounds, or lease steals. One provider call per turn, exactly.
    let calls = fake.call_count.load(Ordering::SeqCst);
    assert_eq!(
        calls,
        (SESSIONS * TURNS) as u64,
        "provider calls under injected store latency: expected one per turn"
    );

    for id in &ids {
        let session = brain.get(id).await.expect("final session state");
        assert!(
            session.current_turn.is_none(),
            "no turn left active for {id}"
        );
    }
}
