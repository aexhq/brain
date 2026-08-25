//! Benchmark gates for Brain. Every number measures the engine, never a model:
//! the provider is the scripted fake (instant unless paced), the environment is an in-process echo,
//! the journal is in-memory — and the drive path is the real public HTTP API with SSE, because
//! that is what production serves.
//!
//! Arms:
//!   density  N resident sessions -> KiB per resident session (journal-neutral: the resident
//!            sample minus the post-discard sample, because an idle session is nothing but its
//!            journal and the production journal lives in DynamoDB, not this process), then
//!            delete-all -> memory returned with explicit allocator reclamation.
//!            The server runs in its OWN process and is sampled via /proc/<pid>: client-side
//!            buffers (reqwest pools, driver vecs) must not pollute the brain's numbers.
//!            Linux-only by construction (smaps_rollup; run on the production target).
//!   turns    K concurrent sessions x M turns, unpaced -> turns/s, per-turn latency, admission
//!            latency (POST -> turn.started), and with --tool-rounds 0 the platform-added TTFT
//!            (POST -> first assistant.delta byte at the client; the provider is instant, so
//!            everything measured is us). In-process server: no memory sampling here.
//!   serve    (internal) the server child for `density`: composes the brain, prints
//!            `READY <base> <pid>`, parks. /bench/trim and /bench/guards ride next to the API.
//!   ci       the CI gate suite: fixed small profiles with thresholds; exits non-zero listing
//!            every gate that failed.

mod echo;
mod mem;
mod stats;
mod writebehind;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use brain::agentloop::{Agentloop, AgentloopRegistry, SequentialAgentloop};
use brain::config::Dialect;
use brain::journal::AgentloopSelectorDoc;
use brain::journal::Journal;
use brain::provider::Provider;
use brain::provider::fake::{FakeMode, FakeProvider};
use brain::session::{Brain, BrainConfig, BrainServices};
use clap::Parser;
use futures_util::StreamExt;
use serde_json::{Value, json};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "brain-bench",
    about = "benchmark gates: density, reclaim, turns/s, TTFT"
)]
struct Args {
    /// density | turns | ci | serve (internal)
    arm: String,
    /// Sessions (density: resident count; turns: concurrency K).
    #[arg(long, default_value_t = 128)]
    sessions: usize,
    /// Turns per session.
    #[arg(long, default_value_t = 4)]
    turns: usize,
    /// Tool rounds per turn (0 = pure text turn; that is the TTFT profile).
    #[arg(long, default_value_t = 2)]
    tool_rounds: u32,
    /// Parallel tool calls per round.
    #[arg(long, default_value_t = 4)]
    parallel: usize,
    /// Assistant text bytes in the final round (what bulks a fold).
    #[arg(long, default_value_t = 8192)]
    text_bytes: usize,
    /// Fully inspect 1 request in N (scan/parse cross-check + arrival record).
    #[arg(long, default_value_t = 1)]
    inspect_every: u64,
    /// Idle residency before a session actor discards its fold (the `serve` arm).
    #[arg(long, default_value_t = 3600)]
    idle_discard_s: u64,
    /// Density: create/turns/delete cycles. Cycle 1 yields density+reclaim; later cycles
    /// exist to separate a leak (grows every cycle) from allocator fragmentation (plateaus).
    #[arg(long, default_value_t = 3)]
    cycles: usize,
    /// Journal backend under test: memory (no durability; the harness floor),
    /// sqlite (WAL + synchronous=FULL, the local-mode store), or dynamo (the
    /// production store; latency only meaningful in-region with the table).
    #[arg(long, default_value = "memory")]
    journal: String,
    /// sqlite backend: database file path. Default: a fresh file per run in the
    /// system temp dir, so runs never measure a pre-grown database.
    #[arg(long)]
    sqlite_path: Option<String>,
    /// dynamo backend: table name (falls back to BRAIN_JOURNAL_TABLE).
    #[arg(long)]
    dynamo_table: Option<String>,
    /// Gate thresholds (used by `ci`; also enforced on other arms when set).
    #[arg(long)]
    gate_max_kib_per_session: Option<f64>,
    #[arg(long)]
    gate_min_reclaim_pct: Option<f64>,
    /// Density: max growth of the post-delete memory floor per cycle (MiB).
    #[arg(long)]
    gate_max_creep_mib: Option<f64>,
    #[arg(long)]
    gate_max_turn_p99_ms: Option<f64>,
    #[arg(long)]
    gate_max_ttft_p99_ms: Option<f64>,
    #[arg(long)]
    gate_min_turns_per_sec: Option<f64>,
}

