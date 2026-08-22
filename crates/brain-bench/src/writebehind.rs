//! Write-behind journal arm: the pi/codex local-first model. The kernel runs on the
//! in-memory journal (acks never wait for durability); every committed record is also
//! queued to a background writer that persists it to sqlite or DynamoDB *outside* the
//! ack path, the way a local agent appends its session file.
//!
//! The foreground numbers of this arm are by construction the memory arm's numbers.
//! What it adds is the async pipeline's honesty metrics:
//!   - persistence lag (commit-ack -> durable-ack) p50/p99,
//!   - peak backlog (records acked but not yet durable), and
//!   - the unpersisted tail at arm end — the loss window a crash would take.
//!
//! No fences, no conditions, no dedup on the durable side: local-first has none.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use brain::journal::Record;

pub struct Metrics {
    pub persisted: AtomicU64,
    pub errors: AtomicU64,
    pub backlog: AtomicUsize,
    pub max_backlog: AtomicUsize,
    pub lag_ns: Mutex<Vec<u128>>,
}

static METRICS: OnceLock<Metrics> = OnceLock::new();

pub fn metrics() -> &'static Metrics {
    METRICS.get_or_init(|| Metrics {
        persisted: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        backlog: AtomicUsize::new(0),
        max_backlog: AtomicUsize::new(0),
        lag_ns: Mutex::new(Vec::new()),
    })
}

pub struct Job {
    pub enqueued: Instant,
    pub session_id: String,
    pub records: Vec<(u64, Record)>,
}

pub enum Sink {
    Sqlite(String),
    Dynamo { table: String },
}

/// Start the background writer; returns the sender the journal hook feeds.
pub fn start(sink: Sink) -> tokio::sync::mpsc::UnboundedSender<Job> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
    let m = metrics();
    tokio::spawn(async move {
        match sink {
            Sink::Sqlite(path) => {
                // One writer connection, WAL + synchronous=FULL like the real local store.
                let conn = tokio::task::spawn_blocking(move || {
                    let conn = rusqlite::Connection::open(path).expect("wb sqlite open");
                    conn.pragma_update(None, "journal_mode", "WAL")
                        .expect("wal");
                    conn.pragma_update(None, "synchronous", "FULL")
                        .expect("sync");
                    conn.execute(
                        "CREATE TABLE IF NOT EXISTS wb_journal (
                             session TEXT NOT NULL, seq INTEGER NOT NULL, body TEXT NOT NULL,
                             PRIMARY KEY (session, seq))",
                        [],
                    )
                    .expect("wb schema");
                    conn
                })
                .await
                .expect("wb sqlite init");
                let conn = std::sync::Arc::new(std::sync::Mutex::new(conn));
                let mut buf: Vec<Job> = Vec::new();
                loop {
                    let n = rx.recv_many(&mut buf, 64).await;
                    if n == 0 {
                        break;
                    }
                    let jobs: Vec<Job> = std::mem::take(&mut buf);
                    let conn = conn.clone();
                    let done: Vec<(Instant, usize)> =
                        jobs.iter().map(|j| (j.enqueued, j.records.len())).collect();
                    let ok = tokio::task::spawn_blocking(move || {
                        let mut conn = conn.lock().expect("wb conn");
                        let tx = conn.transaction().expect("wb tx");
                        for job in &jobs {
                            for (seq, record) in &job.records {
                                tx.execute(
                                    "INSERT OR IGNORE INTO wb_journal (session, seq, body) VALUES (?1, ?2, ?3)",
                                    rusqlite::params![
                                        job.session_id,
                                        *seq as i64,
                                        serde_json::to_string(record).expect("record json"),
                                    ],
                                )
                                .expect("wb insert");
                            }
                        }
                        tx.commit().is_ok()
                    })
                    .await
                    .unwrap_or(false);
                    settle(m, &done, ok);
                }
            }
            Sink::Dynamo { table } => {
                let aws = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                let client = aws_sdk_dynamodb::Client::new(&aws);
                let mut buf: Vec<Job> = Vec::new();
                loop {
                    let n = rx.recv_many(&mut buf, 8).await;
                    if n == 0 {
                        break;
                    }
                    // BatchWriteItem in 25-item pages; unprocessed items are resubmitted.
                    use aws_sdk_dynamodb::types::{AttributeValue, PutRequest, WriteRequest};
                    let mut writes = Vec::new();
                    let mut done: Vec<(Instant, usize)> = Vec::new();
                    for job in buf.drain(..) {
                        done.push((job.enqueued, job.records.len()));
                        for (seq, record) in &job.records {
                            writes.push(
                                WriteRequest::builder()
                                    .put_request(
                                        PutRequest::builder()
                                            .item(
                                                "pk",
                                                AttributeValue::S(format!("WB#{}", job.session_id)),
                                            )
                                            .item("sk", AttributeValue::S(format!("E#{seq:020}")))
                                            .item(
                                                "body",
                                                AttributeValue::S(
                                                    serde_json::to_string(record)
                                                        .expect("record json"),
                                                ),
                                            )
                                            .build()
                                            .expect("put"),
                                    )
                                    .build(),
                            );
                        }
                    }
                    let mut ok = true;
                    for page in writes.chunks(25) {
                        let mut pending = page.to_vec();
                        for _ in 0..8 {
                            match client
                                .batch_write_item()
                                .request_items(table.clone(), pending.clone())
                                .send()
                                .await
                            {
                                Ok(out) => {
                                    pending = out
                                        .unprocessed_items()
                                        .and_then(|u| u.get(&table).cloned())
                                        .unwrap_or_default();
                                    if pending.is_empty() {
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                }
                                Err(_) => {
                                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                }
                            }
                        }
                        if !pending.is_empty() {
                            ok = false;
                        }
                    }
                    settle(m, &done, ok);
                }
            }
        }
    });
    tx
}

