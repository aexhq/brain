//! VoltAgent, driven through its own HTTP API.
//!
//! VoltAgent's unit of work is a *conversation*, scoped by `userId` + `conversationId`
//! and created lazily on first use; `create` measures the explicit form of the same
//! moment (`POST /api/memory/conversations`), and a turn is `POST /agents/{id}/text` —
//! or `/stream` for the first-byte probe, which is the raw AI SDK fullStream over SSE.
//!
//! **Pointing it at the scripted provider.** The server script builds its one agent
//! from `@ai-sdk/openai-compatible` with the launch environment's base URL, so the
//! wiring is done before this driver says anything, and every generate option the
//! server would otherwise default (temperature, maxOutputTokens, maxSteps,
//! contextLimit) is pinned in the request body — an unstated default is still
//! configuration, and it would be VoltAgent's rather than the benchmark's.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

/// The agent id `server.ts` fixes (VoltAgent derives the id from the agent's name).
const AGENT_ID: &str = "bench-agent";

pub struct VoltAgentDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

impl VoltAgentDriver {
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
    /// turn cost and has to be identical across subjects; every optional knob is pinned
    /// because the server fills schema defaults for anything unstated.
    fn turn_body(unit: &Unit) -> Value {
        json!({
            "input": "benchmark",
            "options": {
                "memory": { "userId": "bench", "conversationId": unit.id },
                "temperature": 0,
                "maxOutputTokens": 512,
                "maxSteps": 5,
                "contextLimit": 10,
            },
        })
    }
}

#[async_trait]
impl Driver for VoltAgentDriver {
    async fn create(&self) -> Result<Unit> {
        let conversation_id = format!("bench-{}", uuid_like());
        let response = self
            .client
            .post(self.url("/api/memory/conversations"))
            .json(&json!({
                "userId": "bench",
                "conversationId": conversation_id,
                "resourceId": AGENT_ID,
            }))
            .send()
            .await?;
        let body: Value = ok(response, "creating a conversation").await?.json().await?;
        // The id is read back out of the response rather than assumed, so a server that
        // renamed or regenerated it fails loudly here instead of silently forking a
        // second conversation on the first turn.
        let id = body
            .pointer("/data/conversation/id")
            .and_then(Value::as_str)
            .with_context(|| format!("conversation response carried no id: {body}"))?;
        Ok(Unit { id: id.to_owned() })
    }

    /// Milliseconds until the first `text-delta` part on the turn's own SSE stream.
    ///
    /// Like LangGraph Server's, Letta's, and AgentOS's, and unlike Brain's, the stream
    /// is opened *by* submitting the turn — VoltAgent has no separate subscribe — so the
    /// subscribe cost is inside this number by construction, and the manifest says so.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .client
            .post(self.url(&format!("/agents/{AGENT_ID}/stream")))
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
                let frame: Value = serde_json::from_str(payload)
                    .with_context(|| format!("reading a stream frame: {payload}"))?;
                match frame.get("type").and_then(Value::as_str) {
                    // Output. The first delta is the measurement.
                    Some("text-delta") => return Ok(started.elapsed().as_secs_f64() * 1_000.0),
                    Some("error") => anyhow::bail!("the turn failed: {frame}"),
                    // The terminal part before any content means the turn finished
                    // without the assistant ever saying anything, which is not a
                    // first-byte measurement however fast it arrived.
                    Some("finish") => anyhow::bail!(
                        "the stream reached its terminal part with no assistant output: {frame}"
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
            .post(self.url(&format!("/agents/{AGENT_ID}/text")))
            .json(&Self::turn_body(unit))
            .send()
            .await?;
        let body: Value = ok(response, "sending a turn").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;

        // A turn that produced no assistant text did not happen, however the HTTP
        // status reads: the envelope's own `success` and a non-empty `data.text` are
        // both held.
        anyhow::ensure!(
            body.get("success").and_then(Value::as_bool) == Some(true),
            "the turn did not succeed: {body}"
        );
        let text = body
            .pointer("/data/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        anyhow::ensure!(!text.is_empty(), "the turn produced no assistant text: {body}");
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(self.url(&format!("/api/memory/conversations/{}", unit.id)))
            .send()
            .await?;
        ok(response, "deleting a conversation").await?;
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

/// Distinct conversation names without pulling in a uuid crate for a few probes' worth.
fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}
