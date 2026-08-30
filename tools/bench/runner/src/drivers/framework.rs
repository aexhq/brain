//! The `framework` subjects, through the harness we wrote for them.
//!
//! These are libraries you build an agent *with*, not servers that hold sessions. They
//! answer no probe on their own, so `subjects/_framework_harness/wrapper.py` puts the
//! same two endpoints in front of each of them and the runner drives that. Any number
//! published from here has to say we wrote the harness — which is exactly why the harness
//! is the smallest thing that can be measured, and identical for every framework.
//!
//! What is being compared is the storage model. `persistence` watches the directory the
//! framework was told to write into, so a full checkpoint per super-step and an appended
//! record show up as what they are: one grows with the conversation, the other does not.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::driver::{Driver, Unit};

pub struct FrameworkDriver {
    client: reqwest::Client,
    base_url: String,
    pid: Option<u32>,
    turns_requested: AtomicU64,
}

impl FrameworkDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            pid,
            turns_requested: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl Driver for FrameworkDriver {
    async fn create(&self) -> Result<Unit> {
        let response = self
            .client
            .post(format!("{}/units", self.base_url))
            .send()
            .await?;
        let unit: Value = ok(response, "creating a unit").await?.json().await?;
        Ok(Unit {
            id: unit
                .get("id")
                .and_then(Value::as_str)
                .context("the harness returned no id")?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!("a framework subject answers persistence only; it declares no latency probe")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{}/units/{}/turns", self.base_url, unit.id))
            .send()
            .await?;
        ok(response, "running a turn").await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    async fn destroy(&self, _unit: &Unit) -> Result<()> {
        // Deliberately nothing. The persistence probe is about what a conversation left
        // behind, and deleting it would erase the measurement.
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
