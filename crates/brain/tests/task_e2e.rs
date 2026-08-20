//! Slice-8 gate: in-process `task` subagents over the real session HTTP API.
//!
//! Only the model is scripted. Journal, SSE, local hand calls, MCP calls,
//! cancellation, discard, and rehydrate all use their production seams.

mod support;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use brain::adapter::ToolExecutor;
use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::journal::{Entry, Journal, Lease, Record};
use brain::local::LocalFactory;
use brain::message::{ContentBlock, Message, StopReason, Usage};
use brain::provider::{ModelRequest, Provider, ProviderEvent};
use brain::session::{Brain, BrainConfig};
use brain::{BrainError, Result};
use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Barrier, Notify};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
enum Reply {
    Text(String),
    Tools(Vec<(String, Value)>),
}

#[derive(Debug, Clone)]
struct Arrival {
    seed: String,
    assistant_count: usize,
}

/// A request-driven provider: every response is a pure function of the current
/// agent's seed prompt and round count, so parallel children cannot steal one
/// another's queued response.
#[derive(Debug)]
struct TaskProvider {
    calls: AtomicU64,
    arrivals: Mutex<Vec<Arrival>>,
    parallel_barrier: Barrier,
    parallel_arrivals: AtomicU64,
    cancel_started: Notify,
}

