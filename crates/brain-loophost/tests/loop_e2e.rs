//! End-to-end contract for both loop-host compositions: the same scripted session driven by
//! the in-process BuiltinAexLoop, the in-process wasm guest, and the wasm guest running in a
//! separate loop-host daemon must produce identical public transcripts — and loop-host failures
//! must fail turns honestly, never hang them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig, BrainServices};
use brain_loophost::remote::{SpawnedLoopHost, WireClient, services_with_remote_loop};
use serde_json::{Value, json};

fn guest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("guest")
}

fn component_path() -> PathBuf {
    guest_dir().join("dist/aex-loop.component.wasm")
}

/// Build the guest component when absent. Requires Node + npm, exactly like the standalone
/// managed-tool tests; the build is cached under guest/dist. Once-guarded so parallel tests
/// never race the npm/componentize pipeline.
fn ensure_component() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        if component_path().exists() {
            return;
        }
        let npm: (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/C", "npm", "i", "--ignore-scripts"])
        } else {
            ("npm", &["i", "--ignore-scripts"])
        };
        let install = std::process::Command::new(npm.0)
            .args(npm.1)
            .current_dir(guest_dir())
            .status()
            .expect("npm is required to build the guest loop");
        assert!(install.success(), "npm install failed for the guest loop");
        let build = std::process::Command::new("node")
            .arg("build.mjs")
            .current_dir(guest_dir())
            .status()
            .expect("node is required to build the guest loop");
        assert!(build.success(), "guest loop componentization failed");
        assert!(component_path().exists());
    });
}

fn spawn_loop_host() -> SpawnedLoopHost {
    ensure_component();
    SpawnedLoopHost::spawn(Path::new(env!("CARGO_BIN_EXE_loophost")), &component_path())
        .expect("loop-host daemon")
}

/// One tool round then a final message: the script both parity tests replay.
fn tool_call_script() -> Vec<Scripted> {
    vec![
        Scripted::ToolCalls(vec![(
            "call_echo".into(),
            "echo".into(),
            json!({"value": "ping"}),
        )]),
        Scripted::Text("done after echo".into()),
    ]
}

struct TestBrain {
    base: String,
    token: String,
    http: reqwest::Client,
}

async fn serve_brain(services: BrainServices, script: Vec<Scripted>) -> TestBrain {
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script(script);
    let factory_fake = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("loop-e2e"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(brain::adapter::DisabledToolExecutor),
        services,
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );
    let token = "loop-e2e-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestBrain {
        base,
        token,
        http: reqwest::Client::new(),
    }
}

