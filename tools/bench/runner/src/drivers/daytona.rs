//! Daytona, driven through its public REST API.
//!
//! A sandbox, not a session kernel: the unit of work is a sandbox and a "turn" is a
//! command executed inside it. The probes mean what the class table says they mean for a
//! sandbox — `create` is create-until-it-accepts-an-exec, `round_trip` is exec-until-the
//! command-completed — and its manifest says so in its own terms.
//!
//! This is a hosted service measured across a network, so its numbers carry the round trip
//! to Daytona's region and are not the engine's alone. That is what the `sandbox` class
//! and the `quota` limit source exist to say.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

const API: &str = "https://app.daytona.io/api";
const TOOLBOX: &str = "https://proxy.app.daytona.io/toolbox";

pub struct DaytonaDriver {
    client: reqwest::Client,
    key: String,
    turns_requested: AtomicU64,
}

impl DaytonaDriver {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            key: std::env::var("DAYTONA_API_KEY")
                .context("DAYTONA_API_KEY must be set to measure Daytona")?,
            turns_requested: AtomicU64::new(0),
        })
    }

    fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.post(url).bearer_auth(&self.key)
    }
}

#[async_trait]
impl Driver for DaytonaDriver {
    async fn create(&self) -> Result<Unit> {
        let response = self.post(&format!("{API}/sandbox")).json(&json!({})).send().await?;
        let sandbox: Value = ok(response, "creating a sandbox").await?.json().await?;
        Ok(Unit {
            id: sandbox
                .get("id")
                .and_then(Value::as_str)
                .context("sandbox response carried no id")?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!("not wired: Daytona's exec returns on completion, so there is no first byte to time separately")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .post(&format!("{TOOLBOX}/{}/process/execute", unit.id))
            // Fixed and trivial, because what is being measured is the sandbox's cost of
            // running something, not the something.
            .json(&json!({ "command": "echo benchmark" }))
            .send()
            .await?;
        let result: Value = ok(response, "executing a command").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        // A non-zero exit means the command did not run, whatever the HTTP status says.
        let code = result.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
        anyhow::ensure!(code == 0, "the command exited {code}: {result}");
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(format!("{API}/sandbox/{}", unit.id))
            .bearer_auth(&self.key)
            .send()
            .await?;
        ok(response, "deleting a sandbox").await?;
        Ok(())
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

impl DaytonaDriver {
    fn requesting_a_turn(&self) {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
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
