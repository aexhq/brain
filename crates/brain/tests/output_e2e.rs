//! Typed output over the real HTTP/SSE/journal path with only the provider scripted.

use brain::Result;
use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::journal::{Journal, Record};
use brain::local::LocalFactory;
use brain::message::Message;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::provider::{ModelRequest, OutputControl, OutputMode, Provider, ProviderEvent};
use brain::session::{Brain, BrainConfig};
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct TempDir(PathBuf);

#[derive(Debug)]
struct AuditProvider {
    inner: Arc<FakeProvider>,
    bodies: Mutex<Vec<Value>>,
}

#[async_trait::async_trait]
impl Provider for AuditProvider {
    fn dialect(&self) -> Dialect {
        self.inner.dialect()
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        self.inner.build_request(prefix, history, key, base_url)
    }

    fn build_output_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
        control: &OutputControl,
        mode: OutputMode,
    ) -> Result<ModelRequest> {
        self.inner
            .build_output_request(prefix, history, key, base_url, control, mode)
    }

    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        self.bodies
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&request.body)?);
        self.inner.stream(request).await
    }
}

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aex-output-e2e-{}-{}",
            std::process::id(),
            brain::mint_id("t", 8)
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

async fn events(http: &reqwest::Client, base: &str, token: &str, session: &str) -> Vec<Value> {
    let text = http
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
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

async fn wait_terminal(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    session: &str,
    output: &str,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let all = events(http, base, token, session).await;
        if let Some(event) = all.into_iter().find(|event| {
            event["output_id"] == output
                && matches!(
                    event["type"].as_str(),
                    Some("output.completed" | "output.failed")
                )
        }) {
            return event;
        }
        assert!(Instant::now() < deadline, "timed out waiting for output");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_turn_terminal(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    session: &str,
    turn: &str,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let all = events(http, base, token, session).await;
        if let Some(event) = all.into_iter().find(|event| {
            event["turn_id"] == turn
                && matches!(
                    event["type"].as_str(),
                    Some("turn.completed" | "turn.failed")
                )
        }) {
            return event;
        }
        assert!(Instant::now() < deadline, "timed out waiting for turn");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn output_is_typed_private_repairable_and_idempotent() {
    let temp = TempDir::new();
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        // Normal work phase.
        Scripted::Text("The researched answer is forty-two.".into()),
        // Private commit: wrong type, followed by the one bounded repair.
        Scripted::Text(r#"{"answer":"42"}"#.into()),
        Scripted::Text(r#"{"answer":42}"#.into()),
        // The session remains usable after output commits.
        Scripted::Text("Still here.".into()),
    ]);
    let audit = Arc::new(AuditProvider {
        inner: fake.clone(),
        bodies: Mutex::new(Vec::new()),
    });
    let factory_audit = audit.clone();
    let journal = Journal::new_memory("brain-output-test");
    let brain = Brain::with_parts(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(temp.0.clone())),
        Some(Arc::new(move |_| {
            factory_audit.clone() as Arc<dyn Provider>
        })),
    );

    let token = "output-token";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain: brain.clone(),
        token: token.into(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();

    let created = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(token)
        .json(&json!({
            "model": {"provider":"anthropic","name":"scripted","api_key":"sk-fake"},
            "system_prompt": "Be exact."
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let session = created.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let schema = json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object",
        "additionalProperties":false,
        "required":["answer"],
        "properties":{"answer":{"type":"number"}}
    });
    let schema_hash = brain::output::jcs_sha256(&schema).unwrap();
    let request = json!({
        "schema":schema,
        "schema_hash":schema_hash,
        "input":"Research the answer."
    });

    let accepted = http
        .post(format!("{base}/v1/sessions/{session}/output"))
        .bearer_auth(token)
        .header("Idempotency-Key", "typed-answer-1")
        .json(&request)
        .send()
        .await
        .unwrap();
    let status = accepted.status();
    let accepted = accepted.json::<Value>().await.unwrap();
    assert_eq!(status, 202, "{accepted}");
    let output_id = accepted["output_id"].as_str().unwrap();
    assert_eq!(accepted["schema_hash"], schema_hash);

    let terminal = wait_terminal(&http, &base, token, &session, output_id).await;
    assert_eq!(terminal["type"], "output.completed");
    assert_eq!(terminal["output"]["value"], json!({"answer":42}));
    assert_eq!(terminal["usage"]["input_tokens"], 0);

    let replay = http
        .post(format!("{base}/v1/sessions/{session}/output"))
        .bearer_auth(token)
        .header("Idempotency-Key", "typed-answer-1")
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 202);
    assert_eq!(
        replay.json::<Value>().await.unwrap()["output_id"],
        output_id
    );
    assert_eq!(fake.call_count.load(std::sync::atomic::Ordering::SeqCst), 3);

    let conflict = http
        .post(format!("{base}/v1/sessions/{session}/output"))
        .bearer_auth(token)
        .header("Idempotency-Key", "typed-answer-1")
        .json(&json!({
            "schema": request["schema"],
            "schema_hash": schema_hash,
            "input":"A different request."
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), 409);

    {
        let arrivals = fake.arrivals.lock().unwrap();
        assert_eq!(arrivals.len(), 3);
        assert!(arrivals[0].tools_offered > 0, "work keeps normal tools");
        assert_eq!(arrivals[1].tools_offered, 0, "commit disables tools");
        assert_eq!(arrivals[2].tools_offered, 0, "repair disables tools");
    }

    let records = journal.read_records(&session, 0).await.unwrap();
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::OutputStarted { schema_hash: hash, .. } if hash == &schema_hash
    )));
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::OutputCompleted { value, .. } if value == &json!({"answer":42})
    )));
    // No durable record has a schema field. Repair candidates/issues do not survive success.
    let durable = serde_json::to_string(
        &records
            .iter()
            .map(|entry| &entry.record)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(!durable.contains("$schema"));
    assert!(!durable.contains(r#""required""#));

    let continued = http
        .post(format!("{base}/v1/sessions/{session}/messages"))
        .bearer_auth(token)
        .header("Idempotency-Key", "after-output-1")
        .json(&json!({"content":"Are you still there?"}))
        .send()
        .await
        .unwrap();
    assert_eq!(continued.status(), 202);
    let continued = continued.json::<Value>().await.unwrap();
    let turn_id = continued["turn_id"].as_str().unwrap();
    let terminal = wait_turn_terminal(&http, &base, token, &session, turn_id).await;
    assert_eq!(terminal["type"], "turn.completed");

    {
        let bodies = audit.bodies.lock().unwrap();
        assert_eq!(bodies.len(), 4);
        assert!(bodies[1].get("output_config").is_some());
        assert!(bodies[2].get("output_config").is_some());
        assert!(bodies[3].get("output_config").is_none());
        assert_eq!(bodies[3]["system"], "Be exact.");
        let continuation = serde_json::to_string(&bodies[3]).unwrap();
        assert!(!continuation.contains("$schema"));
        assert!(!continuation.contains("Validation issues"));
        assert!(!continuation.contains("Aex private output commit"));
    }

    fake.assert_drained(4, "typed output e2e and continuation")
        .unwrap();
}

#[tokio::test]
async fn cancelling_output_commits_one_cancelled_terminal_and_never_formats() {
    let temp = TempDir::new();
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.tokens_per_second.store(1, Ordering::SeqCst);
    fake.script([Scripted::Text("slow work phase".into())]);
    let factory_fake = fake.clone();
    let journal = Journal::new_memory("brain-output-cancel-test");
    let brain = Brain::with_parts(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(temp.0.clone())),
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );
    let token = "output-cancel-token";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.into(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();

    let session = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(token)
        .json(&json!({
            "model": {"provider":"anthropic","name":"scripted","api_key":"sk-fake"}
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let schema = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["answer"],
        "properties":{"answer":{"type":"string"}}
    });
    let schema_hash = brain::output::jcs_sha256(&schema).unwrap();
    let accepted = http
        .post(format!("{base}/v1/sessions/{session}/output"))
        .bearer_auth(token)
        .header("Idempotency-Key", "cancel-output-1")
        .json(&json!({
            "schema":schema,
            "schema_hash":schema_hash,
            "input":"Start slow work."
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let output_id = accepted["output_id"].as_str().unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    while fake.call_count.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "work phase never reached provider"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let cancelled = http
        .post(format!("{base}/v1/sessions/{session}/cancel"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 200);

    let terminal = wait_terminal(&http, &base, token, &session, output_id).await;
    assert_eq!(terminal["type"], "output.failed");
    assert_eq!(terminal["error"]["code"], "cancelled");
    assert_eq!(fake.call_count.load(Ordering::SeqCst), 1);
    let records = journal.read_records(&session, 0).await.unwrap();
    assert!(
        !records
            .iter()
            .any(|entry| matches!(entry.record, Record::OutputCompleted { .. }))
    );
}

#[tokio::test]
async fn work_phase_refusal_is_not_formatted_into_success() {
    let temp = TempDir::new();
    let fake = Arc::new(FakeProvider::new(Dialect::OpenAiChat));
    fake.script([Scripted::Refusal("I cannot help with that.".into())]);
    let factory_fake = fake.clone();
    let journal = Journal::new_memory("brain-output-refusal-test");
    let brain = Brain::with_parts(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(temp.0.clone())),
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );
    let token = "output-refusal-token";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.into(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let session = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(token)
        .json(&json!({
            "model": {"provider":"openai","name":"scripted","api_key":"sk-fake"}
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let schema = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["answer"],
        "properties":{"answer":{"type":"string"}}
    });
    let schema_hash = brain::output::jcs_sha256(&schema).unwrap();
    let accepted = http
        .post(format!("{base}/v1/sessions/{session}/output"))
        .bearer_auth(token)
        .header("Idempotency-Key", "refused-output-1")
        .json(&json!({
            "schema":schema,
            "schema_hash":schema_hash,
            "input":"Do the disallowed thing."
        }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let terminal = wait_terminal(
        &http,
        &base,
        token,
        &session,
        accepted["output_id"].as_str().unwrap(),
    )
    .await;
    assert_eq!(terminal["type"], "output.failed");
    assert_eq!(terminal["error"]["code"], "output_refused");
    assert!(
        terminal["error"]["message"]
            .as_str()
            .unwrap()
            .contains("I cannot help")
    );
    assert_eq!(fake.call_count.load(Ordering::SeqCst), 1);
    let records = journal.read_records(&session, 0).await.unwrap();
    assert!(
        !records
            .iter()
            .any(|entry| matches!(&entry.record, Record::OutputCompleted { .. }))
    );
}
