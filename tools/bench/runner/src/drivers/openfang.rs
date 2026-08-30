//! OpenFang, driven through its kernel API.
//!
//! The closest architectural twin in the survey: Rust, a WebAssembly sandbox with fuel
//! metering and epoch interruption, SQLite-backed sessions, an HTTP/SSE surface. Its unit
//! of work is an *agent* and a turn is a message to one, so `create` is an agent create
//! and `round_trip` is `POST /api/agents/{id}/message`.
//!
//! Because the two systems are so alike, this is the comparison where a difference means
//! something about the design rather than about the language or the runtime.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

pub struct OpenFangDriver {
    client: reqwest::Client,
    base_url: String,
    pid: Option<u32>,
    turns_requested: AtomicU64,
}

impl OpenFangDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                // Matched to Brain's, so neither subject's number is its client's pool.
                .pool_max_idle_per_host(512)
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            pid,
            turns_requested: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl Driver for OpenFangDriver {
    async fn create(&self) -> Result<Unit> {
        let response = self
            .client
            .post(format!("{}/api/agents", self.base_url))
            .json(&json!({ "manifest_toml": manifest(&format!("bench-{}", unique())) }))
            .send()
            .await?;
        let agent: Value = ok(response, "creating an agent").await?.json().await?;
        let id = agent
            .get("agent_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("agent response carried no agent_id: {agent}"))?;
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{}/api/agents/{}/message/stream", self.base_url, unit.id))
            .json(&json!({ "message": "benchmark" }))
            .send()
            .await?;
        let response = ok(response, "starting a streamed message").await?;
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt as _;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            // First model output, not first frame: lifecycle frames arrive earlier and
            // timing those would flatter whichever subject emits more of them.
            if text.contains("content") || text.contains("delta") {
                return Ok(started.elapsed().as_secs_f64() * 1_000.0);
            }
        }
        anyhow::bail!("the stream ended before any model output")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{}/api/agents/{}/message", self.base_url, unit.id))
            .json(&json!({ "message": "benchmark" }))
            .send()
            .await?;
        let body: Value = ok(response, "sending a message").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        // A turn that produced no reply did not happen, whatever the status says. The
        // same check Brain's driver makes, applied to the rival.
        anyhow::ensure!(
            body.get("response").is_some()
                || body.get("message").is_some()
                || body.get("content").is_some(),
            "the turn produced no reply: {body}"
        );
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/api/agents/{}", self.base_url, unit.id))
            .send()
            .await?;
        ok(response, "deleting an agent").await?;
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

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

/// The smallest agent OpenFang will accept.
///
/// `provider = "default"` and `model = "default"` defer to the daemon's configured model,
/// which the launcher pointed at the benchmark's scripted provider. No skills and no
/// channels: the probe is agent creation, and a subject configured with more work than
/// another is not a comparison. The agents OpenFang ships carry multi-page system
/// prompts, which would measure their prompt rather than the kernel.
fn manifest(name: &str) -> String {
    format!(
        r#"name = "{name}"
version = "0.1.0"
description = "benchmark agent"
author = "bench"
module = "builtin:chat"

[model]
provider = "default"
model = "default"
max_tokens = 256
temperature = 0.0
system_prompt = "benchmark"
"#
    )
}

fn unique() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}