fn settle(m: &Metrics, done: &[(Instant, usize)], ok: bool) {
    let mut lag = m.lag_ns.lock().expect("wb lag");
    for (enqueued, records) in done {
        m.backlog.fetch_sub(*records, Ordering::Relaxed);
        if ok {
            m.persisted.fetch_add(*records as u64, Ordering::Relaxed);
            lag.push(enqueued.elapsed().as_nanos());
        } else {
            m.errors.fetch_add(*records as u64, Ordering::Relaxed);
        }
    }
}

pub fn enqueue(tx: &tokio::sync::mpsc::UnboundedSender<Job>, job: Job) {
    let m = metrics();
    let records = job.records.len();
    let backlog = m.backlog.fetch_add(records, Ordering::Relaxed) + records;
    m.max_backlog.fetch_max(backlog, Ordering::Relaxed);
    let _ = tx.send(job);
}

/// Report after the measurement window: capture the loss window (records acked but not
/// durable at measurement end), then wait for the writer to drain so lag stats are complete.
pub async fn report_after_drain() -> Option<String> {
    let m = METRICS.get()?;
    let tail_at_end = m.backlog.load(Ordering::Relaxed);
    let t0 = Instant::now();
    while m.backlog.load(Ordering::Relaxed) > 0 && t0.elapsed().as_secs() < 30 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let drain_ms = t0.elapsed().as_millis();
    report().map(|line| format!("{line} tail_at_measure_end={tail_at_end} drain_ms={drain_ms}"))
}

pub fn report() -> Option<String> {
    let m = METRICS.get()?;
    let mut lag: Vec<u128> = m.lag_ns.lock().expect("wb lag").clone();
    lag.sort_unstable();
    let q = |p: f64| {
        if lag.is_empty() {
            0.0
        } else {
            lag[((p * lag.len() as f64) as usize).min(lag.len() - 1)] as f64 / 1e6
        }
    };
    Some(format!(
        "writebehind: persisted={} errors={} lag_ms p50={:.2} p99={:.2} max={:.2} max_backlog={} unpersisted_tail={}",
        m.persisted.load(Ordering::Relaxed),
        m.errors.load(Ordering::Relaxed),
        q(0.5),
        q(0.99),
        lag.last().map(|v| *v as f64 / 1e6).unwrap_or(0.0),
        m.max_backlog.load(Ordering::Relaxed),
        m.backlog.load(Ordering::Relaxed),
    ))
}

