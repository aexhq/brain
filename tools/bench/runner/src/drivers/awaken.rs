//! Awaken, driven through its protocol-neutral runs API.
//!
//! Awaken's unit of work is a *thread*, and its runs API is SSE-first: submitting a run
//! *is* opening its stream, and there is no blocking completion form — so the
//! round-trip probe reads its own stream to the `run_finish` event, and the durable
//! event journal every frame lands in is part of what the number measures.
//!
//! **Pointing it at the scripted provider.** The launch environment does the wiring
//! (OPENAI_BASE_URL with its load-bearing trailing slash, and a dummy OPENAI_API_KEY so
//! the starter cannot silently fall back to its built-in scripted executor); the
//! runner's fixture-count cross-check catches a server that answered from anywhere but
//! the scripted provider.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

/// The agent id the launch environment seeds.
const AGENT_ID: &str = "default";

pub struct AwakenDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

enum TurnEnd {
    FirstDelta,
    Finished,
}

impl AwakenDriver {
    /// `_pid` is deliberately unused: the runner started the server itself, so it
    /// samples that tree without the driver's help.
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

    /// Drives one run's stream until `until`, returning the elapsed milliseconds.
    ///
    /// The stream interleaves named SSE events with data payloads, and some frames name
    /// themselves in the payload instead — so both the last `event:` line and the
    /// payload's own `event`/`type` field are consulted, whichever the server used.
    async fn drive(&self, unit: &Unit, until: TurnEnd) -> Result<f64> {
        self.requesting_a_turn();
        // Awaken finalizes a run asynchronously after its stream's run_finish, and a
        // thread with a run still finalizing answers the next submission with a 409.
        // That window is waited out untimed — the timer restarts on every attempt — so
        // the number is the accepted run's latency, not the previous run's cleanup.
        let waiting_since = Instant::now();
        let mut started;
        let response = loop {
            started = Instant::now();
            let response = self
                .client
                .post(self.url("/v1/runs"))
                .json(&json!({
                    "agentId": AGENT_ID,
                    "threadId": unit.id,
                    // Fixed, because input length is an input to turn cost and has to
                    // be identical across subjects.
                    "messages": [{ "role": "user", "content": "benchmark" }],
                }))
                .send()
                .await?;
            if response.status() == reqwest::StatusCode::CONFLICT {
                anyhow::ensure!(
                    waiting_since.elapsed() < std::time::Duration::from_secs(10),
                    "the thread's previous run never finished finalizing"
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
            break response;
        };
        let response = ok(response, "submitting a run").await?;

        let mut frames = response.bytes_stream();
        let mut pending = String::new();
        let mut named = String::new();
        while let Some(chunk) = frames.next().await {
            pending.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(newline) = pending.find('\n') {
                let line: String = pending.drain(..=newline).collect();
                let line = line.trim();
                if let Some(name) = line.strip_prefix("event: ") {
                    named = name.to_owned();
                    continue;
                }
                let Some(payload) = line.strip_prefix("data: ") else {
                    continue;
                };
                let frame: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
                let kind = frame
                    .get("event_type")
                    .and_then(Value::as_str)
                    .unwrap_or(named.as_str());
                match kind {
                    "text_delta" => {
                        if matches!(until, TurnEnd::FirstDelta) {
                            return Ok(started.elapsed().as_secs_f64() * 1_000.0);
                        }
                    }
                    "run_finish" => {
                        return match until {
                            TurnEnd::Finished => Ok(started.elapsed().as_secs_f64() * 1_000.0),
                            // Finishing with no delta means the turn produced no
                            // assistant output, which is not a first-byte measurement
                            // however fast it arrived.
                            TurnEnd::FirstDelta => anyhow::bail!(
                                "the run finished with no assistant output: {frame}"
                            ),
                        };
                    }
                    "run_error" | "error" => anyhow::bail!("the run failed: {frame}"),
                    _ => continue,
                }
            }
        }
        anyhow::bail!("the run stream ended before its run_finish event")
    }
}

#[async_trait]
impl Driver for AwakenDriver {
    /// One untimed warmup run on a scratch thread, so the stores exist and the provider
    /// path is proven before anything is measured.
    async fn prepare(&mut self) -> Result<()> {
        let scratch = self.create().await?;
        self.drive(&scratch, TurnEnd::Finished).await?;
        eprintln!("awaken: warmup run completed against the scripted provider");
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let response = self
            .client
            .post(self.url("/v1/threads"))
            .json(&json!({ "title": "bench" }))
            .send()
            .await?;
        let thread: Value = ok(response, "creating a thread").await?.json().await?;
        Ok(Unit {
            id: thread
                .get("id")
                .and_then(Value::as_str)
                .with_context(|| format!("thread response carried no id: {thread}"))?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.drive(unit, TurnEnd::FirstDelta).await
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.drive(unit, TurnEnd::Finished).await
    }

    /// Nothing to tear down server-side: the runs API exposes no thread delete, and a
    /// thread is rows in the server's own durable store.
    async fn destroy(&self, _unit: &Unit) -> Result<()> {
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