impl TaskProvider {
    fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            arrivals: Mutex::new(Vec::new()),
            parallel_barrier: Barrier::new(2),
            parallel_arrivals: AtomicU64::new(0),
            cancel_started: Notify::new(),
        }
    }

    fn arrival(req: &ModelRequest) -> Result<Arrival> {
        let body: Value = serde_json::from_slice(&req.body)?;
        let messages = body["messages"]
            .as_array()
            .ok_or_else(|| BrainError::Protocol("scripted request has no messages".into()))?;
        let known_calls: std::collections::HashSet<&str> = messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "tool_use")
            .filter_map(|block| block["id"].as_str())
            .collect();
        for result_id in messages
            .iter()
            .filter(|message| message["role"] == "user")
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .filter_map(|block| block["tool_use_id"].as_str())
        {
            if !known_calls.contains(result_id) {
                return Err(BrainError::Protocol(format!(
                    "tool result {result_id} has no matching assistant tool use"
                )));
            }
        }
        let mut anchor = None;
        let mut seed = None;
        for (idx, message) in messages.iter().enumerate().rev() {
            if message["role"] != "user" {
                continue;
            }
            let text = message["content"].as_array().and_then(|blocks| {
                blocks.iter().rev().find_map(|block| {
                    (block["type"] == "text")
                        .then(|| block["text"].as_str())
                        .flatten()
                })
            });
            if let Some(text) = text {
                anchor = Some(idx);
                seed = Some(text.to_string());
                break;
            }
        }
        let anchor = anchor.ok_or_else(|| BrainError::Protocol("no seed prompt".into()))?;
        let assistant_count = messages[anchor + 1..]
            .iter()
            .filter(|message| message["role"] == "assistant")
            .count();
        Ok(Arrival {
            seed: seed.expect("found with anchor"),
            assistant_count,
        })
    }

    fn task(prompt: &str) -> (String, Value) {
        (
            "task".into(),
            json!({
                "description": format!("delegate {prompt}"),
                "prompt": prompt
            }),
        )
    }

    fn policy(&self, arrival: &Arrival) -> Result<Reply> {
        let seed = arrival.seed.as_str();
        let round = arrival.assistant_count;
        let reply = match (seed, round) {
            ("nested-root", 0) => Reply::Tools(vec![Self::task("nested-1")]),
            ("nested-1", 0) => Reply::Tools(vec![Self::task("nested-2")]),
            ("nested-2", 0) => Reply::Tools(vec![Self::task("nested-3")]),
            ("nested-3", 0) => Reply::Tools(vec![Self::task("nested-4-refused")]),
            ("nested-root", _) => Reply::Text("root saw nested report".into()),
            (seed, _) if seed.starts_with("nested-") => {
                Reply::Text(format!("{seed} handled its child result"))
            }

            ("parallel-root", 0) => {
                Reply::Tools(vec![Self::task("parallel-a"), Self::task("parallel-b")])
            }
            ("parallel-root", _) => Reply::Text("parallel reports received".into()),
            ("parallel-a" | "parallel-b", _) => Reply::Text(format!("{seed} done")),

            ("tools-root", 0) => Reply::Tools(vec![Self::task("tools-worker")]),
            ("tools-root", _) => Reply::Text("worker tools verified".into()),
            ("tools-worker", 0) => Reply::Tools(vec![
                (
                    "write".into(),
                    json!({"path":"from-child.txt","content":"child-hand-ok"}),
                ),
                ("svc__echo".into(), json!({"msg":"child-mcp-ok"})),
                ("host_echo".into(), json!({"msg":"child-server-ok"})),
            ]),
            ("tools-worker", _) => Reply::Text("used hand, mcp, and server tools".into()),

            ("failure-root", 0) => Reply::Tools(vec![Self::task("failure-worker")]),
            ("failure-root", _) => Reply::Text("root recovered from child failure".into()),
            ("failure-worker", _) => {
                return Err(BrainError::Protocol("scripted child failure".into()));
            }

            ("cancel-root", 0) => Reply::Tools(vec![Self::task("cancel-worker")]),
            ("cancel-root", _) => Reply::Text("must not run after cancellation".into()),

            ("cap-root", 0) => Reply::Tools(
                (0..13)
                    .map(|idx| Self::task(&format!("cap-child-{idx}")))
                    .collect(),
            ),
            ("cap-root", _) => Reply::Text("cap results received".into()),
            (seed, _) if seed.starts_with("cap-child-") => Reply::Text(format!("{seed} done")),

            ("cap-fill-root", 0) => Reply::Tools(
                (0..12)
                    .map(|idx| Self::task(&format!("cap-fill-child-{idx}")))
                    .collect(),
            ),
            ("cap-fill-root", _) => Reply::Text("identity budget filled".into()),
            (seed, _) if seed.starts_with("cap-fill-child-") => Reply::Text(format!("{seed} done")),
            ("cap-after-root", 0) => Reply::Tools(vec![Self::task("must-not-start")]),
            ("cap-after-root", _) => Reply::Text("persisted cap verified".into()),
            ("must-not-start", _) => {
                return Err(BrainError::Protocol(
                    "identity-capped child reached the provider".into(),
                ));
            }

            ("recovered-root", _) => Reply::Text("continued after interruption repair".into()),

            other => {
                return Err(BrainError::Protocol(format!(
                    "no task test policy for {other:?}"
                )));
            }
        };
        Ok(reply)
    }

    fn emit(reply: Reply) -> BoxStream<'static, Result<ProviderEvent>> {
        let stream = async_stream::stream! {
            match reply {
                Reply::Text(text) => {
                    yield Ok(ProviderEvent::TextDelta { index: 0, text });
                    yield Ok(ProviderEvent::MessageDone {
                        stop_reason: StopReason::EndTurn,
                        usage: zero_usage(),
                    });
                }
                Reply::Tools(tools) => {
                    for (idx, (name, input)) in tools.into_iter().enumerate() {
                        yield Ok(ProviderEvent::ToolUseStart {
                            index: idx + 1,
                            id: format!("provider-call-{idx}-{name}"),
                            name,
                        });
                        yield Ok(ProviderEvent::ToolInputDelta {
                            index: idx + 1,
                            partial_json: serde_json::to_string(&input).unwrap(),
                        });
                        yield Ok(ProviderEvent::BlockDone { index: idx + 1 });
                    }
                    yield Ok(ProviderEvent::MessageDone {
                        stop_reason: StopReason::ToolUse,
                        usage: zero_usage(),
                    });
                }
            }
        };
        Box::pin(stream)
    }
}

fn zero_usage() -> Usage {
    Usage {
        input_tokens: Some(0),
        output_tokens: Some(0),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reasoning_tokens: None,
    }
}

#[async_trait::async_trait]
impl Provider for TaskProvider {
    fn dialect(&self) -> Dialect {
        Dialect::AnthropicMessages
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        brain::provider::anthropic::Anthropic.build_request(prefix, history, key, base_url)
    }