// ---------------------------------------------------------------------------
// The wrapper store: memory-authoritative, commits also feed the async writer.

use brain::journal::{
    ChildListQuery, ChildPage, CommitDecision, CreateDecision, DeletionStatusDoc, EndFence, Head,
    HeadDoc, JournalRetentionLimits, JournalStore, MemoryStore, RecordPage, RecordPageQuery,
    RecoveryPage, RecoveryQuery, SandboxInventoryDoc, SandboxListQuery, SandboxPage,
    SandboxReserveRequest, SandboxUpdateRequest, SessionListQuery, SessionPage,
};

pub struct WriteBehindStore {
    memory: MemoryStore,
    tx: tokio::sync::mpsc::UnboundedSender<Job>,
}

impl WriteBehindStore {
    pub fn new(sink: Sink) -> Self {
        Self {
            memory: MemoryStore::default(),
            tx: start(sink),
        }
    }
}

#[async_trait::async_trait]
impl JournalStore for WriteBehindStore {
    async fn create(&self, decision: &CreateDecision<'_>) -> brain::Result<()> {
        self.memory.create(decision).await
    }
    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> brain::Result<Head> {
        self.memory.claim(session_id, owner, now_ms).await
    }
    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> brain::Result<EndFence> {
        self.memory
            .fence_end(session_id, now_ms, retention_limits)
            .await
    }
    async fn get_head(&self, session_id: &str) -> brain::Result<Head> {
        self.memory.get_head(session_id).await
    }
    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> brain::Result<RecordPage> {
        self.memory.read_record_page(query).await
    }
    async fn commit(&self, decision: &CommitDecision<'_>) -> brain::Result<()> {
        self.memory.commit(decision).await?;
        enqueue(
            &self.tx,
            Job {
                enqueued: Instant::now(),
                session_id: decision.session_id.to_string(),
                records: decision.records.to_vec(),
            },
        );
        Ok(())
    }
    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> brain::Result<()> {
        self.memory.release(session_id, owner, fence).await
    }
    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> brain::Result<()> {
        self.memory
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
        self.memory
            .renew(session_id, owner, fence, now_ms, recovery_due_ms)
            .await
    }
    async fn purge_history(&self, session_id: &str) -> brain::Result<u64> {
        self.memory.purge_history(session_id).await
    }
    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> brain::Result<()> {
        self.memory.put_deletion_status(status).await
    }
    async fn get_deletion_status(
        &self,
        session_id: &str,
    ) -> brain::Result<Option<DeletionStatusDoc>> {
        self.memory.get_deletion_status(session_id).await
    }
    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> brain::Result<()> {
        self.memory.finalize_deletion(status).await
    }
    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> brain::Result<SessionPage> {
        self.memory.list_session_page(query).await
    }
    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> brain::Result<ChildPage> {
        self.memory.list_child_page(query).await
    }
    async fn reserve_sandbox(
        &self,
        request: &SandboxReserveRequest,
    ) -> brain::Result<SandboxInventoryDoc> {
        self.memory.reserve_sandbox(request).await
    }
    async fn get_sandbox(
        &self,
        root_id: &str,
        sandbox_id: &str,
    ) -> brain::Result<SandboxInventoryDoc> {
        self.memory.get_sandbox(root_id, sandbox_id).await
    }
    async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> brain::Result<SandboxPage> {
        self.memory.list_sandbox_page(query).await
    }
    async fn update_sandbox(
        &self,
        request: &SandboxUpdateRequest,
    ) -> brain::Result<SandboxInventoryDoc> {
        self.memory.update_sandbox(request).await
    }
    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> brain::Result<RecoveryPage> {
        self.memory.list_recovery_page(query).await
    }
    async fn list_sessions(&self, limit: usize) -> brain::Result<Vec<Head>> {
        self.memory.list_sessions(limit).await
    }
}
