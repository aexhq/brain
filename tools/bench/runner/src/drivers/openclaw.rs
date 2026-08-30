//! OpenClaw, driven through its Gateway.
//!
//! The Gateway multiplexes two surfaces on one port, and this driver uses both because
//! neither answers all three probes on its own:
//!
//! * the **control plane**, a WebSocket speaking OpenClaw's own request/response protocol.
//!   `sessions.create` is the session-create call — it is what the Control UI's "new
//!   session" button sends — and `sessions.delete` is its counterpart. There is no HTTP
//!   route for either.
//! * the **OpenAI-compatible HTTP endpoint**, `POST /v1/chat/completions`, which "runs as
//!   a normal Gateway agent run (same codepath as `openclaw agent`)". `model` is an agent
//!   target rather than a provider model, and `x-openclaw-session-key` routes the turn to
//!   a session this driver already created, so a turn probe measures a turn in an existing
//!   session rather than an implicit session create folded into the first message.
//!
//! Like ZeroClaw, OpenClaw documents itself as a single-operator gateway, so it declares
//! `create`, `ttfb` and `round_trip` and neither throughput nor a per-session memory ramp.
//! Node also means a large fixed memory floor, so only a slope or a delta says anything
//! about per-session cost here — never the absolute footprint the sampler records.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Mutex;

// One WebSocket client serves both subjects that put a timed operation behind one; it
// lives next to the driver that needed it first.
use super::zeroclaw::ws;
use crate::driver::{Driver, Unit};

/// The stable alias for whatever agent the gateway is configured to default to. The docs
/// call it "safe to hardcode even if the real default agent id changes between
/// environments", which is exactly what a benchmark wants.
const MODEL_TARGET: &str = "openclaw/default";

/// What every timed turn says. Fixed, because prompt length is a real input to turn cost.
const PROMPT: &str = "benchmark";

/// The protocol revision this driver speaks. Sent as both the minimum and the maximum: a
/// gateway that has moved on should refuse the handshake with a version error rather than
/// negotiate down to something this client has not been read against.
const PROTOCOL: u64 = 4;

pub struct OpenclawDriver {
    http: reqwest::Client,
    base_url: String,
    host: String,
    port: u16,
    pid: Option<u32>,
    turns_requested: AtomicU64,
    next_request_id: AtomicU64,
    /// The control-plane connection, opened in `prepare` so its cost never lands inside a
    /// `create` sample. `None` until then.
    control: Option<Arc<Mutex<ws::WsConn>>>,
}

impl OpenclawDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let (host, port) = split_host_port(&base_url)
            .with_context(|| format!("reading a host and port out of {base_url}"))?;
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                // Matched to Brain's, so neither subject's number is its client's pool.
                .pool_max_idle_per_host(512)
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            base_url,
            host,
            port,
            pid,
            turns_requested: AtomicU64::new(0),
            next_request_id: AtomicU64::new(1),
            control: None,
        })
    }

    /// One control-plane request/response round trip.
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let control = self
            .control
            .as_ref()
            .context("the control-plane connection was never opened")?;
        let mut control = control.lock().await;
        let id = format!("bench-{}", self.next_request_id.fetch_add(1, Ordering::Relaxed));
        control
            .send_text(
                &json!({ "type": "req", "id": id, "method": method, "params": params }).to_string(),
            )
            .await
            .with_context(|| format!("sending {method}"))?;
        response_to(&mut control, &id, method).await
    }
}