impl TestBrain {
    async fn create_session(&self) -> String {
        let created: Value = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(&self.token)
            .json(&json!({
                "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"}
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        created["id"].as_str().expect("session id").to_string()
    }

    async fn send_message(&self, session_id: &str, content: &str) {
        let accepted = self
            .http
            .post(format!("{}/v1/sessions/{session_id}/messages", self.base))
            .bearer_auth(&self.token)
            .json(&json!({"content": content}))
            .send()
            .await
            .unwrap();
        assert_eq!(accepted.status(), 202);
    }

    /// Poll the event stream until the turn concludes (completed or failed).
    async fn wait_turn(&self, session_id: &str) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(20);
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
            let events: Vec<Value> = text
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|data| serde_json::from_str(data).ok())
                .collect();
            if events
                .iter()
                .any(|event| event["type"] == "turn.completed" || event["type"] == "turn.failed")
            {
                return events;
            }
            assert!(Instant::now() < deadline, "turn did not conclude");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

async fn run_one_turn(services: BrainServices) -> Vec<Value> {
    let brain = serve_brain(services, tool_call_script()).await;
    let session = brain.create_session().await;
    brain.send_message(&session, "run the probe").await;
    brain.wait_turn(&session).await
}

fn transcript(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| event["type"].as_str())
        // The replay marker is stream framing, not turn content.
        .filter(|kind| *kind != "replay.complete")
        .map(str::to_string)
        .collect()
}

fn assert_tool_turn_shape(events: &[Value], kinds: &[String]) {
    let completed = events
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the turn completes");
    assert_eq!(completed["stop_reason"], "end_turn");
    // The undeclared tool is answered with a failed result, and the loop continued to a
    // second model round afterwards.
    let tool_result = events
        .iter()
        .find(|event| event["type"] == "tool.result")
        .expect("a tool result exists");
    assert_eq!(tool_result["outcome"], "failed");
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| kind.as_str() == "assistant.message")
            .count(),
        2,
        "two model rounds reached the transcript"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wasm_guest_loop_reproduces_the_builtin_transcript() {
    ensure_component();

    let builtin = run_one_turn(BrainServices::default()).await;
    let wasm = run_one_turn(
        brain_loophost::services_with_wasm_loop(&component_path()).expect("wasm loop"),
    )
    .await;

    let builtin_types = transcript(&builtin);
    let wasm_types = transcript(&wasm);
    assert_eq!(
        builtin_types, wasm_types,
        "the wasm loop must reproduce the builtin event sequence"
    );
    assert_tool_turn_shape(&wasm, &wasm_types);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_hosted_loop_reproduces_the_builtin_transcript() {
    let host = spawn_loop_host();
    let client = WireClient::connect(host.addr, &host.token)
        .await
        .expect("connect to the loop host");

    let builtin = run_one_turn(BrainServices::default()).await;
    let remote = run_one_turn(services_with_remote_loop(client)).await;

    let builtin_types = transcript(&builtin);
    let remote_types = transcript(&remote);
    assert_eq!(
        builtin_types, remote_types,
        "the daemon-hosted loop must reproduce the builtin event sequence"
    );
    assert_tool_turn_shape(&remote, &remote_types);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_sessions_multiplex_one_loop_host_connection() {
    let host = spawn_loop_host();
    let client = WireClient::connect(host.addr, &host.token)
        .await
        .expect("connect to the loop host");

    // Identical text-only turns so any pop order of the shared script is observationally equal.
    let brain = serve_brain(
        services_with_remote_loop(client),
        vec![
            Scripted::Text("solo answer".into()),
            Scripted::Text("solo answer".into()),
        ],
    )
    .await;
    let first = brain.create_session().await;
    let second = brain.create_session().await;
    // Send both before waiting on either: the two activations are in flight together on the
    // one daemon connection, so ctx frames from both interleave and must route by id.
    brain.send_message(&first, "go").await;
    brain.send_message(&second, "go").await;
    let (first_events, second_events) =
        tokio::join!(brain.wait_turn(&first), brain.wait_turn(&second));

    for events in [&first_events, &second_events] {
        let completed = events
            .iter()
            .find(|event| event["type"] == "turn.completed")
            .expect("both turns complete");
        assert_eq!(completed["stop_reason"], "end_turn");
        assert_eq!(
            transcript(events)
                .iter()
                .filter(|kind| kind.as_str() == "assistant.message")
                .count(),
            1
        );
    }
    assert_eq!(transcript(&first_events), transcript(&second_events));
}

#[tokio::test(flavor = "multi_thread")]
async fn loop_host_failures_are_honest() {
    let host = spawn_loop_host();

    // A wrong token is refused at connect, before any activation exists.
    let refused = WireClient::connect(host.addr, "not-the-token").await;
    assert!(refused.is_err(), "a wrong token must not connect");

    let client = WireClient::connect(host.addr, &host.token)
        .await
        .expect("connect to the loop host");
    let brain = serve_brain(services_with_remote_loop(client), tool_call_script()).await;
    let session = brain.create_session().await;

    // Kill the daemon out from under the brain: the turn must fail with a message naming the
    // loop host — never hang, never report a provider problem.
    drop(host);
    tokio::time::sleep(Duration::from_millis(300)).await;
    brain.send_message(&session, "run the probe").await;
    let events = brain.wait_turn(&session).await;
    let failed = events
        .iter()
        .find(|event| event["type"] == "turn.failed")
        .expect("the turn fails");
    assert!(
        failed.to_string().contains("loop host connection lost"),
        "the failure names the loop host: {failed}"
    );
    assert!(
        !events.iter().any(|event| event["type"] == "turn.completed"),
        "a dead loop host must not complete turns"
    );
}
