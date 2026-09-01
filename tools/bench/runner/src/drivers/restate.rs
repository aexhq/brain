//! Restate, driven through its ingress against the benchmark's own harness service.
//!
//! Restate is a durable-execution engine rather than an agent runtime: the agent
//! session lives in the harness service beside the manifest (their published Durable
//! Sessions pattern, verbatim but for the model's base URL), and this driver speaks to
//! it through restate-server's ingress. A Virtual Object exists the moment its key is
//! first invoked, so `create` is a local id mint and no create probe is declared; and
//! ingress answers at handler completion — Restate documents token streaming as not
//! yet supported — so there is no first-byte probe either.
//!
//! `prepare` registers the service deployment through the admin API, retrying while
//! the service process beside restate-server finishes booting, then drives one untimed
//! warmup turn to prove the whole path before anything is measured.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

/// Restate's default admin port; the manifest notes the collision this fixes a host at.
const ADMIN_URL: &str = "http://127.0.0.1:9070";
/// Where the harness service listens, fixed in the service's own source.
const SERVICE_URL: &str = "http://127.0.0.1:9080";

pub struct RestateDriver {
    client: reqwest::Client,
    base_url: String,
    turns_requested: AtomicU64,
}

impl RestateDriver {
    /// `_pid` is deliberately unused: the runner started the launch script's process
    /// group itself — restate-server and the service both live in it — so it samples
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
            .post(format!(
                "{}/restate/call/AgentSession/{}/send",
                self.base_url, unit.id
            ))
            // Fixed, because input length is an input to turn cost and has to be
            // identical across subjects.
            .json(&json!({ "message": "benchmark" }))
            .send()
            .await?;
        let reply: Value = ok(response, "sending a turn").await?.json().await?;
        let text = reply.as_str().unwrap_or("");
        anyhow::ensure!(!text.is_empty(), "the turn produced no assistant text: {reply}");
        Ok(text.to_owned())
    }
}

#[async_trait]
impl Driver for RestateDriver {
    /// Registers the harness service's deployment, waiting out the service's own boot,
    /// then proves one whole turn untimed.
    async fn prepare(&mut self) -> Result<()> {
        let mut last_error = String::new();
        for _ in 0..60 {
            let response = self
                .client
                .post(format!("{ADMIN_URL}/deployments"))
                .json(&json!({ "uri": SERVICE_URL }))
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => {
                    let warmup = self.create().await?;
                    self.turn(&warmup).await?;
                    eprintln!("restate: deployment registered, warmup turn completed");
                    return Ok(());
                }
                Ok(response) => {
                    last_error = format!(
                        "{}: {}",
                        response.status(),
                        response.text().await.unwrap_or_default()
                    );
                }
                Err(error) => last_error = error.to_string(),
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        anyhow::bail!("registering the harness deployment never succeeded: {last_error}")
    }

    /// A local id mint. A Virtual Object exists the moment its key is first invoked, so
    /// there is nothing to time here and the manifest declares no create probe.
    async fn create(&self) -> Result<Unit> {
        Ok(Unit {
            id: format!(
                "bench-{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_nanos())
                    .unwrap_or_default()
            ),
        })
    }

    /// Ingress has no streaming form, so a first-byte probe would time completion and
    /// call it something else; the manifest declares none and this is unreachable.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.round_trip_ms(unit).await
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        self.turn(unit).await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    /// Nothing to tear down server-side: object state is rows in RocksDB, and clearing
    /// them out from under the server would measure the benchmark's own cleanup.
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
