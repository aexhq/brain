//! E2B Cloud, driven through its public REST API.
//!
//! Only `create` and `destroy`. E2B splits its surface: sandbox lifecycle is REST, but
//! command execution is gRPC against the sandbox's own envd, so a turn probe would mean
//! either vendoring their protobufs or shelling out to their SDK — and a number produced
//! through a different client than every other subject's is not the same measurement.
//!
//! `create` is the number a sandbox is actually judged on, and it is measured here the
//! same way it is measured for every other subject: the call that asks for one, until the
//! service says it exists. Hosted, so the network is inside it.

use std::time::Instant;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

const API: &str = "https://api.e2b.dev";

pub struct E2bDriver {
    client: reqwest::Client,
    key: String,
    template: String,
}

impl E2bDriver {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            key: std::env::var("E2B_API_KEY")
                .context("E2B_API_KEY must be set to measure E2B")?,
            // Their published base image. Pinning a custom template would measure our
            // image rather than their service.
            template: std::env::var("BENCH_E2B_TEMPLATE").unwrap_or_else(|_| "base".to_owned()),
        })
    }
}

#[async_trait]
impl Driver for E2bDriver {
    async fn create(&self) -> Result<Unit> {
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{API}/sandboxes"))
            .header("X-API-KEY", &self.key)
            .json(&json!({ "templateID": self.template, "timeout": 60 }))
            .send()
            .await?;
        let sandbox: Value = ok(response, "creating a sandbox").await?.json().await?;
        let _ = started;
        let id = sandbox
            .get("sandboxID")
            .or_else(|| sandbox.get("sandboxId"))
            .or_else(|| sandbox.get("id"))
            .and_then(Value::as_str)
            .context("sandbox response carried no id")?
            .to_owned();
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!("not wired: E2B executes over gRPC, so a turn probe would use a different client than every other subject")
    }

    async fn round_trip_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!("not wired: E2B executes over gRPC, so a turn probe would use a different client than every other subject")
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(format!("{API}/sandboxes/{}", unit.id))
            .header("X-API-KEY", &self.key)
            .send()
            .await?;
        ok(response, "killing a sandbox").await?;
        Ok(())
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
