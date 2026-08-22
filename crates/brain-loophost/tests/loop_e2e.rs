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

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn component_path() -> PathBuf {
    guest_dir().join("dist/aex-loop.component.wasm")
}

fn contract_component_path() -> PathBuf {
    guest_dir().join("dist/contract-loop.component.wasm")
}

fn sdk_component_path() -> PathBuf {
    guest_dir().join("dist/sdk-loop.component.wasm")
}

/// The shared content-addressed loop store the official-loop tests seed into: keeping it
/// under guest/dist means the componentized officials persist across runs (and ride the CI
/// guest cache) exactly like the prebuilt fixtures.
fn shared_loop_store() -> PathBuf {
    guest_dir().join("dist/loop-store")
}

/// One loop package's published artifact pair: the deterministic source bundle plus its
/// sealed identity, built by the package's own `build.mjs` through the public
/// `buildLoopBundle` — the same artifact an external contributor ships.
fn loop_package_artifact(package: &str) -> (Vec<u8>, Value) {
    let dist = repo_root().join("packages").join(package).join("dist");
    let bundle = std::fs::read(dist.join("loop.bundle.mjs"))
        .unwrap_or_else(|error| panic!("{package} bundle missing: {error}"));
    let identity: Value = serde_json::from_str(
        &std::fs::read_to_string(dist.join("identity.json"))
            .unwrap_or_else(|error| panic!("{package} identity missing: {error}")),
    )
    .expect("loop identity is JSON");
    (bundle, identity)
}

fn npm_command(args: &[&str]) -> std::process::Command {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg("npm");
        c
    } else {
        std::process::Command::new("npm")
    };
    command.args(args);
    command
}

