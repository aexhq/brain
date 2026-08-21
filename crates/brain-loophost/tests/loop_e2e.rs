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

fn contract_component_path() -> PathBuf {
    guest_dir().join("dist/contract-loop.component.wasm")
}

/// Build the guest components when absent. Requires Node + npm, exactly like the standalone
/// managed-tool tests; the build is cached under guest/dist. Once-guarded so parallel tests
/// never race the npm/componentize pipeline.
fn ensure_component() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        if component_path().exists() && contract_component_path().exists() {
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
        assert!(contract_component_path().exists());
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
    serve_brain_with(BrainConfig::default(), services, script).await
}

async fn serve_brain_with(
    config: BrainConfig,
    services: BrainServices,
    script: Vec<Scripted>,
) -> TestBrain {
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script(script);
    let factory_fake = fake.clone();
    let brain = Brain::with_parts_and_services(
        config,
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
        self.create_session_from(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"}
        }))
        .await
    }

    /// A session with the sealed engine task tool — the one dispatchable tool that needs no
    /// Hand or customer transport, so a contract loop can drive a real successful dispatch.
    async fn create_session_with_task_tool(&self) -> String {
        self.create_session_from(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"items": [{
                "definition": {
                    "name": "subagents",
                    "description": "spawn a child session",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "action": {"type": "string"},
                            "task_name": {"type": "string"},
                            "message": {"type": "string"},
                            "fork_turns": {"type": "string"}
                        },
                        "required": ["action", "task_name", "message"],
                        "additionalProperties": true
                    },
                    "output_schema": {"type": "object", "additionalProperties": true},
                    "contract_digest": "a".repeat(64),
                },
                "executor": {"kind": "engine", "capability": "brain.subagents"},
            }]}
        }))
        .await
    }

    async fn create_session_from(&self, body: Value) -> String {
        let created: Value = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        created["id"]
            .as_str()
            .unwrap_or_else(|| panic!("session id missing in {created}"))
            .to_string()
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
        self.wait_turn_after(session_id, 0).await
    }

    async fn wait_turn_after(&self, session_id: &str, after: u64) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let text = self
                .http
                .get(format!(
                    "{}/v1/sessions/{session_id}/events?after={after}&follow=false",
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

/// The `data` payloads of the named loop events, in stream order.
fn loop_event_data<'e>(events: &'e [Value], name: &str) -> Vec<&'e Value> {
    events
        .iter()
        .filter(|event| event["type"] == "loop.event" && event["name"] == name)
        .map(|event| &event["data"])
        .collect()
}

