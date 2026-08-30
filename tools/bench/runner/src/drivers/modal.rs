//! Modal, driven through its Python SDK because Modal offers no other way in.
//!
//! Every other hosted subject here is driven with the runner's own HTTP client. Modal
//! cannot be: it publishes no HTTP API for sandboxes, and all three of its SDKs speak
//! protobuf over gRPC to a control plane its own documentation calls "not a public API,
//! and can change without warning". The choice was a driver that shells out to Modal's
//! Python SDK or no Modal row at all, and a row produced through a different client is
//! still worth having as long as the difference is stated rather than hidden — which is
//! what this comment and the manifest's notes are for. The Python interpreter, the SDK
//! and one pipe hop are inside every Modal number; `reqwest` is inside every other one.
//!
//! The sidecar (`tools/bench/subjects/modal/sidecar.py`) is started once and outlives the
//! whole subject, so the interpreter's start-up and the SDK's import cost — the better
//! part of a second each — cannot land inside a sample.
//!
//! A sandbox, not a session kernel: the unit of work is a sandbox and a "turn" is a
//! command executed inside it, so the probes mean what the class table says they mean for
//! a sandbox. Hosted, so the round trip to Modal's region is inside every number.

use std::{
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

use crate::driver::{Driver, Unit};

/// Long enough for a cold sandbox on a bad day, short enough that a wedged sidecar cannot
/// eat a run's budget one sample at a time. The runner's own deadline is only checked
/// between samples, so without this a single hung request never ends.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

pub struct ModalDriver {
    /// An interpreter with the pinned `modal` SDK installed. Overridable because the SDK
    /// is a dependency of this subject, not of the runner, and belongs in whatever
    /// virtualenv the operator put it in.
    python: String,
    script: PathBuf,
    token_id: String,
    token_secret: String,
    /// `None` until `prepare` starts it. Behind a mutex because one pipe pair cannot
    /// interleave two requests — which is also why this subject declares no throughput
    /// probe: serialised requests measure latency, not concurrent load.
    sidecar: Mutex<Option<Sidecar>>,
    turns_requested: AtomicU64,
}

impl ModalDriver {
    pub fn new() -> Result<Self> {
        Ok(Self {
            python: std::env::var("BENCH_MODAL_PYTHON").unwrap_or_else(|_| "python3".to_owned()),
            script: std::env::var("BENCH_MODAL_SIDECAR")
                .unwrap_or_else(|_| "tools/bench/subjects/modal/sidecar.py".to_owned())
                .into(),
            token_id: std::env::var("MODAL_TOKEN_ID")
                .context("MODAL_TOKEN_ID must be set to measure Modal")?,
            token_secret: std::env::var("MODAL_TOKEN_SECRET")
                .context("MODAL_TOKEN_SECRET must be set to measure Modal")?,
            sidecar: Mutex::new(None),
            turns_requested: AtomicU64::new(0),
        })
    }

    async fn call(&self, request: Value, doing: &str) -> Result<Value> {
        let mut sidecar = self.sidecar.lock().await;
        sidecar
            .as_mut()
            .context("the Modal sidecar is not running; prepare() has to succeed first")?
            .call(request, doing)
            .await
    }

    fn requesting_a_turn(&self) {
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
    }
}

#[async_trait]
impl Driver for ModalDriver {
    async fn prepare(&mut self) -> Result<()> {
        let mut child = Command::new(&self.python)
            .arg(&self.script)
            .env("MODAL_TOKEN_ID", &self.token_id)
            .env("MODAL_TOKEN_SECRET", &self.token_secret)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, so the SDK's own traceback reaches the operator unedited rather
            // than being summarised by this driver.
            .stderr(Stdio::inherit())
            // The sidecar holds no state worth draining, and a benchmark that leaves a
            // Python process behind on every failure path leaves it holding sandboxes.
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "starting the Modal sidecar: {} {}. It needs an interpreter with the \
                     pinned modal SDK installed; point BENCH_MODAL_PYTHON at one",
                    self.python,
                    self.script.display()
                )
            })?;
        let stdin = child.stdin.take().context("the sidecar has no stdin")?;
        let stdout = BufReader::new(child.stdout.take().context("the sidecar has no stdout")?);
        let mut sidecar = Sidecar {
            child,
            stdin,
            stdout,
        };

        // The ready line arrives after the app lookup and image resolution, which is the
        // point of doing them here: neither is per-sandbox work, and every subject
        // front-loads a different amount of it, so none of it is timed.
        let ready = sidecar.reply("starting the Modal sidecar").await?;
        // Printed rather than asserted: the driver cannot see the manifest, so the run log
        // is where a drift between the pinned version and the installed one shows up.
        eprintln!(
            "modal: sidecar ready on SDK {}, app {}",
            ready.get("sdk").and_then(Value::as_str).unwrap_or("?"),
            ready.get("app").and_then(Value::as_str).unwrap_or("?"),
        );
        *self.sidecar.lock().await = Some(sidecar);
        Ok(())
    }

    async fn create(&self) -> Result<Unit> {
        let sandbox = self.call(json!({ "op": "create" }), "creating a sandbox").await?;
        Ok(Unit {
            id: sandbox
                .get("id")
                .and_then(Value::as_str)
                .context("the sidecar created a sandbox and returned no id")?
                .to_owned(),
        })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        // The sidecar checks the first chunk carries the command's own output, so a first
        // byte that belonged to something other than the exec comes back as an error and
        // never becomes a sample.
        self.call(
            json!({ "op": "ttfb", "id": unit.id }),
            "reading the first stdout byte of an exec",
        )
        .await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.requesting_a_turn();
        let started = Instant::now();
        // The sidecar checks the exit code, so a command that did not run cannot arrive
        // here as a latency sample.
        self.call(
            json!({ "op": "round_trip", "id": unit.id }),
            "executing a command",
        )
        .await?;
        Ok(started.elapsed().as_secs_f64() * 1_000.0)
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        // Returns once Modal has accepted the termination, not once the sandbox is gone:
        // Modal takes a measured ~31 seconds to release one, and waiting for that would
        // put thirteen minutes of teardown inside a 25-sample create probe. Nothing
        // downstream depends on the release having completed — `pid` is `None`, so the
        // memory sampler never runs for this subject and its reclaim probe is refused —
        // and an unreleased sandbox stops billing at Modal's default timeout regardless.
        self.call(
            json!({ "op": "destroy", "id": unit.id }),
            "terminating a sandbox",
        )
        .await?;
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        // The sidecar's pid would sample the Python client's memory and report it as
        // Modal's. Modal is a hosted service and exposes no process to read.
        None
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

struct Sidecar {
    /// Held so the child is killed when the driver drops.
    #[allow(dead_code)]
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Sidecar {
    async fn call(&mut self, request: Value, doing: &str) -> Result<Value> {
        self.stdin
            .write_all(format!("{request}\n").as_bytes())
            .await
            .with_context(|| format!("{doing}: writing to the sidecar"))?;
        self.stdin
            .flush()
            .await
            .with_context(|| format!("{doing}: flushing to the sidecar"))?;
        self.reply(doing).await
    }

    /// One response line, refused unless it says the work succeeded.
    async fn reply(&mut self, doing: &str) -> Result<Value> {
        let mut line = String::new();
        let read = tokio::time::timeout(REQUEST_TIMEOUT, self.stdout.read_line(&mut line))
            .await
            .with_context(|| {
                format!(
                    "{doing}: the sidecar answered nothing in {}s",
                    REQUEST_TIMEOUT.as_secs()
                )
            })?
            .with_context(|| format!("{doing}: reading from the sidecar"))?;
        anyhow::ensure!(
            read > 0,
            "{doing}: the sidecar exited; its traceback is on stderr above"
        );

        let response: Value = serde_json::from_str(&line)
            .with_context(|| format!("{doing}: the sidecar answered {line:?}, which is not JSON"))?;
        // The failure is carried in the body, and the body is what gets surfaced. A driver
        // that reports only that something failed makes every failure look identical.
        anyhow::ensure!(
            response.get("ok").and_then(Value::as_bool) == Some(true),
            "{doing}: {}",
            response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the sidecar refused without saying why")
        );
        Ok(response)
    }
}
