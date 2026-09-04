//! Codex, driven through `codex app-server` over the stdio bridge.
//!
//! The app-server is what the Codex desktop app and IDE extensions talk to: JSON-RPC 2.0,
//! newline-delimited, over stdio. This driver speaks it as documented in
//! `codex-rs/app-server/README.md`; the bridge only carries the lines.
//!
//! * `create` is `thread/start`, until the response carries the thread id;
//! * a turn is `turn/start`, first text at the first `item/agentMessage/delta` for the
//!   thread, complete at `turn/completed`;
//! * `destroy` is `thread/delete`, which removes the rollout file — the counterpart of
//!   Brain's `DELETE /v1/sessions/{id}`;
//! * after a relaunch a thread the process has not loaded is reached with
//!   `thread/resume` first, which is what the recovery probe measures.
//!
//! A single operator's tool, so throughput and the per-session memory ramp are not
//! declared — the same reasoning as for zeroclaw and openclaw.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use super::{
    bridge::Bridge,
    feed::{self, client},
};
use crate::driver::{Driver, Unit};

const PROMPT: &str = "benchmark";
const TURN_TIMEOUT: Duration = Duration::from_secs(60);

pub struct CodexDriver {
    base_url: String,
    pid: Option<u32>,
    turns_requested: AtomicU64,
    next_id: AtomicU64,
    bridge: Option<Bridge>,
    /// Threads this process has started or resumed. Any other thread is resumed before
    /// its first turn.
    loaded: Mutex<HashSet<String>>,
}

struct Turn {
    first_text_ms: f64,
    complete_ms: f64,
}

impl CodexDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            pid,
            turns_requested: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
            bridge: None,
            loaded: Mutex::new(HashSet::new()),
        })
    }

    fn bridge(&self) -> Result<&Bridge> {
        self.bridge
            .as_ref()
            .context("prepare() must run before any probe")
    }

    /// One JSON-RPC request and its response. Notifications in between are read past.
    async fn request(
        &self,
        receiver: &mut broadcast::Receiver<Arc<Value>>,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.bridge()?
            .send(&json!({ "method": method, "id": id, "params": params }))
            .await?;
        let response = feed::wait_for(receiver, TURN_TIMEOUT, |event| {
            event.get("id").and_then(Value::as_u64) == Some(id) && event.get("method").is_none()
        })
        .await
        .with_context(|| format!("waiting for codex to answer {method}"))?;
        if let Some(error) = response.get("error") {
            anyhow::bail!("codex refused {method}: {error}");
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn turn(&self, unit: &Unit) -> Result<Turn> {
        let mut receiver = self.bridge()?.subscribe();
        let known = self
            .loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&unit.id);
        let started = Instant::now();
        if !known {
            // Inside the clock on purpose: this only happens after a relaunch, and the
            // recovery figure is exactly "until the same session serves a turn".
            self.request(&mut receiver, "thread/resume", json!({ "threadId": unit.id }))
                .await?;
            self.loaded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(unit.id.clone());
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        self.bridge()?
            .send(&json!({
                "method": "turn/start",
                "id": id,
                "params": {
                    "threadId": unit.id,
                    "input": [{ "type": "text", "text": PROMPT }],
                },
            }))
            .await?;

        let deadline = tokio::time::Instant::now() + TURN_TIMEOUT;
        let mut first_text = None;
        let complete_ms = loop {
            let event = feed::next(&mut receiver, deadline)
                .await
                .context("waiting for codex to finish the turn")?;
            let for_this_thread =
                event.pointer("/params/threadId").and_then(Value::as_str) == Some(unit.id.as_str());
            match event.get("method").and_then(Value::as_str) {
                None if event.get("id").and_then(Value::as_u64) == Some(id) => {
                    if let Some(error) = event.get("error") {
                        anyhow::bail!("codex refused turn/start: {error}");
                    }
                }
                Some("item/agentMessage/delta") if for_this_thread => {
                    let delta = event
                        .pointer("/params/delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !delta.is_empty() && first_text.is_none() {
                        first_text = Some(started.elapsed().as_secs_f64() * 1_000.0);
                    }
                }
                Some("turn/completed") if for_this_thread => {
                    let status = event
                        .pointer("/params/turn/status")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    anyhow::ensure!(
                        status == "completed",
                        "the turn ended {status}: {}",
                        event.pointer("/params/turn/error").unwrap_or(&Value::Null)
                    );
                    break started.elapsed().as_secs_f64() * 1_000.0;
                }
                Some("error") if for_this_thread => anyhow::bail!("codex reported: {event}"),
                // A request from the server — an approval, an elicitation — would hang
                // the turn waiting on an answer this driver does not give. With approvals
                // off it should never arrive; if it does, that is the finding.
                Some(method) if event.get("id").is_some() => {
                    anyhow::bail!("codex asked the client something this benchmark cannot answer: {method}")
                }
                _ => {}
            }
        };
        let first_text_ms = first_text.context("the turn completed without any assistant text")?;
        Ok(Turn {
            first_text_ms,
            complete_ms,
        })
    }
}

#[async_trait]
impl Driver for CodexDriver {
    /// The per-connection handshake. Untimed, like every other subject's connection setup.
    async fn prepare(&mut self) -> Result<()> {
        let bridge = Bridge::connect(client()?, &self.base_url).await?;
        let mut receiver = bridge.subscribe();
        self.bridge = Some(bridge);
        self.request(
            &mut receiver,
            "initialize",
            json!({
                "clientInfo": { "name": "brain-bench", "title": "Brain benchmark", "version": "0.1.0" },
                "capabilities": {},
            }),
        )
        .await?;
        self.bridge()?
            .send(&json!({ "method": "initialized", "params": {} }))
            .await?;
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let mut receiver = self.bridge()?.subscribe();
        // Model, provider, approvals and sandbox come from config.toml, written by
        // launch.sh; the working directory is the process's own, set by the bridge.
        let result = self.request(&mut receiver, "thread/start", json!({})).await?;
        let id = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .with_context(|| format!("thread/start returned no thread id: {result}"))?
            .to_owned();
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone());
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        Ok(self.turn(unit).await?.first_text_ms)
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        Ok(self.turn(unit).await?.complete_ms)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let mut receiver = self.bridge()?.subscribe();
        self.request(&mut receiver, "thread/delete", json!({ "threadId": unit.id }))
            .await?;
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&unit.id);
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}