const TOKEN: &str = "bench-token";

/// The client's view of a bench server (in-process or child).
#[derive(Clone)]
struct Api {
    base: String,
    http: reqwest::Client,
    /// Optional tenant header. BRAIN_BENCH_TENANTS=per-session gives each driven session
    /// its own tenant so the durable backend measures per-session commit latency instead
    /// of same-tenant meter contention (which is a separate, real ceiling).
    tenant: Option<String>,
}

/// An in-process bench server, with direct handles on the instruments.
struct Bench {
    api: Api,
    fake: Arc<FakeProvider>,
    executor: Arc<echo::EchoExecutor>,
}

struct BenchAgentloopRegistry;

impl AgentloopRegistry for BenchAgentloopRegistry {
    fn resolve(&self, _selector: &AgentloopSelectorDoc) -> brain::Result<Arc<dyn Agentloop>> {
        Ok(Arc::new(SequentialAgentloop))
    }

<<<<<<< HEAD
    fn admit(
        &self,
        component_digest: &str,
        world: &str,
        component: &[u8],
        config: &serde_json::Map<String, Value>,
    ) -> brain::Result<AgentloopSelectorDoc> {
        Ok(AgentloopSelectorDoc {
            component_digest: component_digest.into(),
            component_bytes: component.len() as u64,
            world: world.into(),
            config: config.clone(),
=======
    fn admit_custom(
        &self,
        source_bundle_sha256: &str,
        toolchain: &str,
        bundle: &[u8],
    ) -> brain::Result<AgentloopSelectorDoc> {
        Ok(AgentloopSelectorDoc {
            source_bundle_sha256: source_bundle_sha256.into(),
            source_bundle_bytes: bundle.len() as u64,
            toolchain: toolchain.into(),
>>>>>>> origin/main
        })
    }
}

/// Compose the brain + the bench sidecar routes and serve on a loopback port.
async fn serve(args: &Args, idle_discard: Duration) -> anyhow::Result<Bench> {
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.set_mode(FakeMode::Policy {
        tool_rounds: args.tool_rounds,
        parallel: args.parallel,
        tool: "bash".into(),
        text_bytes: args.text_bytes,
    });
    // Sampled inspection: full-parse instrumentation on every request would dominate an
    // unpaced measurement.
    fake.inspect_every
        .store(args.inspect_every.max(1), Ordering::Relaxed);
    fake.arrivals_cap.store(64, Ordering::Relaxed);
    let executor = Arc::new(echo::EchoExecutor::default());
    let mut cfg = BrainConfig {
        // Admission must not be what we measure: raise it above any K used here.
        max_concurrent_model_rounds: 4096,
        max_concurrent_turns: 4096,
        max_event_followers: 4096,
        // K>128 sweeps need every driven session resident at once; creates are
        // admission-limited to 4 by default and the sweep opens K sessions up front.
        max_resident_sessions: 4096,
        max_concurrent_creates: 256,
        // Density intentionally packs more simultaneously retained sessions than the hosted
        // tenant policy admits. Keep the per-session retention invariant, but exempt this
        // process-isolated measurement from the aggregate tenant product quota just as it is
        // exempt from production turn/follower admission above.
        journal_max_tenant_bytes: brain::journal::MAX_JOURNAL_BYTES,
        idle_discard,
        ..BrainConfig::default()
    };
    cfg.official_capabilities.insert(
        "brain.bench_echo".into(),
        brain::config::ServerToolPolicy {
            capability: "bench.echo".into(),
            scope: brain_protocol::session::ExternalToolScope::All,
            completion: brain_protocol::session::ExternalToolCompletion::Continue,
            effect: brain_protocol::session::ExternalToolEffect::ReplaySafe,
            max_input_bytes: brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES,
        },
    );
    let factory_fake = fake.clone();
    let journal = match args.journal.as_str() {
        "memory" => Journal::new_memory("bench"),
        "sqlite" => {
            let path = args.sqlite_path.clone().unwrap_or_else(|| {
                std::env::temp_dir()
                    .join(format!("brain-bench-{}.sqlite3", std::process::id()))
                    .to_string_lossy()
                    .into_owned()
            });
            let store = brain_standalone::SqliteStore::open(&path)
                .map_err(|e| anyhow::anyhow!("open sqlite journal {path}: {e}"))?;
            eprintln!("journal: sqlite at {path}");
            Journal::new(Arc::new(store), "bench")
        }
        "dynamo" => {
            let table = args
                .dynamo_table
                .clone()
                .or_else(|| std::env::var("BRAIN_JOURNAL_TABLE").ok())
                .ok_or_else(|| {
                    anyhow::anyhow!("--journal dynamo needs --dynamo-table or BRAIN_JOURNAL_TABLE")
                })?;
            let aws = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            let client = aws_sdk_dynamodb::Client::new(&aws);
            eprintln!(
                "journal: dynamo table {table} in {}",
                aws.region().map(|r| r.to_string()).unwrap_or_default()
            );
            Journal::new(
                Arc::new(brain_aws::dynamo::DynamoJournal::new(client, table)),
                "bench",
            )
        }
        // Local-first arms: memory-authoritative acks, async persistence measured by the
        // writebehind module (persistence lag, backlog, loss window).
        "writebehind-sqlite" => {
            let path = args.sqlite_path.clone().unwrap_or_else(|| {
                std::env::temp_dir()
                    .join(format!("brain-bench-wb-{}.sqlite3", std::process::id()))
                    .to_string_lossy()
                    .into_owned()
            });
            eprintln!("journal: write-behind -> sqlite at {path}");
            Journal::new(
                Arc::new(writebehind::WriteBehindStore::new(
                    writebehind::Sink::Sqlite(path),
                )),
                "bench",
            )
        }
        "writebehind-dynamo" => {
            let table = args
                .dynamo_table
                .clone()
                .or_else(|| std::env::var("BRAIN_JOURNAL_TABLE").ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "--journal writebehind-dynamo needs --dynamo-table or BRAIN_JOURNAL_TABLE"
                    )
                })?;
            eprintln!("journal: write-behind -> dynamo table {table}");
            Journal::new(
                Arc::new(writebehind::WriteBehindStore::new(
                    writebehind::Sink::Dynamo { table },
                )),
                "bench",
            )
        }
        other => anyhow::bail!(
            "unknown --journal {other} (memory|sqlite|dynamo|writebehind-sqlite|writebehind-dynamo)"
        ),
    };
    let brain = Brain::with_parts_and_services(
        cfg,
        journal,
        Arc::new(brain::keys::PlainCustody),
        executor.clone(),
        BrainServices {
            agentloop_registry: Some(Arc::new(BenchAgentloopRegistry)),
            ..BrainServices::default()
        },
        Arc::new(move |_| factory_fake.clone() as Arc<dyn Provider>),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let app = brain_server::api::router(brain_server::api::AppState {
        brain,
        token: TOKEN.into(),
        tenancy: brain_server::api::Tenancy::Implicit("local".into()),
    })
    .merge(bench_routes(fake.clone(), executor.clone()));
    tokio::spawn(async move {
        axum::serve(brain_server::api::nodelay(listener), app)
            .await
            .expect("bench server");
    });
    Ok(Bench {
        api: Api {
            base,
            http: reqwest::Client::new(),
            tenant: None,
        },
        fake,
        executor,
    })
}

/// The bench sidecar: /bench/trim (malloc_trim now) and /bench/guards (instrument counters
/// checked server-side, so a child process can be audited over the wire).
fn bench_routes(fake: Arc<FakeProvider>, executor: Arc<echo::EchoExecutor>) -> axum::Router {
    use axum::extract::Query;
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    #[derive(serde::Deserialize)]
    struct Expect {
        model: u64,
        tools: u64,
    }
    let (f, h) = (fake.clone(), executor.clone());
    axum::Router::new()
        .route(
            "/bench/trim",
            post(move || {
                let freed = brain::reclaim::arena_return();
                async move { format!("{freed}") }
            }),
        )
        .route(
            "/bench/guards",
            get(move |Query(exp): Query<Expect>| {
                let mismatches = f.policy_scan_mismatches.load(Ordering::SeqCst);
                let model = f.call_count.load(Ordering::SeqCst);
                let tools = h.calls.load(Ordering::SeqCst);
                async move {
                    if mismatches == 0 && model == exp.model && tools == exp.tools {
                        (StatusCode::NO_CONTENT, String::new())
                    } else {
                        (
                            StatusCode::CONFLICT,
                            format!(
                                "mismatches={mismatches} model={model} (want {}) tools={tools} (want {})",
                                exp.model, exp.tools
                            ),
                        )
                    }
                }
            }),
        )
}

impl Api {
    fn tenant_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(tenant) = &self.tenant {
            headers.insert("x-brain-tenant-id", tenant.parse().expect("tenant header"));
        }
        headers
    }

    async fn create_session(&self) -> anyhow::Result<String> {
        let r = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(TOKEN)
            .headers(self.tenant_headers())
            .json(&json!({
<<<<<<< HEAD
                "component_artifacts": [{
                    "component_digest": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
                    "component_base64": "eA==",
                    "bytes": 1
                }],
                "agentloop": {
                    "component_digest": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
                    "world": "aex:agentloop/agentloop@1.0.0"
=======
                "agentloop": {
                    "source_bundle_sha256": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
                    "toolchain": "brain-bench",
                    "bundle_base64": "eA=="
>>>>>>> origin/main
                },
                // Keep semantic compaction out of the turn-throughput instrument. Its own
                // correctness and wire-budget gates live in Brain's compaction test suite.
                "model": {
                    "component_digest": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
                    "world": "aex:model/model@1.0.0",
                    "provider": "anthropic",
                    "name": "bench",
                    "api_key": "sk-bench",
                    "context_window_tokens": brain_protocol::MAX_MODEL_CONTEXT_WINDOW_TOKENS
                },
                // Brain deliberately has no implicit tools. The benchmark's scripted provider
                // calls `bash`, so map that model-visible definition to the sealed benchmark
                // host capability. No legacy Environment adapter participates in this measurement.
                "tools": {"items": [{
                    "definition": {
                        "name": "bash",
                        "description": "Execute the benchmark echo tool.",
                        "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "input_schema": {"type": "object", "additionalProperties": true},
                        "output_schema": {"type": "object", "additionalProperties": true}
                    },
                    "executor": {"kind":"engine","capability":"brain.bench_echo"}
                }]}
            }))
            .send()
            .await?;
        anyhow::ensure!(r.status().as_u16() == 201, "create: {}", r.text().await?);
        let v: Value = r.json().await?;
        Ok(v["id"].as_str().unwrap_or_default().to_string())
    }

    async fn delete_session(&self, sid: &str) -> anyhow::Result<()> {
        let r = self
            .http
            .delete(format!("{}/v1/sessions/{sid}", self.base))
            .bearer_auth(TOKEN)
            .headers(self.tenant_headers())
            .send()
            .await?;
        if r.status().as_u16() == 204 {
            return Ok(());
        }
        anyhow::ensure!(r.status().as_u16() == 202, "delete {sid}: {}", r.status());
        let location = r
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("/v1/sessions/{sid}/deletion"));
        let url = if location.starts_with("http://") || location.starts_with("https://") {
            location
        } else {
            format!("{}{location}", self.base)
        };
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let status = self
                    .http
                    .get(&url)
                    .bearer_auth(TOKEN)
                    .headers(self.tenant_headers())
                    .send()
                    .await?;
                anyhow::ensure!(
                    status.status().is_success(),
                    "deletion status {sid}: {}",
                    status.status()
                );
                let body: Value = status.json().await?;
                match body["state"].as_str() {
                    Some("succeeded") => return Ok(()),
                    Some("failed") => anyhow::bail!("deletion failed for {sid}: {body}"),
                    _ => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("deletion stalled for {sid}"))?
    }

    /// Ask the server to malloc_trim, so a sample taken right after is post-trim.
    async fn trim(&self) -> anyhow::Result<()> {
        let r = self
            .http
            .post(format!("{}/bench/trim", self.base))
            .send()
            .await?;
        anyhow::ensure!(r.status().is_success(), "trim: {}", r.status());
        Ok(())
    }

    /// Server-side instrument audit; Err carries the server's account of the disagreement.
    async fn guards(&self, expect_model: u64, expect_tools: u64) -> anyhow::Result<()> {
        let r = self
            .http
            .get(format!(
                "{}/bench/guards?model={expect_model}&tools={expect_tools}",
                self.base
            ))
            .send()
            .await?;
        let status = r.status().as_u16();
        anyhow::ensure!(status == 204, "guards fired: {}", r.text().await?);
        Ok(())
    }
}

fn expected_counts(sessions: usize, turns: usize, args: &Args) -> (u64, u64) {
    (
        (sessions * turns * (args.tool_rounds as usize + 1)) as u64,
        (sessions * turns * args.tool_rounds as usize * args.parallel) as u64,
    )
}

/// A live SSE reader over one session's event stream.
struct EventStream {
    api: Api,
    sid: String,
    last_event_id: u64,
    stream: futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    buf: String,
    pending: std::collections::VecDeque<(Option<u64>, Value)>,
}

impl EventStream {
    async fn open(api: &Api, sid: &str) -> anyhow::Result<EventStream> {
        let stream = Self::connect(api, sid, 0).await?;
        Ok(EventStream {
            api: api.clone(),
            sid: sid.to_string(),
            last_event_id: 0,
            stream,
            buf: String::new(),
            pending: Default::default(),
        })
    }

