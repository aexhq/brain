//! The slice-7 real-wire MCP operator gate: the brain's own MCP client against the OFFICIAL
//! reference server (`@modelcontextprotocol/server-everything`), with a REAL model choosing
//! the calls. The in-test axum servers prove our reading of the spec; this proves interop
//! with the implementation everyone else runs.
//!
//! What a pass proves, over real HTTP end to end:
//!   - `protocol: auto` negotiates with whatever revision the official server speaks today
//!     (2.0.0 is an initialization-era server, so the modern probe's JSON-RPC error must
//!     trigger the legacy fallback rather than fail the create);
//!   - the allowlist filters the server's full catalogue down to the two sealed tools;
//!   - a real model reads the sealed schemas and drives `tools/call` through `McpRuntime`,
//!     and the official server's answers come back as ordinary `tool.result` events.
//!
//! Requirements: BRAIN_MCP_REF_URL (tools/mcp.sh starts the server and sets it) and
//! ANTHROPIC_API_KEY (plus optional BRAIN_MCP_BASE_URL / BRAIN_MCP_MODEL for a gateway).
//! Local mode, no cloud. Ends with `MCP GATE PASS` or a loud failure.

use anyhow::{Context, bail, ensure};
use brain::api::{AppState, serve};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    // Same fix as m0: the linear gate future outgrows the default main stack.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,hyper=warn".into()),
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
    base: String,
    token: String,
}

impl Api {
    async fn post(&self, path: &str, body: Value) -> anyhow::Result<(u16, Value)> {
        let r = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        Ok((status, v))
    }

