//! Agno AgentOS, driven through its own HTTP API.
//!
//! AgentOS's unit of work is a *session* on an agent that has lived since boot: the agent
//! is defined in the server script, and every turn names a `session_id` that Agno creates
//! lazily on first use. So `create` is the explicit form of that same row insert
//! (`POST /sessions`), and a turn is `POST /agents/{id}/runs` — form-encoded, because
//! that is the only body the endpoint takes, and with `stream=false` stated explicitly,
//! because streaming is Agno's default.
//!
//! **Pointing it at the scripted provider.** The server script builds its one agent with
//! `OpenAILike(base_url=BENCH_MODEL_BASE_URL)` from the launch environment, so the wiring
//! is done before this driver says anything. `prepare` still drives one untimed warmup
//! turn: it forces the lazy SQLite store into existence and proves a turn produces
//! content before anything is timed, and the runner's fixture-count cross-check catches a
//! server that answered from anywhere but the scripted provider.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;

use crate::driver::{Driver, Unit};

/// The agent id `server.py` fixes, so the run URL is knowable without discovery.
const AGENT_ID: &str = "bench-agent";

pub struct AgnoAgentOsDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

impl AgnoAgentOsDriver {
    /// `_pid` is deliberately unused: the runner started the uvicorn process itself, so
    /// it samples that tree without the driver's help.
    pub fn new(base_url: impl Into<String>, _pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                // Matched to Brain's, so neither subject's number is its client's pool.
                .pool_max_idle_per_host(512)
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            turns_requested: AtomicU64::new(0),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn requesting_a_turn(&self) {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
    }

    /// The form every timed turn submits, encoded by hand because this reqwest build
    /// carries no urlencoding feature and every value here is plain ASCII. Fixed,
    /// because input length is an input to turn cost and has to be identical across
    /// subjects. `stream` is stated even when false: Agno streams by default, and a
    /// probe that forgot to say so would time the wrong path.
    fn run_form(unit: &Unit, stream: bool) -> String {
        format!(
            "message=benchmark&stream={stream}&session_id={}&user_id=bench",
            unit.id
        )
    }

    async fn blocking_turn(&self, unit: &Unit) -> Result<Value> {
        let response = self
            .client
            .post(self.url(&format!("/agents/{AGENT_ID}/runs")))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Self::run_form(unit, false))
            .send()
            .await?;
        let body: Value = ok(response, "running a blocking turn").await?.json().await?;

        // A turn that produced no assistant content did not happen, however the HTTP
        // status reads. Agno reports the run's own status beside the content, so both
        // are held: content because it is the output, status because RunError arrives
        // with 200 on this endpoint's blocking form as well.
        let content = body.get("content").and_then(Value::as_str).unwrap_or("");
        anyhow::ensure!(
            !content.is_empty(),
            "the turn produced no assistant content: {body}"
        );
        if let Some(status) = body.get("status").and_then(Value::as_str) {
            anyhow::ensure!(
                status.eq_ignore_ascii_case("completed"),
                "the turn did not complete: status {status}: {body}"
            );
        }
        Ok(body)
    }
}

#[async_trait]
impl Driver for AgnoAgentOsDriver {
    /// One untimed warmup turn on a scratch session: the SQLite store and the model path
    /// exist before anything is measured, exactly as they would for a server that had
    /// answered one request in its life.
    async fn prepare(&mut self) -> Result<()> {
        let scratch = Unit {
            id: format!("bench-warmup-{}", uuid_like()),
        };
        self.blocking_turn(&scratch).await?;
        eprintln!("agno-agentos: warmup turn completed against the scripted provider");
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let session_id = format!("bench-{}", uuid_like());
        let response = self
            .client
            .post(self.url("/sessions?session_type=agent"))
            .json(&serde_json::json!({
                "session_id": session_id,
                "agent_id": AGENT_ID,
                "user_id": "bench",
            }))
            .send()
            .await?;
        let session: Value = ok(response, "creating a session").await?.json().await?;
        Ok(Unit {
            id: session
                .get("session_id")
                .and_then(Value::as_str)
                .context("session response carried no session_id")?
                .to_owned(),
        })
    }

    /// Milliseconds until the first `RunContent` event on the run's own SSE stream.
    ///
    /// Like LangGraph Server's and Letta's, and unlike Brain's, the stream is opened *by*
    /// submitting the turn — AgentOS has no separate subscribe — so the subscribe cost is
    /// inside this number by construction, and the manifest says so.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .client
            .post(self.url(&format!("/agents/{AGENT_ID}/runs")))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Self::run_form(unit, true))
            .send()
            .await?;
        let response = ok(response, "opening a streaming turn").await?;

        let mut frames = response.bytes_stream();
        let mut pending = String::new();
        while let Some(chunk) = frames.next().await {
            pending.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(newline) = pending.find('\n') {
                let line: String = pending.drain(..=newline).collect();
                let Some(payload) = line.trim().strip_prefix("data: ") else {
                    continue;
                };
                let frame: Value = serde_json::from_str(payload)
                    .with_context(|| format!("reading a stream frame: {payload}"))?;
                match frame.get("event").and_then(Value::as_str) {
                    // Output. The first delta is the measurement.
                    Some("RunContent") => return Ok(started.elapsed().as_secs_f64() * 1_000.0),
                    Some("RunError") | Some("RunCancelled") => {
                        anyhow::bail!("the turn failed: {frame}")
                    }
                    // Terminal frames before any content mean the turn finished without
                    // the assistant ever saying anything, which is not a first-byte
                    // measurement however fast it arrived.
                    Some("RunCompleted") => anyhow::bail!(
                        "the stream reached its terminal frame with no assistant output: {frame}"
                    ),
                    _ => continue,
                }
            }
        }
        anyhow::bail!("the turn stream ended before any assistant output")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        self.blocking_turn(unit).await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(self.url(&format!("/sessions/{}", unit.id)))
            .send()
            .await?;
        ok(response, "deleting a session").await?;
        Ok(())
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

/// `error_for_status` throws the body away, and the body is where the subject says what
/// went wrong.
async fn ok(response: reqwest::Response, doing: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("<body unreadable: {error}>"));
    anyhow::bail!("{doing}: {status}: {body}")
}

/// Distinct session names without pulling in a uuid crate for a few probes' worth.
fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}
