//! pi, driven through its RPC mode over the stdio bridge.
//!
//! `pi --mode rpc` is the project's documented way to embed the agent: newline-delimited
//! JSON commands on stdin, responses and agent events on stdout. This driver speaks that
//! protocol as written in `docs/rpc.md`; the bridge only carries the lines.
//!
//! pi is one operator's coding agent and holds **one session per process**, so the four
//! moments map like this:
//!
//! * `create` is `new_session`, which replaces the session the process was holding, plus
//!   a `get_state` to read back the session file that names the new one — the only
//!   identity pi exposes for a session;
//! * a turn is `prompt`, first text at the first `message_update` carrying a `text_delta`,
//!   complete at `agent_end` with no retry pending;
//! * `destroy` releases nothing, because pi has nothing to release: a session is a file,
//!   and the next `new_session` moves on from it;
//! * after a relaunch the same session is reached with `switch_session` on its file, which
//!   is what the recovery probe measures.
//!
//! Throughput and the per-session memory ramp are not declared, for the same reason as
//! zeroclaw and openclaw: many concurrent sessions is a design goal the project never had.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};

use super::{
    bridge::Bridge,
    feed::{self, client},
};
use crate::driver::{Driver, Unit};

/// What every timed turn says. Fixed, because prompt length is a real input to turn cost.
const PROMPT: &str = "benchmark";
const TURN_TIMEOUT: Duration = Duration::from_secs(60);

pub struct PiDriver {
    base_url: String,
    pid: Option<u32>,
    turns_requested: AtomicU64,
    next_id: AtomicU64,
    bridge: Option<Bridge>,
    /// The session file the process currently holds, so a turn on another session
    /// switches to it first.
    current: Mutex<Option<String>>,
}

struct Turn {
    first_text_ms: f64,
    complete_ms: f64,
}

impl PiDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            pid,
            turns_requested: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            bridge: None,
            current: Mutex::new(None),
        })
    }

    fn bridge(&self) -> Result<&Bridge> {
        self.bridge
            .as_ref()
            .context("prepare() must run before any probe")
    }

    fn id(&self) -> String {
        format!("bench-{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// One command and its response. Events that arrive in between are read past.
    async fn command(
        &self,
        receiver: &mut tokio::sync::broadcast::Receiver<std::sync::Arc<Value>>,
        mut command: Value,
    ) -> Result<Value> {
        let id = self.id();
        let name = command["type"].as_str().unwrap_or("?").to_owned();
        command["id"] = json!(id);
        self.bridge()?.send(&command).await?;
        let response = feed::wait_for(receiver, TURN_TIMEOUT, |event| {
            event.get("type").and_then(Value::as_str) == Some("response")
                && event.get("id").and_then(Value::as_str) == Some(id.as_str())
        })
        .await
        .with_context(|| format!("waiting for pi to answer {name}"))?;
        anyhow::ensure!(
            response.get("success").and_then(Value::as_bool) == Some(true),
            "pi refused {name}: {}",
            response.get("error").unwrap_or(&response)
        );
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Makes `unit` the session the process holds, if it is not already.
    async fn switch_to(
        &self,
        receiver: &mut tokio::sync::broadcast::Receiver<std::sync::Arc<Value>>,
        unit: &Unit,
    ) -> Result<()> {
        let current = self.current.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if current.as_deref() == Some(unit.id.as_str()) {
            return Ok(());
        }
        let data = self
            .command(
                receiver,
                json!({ "type": "switch_session", "sessionPath": unit.id }),
            )
            .await?;
        anyhow::ensure!(
            data.get("cancelled").and_then(Value::as_bool) != Some(true),
            "an extension cancelled the session switch"
        );
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(unit.id.clone());
        Ok(())
    }

    async fn turn(&self, unit: &Unit) -> Result<Turn> {
        let mut receiver = self.bridge()?.subscribe();
        self.switch_to(&mut receiver, unit).await?;

        let id = self.id();
        let started = Instant::now();
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        self.bridge()?
            .send(&json!({ "id": id, "type": "prompt", "message": PROMPT }))
            .await?;

        let deadline = tokio::time::Instant::now() + TURN_TIMEOUT;
        let mut first_text = None;
        let complete_ms = loop {
            let event = feed::next(&mut receiver, deadline)
                .await
                .context("waiting for pi to finish the turn")?;
            match event.get("type").and_then(Value::as_str).unwrap_or("") {
                "response" if event.get("id").and_then(Value::as_str) == Some(id.as_str()) => {
                    anyhow::ensure!(
                        event.get("success").and_then(Value::as_bool) == Some(true),
                        "pi refused the prompt: {}",
                        event.get("error").unwrap_or(&event)
                    );
                }
                "message_update" => {
                    let delta = event
                        .pointer("/assistantMessageEvent")
                        .filter(|inner| inner.get("type").and_then(Value::as_str) == Some("text_delta"))
                        .and_then(|inner| inner.get("delta"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !delta.is_empty() && first_text.is_none() {
                        first_text = Some(started.elapsed().as_secs_f64() * 1_000.0);
                    }
                }
                "agent_end" => {
                    if event.get("willRetry").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                    break started.elapsed().as_secs_f64() * 1_000.0;
                }
                "extension_error" => anyhow::bail!("a pi extension failed: {event}"),
                _ => {}
            }
        };
        // `agent_settled` follows once nothing automatic remains. Waited for after the
        // clock has stopped, so the next prompt cannot land while pi still counts itself
        // busy and refuse it.
        let _ = feed::wait_for(&mut receiver, Duration::from_millis(200), |event| {
            event.get("type").and_then(Value::as_str) == Some("agent_settled")
        })
        .await;

        let first_text_ms = first_text.context("the turn completed without any assistant text")?;
        Ok(Turn {
            first_text_ms,
            complete_ms,
        })
    }
}

#[async_trait]
impl Driver for PiDriver {
    /// Subscribes to the process's stdout and checks that it loaded a model. Untimed:
    /// the bridge's readiness already covered pi's boot.
    async fn prepare(&mut self) -> Result<()> {
        let bridge = Bridge::connect(client()?, &self.base_url).await?;
        let mut receiver = bridge.subscribe();
        self.bridge = Some(bridge);
        let state = self.command(&mut receiver, json!({ "type": "get_state" })).await?;
        anyhow::ensure!(
            state.get("model").is_some_and(|model| !model.is_null()),
            "pi started without a model; the scripted provider in models.json was not picked up: {state}"
        );
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let mut receiver = self.bridge()?.subscribe();
        let data = self
            .command(&mut receiver, json!({ "type": "new_session" }))
            .await?;
        anyhow::ensure!(
            data.get("cancelled").and_then(Value::as_bool) != Some(true),
            "an extension cancelled new_session"
        );
        let state = self.command(&mut receiver, json!({ "type": "get_state" })).await?;
        let file = state
            .get("sessionFile")
            .and_then(Value::as_str)
            .with_context(|| format!("get_state named no session file after new_session: {state}"))?
            .to_owned();
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(file.clone());
        Ok(Unit { id: file })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        Ok(self.turn(unit).await?.first_text_ms)
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        Ok(self.turn(unit).await?.complete_ms)
    }

    /// Nothing to release: pi holds one session, on disk, and the next `new_session`
    /// moves on from it.
    async fn destroy(&self, _unit: &Unit) -> Result<()> {
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}
