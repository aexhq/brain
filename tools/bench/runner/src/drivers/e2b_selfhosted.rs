//! E2B's own stack, built from source and run on our hardware.
//!
//! A different subject from `e2b-cloud`, and the numbers must never be merged: that one
//! measures their service across a network in their region, this one measures their code
//! on the instance every other subject was measured on. Same API, so the same probes are
//! answerable and the same ones are not.
//!
//! Only `create`, for the reason the cloud driver gives: E2B's sandbox lifecycle is REST,
//! but command execution is gRPC against each sandbox's own `envd`, and a turn number
//! produced through a vendored protobuf client rather than the HTTP client every other
//! subject is driven with is not the same measurement.
//!
//! `resident` is answerable here where it is not in the cloud, because the sandboxes are
//! Firecracker VMMs spawned by an orchestrator running on this box — so the runner can be
//! pointed at that process tree with `--pid` and sum private memory the same way it does
//! for every self-hosted subject.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

pub struct E2bSelfhostedDriver {
    client: reqwest::Client,
    base_url: String,
    key: String,
    template: String,
    pid: Option<u32>,
}

impl E2bSelfhostedDriver {
    pub fn new(base_url: &str, pid: Option<u32>) -> Result<Self> {
        anyhow::ensure!(
            !base_url.trim().is_empty(),
            "pass --base-url pointing at the self-hosted E2B API, which the runner does not \
             start: bringing this stack up is a docker compose plus three services, not a \
             single command a launch block could hold"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_owned(),
            // The local stack ships a fixed development key rather than an account.
            key: std::env::var("E2B_API_KEY")
                .context("E2B_API_KEY must be set to the key the local stack was seeded with")?,
            template: std::env::var("BENCH_E2B_TEMPLATE").unwrap_or_else(|_| "base".to_owned()),
            pid,
        })
    }
}

#[async_trait]
impl Driver for E2bSelfhostedDriver {
    async fn create(&self) -> Result<Unit> {
        let response = self
            .client
            .post(format!("{}/sandboxes", self.base_url))
            .header("X-API-KEY", &self.key)
            .json(&json!({ "templateID": self.template, "timeout": 60 }))
            .send()
            .await?;
        let sandbox: Value = ok(response, "creating a sandbox").await?.json().await?;
        let id = sandbox
            .get("sandboxID")
            .or_else(|| sandbox.get("sandboxId"))
            .or_else(|| sandbox.get("id"))
            .and_then(Value::as_str)
            .with_context(|| format!("sandbox response carried no id: {sandbox}"))?
            .to_owned();
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!(
            "not wired: E2B executes over gRPC against each sandbox's envd, so a turn probe \
             would use a different client than every other subject"
        )
    }

    async fn round_trip_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!(
            "not wired: E2B executes over gRPC against each sandbox's envd, so a turn probe \
             would use a different client than every other subject"
        )
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/sandboxes/{}", self.base_url, unit.id))
            .header("X-API-KEY", &self.key)
            .send()
            .await?;
        ok(response, "killing a sandbox").await?;
        Ok(())
    }

    /// The orchestrator's pid, passed in with `--pid`. Its children are the Firecracker
    /// VMMs, so the ordinary tree walk sums the sandboxes.
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        // No turn probe is declared, so nothing is claimed and nothing needs checking.
        0
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
