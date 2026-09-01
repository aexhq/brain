//! Temporal, driven through the benchmark's own shim in the harness worker.
//!
//! Temporal's client is an SDK rather than an HTTP surface, so the harness worker
//! carries a stdlib HTTP shim: `/create` starts one session workflow, `/send` executes
//! an update on it and returns the reply. The workflow, the shim, and the wiring are
//! all harness code on Temporal's substrate — the manifest says so — mirroring their
//! own customer_service sample, with the model activity running through their official
//! OpenAI Agents plugin against the scripted provider.
//!
//! No first-byte probe: an update returns one complete reply, and the manifest says
//! that too.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

pub struct TemporalDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

impl TemporalDriver {
    /// `_pid` is deliberately unused: the runner started the launch script's process
    /// group itself — the dev server and the worker both live in it — so it samples
    /// that tree without the driver's help.
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

    async fn turn(&self, unit: &Unit) -> Result<String> {
        let response = self
            .client
            .post(format!("{}/send", self.base_url))
            // Fixed, because input length is an input to turn cost and has to be
            // identical across subjects.
            .json(&json!({ "id": unit.id, "message": "benchmark" }))
            .send()
            .await?;
        let reply: Value = ok(response, "sending a turn").await?.json().await?;
        let text = reply.get("text").and_then(Value::as_str).unwrap_or("");
        anyhow::ensure!(!text.is_empty(), "the turn produced no assistant text: {reply}");
        Ok(text.to_owned())
    }
}

#[async_trait]
impl Driver for TemporalDriver {
    /// One untimed warmup turn: the worker's sticky cache, the model activity path, and
    /// the plugin's data conversion all exist before anything is measured.
    async fn prepare(&mut self) -> Result<()> {
        let warmup = self.create().await?;
        self.turn(&warmup).await?;
        eprintln!("temporal: warmup turn completed against the scripted provider");
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let response = self
            .client
            .post(format!("{}/create", self.base_url))
            .json(&json!({}))
            .send()
            .await?;
        let body: Value = ok(response, "starting a session workflow").await?.json().await?;
        Ok(Unit {
            id: body
                .get("id")
                .and_then(Value::as_str)
                .with_context(|| format!("create response carried no id: {body}"))?
                .to_owned(),
        })
    }

    /// An update returns one complete reply; the manifest declares no first-byte probe
    /// and this is unreachable.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.round_trip_ms(unit).await
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        self.turn(unit).await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    /// Nothing to tear down: a session workflow parks in the dev server's store, and
    /// terminating it would measure the benchmark's own cleanup.
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
