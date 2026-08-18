//! Benchmark gates for the brain (slice 5). Every number measures the PLATFORM, never a model:
//! the provider is the scripted fake (instant unless paced), the hand is an in-process echo,
//! the journal is in-memory — and the drive path is the real public HTTP API with SSE, because
//! that is what production serves.
//!
//! Arms:
//!   density  N resident sessions -> KiB per resident session (journal-neutral: the resident
//!            sample minus the post-discard sample, because an idle session is nothing but its
//!            journal and the production journal lives in DynamoDB, not this process), then
//!            delete-all -> memory returned (PD-13 gate: >=97% with explicit malloc_trim).
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

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::Provider;
use brain::provider::fake::{FakeMode, FakeProvider};
use brain::session::{Brain, BrainConfig};
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
    /// Gate thresholds (used by `ci`; also enforced on other arms when set).
    #[arg(long)]
    gate_max_kib_per_session: Option<f64>,
    #[arg(long)]
    gate_min_reclaim_pct: Option<f64>,
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
}

/// An in-process bench server, with direct handles on the instruments.
struct Bench {
    api: Api,
    fake: Arc<FakeProvider>,
    hand: Arc<echo::EchoHand>,
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
    // unpaced measurement (the PD-11 lesson, kept).
    fake.inspect_every
        .store(args.inspect_every.max(1), Ordering::Relaxed);
    fake.arrivals_cap.store(64, Ordering::Relaxed);
    let hand = Arc::new(echo::EchoHand::default());
    let cfg = BrainConfig {
        // Admission must not be what we measure: raise it above any K used here.
        max_concurrent_model_rounds: 4096,
        max_concurrent_turns: 4096,
        idle_discard,
        ..BrainConfig::default()
    };
    let factory_fake = fake.clone();
    let brain = Brain::with_parts(
        cfg,
        Journal::new_memory("bench"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(echo::EchoFactory { hand: hand.clone() }),
        Some(Arc::new(move |_| factory_fake.clone() as Arc<dyn Provider>)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: TOKEN.into(),
    })
    .merge(bench_routes(fake.clone(), hand.clone()));
    tokio::spawn(async move {
        axum::serve(brain::api::nodelay(listener), app)
            .await
            .expect("bench server");
    });
    Ok(Bench {
        api: Api {
            base,
            http: reqwest::Client::new(),
        },
        fake,
        hand,
    })
}

/// The bench sidecar: /bench/trim (malloc_trim now) and /bench/guards (instrument counters
/// checked server-side, so a child process can be audited over the wire).
fn bench_routes(fake: Arc<FakeProvider>, hand: Arc<echo::EchoHand>) -> axum::Router {
    use axum::extract::Query;
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    #[derive(serde::Deserialize)]
    struct Expect {
        model: u64,
        tools: u64,
    }
    let (f, h) = (fake.clone(), hand.clone());
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
    async fn create_session(&self) -> anyhow::Result<String> {
        let r = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(TOKEN)
            .json(&json!({
                "model": {"provider": "anthropic", "name": "bench", "api_key": "sk-bench"},
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
            .send()
            .await?;
        anyhow::ensure!(r.status().as_u16() == 204, "delete {sid}: {}", r.status());
        Ok(())
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
    stream: futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    buf: String,
    pending: std::collections::VecDeque<Value>,
}

impl EventStream {
    async fn open(api: &Api, sid: &str) -> anyhow::Result<EventStream> {
        let resp = api
            .http
            .get(format!(
                "{}/v1/sessions/{sid}/events?after=0&follow=true",
                api.base
            ))
            .bearer_auth(TOKEN)
            .send()
            .await?;
        anyhow::ensure!(resp.status().is_success(), "events: {}", resp.status());
        Ok(EventStream {
            stream: resp.bytes_stream().boxed(),
            buf: String::new(),
            pending: Default::default(),
        })
    }

    async fn next(&mut self) -> anyhow::Result<Value> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(ev);
            }
            let chunk = tokio::time::timeout(Duration::from_secs(30), self.stream.next())
                .await
                .map_err(|_| anyhow::anyhow!("event stream stalled 30s"))?
                .ok_or_else(|| anyhow::anyhow!("event stream closed"))??;
            self.buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = self.buf.find("\n\n") {
                let frame: String = self.buf.drain(..pos + 2).collect();
                for line in frame.lines() {
                    if let Some(data) = line.strip_prefix("data:")
                        && let Ok(v) = serde_json::from_str::<Value>(data.trim_start())
                    {
                        self.pending.push_back(v);
                    }
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
async fn drive_session(api: &Api, sid: &str, turns: usize) -> anyhow::Result<TurnSamples> {
    let mut es = EventStream::open(api, sid).await?;
    let mut out = TurnSamples::default();
    for _ in 0..turns {
        let t0 = Instant::now();
        let r = api
            .http
            .post(format!("{}/v1/sessions/{sid}/messages", api.base))
            .bearer_auth(TOKEN)
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
                "turn.completed" => break,
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
) -> anyhow::Result<(TurnSamples, f64, Vec<String>)> {
    let mut sids = Vec::with_capacity(k);
    for _ in 0..k {
        sids.push(api.create_session().await?);
    }
    let wall = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for sid in &sids {
        let api = api.clone();
        let sid = sid.clone();
        set.spawn(async move { drive_session(&api, &sid, turns).await });
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
    let (all, wall_s, _sids) = drive_all(&b.api, k, args.turns).await?;
    let (em, et) = expected_counts(k, args.turns, args);
    let mismatches = b.fake.policy_scan_mismatches.load(Ordering::SeqCst);
    anyhow::ensure!(mismatches == 0, "fake scan/parse mismatches: {mismatches}");
    let (gm, gt) = (
        b.fake.call_count.load(Ordering::SeqCst),
        b.hand.calls.load(Ordering::SeqCst),
    );
    anyhow::ensure!(gm == em, "model calls: expected {em}, got {gm}");
    anyhow::ensure!(gt == et, "tool calls: expected {et}, got {gt}");

    let turns_per_sec = (k * args.turns) as f64 / wall_s;
    let turn = stats::summarize(&all.turn_ms);
    let admit = stats::summarize(&all.admit_ms);
    println!(
        "turns: K={k} M={} R={} P={} B={} unpaced -> {:.1} turns/s (wall {:.2}s)",
        args.turns, args.tool_rounds, args.parallel, args.text_bytes, turns_per_sec, wall_s
    );
    println!("  {}", fmt_summary("turn_latency", &turn));
    println!("  {}", fmt_summary("admit(POST->turn.started)", &admit));
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
    };
    let sample = |api: Api| async move {
        api.trim().await?;
        mem::sample_pid(pid).map_err(anyhow::Error::msg)
    };

    let n = args.sessions;
    let m_base = sample(api.clone()).await?;
    let (_all, _wall, sids) = drive_all(&api, n, args.turns).await?;
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
    child.kill().await.ok();

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