    async fn connect(
        api: &Api,
        sid: &str,
        after: u64,
    ) -> anyhow::Result<futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>>
    {
        let resp = api
            .http
            .get(format!(
                "{}/v1/sessions/{sid}/events?after={after}&follow=true",
                api.base,
            ))
            .bearer_auth(TOKEN)
            .headers(api.tenant_headers())
            .send()
            .await?;
        anyhow::ensure!(resp.status().is_success(), "events: {}", resp.status());
        Ok(resp.bytes_stream().boxed())
    }

    async fn reconnect(&mut self) -> anyhow::Result<()> {
        self.stream = Self::connect(&self.api, &self.sid, self.last_event_id).await?;
        self.buf.clear();
        Ok(())
    }

    async fn next(&mut self) -> anyhow::Result<Value> {
        loop {
            if let Some((id, ev)) = self.pending.pop_front() {
                if let Some(id) = id {
                    self.last_event_id = self.last_event_id.max(id);
                }
                return Ok(ev);
            }
            let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), self.stream.next())
                .await
                .map_err(|_| anyhow::anyhow!("event stream stalled 30s"))?
            else {
                // The bounded live ring deliberately disconnects a lagging follower. Resume from
                // the last server-issued durable id; journal replay fills the exact gap.
                self.reconnect().await?;
                continue;
            };
            let chunk = chunk?;
            self.buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = self.buf.find("\n\n") {
                let frame: String = self.buf.drain(..pos + 2).collect();
                let mut id = None;
                let mut event = None;
                for line in frame.lines() {
                    if let Some(value) = line.strip_prefix("id:") {
                        id = value.trim().parse().ok();
                    }
                    if let Some(data) = line.strip_prefix("data:")
                        && let Ok(v) = serde_json::from_str::<Value>(data.trim_start())
                    {
                        event = Some(v);
                    }
                }
                if let Some(event) = event {
                    self.pending.push_back((id, event));
                }
            }
        }
    }
}