fn max_seq(events: &[Value]) -> u64 {
    events
        .iter()
        .filter_map(|event| event["seq"].as_u64())
        .max()
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_contract_loop_drives_turns_through_ctx_ops() {
    ensure_component();
    // Queue order is the execution order: the parent's first composed round asks for the task
    // tool, the spawned child's single round pops next while the parent dispatch awaits it,
    // then the parent's follow-up round, then turn 2.
    let brain = serve_brain(
        brain_loophost::services_with_wasm_loop(&contract_component_path()).expect("contract loop"),
        vec![
            Scripted::ToolCalls(vec![(
                "call_task".into(),
                "subagents".into(),
                json!({
                    "action": "spawn_agent",
                    "task_name": "worker",
                    "message": "child prompt",
                    "fork_turns": "all"
                }),
            )]),
            Scripted::Text("child answer".into()),
            Scripted::Text("done after task".into()),
            Scripted::Text("second answer".into()),
        ],
    )
    .await;
    let session = brain.create_session_with_task_tool().await;

    // ---- turn 1: a fresh session with no loop state ----
    brain.send_message(&session, "run the probe").await;
    let first = brain.wait_turn(&session).await;

    let completed = first
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("turn 1 completes");
    assert_eq!(completed["stop_reason"], "end_turn");
    assert_eq!(
        completed["result"]["name"], "agentloop",
        "turn_finish carries the loop-declared result: {completed}"
    );
    assert_eq!(completed["result"]["value"]["turns"], 1);

    let hydration = loop_event_data(&first, "loop.hydration");
    assert_eq!(hydration.len(), 1);
    assert_eq!(hydration[0]["resumed"], false);
    assert_eq!(hydration[0]["kv"], json!({}));
    assert_eq!(hydration[0]["tail_types"], json!([]));
    assert_eq!(hydration[0]["mark_covers"], Value::Null);

    let checks = loop_event_data(&first, "loop.checks");
    assert_eq!(
        checks[0]["unsealed"], "unsealed_tool",
        "an undeclared tool fails the op with a typed code: {}",
        checks[0]
    );
    assert_eq!(checks[0]["kv_limit"], "kv_limit");

    let dispatched = loop_event_data(&first, "loop.dispatched");
    assert_eq!(dispatched[0]["results"][0]["name"], "subagents");
    assert_eq!(
        dispatched[0]["results"][0]["is_error"], false,
        "the sealed engine task tool dispatches successfully: {}",
        dispatched[0]
    );

    assert_eq!(
        transcript(&first)
            .iter()
            .filter(|kind| kind.as_str() == "assistant.message")
            .count(),
        2,
        "the loop drove two composed model rounds on the parent"
    );
    let tool_result = first
        .iter()
        .find(|event| event["type"] == "tool.result")
        .expect("the dispatched call is journaled");
    assert_eq!(tool_result["name"], "subagents");
    assert_eq!(tool_result["outcome"], "completed");

    // ---- turn 2: kv, the mark and the tail all survive the turn boundary ----
    let first_high_water = max_seq(&first);
    brain.send_message(&session, "second").await;
    let second = brain.wait_turn_after(&session, first_high_water).await;

    let completed = second
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("turn 2 completes");
    assert_eq!(
        completed["result"]["value"]["turns"], 2,
        "kv persisted across turns"
    );

    let hydration = loop_event_data(&second, "loop.hydration");
    assert_eq!(hydration[0]["resumed"], true);
    assert_eq!(hydration[0]["kv"]["turns"], 1);
    assert_eq!(hydration[0]["mark_data"]["summary"], "through turn 1");
    assert!(
        hydration[0]["mark_covers"]
            .as_u64()
            .is_some_and(|covers| covers > 0),
        "the latest mark is delivered: {}",
        hydration[0]
    );
    let tail_types: Vec<&str> = hydration[0]["tail_types"]
        .as_array()
        .expect("tail types")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for expected in [
        "assistant_message",
        "tool_result",
        "loop_event",
        "loop_custom",
    ] {
        assert!(
            tail_types.contains(&expected),
            "the tail after the mark carries {expected}: {tail_types:?}"
        );
    }
    assert!(
        !tail_types.contains(&"loop_mark"),
        "the mark itself travels as latest_mark, not in the tail"
    );
    assert!(
        !tail_types.contains(&"user_message"),
        "entries at or before covers_through_seq are covered by the mark"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_round_ceiling_closes_with_a_final_text_round_on_both_loop_hosts() {
    ensure_component();
    // Cap of one: the first round's tool call exhausts it, and the graceful closing round
    // (tool_choice none) produces the wrap-up text instead of a truncation error.
    let capped = || BrainConfig {
        default_max_rounds: 1,
        ..BrainConfig::default()
    };
    let script = || {
        vec![
            Scripted::ToolCalls(vec![(
                "call_echo".into(),
                "echo".into(),
                json!({"value": "ping"}),
            )]),
            Scripted::Text("wrapping up at the ceiling".into()),
        ]
    };

    let mut transcripts = Vec::new();
    for services in [
        BrainServices::default(),
        brain_loophost::services_with_wasm_loop(&component_path()).expect("wasm loop"),
    ] {
        let brain = serve_brain_with(capped(), services, script()).await;
        let session = brain.create_session().await;
        brain.send_message(&session, "run until the cap").await;
        let events = brain.wait_turn(&session).await;

        let completed = events
            .iter()
            .find(|event| event["type"] == "turn.completed")
            .expect("the capped turn completes");
        assert_eq!(
            completed["stop_reason"], "max_rounds",
            "the ceiling stays the honest stop reason: {completed}"
        );
        let kinds = transcript(&events);
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.as_str() == "assistant.message")
                .count(),
            2,
            "the closing round reached the transcript: {kinds:?}"
        );
        transcripts.push(kinds);
    }
    assert_eq!(
        transcripts[0], transcripts[1],
        "the builtin loop and the wasm guest close identically at the cap"
    );
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
