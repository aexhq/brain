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
//!            Linux-only by construction (smaps_rollup; run on the production target).
//!   turns    K concurrent sessions x M turns, unpaced -> turns/s, per-turn latency, admission
//!            latency (POST -> turn.started), and with --tool-rounds 0 the platform-added TTFT
//!            (POST -> first assistant.delta byte at the client; the provider is instant, so
//!            everything measured is us).
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
    /// density | turns | ci
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
    /// Gate thresholds (used by `ci`; also enforced on other arms when set).
    /// Fully inspect 1 request in N (scan/parse cross-check + arrival record).
    #[arg(long, default_value_t = 1)]
    inspect_every: u64,
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

/// One composed brain served over loopback HTTP, plus handles on the instruments.
struct Bench {
    base: String,
    token: String,
    http: reqwest::Client,
    fake: Arc<FakeProvider>,
    hand: Arc<echo::EchoHand>,
}

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
    let token = "bench-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move {
        axum::serve(brain::api::nodelay(listener), app)
            .await
            .expect("bench server");
    });
    Ok(Bench {
        base,
        token,
        http: reqwest::Client::new(),
        fake,
        hand,
    })
}

impl Bench {
    async fn create_session(&self) -> anyhow::Result<String> {
        let r = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(&self.token)
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
            .bearer_auth(&self.token)
            .send()
            .await?;
        anyhow::ensure!(r.status().as_u16() == 204, "delete {sid}: {}", r.status());
        Ok(())
    }
}

/// A live SSE reader over one session's event stream.
struct EventStream {
    stream: futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    buf: String,
    pending: std::collections::VecDeque<Value>,
}

impl EventStream {
    async fn open(b: &Bench, sid: &str) -> anyhow::Result<EventStream> {
        let resp = b
            .http
            .get(format!(
                "{}/v1/sessions/{sid}/events?after=0&follow=true",
                b.base
            ))
            .bearer_auth(&b.token)
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
async fn drive_session(b: &Bench, sid: &str, turns: usize) -> anyhow::Result<TurnSamples> {
    let mut es = EventStream::open(b, sid).await?;
    let mut out = TurnSamples::default();
    for _ in 0..turns {
        let t0 = Instant::now();
        let r = b
            .http
            .post(format!("{}/v1/sessions/{sid}/messages", b.base))
            .bearer_auth(&b.token)
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

/// The instrument guards: a fake that mis-served or a loop that mis-counted invalidates every
/// number, so the run refuses instead of reporting.
fn guards(b: &Bench, sessions: usize, turns: usize, args: &Args) -> anyhow::Result<()> {
    let mismatches = b.fake.policy_scan_mismatches.load(Ordering::SeqCst);
    anyhow::ensure!(mismatches == 0, "fake scan/parse mismatches: {mismatches}");
    let expect_model = (sessions * turns * (args.tool_rounds as usize + 1)) as u64;
    let got_model = b.fake.call_count.load(Ordering::SeqCst);
    anyhow::ensure!(
        got_model == expect_model,
        "model calls: expected {expect_model}, got {got_model}"
    );
    let expect_tools = (sessions * turns * args.tool_rounds as usize * args.parallel) as u64;
    let got_tools = b.hand.calls.load(Ordering::SeqCst);
    anyhow::ensure!(
        got_tools == expect_tools,
        "tool calls: expected {expect_tools}, got {got_tools}"
    );
    Ok(())
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
    let mut sids = Vec::with_capacity(k);
    for _ in 0..k {
        sids.push(b.create_session().await?);
    }
    let wall = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    let b = Arc::new(b);
    for sid in &sids {
        let b = b.clone();
        let sid = sid.clone();
        let turns = args.turns;
        set.spawn(async move { drive_session(&b, &sid, turns).await });
    }
    let mut all = TurnSamples::default();
    while let Some(r) = set.join_next().await {
        let s = r??;
        all.turn_ms.extend(s.turn_ms);
        all.admit_ms.extend(s.admit_ms);
        all.ttft_ms.extend(s.ttft_ms);
    }
    let wall_s = wall.elapsed().as_secs_f64();
    guards(&b, k, args.turns, args)?;

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

async fn arm_density(args: &Args, book: &mut GateBook) -> anyhow::Result<()> {
    let idle = Duration::from_secs(3);
    let b = serve(args, idle).await?;
    let n = args.sessions;

    brain::reclaim::arena_return();
    let m_base = mem::sample().map_err(anyhow::Error::msg)?;

    let mut sids = Vec::with_capacity(n);
    for _ in 0..n {
        sids.push(b.create_session().await?);
    }
    let b = Arc::new(b);
    let mut set = tokio::task::JoinSet::new();
    for sid in &sids {
        let b = b.clone();
        let sid = sid.clone();
        let turns = args.turns;
        set.spawn(async move { drive_session(&b, &sid, turns).await });
    }
    while let Some(r) = set.join_next().await {
        r??;
    }
    guards(&b, n, args.turns, args)?;

    // Resident: every actor alive, fold cached. Sample immediately (discard is 3s away).
    brain::reclaim::arena_return();
    let m_resident = mem::sample().map_err(anyhow::Error::msg)?;

    // Discarded: actors exited, journal still holds every record (as DynamoDB would,
    // off-process, in production). The difference is what a resident session actually costs.
    tokio::time::sleep(idle + Duration::from_secs(4)).await;
    brain::reclaim::arena_return();
    let m_discarded = mem::sample().map_err(anyhow::Error::msg)?;

    for sid in &sids {
        b.delete_session(sid).await?;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    brain::reclaim::arena_return();
    let m_final = mem::sample().map_err(anyhow::Error::msg)?;

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
        "  private MiB: base {:.1} -> resident {:.1} -> discarded {:.1} -> deleted {:.1}",
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
