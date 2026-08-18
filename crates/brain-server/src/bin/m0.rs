//! The M0 gate: create -> message -> model -> bash in a Lambda MicroVM -> streamed result ->
//! suspend (AWS idle policy, for real) -> resume on the next message -> workspace synced ->
//! survives the wall by re-materialising -- end to end, over real HTTP, against real provider
//! keys and real MicroVMs.
//!
//! Two passes:
//!   1. Anthropic (Messages dialect) -- the full arc including suspend, cancel, persist,
//!      wall-loss re-materialise, replay and delete.
//!   2. DeepSeek (OpenAI Chat Completions dialect, real endpoint) -- one build turn, which
//!      certifies the second dialect end to end.
//!
//! Requirements: the brain server env (see bin/brain.rs) plus ANTHROPIC_API_KEY and
//! DEEPSEEK_API_KEY. The server is started in-process on 127.0.0.1:8701; the wire is real
//! HTTP + SSE regardless.
//!
//! Every step prints its wall time; the run ends with `M0 PASS` or a loud failure.

use anyhow::{Context, bail, ensure};
use brain::provider::sse::SseDecoder;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BASE: &str = "http://127.0.0.1:8701";

fn main() -> anyhow::Result<()> {
    // The linear gate future is large; the default main stack overflows (same fix as the
    // hand-lambda e2e).
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,aws_config=warn,hyper=warn".into()),
                )
                .init();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run())
        })?
        .join()
        .expect("gate thread")
}

struct Api {
    http: reqwest::Client,
    token: String,
}

