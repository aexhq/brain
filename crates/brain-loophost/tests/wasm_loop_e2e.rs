//! End-to-end parity: the same scripted session driven once by the in-process BuiltinAexLoop
//! and once by the real wasm guest loop must produce identical public transcripts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig, BrainServices};
use serde_json::{Value, json};

fn guest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("guest")
}

fn component_path() -> PathBuf {
    guest_dir().join("dist/aex-loop.component.wasm")
}

/// Build the guest component when absent. Requires Node + npm, exactly like the standalone
/// managed-tool tests; the build is cached under guest/dist.
fn ensure_component() {
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
}

fn scripted_provider() -> Arc<FakeProvider> {
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::ToolCalls(vec![(
            "call_echo".into(),
            "echo".into(),
            json!({"value": "ping"}),
        )]),
        Scripted::Text("done after echo".into()),
    ]);
    fake
}

async fn serve(brain: Arc<Brain>) -> (String, String, reqwest::Client) {
    let token = "wasm-loop-e2e-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, token, reqwest::Client::new())
}

async fn run_one_turn(services: BrainServices) -> Vec<Value> {
    let fake = scripted_provider();
    let factory_fake = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("wasm-loop-e2e"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(brain::adapter::DisabledToolExecutor),
        services,
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );
    let (base, token, http) = serve(brain).await;

    let created: Value = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_id = created["id"].as_str().expect("session id").to_string();

    let accepted = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(&token)
        .json(&json!({"content": "run the probe"}))
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), 202);

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = http
            .get(format!(
                "{base}/v1/sessions/{session_id}/events?after=0&follow=false"
            ))
            .bearer_auth(&token)
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
        assert!(Instant::now() < deadline, "turn did not complete");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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

    let completed = wasm
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the wasm-driven turn completes");
    assert_eq!(completed["stop_reason"], "end_turn");

    // The undeclared tool is answered with a failed result on both paths, and the loop
    // continued to a second model round afterwards.
    let tool_result = wasm
        .iter()
        .find(|event| event["type"] == "tool.result")
        .expect("a tool result exists");
    assert_eq!(tool_result["outcome"], "failed");
    assert_eq!(
        wasm_types
            .iter()
            .filter(|kind| kind.as_str() == "assistant.message")
            .count(),
        2,
        "two model rounds reached the transcript"
    );
}