    /// Polls the events endpoint (`follow=false`) until `pred` holds over the parsed frames.
    async fn wait_for<F: Fn(&[(String, Value)]) -> bool>(
        &self,
        sid: &str,
        what: &str,
        timeout: Duration,
        pred: F,
    ) -> anyhow::Result<Vec<(String, Value)>> {
        let deadline = Instant::now() + timeout;
        loop {
            let text = self
                .http
                .get(format!(
                    "{}/v1/sessions/{sid}/events?after=0&follow=false",
                    self.base
                ))
                .bearer_auth(&self.token)
                .send()
                .await?
                .text()
                .await?;
            let mut events = Vec::new();
            let mut kind = String::new();
            for line in text.lines() {
                if let Some(k) = line.strip_prefix("event: ") {
                    kind = k.to_string();
                } else if let Some(d) = line.strip_prefix("data: ")
                    && let Ok(v) = serde_json::from_str::<Value>(d)
                {
                    events.push((kind.clone(), v));
                }
            }
            if pred(&events) {
                return Ok(events);
            }
            if Instant::now() > deadline {
                let seen: Vec<&String> = events.iter().map(|(k, _)| k).collect();
                bail!("timed out waiting for {what}; events seen: {seen:?}");
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let ref_url = std::env::var("BRAIN_MCP_REF_URL")
        .context("BRAIN_MCP_REF_URL is not set (use tools/mcp.sh)")?;
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY is not set")?;
    let model =
        std::env::var("BRAIN_MCP_MODEL").unwrap_or_else(|_| "claude-haiku-4-5-20251001".into());
    let base_url = std::env::var("BRAIN_MCP_BASE_URL").ok();

    // The brain: local mode in a scratch dir (the gate is about the MCP wire, not the hand),
    // served over real HTTP on an ephemeral port. `Brain::local` composes the outbound guard
    // permissively, which is what lets the reference server live on 127.0.0.1.
    let data_dir = std::env::temp_dir().join(format!("brain-mcp-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let brain =
        Brain::local(&data_dir, BrainConfig::default()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let token = brain::mint_id("tok", 24);
    let base = "http://127.0.0.1:8703".to_string();
    let state = AppState {
        brain: brain.clone(),
        token: token.clone(),
    };
    tokio::spawn(async move {
        serve(state, "127.0.0.1:8703".parse().expect("addr"))
            .await
            .expect("api serve")
    });
    let api = Api {
        http: reqwest::Client::new(),
        base,
        token,
    };
    // Wait for the listener before the first request.
    for _ in 0..50 {
        if api.http.get(&api.base).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Step 1: create. `auto` must land on whatever the official server speaks, the create must
    // survive it, and the allowlist must cut the catalogue to exactly the two sealed tools.
    let t = Instant::now();
    let mut model_cfg = json!({"provider": "anthropic", "name": model, "api_key": api_key});
    if let Some(u) = &base_url {
        model_cfg["base_url"] = json!(u);
    }
    let (status, created) = api
        .post(
            "/v1/sessions",
            json!({
                "model": model_cfg,
                "system_prompt": "You are the Brain MCP gate. Use the tools exactly as asked.",
                "tools": {"items": [], "mcp": [{
                    "name": "ref",
                    "url": ref_url,
                    "protocol": "auto",
                    "allowed_tools": ["echo", "get-sum"]
                }]}
            }),
        )
        .await?;
    ensure!(status == 201, "create failed ({status}): {created}");
    let sid = created["id"]
        .as_str()
        .context("create returned no id")?
        .to_string();
    let head = brain
        .journal
        .get_head(&sid)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let sealed: Vec<String> = head
        .doc
        .prefix
        .mcp_tools
        .iter()
        .map(|d| d.name.clone())
        .collect();
    ensure!(
        sealed == ["ref__echo", "ref__get-sum"],
        "sealed tool set is {sealed:?}, wanted [ref__echo, ref__get-sum]"
    );
    let negotiated = head.doc.prefix.mcp[0].spec_version.clone();
    println!(
        "create+negotiate+seal  {:>6.2}s  negotiated {negotiated}, sealed {sealed:?}",
        t.elapsed().as_secs_f64()
    );

    // Step 2: one turn where the MODEL drives both tools through the official server.
    let t = Instant::now();
    let ask = "Call ref__echo with the message 'brain-mcp-gate-7'. Also call ref__get-sum \
               to add 40 and 2. Then reply with just DONE.";
    let (status, acc) = api
        .post(
            &format!("/v1/sessions/{sid}/messages"),
            json!({"content": ask}),
        )
        .await?;
    ensure!(status == 202, "message not accepted ({status}): {acc}");
    let events = api
        .wait_for(&sid, "turn.completed", Duration::from_secs(120), |evs| {
            evs.iter().any(|(k, _)| k == "turn.completed")
        })
        .await?;
    let result_of = |name: &str| -> anyhow::Result<&Value> {
        events
            .iter()
            .filter(|(k, _)| k == "tool.result")
            .map(|(_, v)| v)
            .find(|v| v["name"] == name)
            .with_context(|| format!("no tool.result for {name}"))
    };
    let echo = result_of("ref__echo")?;
    ensure!(
        echo["outcome"] == "completed"
            && echo["output_preview"]
                .as_str()
                .unwrap_or("")
                .contains("brain-mcp-gate-7"),
        "echo result wrong: {echo}"
    );
    let sum = result_of("ref__get-sum")?;
    ensure!(
        sum["outcome"] == "completed"
            && sum["output_preview"].as_str().unwrap_or("").contains("42"),
        "get-sum result wrong: {sum}"
    );
    println!(
        "model turn, 2 real MCP calls  {:>6.2}s  echo + get-sum=42 via the official server",
        t.elapsed().as_secs_f64()
    );

    // Cleanup.
    let del = api
        .http
        .delete(format!("{}/v1/sessions/{sid}", api.base))
        .bearer_auth(&api.token)
        .send()
        .await?
        .status()
        .as_u16();
    ensure!(del == 204, "delete failed: {del}");
    let _ = std::fs::remove_dir_all(&data_dir);
    println!(
        "MCP GATE PASS (negotiated {negotiated}, model {})",
        head.doc.prefix.model
    );
    Ok(())
}
