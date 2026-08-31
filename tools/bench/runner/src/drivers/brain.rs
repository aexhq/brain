//! Brain, driven over its public HTTP surface exactly as `examples/raw-http.mjs` does.
//!
//! Nothing here links a brain crate. The subject is the shipped binary with the shipped
//! contract, so Brain's number is produced the same way every competitor's number is —
//! which is the only way the comparison means anything.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};
use crate::fixtures::{Fixture, TOOL_PROMPT};

pub struct BrainDriver {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
    /// The compiled agentloop package, built by `brain build`. Admitted once in `prepare`.
    agentloop_package: PathBuf,
    /// `identity`, not `digest`: the wire renamed it, and reading the old name silently
    /// produced `None` and failed admission with a message that blamed the server.
    agentloop_identity: Option<String>,
    /// The message sent for every timed turn. Fixed, because input length is an input to
    /// turn cost and must be identical across subjects.
    prompt: String,
    pid: Option<u32>,
    /// The echo environment this session's tools are bound to. The dispatch probe reads
    /// arrival straight off it, because the benchmark owns that server.
    environment: Arc<Fixture>,
    /// Turns this driver asked for. Compared against the calls the scripted provider
    /// actually served, so a run cannot report latencies for work that never happened.
    turns_requested: std::sync::atomic::AtomicU64,
}

