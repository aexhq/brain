//! AgentScope Runtime, driven through its own HTTP API.
//!
//! The runtime has no create endpoint at all: the client mints a `session_id` and the
//! first turn creates it, so `create` here is a local id mint that costs nothing and no
//! create probe is declared in the manifest. A turn is `POST /process`, which answers
//! only as an SSE stream — the round-trip probe therefore reads its own stream to the
//! terminal `response`/`completed` event, because there is no blocking form to time
//! instead.
//!
//! **Pointing it at the scripted provider.** The subject's `app.py` builds its model
//! with the launch environment's base URL, so the wiring is done before this driver
//! says anything; `prepare` drives one untimed warmup turn so the lazily created
//! session store exists before anything is measured.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

pub struct AgentScopeDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

enum TurnEnd {
    FirstContent,
    Completed,
}

impl AgentScopeDriver {
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

    fn requesting_a_turn(&self) {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
    }

    /// The body every timed turn submits. Fixed, because input length is an input to
    /// turn cost and has to be identical across subjects. Content is a block list —
    /// a bare string is rejected by the schema.
    fn turn_body(unit: &Unit) -> Value {
        json!({
            "input": [{
                "role": "user",
                "content": [{ "type": "text", "text": "benchmark" }],
            }],
            "session_id": unit.id,
            "user_id": "bench",
        })
    }

    /// Drives one turn's stream until `until`, returning the elapsed milliseconds.
    ///
    /// One walker for both probes, because the framing is the same and only the stopping
    /// point differs: the first `content` event is the first model token, and the
    /// `response`/`completed` event is the turn's own report that it ended. A `failed`
    /// response or an in-band `error` line fails the sample rather than timing it.
    async fn drive(&self, unit: &Unit, until: TurnEnd) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{}/process", self.base_url))
            .json(&Self::turn_body(unit))
            .send()
            .await?;
        let response = ok(response, "submitting a turn").await?;

        let mut frames = response.bytes_stream();
        let mut pending = String::new();
        let mut first_content: Option<std::time::Duration> = None;
        while let Some(chunk) = frames.next().await {
            pending.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(newline) = pending.find('\n') {
                let line: String = pending.drain(..=newline).collect();
                let Some(payload) = line.trim().strip_prefix("data: ") else {
                    continue;
                };
                let frame: Value = serde_json::from_str(payload)
                    .with_context(|| format!("reading a stream frame: {payload}"))?;
                // Ordinary frames carry `"error": null`; only a non-null error is one.
                if let Some(error) = frame.get("error").filter(|error| !error.is_null()) {
                    anyhow::bail!("the turn failed: {error}");
                }
                let object = frame.get("object").and_then(Value::as_str);
                let status = frame.get("status").and_then(Value::as_str);
                match (object, status) {
                    (Some("content"), _) => {
                        // The measurement for the first-byte probe — but the stream is
                        // drained to completion either way, because hanging up early
                        // cancels the handler mid-save and truncates the session's JSON
                        // file, poisoning every turn after it.
                        first_content = first_content.or(Some(started.elapsed()));
                    }
                    (Some("response"), Some("completed")) => {
                        return match until {
                            TurnEnd::Completed => Ok(started.elapsed().as_secs_f64() * 1_000.0),
                            TurnEnd::FirstContent => first_content
                                .map(|at| at.as_secs_f64() * 1_000.0)
                                // Completing with no content event means the turn
                                // finished without the assistant ever saying anything,
                                // which is not a first-byte measurement however fast it
                                // arrived.
                                .with_context(|| {
                                    format!("the stream completed with no assistant output: {frame}")
                                }),
                        };
                    }
                    (Some("response"), Some("failed")) => {
                        anyhow::bail!("the turn failed: {frame}")
                    }
                    _ => continue,
                }
            }
        }
        anyhow::bail!("the turn stream ended before its terminal response event")
    }
}

#[async_trait]
impl Driver for AgentScopeDriver {
    /// One untimed warmup turn on a scratch session: the session store and the model
    /// path exist before anything is measured.
    async fn prepare(&mut self) -> Result<()> {
        let scratch = Unit {
            id: format!("bench-warmup-{}", uuid_like()),
        };
        self.drive(&scratch, TurnEnd::Completed).await?;
        eprintln!("agentscope-runtime: warmup turn completed against the scripted provider");
        Ok(())
    }

    /// A local id mint. The runtime has no create endpoint — the first turn creates the
    /// session — so there is nothing to time here and the manifest declares no create
    /// probe.
    async fn create(&self) -> Result<Unit> {
        Ok(Unit {
            id: format!("bench-{}", uuid_like()),
        })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.drive(unit, TurnEnd::FirstContent).await
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.drive(unit, TurnEnd::Completed).await
    }

    /// Nothing to tear down server-side: a session is a JSON file the runtime owns, and
    /// deleting it out from under the server would measure the benchmark's own cleanup.
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