#[async_trait]
impl Driver for OpenclawDriver {
    /// Opens the control plane and completes the handshake.
    ///
    /// Never timed, which is the right place for it: the connection is per-client, not
    /// per-session, and folding a WebSocket upgrade into the first `create` would make the
    /// first sample incomparable to the other nineteen.
    async fn prepare(&mut self) -> Result<()> {
        let mut control = ws::WsConn::connect(&self.host, self.port, "/")
            .await
            .with_context(|| format!("opening the control plane at {}", self.base_url))?;

        // Sent without waiting for the gateway's `connect.challenge` event: the challenge
        // only matters to a client authenticating with a signed device key, and the frame
        // ordering is not guaranteed to put it first. `response_to` reads past any events
        // that arrive in the meantime.
        let id = "bench-connect";
        control
            .send_text(
                &json!({
                    "type": "req",
                    "id": id,
                    "method": "connect",
                    "params": {
                        "minProtocol": PROTOCOL,
                        "maxProtocol": PROTOCOL,
                        // `id` and `mode` are closed enums (GATEWAY_CLIENT_IDS and
                        // GATEWAY_CLIENT_MODES); anything else is refused at the
                        // handshake. This is the pair a non-interactive backend client
                        // declares.
                        "client": {
                            "id": "gateway-client",
                            "version": "0.1.0",
                            "platform": "linux",
                            "mode": "backend",
                        },
                        "role": "operator",
                        // The default operator scope set, which is exactly what the docs
                        // say a shared-secret bearer caller is granted. Asked for
                        // explicitly because a control-plane client that names no scopes
                        // is refused `sessions.create` with "missing scope:
                        // operator.write" — the HTTP endpoint's fall-back to the default
                        // set does not apply here.
                        "scopes": [
                            "operator.admin",
                            "operator.approvals",
                            "operator.pairing",
                            "operator.read",
                            "operator.talk.secrets",
                            "operator.write",
                        ],
                        "userAgent": "brain-bench",
                    },
                })
                .to_string(),
            )
            .await
            .context("sending the connect frame")?;
        let hello = response_to(&mut control, id, "connect").await?;
        anyhow::ensure!(
            hello.get("type").and_then(Value::as_str) == Some("hello-ok"),
            "the gateway answered connect with something other than hello-ok: {hello}"
        );

        self.control = Some(Arc::new(Mutex::new(control)));
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let created = self.request("sessions.create", json!({})).await?;
        // `key` is what every other call routes by; the SDK also accepts `sessionKey` from
        // older gateways, so both are read rather than assuming one shape.
        let id = created
            .get("key")
            .or_else(|| created.get("sessionKey"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("sessions.create returned no session key: {created}"))?;
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let response = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("x-openclaw-session-key", &unit.id)
            .json(&json!({
                "model": MODEL_TARGET,
                "stream": true,
                "messages": [{ "role": "user", "content": PROMPT }],
            }))
            .send()
            .await?;
        let response = ok(response, "starting a streamed completion").await?;
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt as _;
        let mut pending = String::new();
        let mut first_byte_ms: Option<f64> = None;
        let mut finished = false;
        // Read to the end of the stream rather than returning at the first delta. Hanging
        // up early aborts the run inside OpenClaw — the gateway logs `stopReason=aborted`
        // and starts failover handling — which is both work this benchmark did not intend
        // to measure and a state the next sample would start from.
        while let Some(chunk) = stream.next().await {
            pending.push_str(&String::from_utf8_lossy(&chunk?));
            for frame in sse_frames(&mut pending) {
                if frame == "[DONE]" {
                    finished = true;
                    continue;
                }
                let Ok(frame) = serde_json::from_str::<Value>(&frame) else {
                    continue;
                };
                if frame
                    .pointer("/choices/0/finish_reason")
                    .and_then(Value::as_str)
                    .is_some()
                {
                    finished = true;
                }
                // Assistant text, not the first frame on the wire: a role-only opening
                // delta carries no output, and timing it would flatter whichever subject
                // sends one.
                let delta = frame
                    .pointer("/choices/0/delta/content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !delta.is_empty() && first_byte_ms.is_none() {
                    first_byte_ms = Some(started.elapsed().as_secs_f64() * 1_000.0);
                }
            }
        }
        anyhow::ensure!(finished, "the stream ended before the turn completed");
        first_byte_ms.context("the turn completed without ever emitting assistant text")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        let response = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("x-openclaw-session-key", &unit.id)
            .json(&json!({
                "model": MODEL_TARGET,
                "stream": false,
                "messages": [{ "role": "user", "content": PROMPT }],
            }))
            .send()
            .await?;
        let body: Value = ok(response, "sending a completion").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        // A turn that produced no reply did not happen, whatever the status says.
        let reply = body
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        anyhow::ensure!(!reply.trim().is_empty(), "the turn produced no reply: {body}");
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        self.request("sessions.delete", json!({ "key": unit.id }))
            .await
            .map(|_| ())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

/// Reads control-plane frames until the response to `id` arrives, and returns its payload.
///
/// The control plane pushes events — state snapshots, channel health, the pre-connect
/// challenge — on the same socket, so a client that treated the next frame as its answer
/// would break the moment the gateway had anything to say.
async fn response_to(control: &mut ws::WsConn, id: &str, method: &str) -> Result<Value> {
    loop {
        let frame = control
            .next_text()
            .await
            .with_context(|| format!("waiting for the {method} response"))?;
        let frame: Value = serde_json::from_str(&frame)
            .with_context(|| format!("parsing a control-plane frame: {frame}"))?;
        if frame.get("type").and_then(Value::as_str) != Some("res")
            || frame.get("id").and_then(Value::as_str) != Some(id)
        {
            continue;
        }
        anyhow::ensure!(
            frame.get("ok").and_then(Value::as_bool) == Some(true),
            "{method} failed: {}",
            frame.get("error").unwrap_or(&frame)
        );
        return Ok(frame.get("payload").cloned().unwrap_or(Value::Null));
    }
}

/// Pulls whole `data:` payloads out of an SSE buffer, leaving any partial frame behind.
///
/// A chunk boundary lands mid-frame often enough that parsing what has arrived so far and
/// discarding the remainder silently loses deltas — including, on a short reply, the only
/// one there was.
fn sse_frames(pending: &mut String) -> Vec<String> {
    let mut frames = Vec::new();
    while let Some(end) = pending.find("\n\n") {
        let block = pending[..end].to_owned();
        pending.drain(..end + 2);
        for line in block.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                frames.push(data.trim().to_owned());
            }
        }
    }
    frames
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

/// Splits `http://127.0.0.1:18789` into its host and port.
fn split_host_port(base_url: &str) -> Option<(String, u16)> {
    let authority = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest)
        .split('/')
        .next()?;
    let (host, port) = authority.rsplit_once(':')?;
    Some((host.to_owned(), port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_frames_keep_a_partial_frame_for_the_next_chunk() {
        let mut pending = String::from("data: {\"a\":1}\n\ndata: {\"b\"");
        assert_eq!(sse_frames(&mut pending), vec!["{\"a\":1}".to_owned()]);
        assert_eq!(pending, "data: {\"b\"");
        pending.push_str(":2}\n\n");
        assert_eq!(sse_frames(&mut pending), vec!["{\"b\":2}".to_owned()]);
        assert!(pending.is_empty());
    }

    #[test]
    fn sse_frames_ignore_comment_and_event_lines() {
        let mut pending = String::from(": keep-alive\nevent: message\ndata: [DONE]\n\n");
        assert_eq!(sse_frames(&mut pending), vec!["[DONE]".to_owned()]);
    }

    #[test]
    fn host_and_port_come_out_of_a_base_url() {
        assert_eq!(
            split_host_port("http://127.0.0.1:18789"),
            Some(("127.0.0.1".to_owned(), 18789))
        );
        assert_eq!(split_host_port("http://127.0.0.1"), None);
    }
}
