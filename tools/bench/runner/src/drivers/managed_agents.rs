//! Claude Managed Agents, driven through the public REST API.
//!
//! Anthropic runs the agent loop and hosts a per-session container. The unit of work is a
//! *session* against a stored agent, and a turn is a `user.message` event followed by the
//! agent reaching idle again — so `create` is a session create and `round_trip` is
//! message-until-idle.
//!
//! **This subject's numbers contain a real model.** There is no way to point Managed
//! Agents at the benchmark's scripted provider: the model runs on Anthropic's side by
//! definition. So every latency here includes inference and a network round trip, and the
//! manifest sets `model_included`, which keeps it out of engine comparisons. It is
//! collected and published as what it is — the cost of a hosted agent — not as a number to
//! put beside a kernel running against a fixture.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

const API: &str = "https://api.anthropic.com/v1";
/// The Managed Agents beta. Every call below needs it.
const BETA: &str = "managed-agents-2026-04-01";

pub struct ManagedAgentsDriver {
    client: reqwest::Client,
    key: String,
    model: String,
    /// The stored agent every session references. Created once in `prepare`, because
    /// creating one per session would put a control-plane write inside the create probe.
    agent_id: Option<String>,
    turns_requested: AtomicU64,
}

impl ManagedAgentsDriver {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(180))
                .build()?,
            key: std::env::var("ANTHROPIC_API_KEY")
                .context("ANTHROPIC_API_KEY must be set to measure Claude Managed Agents")?,
            model: std::env::var("BENCH_MANAGED_AGENTS_MODEL")
                .unwrap_or_else(|_| "claude-haiku-4-5".to_owned()),
            agent_id: None,
            turns_requested: AtomicU64::new(0),
        })
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{API}{path}"))
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", BETA)
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!("{API}{path}"))
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", BETA)
    }

    /// Waits for the session to go idle, which is what "the turn finished" means here.
    /// Polling rather than streaming so the number is not the SSE client's.
    async fn wait_for_idle(&self, session: &str, deadline: Instant) -> Result<()> {
        loop {
            anyhow::ensure!(
                Instant::now() < deadline,
                "the session never went idle within the turn timeout"
            );
            let response = self.get(&format!("/sessions/{session}")).send().await?;
            let session_state: Value = ok(response, "reading session status").await?.json().await?;
            match session_state.get("status").and_then(Value::as_str) {
                Some("idle") => return Ok(()),
                // A session that ended or failed did not complete the turn, and must not
                // become a latency sample.
                Some(other @ ("failed" | "terminated" | "expired")) => {
                    anyhow::bail!("the session went {other} instead of idle: {session_state}")
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }
}

#[async_trait]
impl Driver for ManagedAgentsDriver {
    async fn prepare(&mut self) -> Result<()> {
        let response = self
            .post("/agents")
            .json(&json!({
                "name": format!("bench-{}", unique()),
                "model": self.model,
                // The hosted toolset, which is what a Managed Agent is for. No skills, no
                // MCP: the probe is the platform, not a configuration we chose.
                "tools": [{ "type": "agent_toolset_20260401" }],
            }))
            .send()
            .await?;
        let agent: Value = ok(response, "creating the agent").await?.json().await?;
        self.agent_id = Some(
            agent
                .get("id")
                .and_then(Value::as_str)
                .with_context(|| format!("agent response carried no id: {agent}"))?
                .to_owned(),
        );
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let agent_id = self
            .agent_id
            .as_ref()
            .context("prepare() must run before create()")?;
        let response = self
            .post("/sessions")
            .json(&json!({ "agent": { "type": "agent", "id": agent_id } }))
            .send()
            .await?;
        let session: Value = ok(response, "creating a session").await?.json().await?;
        Ok(Unit {
            id: session
                .get("id")
                .and_then(Value::as_str)
                .with_context(|| format!("session response carried no id: {session}"))?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!(
            "not wired: first-token here would be Anthropic's inference latency, which is \
             already what round_trip measures for this subject"
        )
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let response = self
            .post(&format!("/sessions/{}/events", unit.id))
            .json(&json!({
                "events": [{
                    "type": "user.message",
                    // Fixed and trivial: input length is an input to turn cost and has to
                    // be identical across subjects.
                    "content": [{ "type": "text", "text": "benchmark" }],
                }],
            }))
            .send()
            .await?;
        ok(response, "sending a user message").await?;
        self.wait_for_idle(&unit.id, started + Duration::from_secs(150))
            .await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(format!("{API}/sessions/{}", unit.id))
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", BETA)
            .send()
            .await?;
        ok(response, "deleting a session").await?;
        Ok(())
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

/// `error_for_status` throws the body away, and the body is where the service says what
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

fn unique() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}