    async fn stream(&self, req: ModelRequest) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let arrival = Self::arrival(&req)?;
        self.arrivals.lock().unwrap().push(arrival.clone());

        if matches!(arrival.seed.as_str(), "parallel-a" | "parallel-b")
            && arrival.assistant_count == 0
        {
            self.parallel_arrivals.fetch_add(1, Ordering::SeqCst);
            tokio::time::timeout(Duration::from_secs(3), self.parallel_barrier.wait())
                .await
                .map_err(|_| BrainError::Protocol("parallel children were serialized".into()))?;
        }
        if arrival.seed == "cancel-worker" && arrival.assistant_count == 0 {
            self.cancel_started.notify_one();
            std::future::pending::<()>().await;
            unreachable!("cancelled provider future must be dropped");
        }
        Ok(Self::emit(self.policy(&arrival)?))
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "brain-task-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
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
struct ChildServer {
    calls: Mutex<Vec<ExternalToolCallRequest>>,
}

#[async_trait::async_trait]
impl ToolExecutor for ChildServer {
    fn supports(&self, capability: &str) -> bool {
        capability == "test.child_echo"
    }

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        assert_eq!(capability, "test.child_echo");
        self.calls.lock().unwrap().push(request);
        Ok(serde_json::from_value(json!({
            "outcome": "completed",
            "content": "child server result",
            "result": "child server result",
            "is_error": false,
            "disposition": "continue"
        }))?)
    }
}

struct Harness {
    journal: Journal,
    provider: Arc<TaskProvider>,
    base: String,
    token: String,
    http: reqwest::Client,
    tmp: TempDir,
    server: Arc<ChildServer>,
}

impl Harness {
    async fn new(idle_discard: Duration, outbound_allow_private: bool) -> Self {
        let tmp = TempDir::new();
        let provider = Arc::new(TaskProvider::new());
        let provider_factory = provider.clone();
        let server = Arc::new(ChildServer::default());
        let journal = Journal::new_memory(format!("task-e2e-{}", std::process::id()));
        let brain = Brain::with_parts_and_external(
            BrainConfig {
                max_concurrent_model_rounds: 32,
                max_concurrent_turns: 8,
                idle_discard,
                outbound_allow_private,
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(brain::keys::PlainCustody),
            Arc::new(LocalFactory::new(tmp.0.clone())),
            server.clone(),
            Some(Arc::new(move |_| {
                provider_factory.clone() as Arc<dyn Provider>
            })),
        );
        let token = "task-e2e-token".to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = brain::api::router(brain::api::AppState {
            brain: brain.clone(),
            token: token.clone(),
        });
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            journal,
            provider,
            base,
            token,
            http: reqwest::Client::new(),
            tmp,
            server,
        }
    }

    async fn create(&self, tools: Option<Value>) -> String {
        let mut body = json!({
            "model": {
                "provider": "anthropic",
                "name": "scripted",
                "api_key": "sk-fake"
            },
            "system_prompt": "task e2e agent"
        });
        body["tools"] = tools.unwrap_or_else(|| json!({"items": [support::subagents_tool()]}));
        let response = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        let value: Value = serde_json::from_str(&text)
            .unwrap_or_else(|_| panic!("create returned {status} with non-JSON body: {text:?}"));
        assert_eq!(status, reqwest::StatusCode::CREATED, "{value}");
        value["id"].as_str().unwrap().to_string()
    }