#[derive(Default, Clone)]
struct TurnSamples {
    turn_ms: Vec<f64>,
    admit_ms: Vec<f64>,
    ttft_ms: Vec<f64>,
}

/// Drive one session for `turns` sequential turns; every latency is measured at the client
/// through real HTTP+SSE.
async fn drive_session(
    api: &Api,
    sid: &str,
    turns: usize,
    require_ttft: bool,
) -> anyhow::Result<TurnSamples> {
    let mut es = EventStream::open(api, sid).await?;
    let mut out = TurnSamples::default();
    for _ in 0..turns {
        let t0 = Instant::now();
        let r = api
            .http
            .post(format!("{}/v1/sessions/{sid}/messages", api.base))
            .bearer_auth(TOKEN)
            .headers(api.tenant_headers())
            .json(&json!({"content": "go"}))
            .send()
            .await?;
        anyhow::ensure!(r.status().as_u16() == 202, "message: {}", r.text().await?);
        let mut admit = None;
        let mut ttft = None;
        loop {
            let ev = es.next().await?;
            match ev["type"].as_str().unwrap_or("") {
                "turn.started" => {
                    admit.get_or_insert(t0.elapsed());
                }
                "assistant.delta" => {
                    ttft.get_or_insert(t0.elapsed());
                }
                "turn.completed" => {
                    anyhow::ensure!(
                        !require_ttft || ttft.is_some(),
                        "pure-text benchmark lost its first live delta"
                    );
                    break;
                }
                "turn.failed" => anyhow::bail!("turn failed: {ev}"),
                _ => {}
            }
        }
        out.turn_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        if let Some(a) = admit {
            out.admit_ms.push(a.as_secs_f64() * 1000.0);
        }
        if let Some(t) = ttft {
            out.ttft_ms.push(t.as_secs_f64() * 1000.0);
        }
    }
    Ok(out)
}

