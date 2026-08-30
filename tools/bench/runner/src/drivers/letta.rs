//! Letta, driven through its own HTTP API.
//!
//! Letta's unit of work is an *agent*, not a session: state lives on the agent and a turn
//! is a message sent to it. So `create` is an agent create, and a turn is
//! `POST /v1/agents/{id}/messages`. That mapping is the driver's whole job — the probe
//! means the same thing for both subjects, and each manifest says what it means in its
//! own terms.
//!
//! **Pointing it at the scripted provider.** Letta enables its built-in `openai` provider
//! only when `OPENAI_API_KEY` is set, and gives that provider whatever `OPENAI_BASE_URL`
//! says (`server.py` builds `OpenAIProvider(base_url=model_settings.openai_api_base)`;
//! `settings.py` binds that field to `AliasChoices("OPENAI_BASE_URL","OPENAI_API_BASE")`).
//! Both are in the launch block, so the provider is already pointed at the benchmark's
//! endpoint before this driver says anything — there is no provider to register here, and
//! registering one was what previously drove the run into `api.openai.com`.
//!
//! What this driver must do instead is *check*, because the failure mode it is guarding
//! against is silent: if Letta ever fell back to its own hosted inference, every latency
//! below would be a network round trip to someone else's service wearing our label.
//! `prepare` therefore resolves the model handle out of Letta's own model list and
//! refuses to go on unless the handle Letta will use points at the scripted provider.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

/// The models `fixtures::scripted_provider` advertises on `/v1/models`. Letta discovers
/// them from there rather than being told, so these are the names to look for in what it
/// discovered — not names this driver gets to choose.
const SCRIPTED_MODEL: &str = "gpt-4o-mini";
const SCRIPTED_EMBEDDING: &str = "text-embedding-3-small";

pub struct LettaDriver {
    client: reqwest::Client,
    base_url: String,
    /// Where the scripted provider listens. Every handle Letta offers is checked against
    /// this before a single sample is taken.
    model_base_url: String,
    /// Resolved in `prepare` from Letta's own model list. Letta renames the handles of any
    /// OpenAI-compatible endpoint that is not `api.openai.com` to `openai-proxy/<model>`,
    /// so guessing the handle from the provider's name produces "model must be one of []".
    model_handle: String,
    embedding_handle: String,
    turns_requested: AtomicU64,
}

impl LettaDriver {
    /// `_pid` is deliberately unused: Letta runs in its own container, so the process the
    /// runner started is the docker client rather than the server, and sampling that
    /// process tree would report the client's memory as the subject's. The manifest
    /// declares no memory probe for the same reason.
    pub fn new(
        base_url: impl Into<String>,
        model_base_url: impl Into<String>,
        _pid: Option<u32>,
    ) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                // Matched to Brain's, so neither subject's number is its client's pool.
                .pool_max_idle_per_host(512)
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            model_base_url: model_base_url.into().trim_end_matches('/').to_owned(),
            model_handle: String::new(),
            embedding_handle: String::new(),
            turns_requested: AtomicU64::new(0),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn requesting_a_turn(&self) {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
    }

    /// The message every timed turn submits. Fixed, because input length is an input to
    /// turn cost and has to be identical across subjects.
    fn message(&self) -> Value {
        json!({ "role": "user", "content": "benchmark" })
    }

    /// The handle Letta has registered for `model` on the scripted provider.
    ///
    /// Read out of Letta rather than assumed, and matched on the *endpoint* rather than on
    /// the name: the endpoint is the only field that says where a turn would actually go.
    /// Letta ships `letta/letta-free` against `inference.letta.com` whether or not anything
    /// else is configured, so a handle that merely exists proves nothing.
    async fn handle_for(&self, path: &str, endpoint_field: &str, model: &str) -> Result<String> {
        let response = self.client.get(self.url(path)).send().await?;
        let listed: Value = ok(response, "listing the models Letta will accept")
            .await?
            .json()
            .await?;
        let listed = listed
            .as_array()
            .with_context(|| format!("{path} did not answer with a list: {listed}"))?;

        let mut seen = Vec::new();
        for entry in listed {
            let handle = entry.get("handle").and_then(Value::as_str).unwrap_or("");
            let endpoint = entry
                .get(endpoint_field)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim_end_matches('/');
            seen.push(format!("{handle} -> {endpoint}"));
            if endpoint == self.model_base_url
                && entry.get("name").and_then(Value::as_str) == Some(model)
            {
                return Ok(handle.to_owned());
            }
        }
        anyhow::bail!(
            "Letta has no handle for {model} pointed at the scripted provider at {}. It \
             offers: {}. Letta only enables its built-in openai provider when OPENAI_API_KEY \
             is set, and only sends it somewhere else when OPENAI_BASE_URL is set; without \
             both, agents would be measured against a real inference service",
            self.model_base_url,
            seen.join(", "),
        )
    }
}

