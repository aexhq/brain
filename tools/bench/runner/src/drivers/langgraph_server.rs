//! LangGraph Server, driven through its own HTTP API.
//!
//! The closest live rival with a documented server surface. Its unit of work is a
//! *thread*, and a turn is a *run* against one, so `create` is a thread create and both
//! turn probes are runs — `runs/wait` for the whole turn, `runs/stream` for the first
//! output byte. That mapping is the driver's whole job: the probe means the same thing
//! for both subjects, and each subject's manifest says what it means in its own terms.
//!
//! The interesting arm here is not latency. LangGraph checkpoints per super-step against
//! Brain's append, so `persistence` — bytes written per turn against turn index — is
//! where the two diverge and keep diverging. That probe belongs to the `langgraph`
//! library subject; this one is the running server measured end to end.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::driver::{Driver, Unit};

pub struct LangGraphServerDriver {
    client: reqwest::Client,
    base_url: String,
    /// The graph to run. LangGraph names these in `langgraph.json`; the benchmark's
    /// fixture graph is registered under this name.
    assistant_id: String,
    pid: Option<u32>,
    turns_requested: AtomicU64,
}

impl LangGraphServerDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                // Matched to Brain's, so neither subject's number is its client's pool.
                .pool_max_idle_per_host(512)
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            assistant_id: std::env::var("BENCH_LANGGRAPH_ASSISTANT")
                .unwrap_or_else(|_| "agent".to_owned()),
            pid,
            turns_requested: AtomicU64::new(0),
        })
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.post(format!("{}{path}", self.base_url))
    }

    /// The input every timed run submits. Fixed, because input length is an input to turn
    /// cost and has to be identical across subjects.
    fn input(&self) -> Value {
        json!({ "messages": [{ "role": "user", "content": "benchmark" }] })
    }

    fn requesting_a_turn(&self) {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
    }
}

#[async_trait]
impl Driver for LangGraphServerDriver {
    async fn create(&self) -> Result<Unit> {
        let thread: Value = self
            .post("/threads")
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()
            .context("creating a thread")?
            .json()
            .await?;
        Ok(Unit {
            id: thread
                .get("thread_id")
                .and_then(Value::as_str)
                .context("thread response carried no thread_id")?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        // Unlike Brain's, this stream is opened *by* submitting the run: LangGraph has no
        // separate subscribe, so the subscribe cost is inside the number by construction.
        // The manifest says so, which is what keeps the two rows honest side by side.
        let response = self
            .post(&format!("/threads/{}/runs/stream", urlencode(&unit.id)))
            .json(&json!({
                "assistant_id": self.assistant_id,
                "input": self.input(),
                "stream_mode": "messages",
            }))
            .send()
            .await?
            .error_for_status()
            .context("starting a streaming run")?;

        let mut events = response.bytes_stream();
        while let Some(chunk) = events.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            // First model output, not first frame: LangGraph emits metadata events before
            // any token, and timing those would flatter it against a subject that emits
            // fewer of them.
            if text.contains("\"content\"") {
                return Ok(started.elapsed().as_secs_f64() * 1_000.0);
            }
        }
        anyhow::bail!("the run stream ended before any model output")
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        let finished: Value = self
            .post(&format!("/threads/{}/runs/wait", urlencode(&unit.id)))
            .json(&json!({
                "assistant_id": self.assistant_id,
                "input": self.input(),
            }))
            .send()
            .await?
            .error_for_status()
            .context("running a turn to completion")?
            .json()
            .await
            .context("reading the state a run returned")?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        // A run that errored still answers 200 with the error in its body, exactly as
        // Brain's did before this benchmark started checking. A latency sample is only a
        // latency sample if the work happened.
        anyhow::ensure!(
            finished.get("__error__").is_none(),
            "the run did not complete: {finished}"
        );
        Ok(elapsed)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        self.client
            .delete(format!("{}/threads/{}", self.base_url, urlencode(&unit.id)))
            .send()
            .await?
            .error_for_status()
            .context("deleting a thread")?;
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

/// Thread ids are server-generated UUIDs, but they go into a path, and a benchmark that
/// assumes an id is path-safe is one server change away from measuring a 404.
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::urlencode;

    #[test]
    fn an_id_cannot_escape_its_path_segment() {
        assert_eq!(urlencode("2f5a-41b0"), "2f5a-41b0");
        assert_eq!(urlencode("../runs"), "..%2Fruns");
        assert_eq!(urlencode("a b"), "a%20b");
    }
}