/// Build the guest components and loop-package bundles when absent. Requires Node + npm,
/// exactly like the standalone managed-tool tests; builds are cached under guest/dist and
/// packages/*/dist. Once-guarded so parallel tests never race the npm/componentize pipeline.
fn ensure_component() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        // The componentize toolchain must be installed even when every prebuilt component is
        // cache-hit: the upload and official-seeding e2es componentize server-side through
        // this install. It is a runtime dependency of the tests, not only of the build below.
        if !guest_dir()
            .join("node_modules/@bytecodealliance/componentize-js")
            .exists()
        {
            let install = npm_command(&["i", "--ignore-scripts"])
                .current_dir(guest_dir())
                .status()
                .expect("npm is required for the loop toolchain");
            assert!(install.success(), "npm install failed for the guest loop");
        }
        // The loop packages and the SDK build through the root npm workspaces — the public
        // toolchain path, shared with any external contributor.
        let workspace_dists = [
            repo_root().join("packages/agentloop/dist/build.js"),
            repo_root().join("packages/loop-pi/dist/identity.json"),
            repo_root().join("packages/loop-codex/dist/identity.json"),
        ];
        if workspace_dists.iter().any(|path| !path.exists()) {
            if !repo_root().join("node_modules/@aexhq/agentloop").exists() {
                let install = npm_command(&["ci"])
                    .current_dir(repo_root())
                    .status()
                    .expect("npm is required for the workspace install");
                assert!(install.success(), "npm ci failed at the workspace root");
            }
            let build = npm_command(&[
                "run",
                "build",
                "-w",
                "packages/agentloop",
                "-w",
                "packages/loop-pi",
                "-w",
                "packages/loop-codex",
            ])
            .current_dir(repo_root())
            .status()
            .expect("npm is required to build the loop packages");
            assert!(build.success(), "loop package builds failed");
        }
        if component_path().exists()
            && contract_component_path().exists()
            && sdk_component_path().exists()
            && guest_dir().join("dist/rogue-loop.source.mjs").exists()
        {
            return;
        }
        let build = std::process::Command::new("node")
            .arg("build.mjs")
            .current_dir(guest_dir())
            .status()
            .expect("node is required to build the guest loop");
        assert!(build.success(), "guest loop componentization failed");
        assert!(component_path().exists());
        assert!(contract_component_path().exists());
        assert!(sdk_component_path().exists());
        assert!(guest_dir().join("dist/rogue-loop.source.mjs").exists());
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
        Arc::new(move |_| factory_fake.clone() as Arc<dyn brain::provider::Provider>),
    );
    let token = "loop-e2e-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
        tenancy: brain::api::Tenancy::Implicit("local".into()),
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
    assert_eq!(
        hydration[0]["activation_kinds"],
        json!(["session_start", "message"]),
        "a fresh instance receives its session_start before the first message"
    );
    assert_eq!(hydration[0]["start_delivered"], true);
    assert_eq!(hydration[0]["start_resumed"], false);

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
    assert_eq!(
        hydration[0]["activation_kinds"],
        json!(["session_start", "message", "message"]),
        "the resident instance survived the turn boundary: no second session_start, and its \
         module state accumulated"
    );
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
async fn an_sdk_authored_loop_drives_turns_end_to_end() {
    ensure_component();
    let brain = serve_brain(
        brain_loophost::services_with_wasm_loop(&sdk_component_path()).expect("sdk loop"),
        vec![
            Scripted::Text("sdk answer one".into()),
            Scripted::Text("sdk answer two".into()),
        ],
    )
    .await;
    let session = brain.create_session().await;

    brain.send_message(&session, "first").await;
    let first = brain.wait_turn(&session).await;
    let completed = first
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the sdk-driven turn completes");
    assert_eq!(completed["stop_reason"], "end_turn");
    assert_eq!(
        completed["result"]["value"]["n"], 1,
        "ctx.turn.finish carried the structured result: {completed}"
    );
    let turn_events = loop_event_data(&first, "sdk.turn");
    assert_eq!(turn_events[0]["n"], 1);
    assert_eq!(turn_events[0]["text"], "sdk answer one");
    assert_eq!(
        turn_events[0]["resumed"], false,
        "the delivered session_start hydration reached ctx.start: {}",
        turn_events[0]
    );

    let high_water = max_seq(&first);
    brain.send_message(&session, "second").await;
    let second = brain.wait_turn_after(&session, high_water).await;
    let completed = second
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("turn 2 completes");
    assert_eq!(
        completed["result"]["value"]["n"], 2,
        "kv persisted across turns"
    );
    let turn_events = loop_event_data(&second, "sdk.turn");
    assert_eq!(turn_events[0]["text"], "sdk answer two");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_official_pi_loop_drives_turns() {
    ensure_component();
    // The official pi loop arrives exactly like a customer bundle: the @aexhq/loop-pi
    // package's public-toolchain artifact, seeded through the admission path.
    let (bundle, identity) = loop_package_artifact("loop-pi");
    let pi_version = identity["version"]
        .as_str()
        .expect("pi version")
        .to_string();
    let aex: Arc<dyn brain::agentloop::Agentloop> = Arc::new(
        brain_loophost::WasmAgentloop::from_component_file(&component_path()).expect("aex loop"),
    );
    let registry = brain_loophost::registry::LoophostRegistry::new(
        aex.clone(),
        shared_loop_store(),
        guest_dir(),
    )
    .expect("registry")
    .seed_official(
        "pi",
        &pi_version,
        identity["toolchain"].as_str().expect("toolchain"),
        &bundle,
    )
    .await
    .expect("pi seeds through the customer admission path");
    let brain = serve_brain(
        BrainServices {
            agentloop: Some(aex),
            agentloop_registry: Some(Arc::new(registry)),
            ..BrainServices::default()
        },
        // The parent's first pi round asks for the task tool, the spawned child's single round
        // pops next while pi's execute awaits the dispatch, then pi's follow-up round, then a
        // text-only second turn.
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
            Scripted::Text("pi final answer".into()),
            Scripted::Text("pi second answer".into()),
        ],
    )
    .await;

    let created: Value = brain
        .http
        .post(format!("{}/v1/sessions", brain.base))
        .bearer_auth(&brain.token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "agentloop": "pi",
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
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        created["agentloop"],
        json!({"kind": "official", "name": "pi", "version": pi_version}),
        "the pinned pi identity seals: {created}"
    );
    let session = created["id"].as_str().expect("session id").to_string();

    brain.send_message(&session, "hello pi").await;
    let first = brain.wait_turn(&session).await;
    let completed = first
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the pi-driven turn completes");
    assert_eq!(completed["stop_reason"], "end_turn");
    assert_eq!(
        transcript(&first)
            .iter()
            .filter(|kind| kind.as_str() == "assistant.message")
            .count(),
        2,
        "pi drove the tool round and its follow-up: {:?}",
        transcript(&first)
    );
    let tool_result = first
        .iter()
        .find(|event| event["type"] == "tool.result")
        .expect("pi dispatched the sealed task tool");
    assert_eq!(tool_result["name"], "subagents");
    assert_eq!(tool_result["outcome"], "completed");

    let high_water = max_seq(&first);
    brain.send_message(&session, "and again").await;
    let second = brain.wait_turn_after(&session, high_water).await;
    assert!(
        second
            .iter()
            .any(|event| event["type"] == "turn.completed" && event["stop_reason"] == "end_turn"),
        "the resident pi conversation carries into turn 2"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_codex_style_loop_executes_tools_sequentially() {
    ensure_component();
    let (bundle, identity) = loop_package_artifact("loop-codex");
    let codex_version = identity["version"]
        .as_str()
        .expect("codex version")
        .to_string();
    let aex: Arc<dyn brain::agentloop::Agentloop> = Arc::new(
        brain_loophost::WasmAgentloop::from_component_file(&component_path()).expect("aex loop"),
    );
    let registry = brain_loophost::registry::LoophostRegistry::new(
        aex.clone(),
        shared_loop_store(),
        guest_dir(),
    )
    .expect("registry")
    .seed_official(
        "codex-style",
        &codex_version,
        identity["toolchain"].as_str().expect("toolchain"),
        &bundle,
    )
    .await
    .expect("codex-style seeds through the customer admission path");
    let brain = serve_brain(
        BrainServices {
            agentloop: Some(aex),
            agentloop_registry: Some(Arc::new(registry)),
            ..BrainServices::default()
        },
        // Two task calls in one round: sequential execution means child A runs to completion
        // (popping its answer) before child B is even dispatched.
        vec![
            Scripted::ToolCalls(vec![
                (
                    "call_a".into(),
                    "subagents".into(),
                    json!({"action": "spawn_agent", "task_name": "a", "message": "child prompt", "fork_turns": "all"}),
                ),
                (
                    "call_b".into(),
                    "subagents".into(),
                    json!({"action": "spawn_agent", "task_name": "b", "message": "child prompt", "fork_turns": "all"}),
                ),
            ]),
            Scripted::Text("child a answer".into()),
            Scripted::Text("child b answer".into()),
            Scripted::Text("codex-style wrap-up".into()),
        ],
    )
    .await;

    let created: Value = brain
        .http
        .post(format!("{}/v1/sessions", brain.base))
        .bearer_auth(&brain.token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "agentloop": "codex-style",
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
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        created["agentloop"],
        json!({"kind": "official", "name": "codex-style", "version": codex_version}),
        "{created}"
    );
    let session = created["id"].as_str().expect("session id").to_string();

    brain.send_message(&session, "run two tools").await;
    let events = brain.wait_turn(&session).await;
    let completed = events
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the codex-style turn completes");
    assert_eq!(completed["stop_reason"], "end_turn");

    // The port's distinctive shape: strictly sequential execution — each call's result is
    // journaled before the next call is journaled (the aex batch loop journals both calls,
    // then both results).
    let tool_order: Vec<&str> = events
        .iter()
        .filter_map(|event| match event["type"].as_str() {
            Some("tool.call") => Some("call"),
            Some("tool.result") => Some("result"),
            _ => None,
        })
        .collect();
    assert_eq!(
        tool_order,
        vec!["call", "result", "call", "result"],
        "sequential dispatch is visible in the public transcript"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_customer_bundle_uploads_componentizes_and_drives_turns() {
    use base64::Engine as _;
    use sha2::Digest as _;
    ensure_component();
    let source =
        std::fs::read(guest_dir().join("dist/sdk-loop.source.mjs")).expect("the source bundle");
    let digest = hex::encode(sha2::Sha256::digest(&source));
    let encoded = base64::engine::general_purpose::STANDARD.encode(&source);
    let store = std::env::temp_dir().join(format!(
        "brain-loop-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    let brain = serve_brain(
        brain_loophost::registry::services_with_loop_store(&component_path(), &store, &guest_dir())
            .expect("loop store composition"),
        vec![Scripted::Text("uploaded answer".into())],
    )
    .await;

    // The wrong toolchain refuses before anything componentizes.
    let refused: Value = brain
        .http
        .post(format!("{}/v1/sessions", brain.base))
        .bearer_auth(&brain.token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "agentloop": {
                "source_bundle_sha256": digest,
                "toolchain": "some-other-toolchain",
                "bundle_base64": encoded,
            }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(refused["error"]["code"], "invalid_request", "{refused}");

    // The customer flow: upload the SDK-built source bundle at create; the composition
    // componentizes it under the pinned toolchain and the sealed loop drives the turn.
    let session = brain
        .create_session_from(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "agentloop": {
                "source_bundle_sha256": digest,
                "toolchain": brain_loophost::registry::LOOP_TOOLCHAIN,
                "bundle_base64": encoded,
            }
        }))
        .await;
    brain
        .send_message(&session, "drive the uploaded loop")
        .await;
    let events = brain.wait_turn(&session).await;
    let completed = events
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the uploaded loop completes its turn");
    assert_eq!(completed["result"]["value"]["n"], 1);
    let turn_events = loop_event_data(&events, "sdk.turn");
    assert_eq!(turn_events[0]["text"], "uploaded answer");

    // The componentized artifact is cached content-addressed for every later admission.
    let cached = store.join("component").join(format!(
        "{digest}-{}.wasm",
        brain_loophost::registry::LOOP_TOOLCHAIN
    ));
    assert!(
        cached.exists(),
        "the componentized loop is cached at {cached:?}"
    );
    let _ = std::fs::remove_dir_all(store);
}

/// The wit doc's claim, enforced: a contract-only guest (here a customer upload) gets
/// `invalid_request` for every `engine.*` round op, while the read-only
/// `engine.session_start` hydration stays reachable.
#[tokio::test(flavor = "multi_thread")]
async fn a_customer_loop_cannot_reach_the_engine_vocabulary() {
    use base64::Engine as _;
    use sha2::Digest as _;
    ensure_component();
    let source =
        std::fs::read(guest_dir().join("dist/rogue-loop.source.mjs")).expect("the rogue bundle");
    let digest = hex::encode(sha2::Sha256::digest(&source));
    let encoded = base64::engine::general_purpose::STANDARD.encode(&source);

    let brain = serve_brain(
        brain_loophost::registry::services_with_loop_store(
            &component_path(),
            &shared_loop_store(),
            &guest_dir(),
        )
        .expect("loop store composition"),
        vec![Scripted::Text("never reached".into())],
    )
    .await;

    let session = brain
        .create_session_from(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "agentloop": {
                "source_bundle_sha256": digest,
                "toolchain": brain_loophost::registry::LOOP_TOOLCHAIN,
                "bundle_base64": encoded,
            }
        }))
        .await;
    brain.send_message(&session, "probe the engine ops").await;
    let events = brain.wait_turn(&session).await;
    let completed = events
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("the probe turn completes");
    let result = &completed["result"]["value"];
    for op in [
        "engine.model_round",
        "engine.dispatch_pending",
        "engine.budget",
    ] {
        assert_eq!(
            result["refusals"][op]["code"], "invalid_request",
            "{op} must be refused for a contract-only guest: {result}"
        );
        assert!(
            result["refusals"][op]["message"]
                .as_str()
                .expect("refusal message")
                .contains("reserved"),
            "{op} refusal names the reservation: {result}"
        );
    }
    assert_eq!(
        result["session_start_served"], true,
        "the read-only hydration exception stays reachable: {result}"
    );
}

/// A sealed official identity means what it says: a composition registered with a different
/// version of the same named loop refuses to run the session rather than silently
/// substituting.
#[tokio::test(flavor = "multi_thread")]
async fn an_official_version_mismatch_refuses_resolution() {
    use brain::agentloop::AgentloopRegistry as _;
    use brain::journal::AgentloopSelectorDoc;
    ensure_component();
    let (bundle, identity) = loop_package_artifact("loop-codex");
    let aex: Arc<dyn brain::agentloop::Agentloop> = Arc::new(
        brain_loophost::WasmAgentloop::from_component_file(&component_path()).expect("aex loop"),
    );
    let registry = brain_loophost::registry::LoophostRegistry::new(
        aex.clone(),
        shared_loop_store(),
        guest_dir(),
    )
    .expect("registry")
    .seed_official(
        "codex-style",
        identity["version"].as_str().expect("version"),
        identity["toolchain"].as_str().expect("toolchain"),
        &bundle,
    )
    .await
    .expect("codex-style seeds");

    let sealed_elsewhere = AgentloopSelectorDoc::Official {
        name: "codex-style".into(),
        version: "0.0.1-not-here".into(),
    };
    let error = registry
        .resolve(&sealed_elsewhere)
        .err()
        .expect("a version mismatch must refuse");
    let message = format!("{error:?}");
    assert!(
        message.contains("0.0.1-not-here") && message.contains("this composition runs"),
        "the refusal names both versions: {message}"
    );

    // The bootstrap default is version-checked the same way.
    let wrong_aex = AgentloopSelectorDoc::Official {
        name: "aex".into(),
        version: "999".into(),
    };
    assert!(
        registry.resolve(&wrong_aex).is_err(),
        "a foreign aex version must refuse"
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