impl Api {
    async fn post(&self, path: &str, body: Value) -> anyhow::Result<(u16, Value)> {
        let r = self
            .http
            .post(format!("{BASE}{path}"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        Ok((status, v))
    }
    async fn post_empty(&self, path: &str) -> anyhow::Result<(u16, Value)> {
        let r = self
            .http
            .post(format!("{BASE}{path}"))
            .bearer_auth(&self.token)
            .header("content-length", "0")
            .send()
            .await?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        Ok((status, v))
    }
    async fn get(&self, path: &str) -> anyhow::Result<(u16, Value)> {
        let r = self
            .http
            .get(format!("{BASE}{path}"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        Ok((status, v))
    }
    async fn delete(&self, path: &str) -> anyhow::Result<u16> {
        Ok(self
            .http
            .delete(format!("{BASE}{path}"))
            .bearer_auth(&self.token)
            .send()
            .await?
            .status()
            .as_u16())
    }
}

#[derive(Debug, Clone)]
struct Ev {
    seq: u64,
    kind: String,
    data: Value,
}

/// Follows the SSE stream on a background task, accumulating events.
struct Follower {
    events: Arc<Mutex<Vec<Ev>>>,
    _task: tokio::task::JoinHandle<()>,
}

impl Follower {
    fn start(api: &Api, session_id: &str) -> Self {
        let events: Arc<Mutex<Vec<Ev>>> = Arc::default();
        let sink = events.clone();
        let url = format!("{BASE}/v1/sessions/{session_id}/events?after=0&follow=true");
        let http = api.http.clone();
        let token = api.token.clone();
        let task = tokio::spawn(async move {
            let resp = match http.get(&url).bearer_auth(&token).send().await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("sse connect failed: {e}");
                    return;
                }
            };
            let mut dec = SseDecoder::default();
            let mut bytes = resp.bytes_stream();
            let mut cur_id: Option<u64> = None;
            while let Some(chunk) = bytes.next().await {
                let Ok(chunk) = chunk else { break };
                // Track `id:` lines ourselves; SseDecoder yields event+data.
                for line in String::from_utf8_lossy(&chunk).lines() {
                    if let Some(id) = line.strip_prefix("id: ") {
                        cur_id = id.trim().parse().ok();
                    }
                }
                let Ok(frames) = dec.feed(&chunk) else { break };
                for f in frames {
                    let Ok(data) = serde_json::from_str::<Value>(&f.data) else {
                        continue;
                    };
                    let kind = f
                        .event
                        .clone()
                        .or_else(|| data.get("type").and_then(|t| t.as_str()).map(String::from))
                        .unwrap_or_default();
                    let seq = data
                        .get("seq")
                        .and_then(|s| s.as_u64())
                        .or(cur_id)
                        .unwrap_or(0);
                    sink.lock().expect("events").push(Ev { seq, kind, data });
                }
            }
        });
        Self {
            events,
            _task: task,
        }
    }

    fn snapshot(&self) -> Vec<Ev> {
        self.events.lock().expect("events").clone()
    }

    async fn wait_for<F: Fn(&Ev) -> bool>(
        &self,
        what: &str,
        pred: F,
        timeout: Duration,
    ) -> anyhow::Result<Ev> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(e) = self.snapshot().iter().find(|e| pred(e)) {
                return Ok(e.clone());
            }
            if Instant::now() > deadline {
                let seen: Vec<String> = self
                    .snapshot()
                    .iter()
                    .map(|e| format!("{}#{}", e.kind, e.seq))
                    .collect();
                bail!("timed out waiting for {what}; events seen: {seen:?}");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

async fn microvm_of(
    control: &hand_lambda::control::Control,
) -> anyhow::Result<Option<(String, String)>> {
    let vms = control.list().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(vms
        .into_iter()
        .map(|v| (v.id, format!("{:?}", v.state).to_uppercase()))
        .find(|(_, s)| !s.contains("TERMINAT")))
}

async fn wait_vm_state(
    control: &hand_lambda::control::Control,
    vm: &str,
    want: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match control.get(vm).await {
            Ok(v) => {
                let s = format!("{:?}", v.state).to_uppercase();
                if s.contains(&want.to_uppercase()) {
                    return Ok(());
                }
            }
            Err(e) => {
                if want == "TERMINATED" {
                    // Terminated VMs can drop off the API entirely.
                    let _ = e;
                    return Ok(());
                }
            }
        }
        if Instant::now() > deadline {
            bail!("vm {vm} did not reach {want} in time");
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn run_turn(
    api: &Api,
    follower: &Follower,
    session: &str,
    text: &str,
    timeout: Duration,
) -> anyhow::Result<(String, Vec<Ev>)> {
    let (status, acc) = api
        .post(
            &format!("/v1/sessions/{session}/messages"),
            json!({ "content": text }),
        )
        .await?;
    ensure!(status == 202, "message not accepted: {status} {acc}");
    let turn = acc["turn_id"].as_str().context("turn_id")?.to_string();
    let done = follower
        .wait_for(
            &format!("turn {turn} to finish"),
            |e| {
                (e.kind == "turn.completed" || e.kind == "turn.failed")
                    && e.data["turn_id"] == json!(turn)
            },
            timeout,
        )
        .await?;
    ensure!(
        done.kind == "turn.completed",
        "turn failed: {}",
        serde_json::to_string_pretty(&done.data).unwrap_or_default()
    );
    let evs: Vec<Ev> = follower
        .snapshot()
        .into_iter()
        .filter(|e| e.data["turn_id"] == json!(turn))
        .collect();
    Ok((turn, evs))
}

fn tool_results(evs: &[Ev]) -> Vec<&Ev> {
    evs.iter().filter(|e| e.kind == "tool.result").collect()
}

async fn run() -> anyhow::Result<()> {
    // ---- Server, in-process on a real socket. ------------------------------------------------
    let token = std::env::var("AEX_API_TOKEN").unwrap_or_else(|_| "m0-dev-token".into());
    // SAFETY: single-threaded here; the runtime workers start on Brain::new below.
    unsafe {
        std::env::set_var("AEX_API_TOKEN", &token);
        std::env::set_var("AEX_LISTEN", "127.0.0.1:8701");
    }
    // The gate is the AWS-composition gate by definition.
    let cfg = brain::session::BrainConfig::default();
    let hand_cfg =
        brain_aws::lambda::HandPlaneConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let brain_arc = brain_aws::brain_from_env(cfg)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let state = brain::api::AppState {
        brain: brain_arc,
        token: token.clone(),
    };
    tokio::spawn(brain::api::serve(
        state,
        "127.0.0.1:8701".parse().expect("addr"),
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let api = Api {
        http: reqwest::Client::new(),
        token,
    };
    let control = hand_lambda::control::Control::from_env(&hand_cfg.region).await;
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY")?;
    let deepseek_key = std::env::var("DEEPSEEK_API_KEY").context("DEEPSEEK_API_KEY")?;
    let anthropic_model =
        std::env::var("AEX_M0_ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-haiku-4-5".into());
    let deepseek_model =
        std::env::var("AEX_M0_DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".into());
    let deepseek_provider =
        std::env::var("AEX_M0_DEEPSEEK_PROVIDER").unwrap_or_else(|_| "deepseek".into());
    // Optional base-URL overrides: the dialects are certified by their WIRE format, and a
    // gateway serving the identical format (e.g. Vercel AI Gateway) is a real endpoint with
    // real streaming, tools and usage. Direct provider hosts are simply the empty default.
    let anthropic_base = std::env::var("AEX_M0_ANTHROPIC_BASE_URL").ok();
    let deepseek_base = std::env::var("AEX_M0_DEEPSEEK_BASE_URL").ok();

    // Pre-flight: the gate identifies "the session's VM" as the one non-terminated MicroVM
    // in the dev account, so a stray from a previous run would make it watch (and kill) the
    // wrong incarnation. Refuse to start dirty.
    if let Some((vm, state)) = microvm_of(&control).await? {
        bail!("pre-flight: stray MicroVM {vm} in {state}; terminate it and re-run");
    }

    let mut timings: Vec<(String, u128)> = Vec::new();
    macro_rules! step {
        ($name:expr, $t:expr) => {
            let ms = $t.elapsed().as_millis();
            println!("  {}: {} ms", $name, ms);
            timings.push(($name.to_string(), ms));
        };
    }

    // ---- Pass 1: Anthropic, the full arc. ----------------------------------------------------
    println!("== pass 1: anthropic ({anthropic_model}) ==");
    let t = Instant::now();
    let (status, ses) = api
        .post(
            "/v1/sessions",
            json!({
                "model": { "provider": "anthropic", "name": anthropic_model, "api_key": anthropic_key, "base_url": anthropic_base },
                "system_prompt": "You are a build agent. Use bash to do exactly what is asked; be brief in prose.",
                "metadata": { "purpose": "m0-gate" }
            }),
        )
        .await?;
    ensure!(status == 201, "create failed: {status} {ses}");
    let sid = ses["id"].as_str().context("session id")?.to_string();
    step!("create", t);
    println!("  session {sid}");

    let follower = Follower::start(&api, &sid);

    // Turn 1: a real build in the MicroVM, streamed.
    let t = Instant::now();
    let (_, evs) = run_turn(
        &api,
        &follower,
        &sid,
        "Create hello.c that prints exactly m0-gate-ok, compile it with gcc to ./hello, run ./hello, and show me the output.",
        Duration::from_secs(300),
    )
    .await?;
    step!("turn1_build", t);
    ensure!(
        evs.iter().any(|e| e.kind == "assistant.delta"),
        "no streamed deltas reached the SSE consumer"
    );
    ensure!(
        evs.iter()
            .any(|e| e.kind == "tool.call" && e.data["name"] == json!("bash")),
        "no bash tool call in turn 1"
    );
    let results = tool_results(&evs);
    ensure!(!results.is_empty(), "no tool results in turn 1");
    ensure!(
        results.iter().any(|e| e.data["output_preview"]
            .as_str()
            .unwrap_or("")
            .contains("m0-gate-ok")),
        "build output does not show m0-gate-ok: {:?}",
        results
            .iter()
            .map(|e| &e.data["output_preview"])
            .collect::<Vec<_>>()
    );
    ensure!(
        evs.iter().any(|e| e.kind == "model.usage"
            && e.data["usage"]["input_tokens"].as_u64().unwrap_or(0) > 0),
        "model.usage carries no raw input_tokens"
    );

    // The suspend: stop touching the hand and let the 180 s idle policy fire FOR REAL.
    let t = Instant::now();
    let (vm1, _) = microvm_of(&control)
        .await?
        .context("no microvm after turn 1")?;
    wait_vm_state(&control, &vm1, "SUSPENDED", Duration::from_secs(420)).await?;
    step!("aws_idle_suspend", t);

    // Turn 2: resume on the next message; the compiled binary must still be there (state
    // intact across suspend/resume -- no recompilation).
    let t = Instant::now();
    let (_, evs) = run_turn(
        &api,
        &follower,
        &sid,
        "Run ./hello again (do not recreate or recompile anything) and show the output.",
        Duration::from_secs(240),
    )
    .await?;
    step!("turn2_resume", t);
    ensure!(
        tool_results(&evs).iter().any(|e| e.data["output_preview"]
            .as_str()
            .unwrap_or("")
            .contains("m0-gate-ok")),
        "post-resume run lost the compiled binary"
    );
    let (_, ses_now) = api.get(&format!("/v1/sessions/{sid}")).await?;
    ensure!(
        ses_now["hand"]["generation"] == json!(1),
        "resume must keep the same incarnation, got {}",
        ses_now["hand"]["generation"]
    );

    // Cancellation: a long-running command, cancelled mid-flight.
    let t = Instant::now();
    let (status, acc) = api
        .post(
            &format!("/v1/sessions/{sid}/messages"),
            json!({ "content": "Run: sleep 300 && echo done. Use bash. Do not use a timeout." }),
        )
        .await?;
    ensure!(
        status == 202,
        "cancel-turn message not accepted: {status} {acc}"
    );
    let turn = acc["turn_id"].as_str().context("turn_id")?.to_string();
    follower
        .wait_for(
            "the sleep to start",
            |e| e.kind == "tool.call" && e.data["turn_id"] == json!(turn),
            Duration::from_secs(120),
        )
        .await?;
    let (status, _) = api
        .post_empty(&format!("/v1/sessions/{sid}/cancel"))
        .await?;
    ensure!(status == 200, "cancel failed: {status}");
    let done = follower
        .wait_for(
            "the cancelled turn to complete",
            |e| e.kind == "turn.completed" && e.data["turn_id"] == json!(turn),
            Duration::from_secs(90),
        )
        .await?;
    ensure!(
        done.data["stop_reason"] == json!("cancelled"),
        "expected stop_reason=cancelled, got {}",
        done.data["stop_reason"]
    );
    step!("cancel", t);

    // Persist an artifact.
    let t = Instant::now();
    let (status, art) = api
        .post(
            &format!("/v1/sessions/{sid}/persist"),
            json!({ "name": "hello.c", "path": "/workspace/hello.c" }),
        )
        .await?;
    ensure!(status == 201, "persist failed: {status} {art}");
    ensure!(
        art["bytes"].as_u64().unwrap_or(0) > 0,
        "artifact has no bytes"
    );
    step!("persist", t);

    // The wall, simulated exactly as AWS enforces it: the incarnation dies out from under the
    // session. The last turn-end sync is the restore point.
    let t = Instant::now();
    control
        .terminate(&vm1)
        .await
        .map_err(|e| anyhow::anyhow!("terminate: {e}"))?;
    wait_vm_state(&control, &vm1, "TERMINATED", Duration::from_secs(180)).await?;
    step!("wall_terminate", t);

    // Turn 3: the next message re-materialises a fresh incarnation from the sync,
    // byte-for-byte (the binary compiled in turn 1 must run unmodified).
    let t = Instant::now();
    let (_, evs) = run_turn(
        &api,
        &follower,
        &sid,
        "Run ./hello one more time (again: no recreation, no recompilation) and show the output.",
        Duration::from_secs(300),
    )
    .await?;
    step!("turn3_rematerialise", t);
    ensure!(
        tool_results(&evs).iter().any(|e| e.data["output_preview"]
            .as_str()
            .unwrap_or("")
            .contains("m0-gate-ok")),
        "re-materialised workspace lost the compiled binary"
    );
    let (_, ses_now) = api.get(&format!("/v1/sessions/{sid}")).await?;
    ensure!(
        ses_now["hand"]["generation"] == json!(2),
        "re-materialise must be a second incarnation, got {}",
        ses_now["hand"]["generation"]
    );

    // Replay: a fresh, non-following read of the journal must return every durable event in
    // seq order.
    let t = Instant::now();
    let replay = api
        .http
        .get(format!(
            "{BASE}/v1/sessions/{sid}/events?after=0&follow=false"
        ))
        .bearer_auth(&api.token)
        .send()
        .await?
        .text()
        .await?;
    let mut seqs = Vec::new();
    for line in replay.lines() {
        if let Some(id) = line.strip_prefix("id: ") {
            seqs.push(id.trim().parse::<u64>().unwrap_or(0));
        }
    }
    ensure!(!seqs.is_empty(), "replay returned no events");
    ensure!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "replay seqs not strictly increasing: {seqs:?}"
    );
    ensure!(
        replay.contains("event: turn.completed"),
        "replay is missing turn.completed"
    );
    step!("replay", t);

    // End: hand released, session stays.
    let t = Instant::now();
    let (status, ended) = api.post_empty(&format!("/v1/sessions/{sid}/end")).await?;
    ensure!(status == 200, "end failed: {status} {ended}");
    ensure!(
        ended["hand"]["state"] == json!("released"),
        "end must release the hand: {ended}"
    );
    step!("end", t);

    // ---- Pass 2: DeepSeek over the OpenAI dialect (real key, real endpoint). -----------------
    println!(
        "== pass 2: {deepseek_provider}/{deepseek_model} (openai chat completions dialect) =="
    );
    let t = Instant::now();
    let (status, ses2) = api
        .post(
            "/v1/sessions",
            json!({
                "model": { "provider": deepseek_provider, "name": deepseek_model, "api_key": deepseek_key, "base_url": deepseek_base },
                "system_prompt": "You are a build agent. Use bash to do exactly what is asked; be brief in prose."
            }),
        )
        .await?;
    ensure!(status == 201, "deepseek create failed: {status} {ses2}");
    let sid2 = ses2["id"].as_str().context("session id")?.to_string();
    let follower2 = Follower::start(&api, &sid2);
    let (_, evs) = run_turn(
        &api,
        &follower2,
        &sid2,
        "Use bash to run: echo openai-dialect-ok. Show the output.",
        Duration::from_secs(300),
    )
    .await?;
    step!("deepseek_turn", t);
    ensure!(
        tool_results(&evs).iter().any(|e| e.data["output_preview"]
            .as_str()
            .unwrap_or("")
            .contains("openai-dialect-ok")),
        "deepseek turn produced no tool output"
    );
    ensure!(
        evs.iter().any(|e| e.kind == "model.usage"
            && e.data["usage"]["input_tokens"].as_u64().unwrap_or(0) > 0),
        "deepseek model.usage carries no raw input_tokens"
    );

    // ---- Cleanup: delete both sessions (removes journals, workspaces, VMs). ------------------
    let t = Instant::now();
    ensure!(
        api.delete(&format!("/v1/sessions/{sid2}")).await? == 204,
        "delete 2 failed"
    );
    ensure!(
        api.delete(&format!("/v1/sessions/{sid}")).await? == 204,
        "delete 1 failed"
    );
    let (status, _) = api.get(&format!("/v1/sessions/{sid}")).await?;
    ensure!(status == 404, "deleted session must 404, got {status}");
    step!("delete", t);

    // Leftover-VM check: nothing should still be running.
    if let Some((vm, state)) = microvm_of(&control).await? {
        println!("  warning: leftover vm {vm} in {state}; terminating");
        let _ = control.terminate(&vm).await;
    }

    println!("M0 PASS");
    for (name, ms) in &timings {
        println!("  {name}: {ms} ms");
    }
    Ok(())
}
