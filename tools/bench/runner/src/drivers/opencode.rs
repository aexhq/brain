//! OpenCode, driven through `opencode serve`.
//!
//! The server is what the TUI, the desktop app and the SDK all talk to, so the session
//! API here is the product's own surface rather than a harness:
//!
//! * `create` is `POST /session`;
//! * `round_trip` is `POST /session/{id}/message`, which returns the completed assistant
//!   message;
//! * `ttfb` is `POST /session/{id}/prompt_async` with the `/event` stream already open,
//!   until the first assistant text part for the session appears on it;
//! * `destroy` is `DELETE /session/{id}`.
//!
//! Sessions are stored under the data directory the runner owns, so a relaunch finds them
//! by id with no resume step — the recovery probe measures exactly that.
//!
//! A single operator's tool: throughput and the per-session memory ramp are not declared.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use super::feed::{self, Feed, client, ok};
use crate::driver::{Driver, Unit};

const PROMPT: &str = "benchmark";
const TURN_TIMEOUT: Duration = Duration::from_secs(60);

pub struct OpencodeDriver {
    http: reqwest::Client,
    base_url: String,
    pid: Option<u32>,
    turns_requested: AtomicU64,
    /// The `/event` bus, opened in `prepare` so its subscribe latency is never in a number.
    feed: Option<Feed>,
}

impl OpencodeDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            http: client()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            pid,
            turns_requested: AtomicU64::new(0),
            feed: None,
        })
    }

    fn subscribe(&self) -> Result<broadcast::Receiver<Arc<Value>>> {
        Ok(self
            .feed
            .as_ref()
            .context("prepare() must run before any probe")?
            .subscribe())
    }

    fn message_body() -> Value {
        json!({ "parts": [{ "type": "text", "text": PROMPT }] })
    }
}

/// Whether a bus event is a text part of an assistant message in `session`, with text.
///
/// The user's own text part is published on the same bus under the same event, so the
/// part is attributed by the message it belongs to rather than by its type alone.
fn assistant_text(event: &Value, session: &str, assistant_messages: &[String]) -> bool {
    let properties = event.get("properties").unwrap_or(&Value::Null);
    let from_assistant = |carrier: &Value| {
        carrier.get("sessionID").and_then(Value::as_str) == Some(session)
            && carrier
                .get("messageID")
                .and_then(Value::as_str)
                .is_some_and(|id| assistant_messages.iter().any(|known| known == id))
    };
    match event.get("type").and_then(Value::as_str) {
        // The streamed chunk: `{sessionID, messageID, partID, field: "text", delta}`.
        Some("message.part.delta") => {
            from_assistant(properties)
                && properties.get("field").and_then(Value::as_str) == Some("text")
                && properties
                    .get("delta")
                    .and_then(Value::as_str)
                    .is_some_and(|delta| !delta.is_empty())
        }
        // The part itself, republished whole after each change.
        Some("message.part.updated") => {
            let part = properties.get("part").unwrap_or(&Value::Null);
            from_assistant(part)
                && part.get("type").and_then(Value::as_str) == Some("text")
                && part
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
        }
        _ => false,
    }
}

fn idle(event: &Value, session: &str) -> bool {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    let for_session = |key: &str| {
        event
            .pointer(&format!("/properties/{key}"))
            .and_then(Value::as_str)
            == Some(session)
    };
    match kind {
        "session.idle" => for_session("sessionID"),
        "session.status" => {
            for_session("sessionID")
                && event.pointer("/properties/status/type").and_then(Value::as_str) == Some("idle")
        }
        _ => false,
    }
}

#[async_trait]
impl Driver for OpencodeDriver {
    async fn prepare(&mut self) -> Result<()> {
        self.feed = Some(Feed::open(&format!("{}/event", self.base_url)).await?);
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let response = self
            .http
            .post(format!("{}/session", self.base_url))
            .json(&json!({}))
            .send()
            .await?;
        let session: Value = ok(response, "creating a session").await?.json().await?;
        let id = session
            .get("id")
            .and_then(Value::as_str)
            .with_context(|| format!("POST /session returned no id: {session}"))?
            .to_owned();
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        let mut receiver = self.subscribe()?;
        let started = Instant::now();
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let accepted = self
            .http
            .post(format!("{}/session/{}/prompt_async", self.base_url, unit.id))
            .json(&Self::message_body())
            .send()
            .await?;
        ok(accepted, "submitting a prompt").await?;

        let deadline = tokio::time::Instant::now() + TURN_TIMEOUT;
        let mut assistant_messages = Vec::new();
        let mut first_text = None;
        loop {
            let event = feed::next(&mut receiver, deadline)
                .await
                .context("waiting for opencode to finish the turn")?;
            match event.get("type").and_then(Value::as_str).unwrap_or("") {
                "message.updated" => {
                    let info = event.pointer("/properties/info").unwrap_or(&Value::Null);
                    if info.get("sessionID").and_then(Value::as_str) == Some(unit.id.as_str())
                        && info.get("role").and_then(Value::as_str) == Some("assistant")
                        && let Some(id) = info.get("id").and_then(Value::as_str)
                        && !assistant_messages.iter().any(|known| known == id)
                    {
                        assistant_messages.push(id.to_owned());
                    }
                }
                "message.part.updated" | "message.part.delta" => {
                    if first_text.is_none() && assistant_text(&event, &unit.id, &assistant_messages) {
                        first_text = Some(started.elapsed().as_secs_f64() * 1_000.0);
                    }
                }
                "session.error" => {
                    if event.pointer("/properties/sessionID").and_then(Value::as_str)
                        == Some(unit.id.as_str())
                    {
                        anyhow::bail!("opencode reported: {event}");
                    }
                }
                _ => {}
            }
            // The turn is allowed to finish after the clock has stopped, so the next
            // sample starts from an idle session.
            if first_text.is_some() && idle(&event, &unit.id) {
                break;
            }
        }
        first_text.context("the turn went idle without any assistant text")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        let started = Instant::now();
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let response = self
            .http
            .post(format!("{}/session/{}/message", self.base_url, unit.id))
            .json(&Self::message_body())
            .send()
            .await?;
        let message: Value = ok(response, "sending a message").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        let reply = message
            .get("parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>();
        anyhow::ensure!(!reply.trim().is_empty(), "the turn produced no reply: {message}");
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .http
            .delete(format!("{}/session/{}", self.base_url, unit.id))
            .send()
            .await?;
        ok(response, "deleting a session").await?;
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}