    async fn send(&self, session_id: &str, content: &str) {
        let response = self
            .http
            .post(format!("{}/v1/sessions/{session_id}/messages", self.base))
            .bearer_auth(&self.token)
            .json(&json!({"content": content}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::ACCEPTED,
            "{}",
            response.text().await.unwrap()
        );
    }

    async fn cancel(&self, session_id: &str) {
        let response = self
            .http
            .post(format!("{}/v1/sessions/{session_id}/cancel", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }

    async fn records(&self, session_id: &str) -> Vec<Entry> {
        self.journal.read_records(session_id, 0).await.unwrap()
    }

    async fn wait_completed(&self, session_id: &str, count: usize) -> Vec<(String, Value)> {
        self.wait_for(session_id, "turn.completed", |events| {
            events
                .iter()
                .filter(|(kind, _)| kind == "turn.completed")
                .count()
                >= count
        })
        .await
    }

    async fn wait_for<F>(&self, session_id: &str, what: &str, predicate: F) -> Vec<(String, Value)>
    where
        F: Fn(&[(String, Value)]) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let text = self
                .http
                .get(format!(
                    "{}/v1/sessions/{session_id}/events?after=0&follow=false",
                    self.base
                ))
                .bearer_auth(&self.token)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            let mut events = Vec::new();
            let mut kind = String::new();
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("event: ") {
                    kind = value.to_string();
                } else if let Some(value) = line.strip_prefix("data: ")
                    && let Ok(value) = serde_json::from_str(value)
                {
                    events.push((kind.clone(), value));
                }
            }
            if predicate(&events) {
                return events;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; saw {events:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn child_agents(records: &[Entry]) -> Vec<String> {
    let mut agents: Vec<String> = records
        .iter()
        .filter_map(|entry| entry.record.agent())
        .filter(|agent| *agent != "root")
        .map(str::to_string)
        .collect();
    agents.sort();
    agents.dedup();
    agents
}

fn task_results<'a>(records: &'a [Entry], agent: &str) -> Vec<&'a Record> {
    records
        .iter()
        .filter_map(|entry| match &entry.record {
            record @ Record::ToolResult {
                agent: record_agent,
                name,
                ..
            } if record_agent == agent && name == "task" => Some(record),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Tiny 2026-07 MCP server used by the hand+MCP child test.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct McpState {
    calls: Mutex<Vec<Value>>,
}

async fn mcp_handler(
    State(state): State<Arc<McpState>>,
    _headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request: Value = serde_json::from_slice(&body).unwrap();
    let id = request["id"].clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "tools/list" => json!({
            "tools": [{
                "name": "echo",
                "description": "echo text",
                "inputSchema": {
                    "type": "object",
                    "properties": {"msg": {"type": "string"}},
                    "required": ["msg"]
                }
            }]
        }),
        "tools/call" => {
            state.calls.lock().unwrap().push(request["params"].clone());
            let message = request["params"]["arguments"]["msg"].as_str().unwrap_or("");
            json!({"content":[{"type":"text","text":format!("echo:{message}")}]})
        }
        _ => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "error":{"code":-32601,"message":"method not found"}
                })),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        axum::Json(json!({"jsonrpc":"2.0","id":id,"result":result})),
    )
        .into_response()
}

async fn serve_mcp() -> (String, Arc<McpState>) {
    let state = Arc::new(McpState::default());
    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{address}/mcp"), state)
}

// ---------------------------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn depth_parallel_and_identity_caps_are_enforced() {
    let harness = Harness::new(Duration::from_secs(300), false).await;

    let nested = harness.create(None).await;
    harness.send(&nested, "nested-root").await;
    harness.wait_completed(&nested, 1).await;
    let records = harness.records(&nested).await;
    assert_eq!(child_agents(&records).len(), 3, "depths 1, 2, and 3");
    assert!(records.iter().any(|entry| {
        matches!(
            &entry.record,
            Record::ToolResult { agent, name, content, is_error: true, .. }
                if agent != "root" && name == "task" && content.contains("depth limit")
        )
    }));

    let parallel = harness.create(None).await;
    harness.send(&parallel, "parallel-root").await;
    harness.wait_completed(&parallel, 1).await;
    let records = harness.records(&parallel).await;
    assert_eq!(child_agents(&records).len(), 2);
    assert_eq!(
        harness.provider.parallel_arrivals.load(Ordering::SeqCst),
        2,
        "the barrier proves both child rounds overlapped"
    );

    let capped = harness.create(None).await;
    harness.send(&capped, "cap-root").await;
    harness.wait_completed(&capped, 1).await;
    let records = harness.records(&capped).await;
    let results = task_results(&records, "root");
    assert_eq!(results.len(), 13);
    assert_eq!(child_agents(&records).len(), 12);
    assert_eq!(
        results
            .iter()
            .filter(|record| matches!(record, Record::ToolResult { is_error: true, content, .. } if content.contains("identity limit")))
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_child_uses_the_hand_and_mcp_and_failure_stays_local() {
    let (mcp_url, mcp) = serve_mcp().await;
    let harness = Harness::new(Duration::from_secs(300), true).await;
    let session = harness
        .create(Some(json!({
            "items": [
                support::hand_tool("write"),
                support::subagents_tool(),
                {
                    "definition": {
                        "name": "host_echo",
                        "description": "Echo through the host executor.",
                        "input_schema": {"type": "object"},
                        "output_schema": {"type": "string"}
                    },
                    "executor": {
                        "kind": "server",
                        "capability": "test.child_echo",
                        "scope": "all",
                        "completion": "continue",
                        "effect": "replay_safe",
                        "max_input_bytes": 1024
                    }
                }
            ],
            "mcp": [{
                "name": "svc",
                "url": mcp_url,
                "protocol": "2026-07",
                "allowed_tools": ["echo"]
            }]
        })))
        .await;
    harness.send(&session, "tools-root").await;
    harness.wait_completed(&session, 1).await;

    assert_eq!(
        std::fs::read_to_string(
            harness
                .tmp
                .0
                .join(&session)
                .join("workspace")
                .join("from-child.txt")
        )
        .unwrap(),
        "child-hand-ok"
    );
    let calls = mcp.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["name"], "echo");
    let records = harness.records(&session).await;
    let child = child_agents(&records);
    assert_eq!(child.len(), 1);
    for name in ["write", "svc__echo", "host_echo"] {
        assert!(records.iter().any(|entry| {
            matches!(
                &entry.record,
                Record::ToolResult { agent, name: record_name, is_error: false, .. }
                    if agent == &child[0] && record_name == name
            )
        }));
    }
    {
        let server_calls = harness.server.calls.lock().unwrap();
        assert_eq!(server_calls.len(), 1);
        assert_ne!(server_calls[0].agent_id.as_str(), "root");
        assert!(server_calls[0].agent_id.as_str().starts_with("agt_"));
        assert_eq!(server_calls[0].name, "host_echo");
    }

    let failed = harness.create(None).await;
    harness.send(&failed, "failure-root").await;
    let events = harness.wait_completed(&failed, 1).await;
    let records = harness.records(&failed).await;
    assert!(matches!(
        task_results(&records, "root").as_slice(),
        [Record::ToolResult { is_error: true, content, .. }]
            if content.contains("scripted child failure")
    ));
    assert!(
        events.iter().all(|(kind, _)| kind != "turn.failed"),
        "a child failure is only the parent's task result"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_drains_a_child_as_a_failed_task_result() {
    let harness = Harness::new(Duration::from_secs(300), false).await;
    let session = harness.create(None).await;
    harness.send(&session, "cancel-root").await;
    tokio::time::timeout(
        Duration::from_secs(5),
        harness.provider.cancel_started.notified(),
    )
    .await
    .expect("child entered its model round");
    harness.cancel(&session).await;
    let events = harness.wait_completed(&session, 1).await;
    let records = harness.records(&session).await;
    assert!(matches!(
        task_results(&records, "root").as_slice(),
        [Record::ToolResult { outcome, is_error: true, .. }] if outcome == "cancelled"
    ));
    assert!(
        events.iter().any(|(kind, value)| {
            kind == "turn.completed" && value["stop_reason"] == "cancelled"
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_identity_cap_and_child_attribution_survive_cold_rehydrate() {
    let harness = Harness::new(Duration::from_millis(100), false).await;
    let session = harness.create(None).await;
    harness.send(&session, "cap-fill-root").await;
    harness.wait_completed(&session, 1).await;
    let before = harness.records(&session).await;
    assert_eq!(child_agents(&before).len(), 12);

    // Let the session actor discard its fold, adapter, and in-memory identity
    // counter. The next message must claim and rebuild from the journal.
    tokio::time::sleep(Duration::from_millis(350)).await;
    harness.send(&session, "cap-after-root").await;
    harness.wait_completed(&session, 2).await;
    let after = harness.records(&session).await;
    assert_eq!(
        child_agents(&after).len(),
        12,
        "the 13th child did not start"
    );
    let last_root_task = task_results(&after, "root").pop().unwrap();
    assert!(matches!(
        last_root_task,
        Record::ToolResult { is_error: true, content, .. }
            if content.contains("identity limit")
    ));

    // Every non-root journal record and SSE event uses a parseable agt_ id.
    assert!(
        child_agents(&after)
            .iter()
            .all(|agent| agent.starts_with("agt_"))
    );
    let events = harness.wait_completed(&session, 2).await;
    assert!(events.iter().any(|(_, value)| {
        value["agent_id"]
            .as_str()
            .is_some_and(|agent| agent.starts_with("agt_"))
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_hydrate_answers_an_interrupted_task_without_replaying_it() {
    let harness = Harness::new(Duration::from_millis(75), false).await;
    let session = harness.create(None).await;

    // Let the eager actor release its lease, then emulate the durable point at
    // which a process died: root assistant + task intent committed, no result.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let writer = harness.journal.cloned_as("task-crash-writer");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut head = loop {
        match writer.claim(&session).await {
            Ok(head) => break head,
            Err(BrainError::Fenced) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => panic!("failed to claim crash fixture: {error}"),
        }
    };
    let turn = brain::mint_id("trn", 24);
    let call = brain::mint_id("op", 16);
    let mut seq = head.last_seq + 1;
    let mut records = Vec::new();
    records.push((
        seq,
        Record::UserMessage {
            turn: turn.clone(),
            content: vec![ContentBlock::text("crash-root")],
            metadata: std::collections::HashMap::new(),
            idempotency_key_hash: None,
            request_hash: None,
        },
    ));
    seq += 1;
    records.push((seq, Record::TurnStarted { turn: turn.clone() }));
    seq += 1;
    records.push((
        seq,
        Record::Assistant {
            turn: turn.clone(),
            agent: "root".into(),
            content: vec![ContentBlock::ToolUse {
                id: call.clone(),
                name: "task".into(),
                input: json!({
                    "description":"interrupted child",
                    "prompt":"must never be replayed"
                }),
            }],
            stop: StopReason::ToolUse,
        },
    ));
    seq += 1;
    records.push((
        seq,
        Record::ToolCall {
            turn: turn.clone(),
            agent: "root".into(),
            call: call.clone(),
            name: "task".into(),
            input: json!({
                "description":"interrupted child",
                "prompt":"must never be replayed"
            }),
            detach: false,
        },
    ));
    seq += 1;
    head.doc.state = "active".into();
    head.doc.turn = Some(turn.clone());
    head.doc.turns += 1;
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
    };
    writer
        .commit(&session, &mut lease, &records, &head.doc, seq - 1)
        .await
        .unwrap();
    writer.release(&session, &lease).await.unwrap();

    harness.send(&session, "recovered-root").await;
    harness.wait_completed(&session, 2).await;
    let records = harness.records(&session).await;
    let repaired: Vec<_> = records
        .iter()
        .filter_map(|entry| match &entry.record {
            record @ Record::ToolResult {
                call: record_call,
                name,
                ..
            } if record_call == &call && name == "task" => Some(record),
            _ => None,
        })
        .collect();
    assert!(matches!(
        repaired.as_slice(),
        [Record::ToolResult {
            agent,
            outcome,
            content,
            is_error: true,
            ..
        }] if agent == "root"
            && outcome == "interrupted"
            && content.contains("call was not replayed")
    ));
    assert!(records.iter().any(|entry| {
        matches!(
            &entry.record,
            Record::TurnCompleted { turn: record_turn, stop_reason, .. }
                if record_turn == &turn && stop_reason == "interrupted"
        )
    }));
    assert!(
        harness
            .provider
            .arrivals
            .lock()
            .unwrap()
            .iter()
            .all(|arrival| arrival.seed != "must never be replayed")
    );
}