#[async_trait]
impl Driver for LettaDriver {
    /// Establish that Letta will talk to the scripted provider, and learn what it calls it.
    async fn prepare(&mut self) -> Result<()> {
        self.model_handle = self
            .handle_for("/v1/models/", "model_endpoint", SCRIPTED_MODEL)
            .await?;
        self.embedding_handle = self
            .handle_for("/v1/models/embedding", "embedding_endpoint", SCRIPTED_EMBEDDING)
            .await?;
        eprintln!(
            "letta: model {} embedding {} (both on the scripted provider)",
            self.model_handle, self.embedding_handle
        );
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        // The trailing slash is load-bearing. Letta's router mounts the create route at
        // `/v1/agents/`, and FastAPI answers `/v1/agents` with a 307 to it — which a
        // client that does not follow redirects reads as a successful call carrying an
        // empty body, and then as an agent with no id.
        let agent = self
            .client
            .post(self.url("/v1/agents/"))
            .json(&json!({
                "name": format!("bench-{}", uuid_like()),
                "model": self.model_handle,
                "embedding": self.embedding_handle,
                // No memory blocks: the probe is agent creation, and a subject configured
                // with more work than another is not a comparison. Everything else is left
                // at Letta's defaults, including the base tool set it attaches.
                "memory_blocks": [],
            }))
            .send()
            .await?;
        let agent: Value = ok(agent, "creating an agent").await?.json().await?;
        Ok(Unit {
            id: agent
                .get("id")
                .and_then(Value::as_str)
                .context("agent response carried no id")?
                .to_owned(),
        })
    }

    /// Milliseconds until the first assistant output frame on the turn's own SSE stream.
    ///
    /// Like LangGraph Server's and unlike Brain's, the stream is opened *by* submitting
    /// the turn — Letta has no separate subscribe — so the subscribe cost is inside this
    /// number by construction, and the manifest says so.
    ///
    /// Letta refuses `streaming` to what it decides is a pre-1.0 client, which it works
    /// out from `User-Agent`: an absent one it documents as "no expectation of pinned
    /// behaviour", and treats as current. This client sends none, which is why the probe
    /// is allowed to stream at all — and it is not spoofing a header to claim to be
    /// Letta's own SDK. Should that ever change, the 422 arrives here with Letta's own
    /// wording rather than as a mystery.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .client
            .post(self.url(&format!("/v1/agents/{}/messages", unit.id)))
            .json(&json!({
                "messages": [self.message()],
                "streaming": true,
                "stream_tokens": true,
                // Keepalives would otherwise arrive as frames and this probe would time
                // the heartbeat rather than the answer.
                "include_pings": false,
            }))
            .send()
            .await?;
        let response = ok(response, "opening a streaming turn").await?;

        let mut frames = response.bytes_stream();
        let mut pending = String::new();
        while let Some(chunk) = frames.next().await {
            pending.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(newline) = pending.find('\n') {
                let line: String = pending.drain(..=newline).collect();
                let Some(payload) = line.trim().strip_prefix("data: ") else {
                    continue;
                };
                if payload == "[DONE]" {
                    break;
                }
                let frame: Value = serde_json::from_str(payload)
                    .with_context(|| format!("reading a stream frame: {payload}"))?;
                match frame.get("message_type").and_then(Value::as_str) {
                    // Bookkeeping, not output. Reaching either of the last two means the
                    // turn finished without the assistant ever saying anything, which is
                    // not a first-byte measurement however fast it arrived.
                    Some("ping") => continue,
                    Some("error_message") => anyhow::bail!("the turn failed: {frame}"),
                    Some("stop_reason") | Some("usage_statistics") => anyhow::bail!(
                        "the stream reached its terminal frames with no assistant output: {frame}"
                    ),
                    Some(_) => return Ok(started.elapsed().as_secs_f64() * 1_000.0),
                    None => continue,
                }
            }
        }
        anyhow::bail!("the turn stream ended before any assistant output")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let response = self
            .client
            .post(self.url(&format!("/v1/agents/{}/messages", unit.id)))
            .json(&json!({ "messages": [self.message()] }))
            .send()
            .await?;
        let body: Value = ok(response, "sending a message").await?.json().await?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;

        // A turn that produced no assistant message did not happen, however the HTTP
        // status reads. Letta says so itself: it answers 200 with a `stop_reason` that
        // names the failure — `error`, `llm_api_error`, `no_tool_call` — so the sample is
        // only taken when Letta reports the turn ended on its own terms.
        let stop = body
            .pointer("/stop_reason/stop_reason")
            .and_then(Value::as_str)
            .unwrap_or("<absent>");
        anyhow::ensure!(
            stop == "end_turn",
            "the turn did not complete: stop_reason {stop}: {body}"
        );
        anyhow::ensure!(
            body.get("messages")
                .and_then(Value::as_array)
                .is_some_and(|messages| messages.iter().any(|message| {
                    message.get("message_type").and_then(Value::as_str) == Some("assistant_message")
                })),
            "the turn produced no assistant message: {body}"
        );
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        let response = self
            .client
            .delete(self.url(&format!("/v1/agents/{}", unit.id)))
            .send()
            .await?;
        ok(response, "deleting an agent").await?;
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

/// Distinct agent names without pulling in a uuid crate for four probes' worth of them.
fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}