/// Create K sessions and drive them all concurrently for M turns each.
async fn drive_all(
    api: &Api,
    k: usize,
    turns: usize,
    require_ttft: bool,
) -> anyhow::Result<(TurnSamples, f64, Vec<String>)> {
    // BRAIN_BENCH_TENANTS=per-session isolates each session under its own tenant, so a
    // durable backend's shared tenant-meter items measure per-session latency rather than
    // same-tenant transaction contention.
    let per_session_tenants = std::env::var("BRAIN_BENCH_TENANTS")
        .map(|v| v == "per-session")
        .unwrap_or(false);
    let mut apis = Vec::with_capacity(k);
    let mut sids = Vec::with_capacity(k);
    for i in 0..k {
        let mut session_api = api.clone();
        if per_session_tenants {
            session_api.tenant = Some(format!("bench-t{i}"));
        }
        sids.push(session_api.create_session().await?);
        apis.push(session_api);
    }
    let wall = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for (sid, session_api) in sids.iter().zip(apis) {
        let sid = sid.clone();
        set.spawn(async move { drive_session(&session_api, &sid, turns, require_ttft).await });
    }
    let mut all = TurnSamples::default();
    while let Some(r) = set.join_next().await {
        let s = r??;
        all.turn_ms.extend(s.turn_ms);
        all.admit_ms.extend(s.admit_ms);
        all.ttft_ms.extend(s.ttft_ms);
    }
    Ok((all, wall.elapsed().as_secs_f64(), sids))
}

