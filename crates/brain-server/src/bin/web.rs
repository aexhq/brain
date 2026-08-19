//! Slice-10 operator gate: a real model drives managed Serper search and guarded fetch through
//! the complete session HTTP surface. Credentials come from the process environment and are
//! never printed. Ends with `WEB GATE PASS` or a non-zero exit.

use anyhow::{Context, bail, ensure};
use brain::api::{AppState, serve};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,hyper=warn".into()),
                )
                .init();
            tokio::runtime::Runtime::new()?.block_on(run())
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
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let value = response.json().await.unwrap_or(Value::Null);
        Ok((status, value))
    }

    async fn wait_completed(&self, session: &str) -> anyhow::Result<Vec<Value>> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let text = self
                .http
                .get(format!(
                    "{}/v1/sessions/{session}/events?after=0&follow=false",
                    self.base
                ))
                .bearer_auth(&self.token)
                .send()
                .await?
                .text()
                .await?;
            let events: Vec<Value> = text
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|data| serde_json::from_str(data).ok())
                .collect();
            if events
                .iter()
                .any(|event| event["type"] == "turn.completed" || event["type"] == "turn.failed")
            {
                return Ok(events);
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for the managed-web turn");
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

async fn run() -> anyhow::Result<()> {
    ensure!(
        std::env::var("SERPER_API_KEY").is_ok(),
        "SERPER_API_KEY is not set"
    );
    let (provider_key, base_url, model) = if let Ok(key) =
        std::env::var("VERCEL_AI_GATEWAY_API_KEY")
    {
        (
            key,
            Some(
                std::env::var("AEX_WEB_MODEL_BASE_URL")
                    .unwrap_or_else(|_| "https://ai-gateway.vercel.sh".into()),
            ),
            std::env::var("AEX_WEB_MODEL").unwrap_or_else(|_| "anthropic/claude-haiku-4.5".into()),
        )
    } else {
        (
            std::env::var("ANTHROPIC_API_KEY")
                .context("VERCEL_AI_GATEWAY_API_KEY or ANTHROPIC_API_KEY is required")?,
            std::env::var("AEX_WEB_MODEL_BASE_URL").ok(),
            std::env::var("AEX_WEB_MODEL").unwrap_or_else(|_| "claude-haiku-4-5-20251001".into()),
        )
    };

    let data_dir = std::env::temp_dir().join(format!("aex-web-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let brain = Brain::local(&data_dir, BrainConfig::default())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let token = brain::mint_id("tok", 24);
    let address = "127.0.0.1:8704".parse().expect("web gate address");
    let base = "http://127.0.0.1:8704".to_string();
    let state = AppState {
        brain,
        token: token.clone(),
    };
    tokio::spawn(async move { serve(state, address).await });
    let api = Api {
        http: reqwest::Client::new(),
        base,
        token,
    };
    for _ in 0..50 {
        if api
            .http
            .get(format!("{}/healthz", api.base))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let mut model_config = json!({
        "provider":"anthropic",
        "name": model,
        "api_key": provider_key
    });
    if let Some(base_url) = base_url {
        model_config["base_url"] = base_url.into();
    }
    let (status, created) = api
        .post(
            "/v1/sessions",
            json!({
                "model": model_config,
                "system_prompt":"You are an operator gate. Follow the requested tool calls exactly.",
                "tools":{"builtin":["web_search", "web_fetch"]}
            }),
        )
        .await?;
    ensure!(status == 201, "create failed ({status}): {created}");
    let session = created["id"]
        .as_str()
        .context("create returned no session id")?
        .to_string();
    let (status, accepted) = api
        .post(
            &format!("/v1/sessions/{session}/messages"),
            json!({"content":"First call web_search for `IANA example domain`. Then call web_fetch for https://example.com/. After both results, reply DONE."}),
        )
        .await?;
    ensure!(status == 202, "message failed ({status}): {accepted}");
    let events = api.wait_completed(&session).await?;
    for name in ["web_search", "web_fetch"] {
        let result = events
            .iter()
            .find(|event| event["type"] == "tool.result" && event["name"] == name)
            .with_context(|| format!("model did not call {name}"))?;
        ensure!(
            result["outcome"] == "completed",
            "{name} did not complete: {result}"
        );
    }
    let deleted = api
        .http
        .delete(format!("{}/v1/sessions/{session}", api.base))
        .bearer_auth(&api.token)
        .send()
        .await?
        .status();
    ensure!(deleted == reqwest::StatusCode::NO_CONTENT, "delete failed");
    let _ = std::fs::remove_dir_all(&data_dir);
    println!("WEB GATE PASS (real model, managed search, guarded fetch)");
    Ok(())
}
