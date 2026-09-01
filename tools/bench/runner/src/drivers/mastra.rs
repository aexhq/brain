//! Mastra, driven through its own HTTP API.
//!
//! Mastra's unit of work is a *thread*, scoped by thread + resource and created lazily
//! on first use; `create` measures the explicit form of the same moment
//! (`POST /api/memory/threads`), and a turn is `POST /api/agents/{id}/generate` — or
//! `/stream` for the first-byte probe, whose SSE chunks wrap AI-SDK-shaped parts in
//! Mastra's own `{type, payload}` envelope.
//!
//! **Pointing it at the scripted provider.** The subject's `src/mastra/index.ts` builds
//! its one agent through Mastra's model router with the launch environment's base URL,
//! so the wiring is done before this driver says anything.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

/// The agent id `src/mastra/index.ts` fixes.
const AGENT_ID: &str = "bench-agent";

pub struct MastraDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

impl MastraDriver {
    /// `_pid` is deliberately unused: the runner started the node process itself, so it
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

    /// The body every timed turn submits. Fixed, because input length is an input to
    /// turn cost and has to be identical across subjects. The v1 memory shape is the
    /// nested `memory: {thread, resource}` object — the flat 0.x keys are silently
    /// ignored and would fork a fresh context per turn.
    fn turn_body(unit: &Unit) -> Value {
        json!({
            "messages": [{ "role": "user", "content": "benchmark" }],
            "memory": { "thread": unit.id, "resource": "bench" },
        })
    }
}

#[async_trait]
impl Driver for MastraDriver {
    async fn create(&self) -> Result<Unit> {
        let thread_id = format!("bench-{}", uuid_like());
        let response = self
            .client
            .post(self.url(&format!("/api/memory/threads?agentId={AGENT_ID}")))
            .json(&json!({
                "threadId": thread_id,
                "resourceId": "bench",
                "title": "bench",
            }))
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

    /// Milliseconds until the first `text-delta` chunk on the turn's own SSE stream.
    ///
    /// Like every other served framework here and unlike Brain, the stream is opened
    /// *by* submitting the turn — Mastra has no separate subscribe — so the subscribe
    /// cost is inside this number by construction, and the manifest says so.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .client
            .post(self.url(&format!("/api/agents/{AGENT_ID}/stream")))
            .json(&Self::turn_body(unit))
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
                if payload == "[DONE]" {
                    anyhow::bail!("the turn stream ended before any assistant output");
                }
                let frame: Value = serde_json::from_str(payload)
                    .with_context(|| format!("reading a stream frame: {payload}"))?;
                match frame.get("type").and_then(Value::as_str) {
                    // Output. The first delta is the measurement.
                    Some("text-delta") => return Ok(started.elapsed().as_secs_f64() * 1_000.0),
                    Some("error") => anyhow::bail!("the turn failed: {frame}"),
                    // The terminal chunk before any content means the turn finished
                    // without the assistant ever saying anything, which is not a
                    // first-byte measurement however fast it arrived.
                    Some("finish") => anyhow::bail!(
                        "the stream reached its terminal chunk with no assistant output: {frame}"
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
        let response = self
            .client
            .post(self.url(&format!("/api/agents/{AGENT_ID}/generate")))
            .json(&Self::turn_body(unit))
            .send()
            .await?;
        let body: Value = ok(response, "sending a turn").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;

        // A turn that produced no assistant text did not happen, however the HTTP
        // status reads.
        let text = body.get("text").and_then(Value::as_str).unwrap_or("");
        anyhow::ensure!(!text.is_empty(), "the turn produced no assistant text: {body}");
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(self.url(&format!("/api/memory/threads/{}?agentId={AGENT_ID}", unit.id)))
            .send()
            .await?;
        ok(response, "deleting a thread").await?;
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

/// Distinct thread names without pulling in a uuid crate for a few probes' worth.
fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}