struct GateBook {
    failures: Vec<String>,
}

impl GateBook {
    fn new() -> Self {
        GateBook { failures: vec![] }
    }
    // NaN must FAIL a gate, never slip past it — hence the explicit is_nan arms.
    fn max(&mut self, name: &str, value: f64, limit: Option<f64>) {
        if let Some(l) = limit
            && (value.is_nan() || value > l)
        {
            self.failures.push(format!("{name} = {value:.2} > {l}"));
        }
    }
    fn min(&mut self, name: &str, value: f64, limit: Option<f64>) {
        if let Some(l) = limit
            && (value.is_nan() || value < l)
        {
            self.failures.push(format!("{name} = {value:.2} < {l}"));
        }
    }
}

fn fmt_summary(name: &str, s: &stats::Summary) -> String {
    format!(
        "{name}: n={} p50={:.2} p90={:.2} p99={} max={:.2} (ms)",
        s.n, s.p50, s.p90, s.p99, s.max
    )
}

async fn arm_turns(args: &Args, book: &mut GateBook) -> anyhow::Result<()> {
    let b = serve(args, Duration::from_secs(3600)).await?;
    let k = args.sessions;
    let (all, wall_s, _sids) = drive_all(&b.api, k, args.turns, args.tool_rounds == 0).await?;
    let (em, et) = expected_counts(k, args.turns, args);
    let mismatches = b.fake.policy_scan_mismatches.load(Ordering::SeqCst);
    anyhow::ensure!(mismatches == 0, "fake scan/parse mismatches: {mismatches}");
    let (gm, gt) = (
        b.fake.call_count.load(Ordering::SeqCst),
        b.executor.calls.load(Ordering::SeqCst),
    );
    // A durable backend can legitimately add a kernel-side provider retry (one extra
    // model call), which the strict count guard treats as corruption. BRAIN_BENCH_LAX=1
    // downgrades count drift to a warning so backend-latency spikes keep their samples;
    // the CI composition (memory journal) stays strict.
    let lax = std::env::var("BRAIN_BENCH_LAX")
        .map(|v| v == "1")
        .unwrap_or(false);
    if lax {
        if gm != em {
            eprintln!("WARN model calls: expected {em}, got {gm}");
        }
        if gt != et {
            eprintln!("WARN tool calls: expected {et}, got {gt}");
        }
    } else {
        anyhow::ensure!(gm == em, "model calls: expected {em}, got {gm}");
        anyhow::ensure!(gt == et, "tool calls: expected {et}, got {gt}");
    }

    let turns_per_sec = (k * args.turns) as f64 / wall_s;
    let turn = stats::summarize(&all.turn_ms);
    let admit = stats::summarize(&all.admit_ms);
    println!(
        "turns: K={k} M={} R={} P={} B={} unpaced -> {:.1} turns/s (wall {:.2}s)",
        args.turns, args.tool_rounds, args.parallel, args.text_bytes, turns_per_sec, wall_s
    );
    println!("  {}", fmt_summary("turn_latency", &turn));
    println!("  {}", fmt_summary("admit(POST->turn.started)", &admit));
    if let Some(line) = writebehind::report_after_drain().await {
        println!("  {line}");
    }
    if args.tool_rounds == 0 {
        let ttft = stats::summarize(&all.ttft_ms);
        println!("  {}", fmt_summary("ttft_added(POST->first delta)", &ttft));
        book.max(
            "ttft_p99_ms",
            stats::p99_or_max(&ttft),
            args.gate_max_ttft_p99_ms,
        );
    }
    book.max(
        "turn_p99_ms",
        stats::p99_or_max(&turn),
        args.gate_max_turn_p99_ms,
    );
    book.min("turns_per_sec", turns_per_sec, args.gate_min_turns_per_sec);
    Ok(())
}