impl BrainDriver {
    fn requesting_a_turn(&self) {
        self.turns_requested
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl BrainDriver {
    pub fn new(
        base_url: impl Into<String>,
        agentloop_package: PathBuf,
        pid: Option<u32>,
        environment: Arc<Fixture>,
    ) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                // One connection per concurrent session, so the pool is never the queue
                // that a throughput number is actually measuring.
                .pool_max_idle_per_host(512)
                .timeout(Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: std::env::var("BRAIN_API_TOKEN").ok(),
            agentloop_package,
            agentloop_identity: None,
            prompt: "benchmark".to_owned(),
            turns_requested: std::sync::atomic::AtomicU64::new(0),
            pid,
            environment,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut builder = self
            .client
            .request(method, format!("{}{path}", self.base_url));
        if let Some(token) = &self.token {
            builder = builder.bearer_auth(token);
        }
        builder
    }

    /// A fresh idempotency key per call. Reusing one would have the server answer from
    /// its replay path, which is a different — and much faster — code path than the one
    /// under test.
    fn idempotent(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.request(method, path).header("idempotency-key", uuid())
    }
}

/// `error_for_status` throws the body away, and the body is where the subject says what
/// went wrong. A benchmark that reports "500 Internal Server Error" and nothing else
/// makes every failure look the same and sends you to the wrong place twice.
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

#[async_trait]
impl Driver for BrainDriver {
    async fn prepare(&mut self) -> Result<()> {
        let package = tokio::fs::read(&self.agentloop_package)
            .await
            .with_context(|| {
                format!(
                    "reading agentloop package {}; build it with `brain build` first",
                    self.agentloop_package.display()
                )
            })?;
        let response = self
            .idempotent(reqwest::Method::POST, "/v1/agentloops")
            .header("content-type", "application/octet-stream")
            .body(package)
            .send()
            .await?;
        let admission: Value = ok(response, "admitting the agentloop")
            .await?
            .json()
            .await?;
        self.agentloop_identity = admission
            .get("identity")
            .and_then(Value::as_str)
            .map(str::to_owned);
        anyhow::ensure!(
            self.agentloop_identity.is_some(),
            "agentloop admission returned no identity: {admission}"
        );
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let identity = self
            .agentloop_identity
            .as_ref()
            .context("prepare() must run before create()")?;
        let body = json!({
            "agentloop": { "identity": identity, "configuration": {} },
            "model": {
                "provider": "vercel-ai-gateway",
                // The contract requires a provider-qualified name, so this must carry a
                // slash even though the scripted provider ignores it.
                "name": "bench/scripted",
                // Ignored too, but the server requires one, and a benchmark must not run
                // a server in a configuration nobody ships.
                "api_key": "bench",
            },
            "system": "",
            // Tools are bound on every session, not only for the dispatch probe. Binding
            // them changes the presentation the model sees and the work the kernel does
            // per turn, so a turn measured without them is a different turn.
            "tools": [{
                "name": "echo",
                "description": "Returns its input unchanged.",
                "input_schema": {"type": "object", "properties": {}},
                "environment_id": "bench",
                "remote_tool_id": "echo",
                "configuration": {},
                "grant": {},
            }],
            "environments": [{
                "environment_id": "bench",
                "configuration": {},
                "lifecycle_policy": "shared",
            }],
        });
        let session = self
            .idempotent(reqwest::Method::POST, "/v1/sessions")
            .json(&body)
            .send()
            .await?;
        let session: Value = ok(session, "creating a session").await?.json().await?;
        Ok(Unit {
            id: session
                .get("session_id")
                .and_then(Value::as_str)
                .context("session response carried no session_id")?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        // The stream is opened before the message is sent, so the measurement cannot
        // include the client's own subscribe latency -- that would be timing us.
        let stream = self
            .request(
                reqwest::Method::GET,
                &format!("/v1/sessions/{}/events", unit.id),
            )
            .header("accept", "text/event-stream")
            .send()
            .await?
            .error_for_status()?;
        let mut events = stream.bytes_stream();

        let started = Instant::now();
        self.requesting_a_turn();
        let sending = self
            .idempotent(
                reqwest::Method::POST,
                &format!("/v1/sessions/{}/messages", unit.id),
            )
            .json(&json!({"content": self.prompt}))
            .send();

        // The send is not awaited first: the whole point is that the first token arrives
        // before the turn returns, so waiting for the turn here would measure the turn.
        tokio::pin!(sending);
        loop {
            tokio::select! {
                // The stream is polled first. Both branches are usually ready at once --
                // the deltas arrive while the turn runs and the turn then returns -- and an
                // unbiased select would toss a coin between "first token" and "no first
                // token", which is not a measurement.
                biased;
                chunk = events.next() => {
                    let Some(chunk) = chunk else {
                        anyhow::bail!("the event stream ended before any model output")
                    };
                    let text = String::from_utf8_lossy(&chunk?).into_owned();
                    // First model output, not first frame: lifecycle records arrive
                    // earlier and timing those would flatter every subject that emits
                    // more of them.
                    if text.contains("assistant_delta") {
                        let first_token = started.elapsed().as_secs_f64() * 1_000.0;
                        // The turn is still running. Leaving it there and deleting the
                        // session hands back "session is not idle" and loses the sample
                        // that was just taken, so the turn is allowed to finish first --
                        // after the clock has stopped, so none of it is in the number.
                        let session: Value = sending
                            .await?
                            .error_for_status()
                            .context("sending a message")?
                            .json()
                            .await
                            .context("reading the session a message returned")?;
                        completed(&session)?;
                        return Ok(first_token);
                    }
                }
                finished = &mut sending => {
                    let session: Value = finished?
                        .error_for_status()
                        .context("sending a message")?
                        .json()
                        .await
                        .context("reading the session a message returned")?;
                    completed(&session)?;
                    // The turn returned before a delta arrived. Whether that means the
                    // stream carried nothing at all or merely carried it late is the
                    // difference between a broken feature and a race, so drain what is
                    // there and say which: an error that does not distinguish them sends
                    // the reader looking in the wrong place.
                    let mut seen = String::new();
                    let grace = tokio::time::sleep(std::time::Duration::from_millis(500));
                    tokio::pin!(grace);
                    loop {
                        tokio::select! {
                            chunk = events.next() => match chunk {
                                Some(Ok(chunk)) => {
                                    seen.push_str(&String::from_utf8_lossy(&chunk));
                                    if seen.len() > 4000 { break }
                                }
                                _ => break,
                            },
                            _ = &mut grace => break,
                        }
                    }
                    let streamed = seen.contains("assistant_delta");
                    // Two different findings, and they must not read alike. Brain does
                    // stream: the frame is on the wire, mid-turn, between `model_intent`
                    // and `model_result`. But the scripted provider answers instantly, so
                    // the first token and the end of the turn are separated by less than
                    // the round trip that carries either to this client -- there is no gap
                    // here to measure. Against a model that takes hundreds of milliseconds
                    // to think, there is.
                    anyhow::ensure!(
                        !streamed,
                        "first token and turn completion are not separable against the                          scripted provider: it answers instantly, so the delta and the                          turn's response leave the server within the same millisecond and                          the gap this probe measures is smaller than the round trip that                          reports it. The stream did carry the model output."
                    );
                    let seen: String = seen.chars().take(4000).collect();
                    anyhow::bail!(
                        "the turn returned and no model output reached the event stream at                          all. What the stream carried in the 500ms after: {seen:?}"
                    )
                }
            }
        }
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        let started = Instant::now();
        self.requesting_a_turn();
        // sendMessage returns the updated session once the turn has been carried out.
        let session: Value = self
            .idempotent(
                reqwest::Method::POST,
                &format!("/v1/sessions/{}/messages", unit.id),
            )
            .json(&json!({"content": self.prompt}))
            .send()
            .await?
            .error_for_status()
            .context("sending a message")?
            .json()
            .await
            .context("reading the session a message returned")?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        completed(&session)?;
        Ok(elapsed)
    }

    /// Send until the bound environment received the tool call.
    ///
    /// Timed at the environment rather than from the session log: the benchmark owns that
    /// server, so it stamps arrival itself and waits on a notification instead of polling.
    /// The event log's `recorded_at_ms` is the alternative, and millisecond granularity is
    /// too coarse for a number expected to land below one.
    async fn tool_dispatch_ms(&self, unit: &Unit) -> Result<f64> {
        // Registered before the send, so an arrival cannot land in the gap and be missed.
        let arrived = self.environment.arrival.notified();
        tokio::pin!(arrived);

        let started = Instant::now();
        self.requesting_a_turn();
        self.idempotent(
            reqwest::Method::POST,
            &format!("/v1/sessions/{}/messages", unit.id),
        )
        .json(&json!({"content": TOOL_PROMPT}))
        .send()
        .await?
        .error_for_status()
        .context("sending the tool-provoking message")?;

        tokio::time::timeout(Duration::from_secs(30), arrived)
            .await
            .context("the environment never received a tool call")?;
        let at = self
            .environment
            .last_call_at()
            .context("the environment recorded no arrival time")?;
        Ok(at.duration_since(started).as_secs_f64() * 1_000.0)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        // Ended, then deleted. Brain refuses to delete a session that is still open, and
        // deleting without ending returned 400 for every probe that tears a session down
        // — which is every probe. `destroy` must not return until the subject has really
        // released it, or a reclaim measurement taken afterwards means nothing.
        let ended = self
            .idempotent(
                reqwest::Method::POST,
                &format!("/v1/sessions/{}/end", unit.id),
            )
            .send()
            .await?;
        ok(ended, "ending a session").await?;
        let deleted = self
            .idempotent(
                reqwest::Method::DELETE,
                &format!("/v1/sessions/{}", unit.id),
            )
            .send()
            .await?;
        ok(deleted, "deleting a session").await?;
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// A v4-shaped identifier. The server only requires uniqueness per request, so this
/// avoids pulling a uuid crate into the workspace for one header.
fn uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    format!("bench-{now:032x}-{count:016x}")
}

/// A turn that never reached the model still answers HTTP 200, carrying a session that
/// says so. Before the loophost was fixed, concurrency 64 put 12 of 256 requests through
/// to the model and the runner counted the other 244 as completed turns, reporting 1,926
/// turns/s. A latency sample is only a latency sample if the work happened.
fn completed(session: &Value) -> Result<()> {
    let status = session
        .get("status")
        .and_then(Value::as_str)
        .context("the session a message returned carried no status")?;
    if status == "idle" {
        return Ok(());
    }
    anyhow::bail!("the turn did not complete: the session came back {status}")
}
