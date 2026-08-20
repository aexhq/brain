//! Generic host-executed terminal-tool gate. The model and host executor are scripted; session
//! admission, provider requests, journal decisions, SSE replay, and turn termination are real.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use brain::Result;
use brain::adapter::ToolExecutor;
use brain::config::Dialect;
use brain::journal::Journal;
use brain::local::LocalFactory;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig};
use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "brain-external-e2e-{}-{}",
            std::process::id(),
            brain::wall_ms()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct RepairExecutor {
    requests: Mutex<Vec<Value>>,
}

#[async_trait::async_trait]
impl ToolExecutor for RepairExecutor {
    fn supports(&self, capability: &str) -> bool {
        matches!(capability, "test.lookup" | "test.submit_result")
    }

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        let mut requests = self.requests.lock().unwrap();
        requests.push(serde_json::to_value(&request).unwrap());
        let submission = requests
            .iter()
            .filter(|request| request["name"] == "submit_result")
            .count();
        let response = if capability == "test.lookup" {
            json!({
                "outcome": "completed",
                "content": "ordinary tool result",
                "is_error": false,
                "disposition": "continue",
                "result": "ordinary tool result"
            })
        } else if submission == 1 {
            json!({
                "outcome": "failed",
                "content": "answer must be an integer; correct it and submit again",
                "is_error": true,
                "disposition": "continue"
            })
        } else {
            json!({
                "outcome": "completed",
                "content": "accepted",
                "is_error": false,
                "disposition": "complete_turn",
                "result": request.input,
                "result_metadata": {
                    "request_id": "req_12345678901234567890",
                    "schema_revision": "v1"
                }
            })
        };
        Ok(serde_json::from_value(response).unwrap())
    }
}

async fn replay_events(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    session: &str,
) -> Vec<Value> {
    let response = client
        .get(format!(
            "{base}/v1/sessions/{session}/events?after=0&follow=false"
        ))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    response
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

#[tokio::test]
async fn repair_then_return_direct_completes_without_an_extra_model_round() {
    let temp = TempDir::new();
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::ToolCalls(vec![(
            "call_lookup".into(),
            "lookup".into(),
            json!({"topic": "ordinary work"}),
        )]),
        Scripted::ToolCalls(vec![(
            "call_invalid".into(),
            "submit_result".into(),
            json!({"answer": "wrong"}),
        )]),
        Scripted::ToolCalls(vec![(
            "call_valid".into(),
            "submit_result".into(),
            json!({"answer": 42}),
        )]),
    ]);
    let executor = Arc::new(RepairExecutor::default());
    let provider = fake.clone();
    let brain = Brain::with_parts_and_external(
        BrainConfig {
            max_concurrent_model_rounds: 4,
            max_concurrent_turns: 4,
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        Journal::new_memory("brain-external-test"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(temp.0.clone())),
        executor.clone(),
        Some(Arc::new(move |_| {
            provider.clone() as Arc<dyn brain::provider::Provider>
        })),
    );

    let token = "external-e2e-token";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.into(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"items": [
                {
                    "definition": {
                        "name": "lookup",
                        "description": "Perform ordinary work before the final submission",
                        "input_schema": {"type": "object", "additionalProperties": true},
                        "output_schema": {"type": "string"}
                    },
                    "executor": {
                        "kind": "server",
                        "capability": "test.lookup",
                        "scope": "all",
                        "completion": "continue",
                        "effect": "replay_safe",
                        "max_input_bytes": 98304
                    }
                },
                {
                    "definition": {
                        "name": "submit_result",
                        "description": "Submit a result",
                        "input_schema": {"type": "object", "additionalProperties": true},
                        "output_schema": {"type": "object"}
                    },
                    "executor": {
                        "kind": "server",
                        "capability": "test.submit_result",
                        "scope": "root",
                        "completion": "return_direct",
                        "effect": "replay_safe",
                        "max_input_bytes": 98304
                    }
                }
            ]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{}", response.text().await.unwrap());
    let session: Value = response.json().await.unwrap();
    let session_id = session["id"].as_str().unwrap();

    let response = client
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(token)
        .json(&json!({
            "content": "Return the requested object.",
            "metadata": {"host.request_id": "req_12345678901234567890"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202, "{}", response.text().await.unwrap());

    let deadline = Instant::now() + Duration::from_secs(10);
    let completed = loop {
        let events = replay_events(&client, &base, token, session_id).await;
        if let Some(event) = events
            .into_iter()
            .find(|event| event["type"] == "turn.completed")
        {
            break event;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for turn completion"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };

    assert_eq!(completed["result"]["name"], "submit_result");
    assert_eq!(completed["result"]["value"], json!({"answer": 42}));
    assert_eq!(completed["tool_calls"], 3);
    fake.assert_drained(3, "ordinary work plus external terminal result")
        .unwrap();
    let requests = executor.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0]["context"]["host.request_id"],
        "req_12345678901234567890"
    );
    assert_eq!(requests[0]["agent_id"], "root");
}