/// The server child for `density`: parent samples /proc/<pid> while this process holds ONLY
/// the brain (and its instruments). Parked until killed.
async fn arm_serve(args: &Args) -> anyhow::Result<()> {
    let b = serve(args, Duration::from_secs(args.idle_discard_s)).await?;
    println!("READY {} {}", b.api.base, std::process::id());
    // An unflushed READY line deadlocks the parent.
    use std::io::Write as _;
    std::io::stdout().flush().ok();
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

async fn arm_density(args: &Args, book: &mut GateBook) -> anyhow::Result<()> {
    let idle_s = 3u64;
    // The brain under measurement lives in its own process.
    let exe = std::env::current_exe()?;
    let mut child = tokio::process::Command::new(exe)
        .args([
            "serve",
            "--tool-rounds",
            &args.tool_rounds.to_string(),
            "--parallel",
            &args.parallel.to_string(),
            "--text-bytes",
            &args.text_bytes.to_string(),
            "--inspect-every",
            &args.inspect_every.to_string(),
            "--idle-discard-s",
            &idle_s.to_string(),
        ])
        // The reclaim floor is allocator-dependent: cap the SERVER's arenas so the number is
        // deterministic (and matches the deploy recommendation). Only the child — fewer
        // arenas cost hot-path throughput, which the latency arms must not pay.
        .env("MALLOC_ARENA_MAX", "2")
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut ready = String::new();
    use tokio::io::AsyncBufReadExt as _;
    tokio::time::timeout(Duration::from_secs(30), reader.read_line(&mut ready))
        .await
        .map_err(|_| anyhow::anyhow!("server child never became ready"))??;
    let mut parts = ready.trim().split(' ');
    anyhow::ensure!(parts.next() == Some("READY"), "child said: {ready}");
    let base = parts.next().unwrap_or_default().to_string();
    let pid: u32 = parts.next().unwrap_or_default().parse()?;
    let api = Api {
        base,
        http: reqwest::Client::new(),
        tenant: None,
    };
    let sample = |api: Api| async move {
        api.trim().await?;
        mem::sample_pid(pid).map_err(anyhow::Error::msg)
    };

    let n = args.sessions;
    let m_base = sample(api.clone()).await?;
    let (_all, _wall, sids) = drive_all(&api, n, args.turns, args.tool_rounds == 0).await?;
    let (em, et) = expected_counts(n, args.turns, args);
    api.guards(em, et).await?;

    // Resident: every actor alive, fold cached. Sample immediately (discard is 3s away).
    let m_resident = sample(api.clone()).await?;

    // Discarded: actors exited, journal still holds every record (as DynamoDB would,
    // off-process, in production). The difference is what a resident session actually costs.
    tokio::time::sleep(Duration::from_secs(idle_s + 4)).await;
    let m_discarded = sample(api.clone()).await?;

    // Delete everything; what the process keeps above baseline is the reclaim gate.
    for sid in &sids {
        api.delete_session(sid).await?;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let m_final = sample(api.clone()).await?;

    let per = |a: u64, z: u64| (a.saturating_sub(z)) as f64 / n as f64 / 1024.0;
    let kib_private = per(m_resident.private_bytes(), m_discarded.private_bytes());
    let kib_pss = per(m_resident.pss_bytes, m_discarded.pss_bytes);
    let kib_with_journal = per(m_resident.private_bytes(), m_base.private_bytes());
    let grown = m_resident
        .private_bytes()
        .saturating_sub(m_base.private_bytes());
    let returned = m_resident
        .private_bytes()
        .saturating_sub(m_final.private_bytes());
    let reclaim_pct = if grown == 0 {
        100.0
    } else {
        returned as f64 / grown as f64 * 100.0
    };
    println!(
        "density: N={n} T={} R={} P={} B={} -> {kib_private:.1} KiB/resident session private \
         ({kib_pss:.1} pss; {kib_with_journal:.1} incl. in-memory journal)",
        args.turns, args.tool_rounds, args.parallel, args.text_bytes
    );
    println!(
        "  server private MiB: base {:.1} -> resident {:.1} -> discarded {:.1} -> deleted {:.1}",
        m_base.private_bytes() as f64 / 1048576.0,
        m_resident.private_bytes() as f64 / 1048576.0,
        m_discarded.private_bytes() as f64 / 1048576.0,
        m_final.private_bytes() as f64 / 1048576.0,
    );
    println!(
        "  reclaim: {reclaim_pct:.1}% of grown private returned after delete-all + trim \
         (mechanism: {})",
        brain::reclaim::MECHANISM
    );
    book.max(
        "kib_per_resident_session",
        kib_private,
        args.gate_max_kib_per_session,
    );
    book.min("reclaim_pct", reclaim_pct, args.gate_min_reclaim_pct);

    // Steady state: repeat the whole create/turns/delete cycle. A leak grows every cycle;
    // glibc fragmentation plateaus and the pages get REUSED by the next cycle. The gate is
    // that the post-delete floor stops rising.
    let mut finals = vec![m_final.private_bytes()];
    for cycle in 2..=args.cycles.max(1) {
        let (_a, _w, sids) = drive_all(&api, n, args.turns, args.tool_rounds == 0).await?;
        api.guards(em * cycle as u64, et * cycle as u64).await?;
        for sid in &sids {
            api.delete_session(sid).await?;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let m = sample(api.clone()).await?;
        finals.push(m.private_bytes());
    }
    child.kill().await.ok();
    if finals.len() > 1 {
        // Cycle 1 -> 2 includes one-time warmup (arena growth, runtime slabs); the creep that
        // matters is the tail. A leak climbs every cycle; a plateau wobbles or declines.
        let from = if finals.len() >= 3 { 1 } else { 0 };
        let creep_mib = (*finals.last().expect("nonempty") as f64 - finals[from] as f64)
            / 1048576.0
            / (finals.len() - 1 - from) as f64;
        println!(
            "  steady state over {} cycles: post-delete floor {:?} MiB -> creep {:+.1} MiB/cycle",
            finals.len(),
            finals
                .iter()
                .map(|b| (*b as f64 / 1048576.0 * 10.0).round() / 10.0)
                .collect::<Vec<_>>(),
            creep_mib,
        );
        book.max(
            "creep_mib_per_cycle",
            creep_mib.max(0.0),
            args.gate_max_creep_mib,
        );
    }
    Ok(())
}

async fn arm_ci(args: &Args) -> anyhow::Result<()> {
    let mut book = GateBook::new();
    // 1. Pure text turns, K=1: the platform-added TTFT and baseline turn latency.
    let mut a = args.clone();
    a.sessions = 1;
    a.turns = 100;
    a.tool_rounds = 0;
    a.text_bytes = 512;
    a.gate_min_turns_per_sec = None; // K=1 throughput is not the throughput gate
    arm_turns(&a, &mut book).await?;
    // 2. Pure text turns, K=16: concurrency must not melt the tail.
    let mut a = args.clone();
    a.sessions = 16;
    a.turns = 25;
    a.tool_rounds = 0;
    a.text_bytes = 512;
    arm_turns(&a, &mut book).await?;
    // 3. The tool loop at concurrency: 2 rounds x 4 parallel calls per turn.
    let mut a = args.clone();
    a.sessions = 16;
    a.turns = 10;
    a.tool_rounds = 2;
    a.parallel = 4;
    a.text_bytes = 2048;
    a.gate_max_ttft_p99_ms = None;
    arm_turns(&a, &mut book).await?;
    // 4. Density + reclaim (Linux only; the memory gates are meaningless elsewhere).
    if cfg!(target_os = "linux") {
        let mut a = args.clone();
        a.sessions = 128;
        a.turns = 2;
        a.tool_rounds = 1;
        a.parallel = 2;
        a.text_bytes = 4096;
        arm_density(&a, &mut book).await?;
    } else {
        println!("density: SKIPPED (not linux; smaps_rollup unavailable — run on the target)");
    }
    if book.failures.is_empty() {
        println!("CI GATES PASS");
        Ok(())
    } else {
        for f in &book.failures {
            eprintln!("GATE FAILED: {f}");
        }
        anyhow::bail!("{} gate(s) failed", book.failures.len());
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let mut book = GateBook::new();
        match args.arm.as_str() {
            "turns" => arm_turns(&args, &mut book).await?,
            "density" => arm_density(&args, &mut book).await?,
            "serve" => return arm_serve(&args).await,
            "ci" => return arm_ci(&args).await,
            other => anyhow::bail!("unknown arm {other}: use density | turns | ci"),
        }
        if book.failures.is_empty() {
            Ok(())
        } else {
            for f in &book.failures {
                eprintln!("GATE FAILED: {f}");
            }
            anyhow::bail!("{} gate(s) failed", book.failures.len());
        }
    })
}
