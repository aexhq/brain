//! End-to-end conformance for Brain's neutral loop-host contract and failure behavior.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig, BrainServices};
use brain_loophost::remote::{RemoteAgentloop, SpawnedLoopHost, WireClient};
use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use serde_json::{Value, json};

fn guest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("guest")
}

fn component_path() -> PathBuf {
    guest_dir().join("dist/contract-loop.component.wasm")
}

/// Shared content-addressed test loop store under the cacheable guest output directory.
fn shared_loop_store() -> PathBuf {
    guest_dir().join("dist/loop-store")
}

struct FixedRegistry {
    agentloop: Arc<dyn brain::agentloop::Agentloop>,
}

impl brain::agentloop::AgentloopRegistry for FixedRegistry {
    fn resolve(
        &self,
        _selector: &brain::journal::AgentloopSelectorDoc,
    ) -> brain::Result<Arc<dyn brain::agentloop::Agentloop>> {
        Ok(self.agentloop.clone())
    }

    fn admit_custom(
        &self,
        source_bundle_sha256: &str,
        toolchain: &str,
        bundle: &[u8],
    ) -> brain::Result<brain::journal::AgentloopSelectorDoc> {
        Ok(brain::journal::AgentloopSelectorDoc {
            source_bundle_sha256: source_bundle_sha256.into(),
            source_bundle_bytes: bundle.len() as u64,
            toolchain: toolchain.into(),
        })
    }
}

fn fixed_services(agentloop: Arc<dyn brain::agentloop::Agentloop>) -> BrainServices {
    BrainServices {
        agentloop_registry: Some(Arc::new(FixedRegistry { agentloop })),
        ..BrainServices::default()
    }
}

fn builtin_services() -> BrainServices {
    fixed_services(Arc::new(brain::agentloop::SequentialAgentloop))
}

fn wasm_services(component: &Path) -> BrainServices {
    fixed_services(Arc::new(
        brain_loophost::WasmAgentloop::from_component_file(component).expect("wasm loop"),
    ))
}

fn remote_services(client: Arc<WireClient>) -> BrainServices {
    fixed_services(Arc::new(RemoteAgentloop::new(client)))
}

fn test_loop_config() -> Value {
    use base64::Engine as _;
    use sha2::Digest as _;
    let bundle = b"loophost integration test loop";
    json!({
        "source_bundle_sha256": hex::encode(sha2::Sha256::digest(bundle)),
        "toolchain": "test-loop",
        "bundle_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
    })
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

/// Build the neutral guest fixtures when absent. Once-guarded so parallel tests never race.
fn ensure_component() {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        // Custom-bundle admission componentizes through this install even on a fixture cache hit.
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
        if component_path().exists()
            && guest_dir().join("dist/contract-loop.source.mjs").exists()
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
        assert!(guest_dir().join("dist/contract-loop.source.mjs").exists());
        assert!(guest_dir().join("dist/rogue-loop.source.mjs").exists());
    });
}

fn spawn_loop_host() -> SpawnedLoopHost {
    ensure_component();
    SpawnedLoopHost::spawn(Path::new(env!("CARGO_BIN_EXE_loophost")), &component_path())
        .expect("loop-host daemon")
}

/// One tool round then a final message: the script both host compositions replay.
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

struct EchoExecutor;

#[async_trait::async_trait]
impl brain::adapter::ToolExecutor for EchoExecutor {
    fn supports(&self, capability: &str) -> bool {
        capability == "brain.test.echo"
    }

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> brain::Result<ExternalToolCallResponse> {
        assert_eq!(capability, "brain.test.echo");
        Ok(serde_json::from_value(json!({
            "outcome": "completed",
            "content": request.input.to_string(),
            "is_error": false,
            "disposition": "continue",
            "result": request.input,
        }))?)
    }
}

async fn serve_brain(services: BrainServices, script: Vec<Scripted>) -> TestBrain {
    serve_brain_with(BrainConfig::default(), services, script).await
}

async fn serve_brain_with(
    mut config: BrainConfig,
    services: BrainServices,
    script: Vec<Scripted>,
) -> TestBrain {
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script(script);
    let factory_fake = fake.clone();
    config.official_capabilities.insert(
        "brain.test.echo".into(),
        brain::config::ServerToolPolicy {
            capability: "brain.test.echo".into(),
            scope: brain_protocol::session::ExternalToolScope::All,
            completion: brain_protocol::session::ExternalToolCompletion::Continue,
            effect: brain_protocol::session::ExternalToolEffect::ReplaySafe,
            max_input_bytes: 1024,
        },
    );
    let brain = Brain::with_parts_and_services(
        config,
        Journal::new_memory("loop-e2e"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(EchoExecutor),
        services,
        Arc::new(move |_| factory_fake.clone() as Arc<dyn brain::provider::Provider>),
    );
    let token = "loop-e2e-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain_server::api::router(brain_server::api::AppState {
        brain,
        token: token.clone(),
        tenancy: brain_server::api::Tenancy::Implicit("local".into()),
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

    /// A session with one sealed host capability so the contract loop can drive a real dispatch.
    async fn create_session_with_echo_tool(&self) -> String {
        self.create_session_from(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"items": [{
                "definition": {
                    "name": "echo",
                    "description": "echo structured input",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "value": {"type": "string"}
                        },
                        "required": ["value"],
                        "additionalProperties": false
                    },
                    "output_schema": {"type": "object", "additionalProperties": true},
                    "contract_digest": "a".repeat(64),
                },
                "executor": {"kind": "engine", "capability": "brain.test.echo"},
            }]}
        }))
        .await
    }

    async fn create_session_from(&self, mut body: Value) -> String {
        body.as_object_mut()
            .expect("create request is an object")
            .entry("agentloop")
            .or_insert_with(test_loop_config);
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
async fn in_process_and_daemon_hosts_run_the_same_loop_contract() {
    let host = spawn_loop_host();
    let client = WireClient::connect(host.addr, &host.token)
        .await
        .expect("connect to the loop host");

    let in_process = run_one_turn(wasm_services(&component_path())).await;
    let remote = run_one_turn(remote_services(client)).await;

    let in_process_types = transcript(&in_process);
    let remote_types = transcript(&remote);
    assert_eq!(
        in_process_types, remote_types,
        "both loop hosts must reproduce the same event sequence"
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
        remote_services(client),
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
    // Queue order is the execution order: the first round asks for echo, followed by the
    // parent's follow-up round and then turn 2.
    let brain = serve_brain(
        wasm_services(&component_path()),
        vec![
            Scripted::ToolCalls(vec![(
                "call_echo".into(),
                "echo".into(),
                json!({"value": "ping"}),
            )]),
            Scripted::Text("done after echo".into()),
            Scripted::Text("second answer".into()),
        ],
    )
    .await;
    let session = brain.create_session_with_echo_tool().await;

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
        checks[0]["unsealed"], "failed_result",
        "an undeclared tool is answered with a journaled failed result, never a route: {}",
        checks[0]
    );
    assert_eq!(checks[0]["kv_limit"], "kv_limit");

    let dispatched = loop_event_data(&first, "loop.dispatched");
    assert_eq!(dispatched[0]["results"][0]["name"], "echo");
    assert_eq!(
        dispatched[0]["results"][0]["is_error"], false,
        "the sealed host tool dispatches successfully: {}",
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
    let undeclared = first
        .iter()
        .find(|event| event["type"] == "tool.result" && event["name"] == "not_sealed")
        .expect("the undeclared call is journaled as a failed result");
    assert_eq!(undeclared["outcome"], "failed");
    let tool_result = first
        .iter()
        .find(|event| event["type"] == "tool.result" && event["name"] == "echo")
        .expect("the dispatched call is journaled");
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
async fn a_customer_bundle_uploads_componentizes_and_drives_turns() {
    use base64::Engine as _;
    use sha2::Digest as _;
    ensure_component();
    let source = std::fs::read(guest_dir().join("dist/contract-loop.source.mjs"))
        .expect("the source bundle");
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
        brain_loophost::registry::services_with_loop_store(&store, &guest_dir())
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

    // The customer flow: upload a contract source bundle at create; the composition
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
    assert_eq!(completed["result"]["value"]["turns"], 1);
    assert_eq!(loop_event_data(&events, "loop.hydration").len(), 1);

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

/// The wit doc's claim, enforced: the `engine.*` round vocabulary no longer exists for any
/// guest (here a customer upload) — every round op answers `invalid_request` as an unknown
/// op — while the read-only `engine.session_start` hydration stays reachable.
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
        brain_loophost::registry::services_with_loop_store(&shared_loop_store(), &guest_dir())
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
                .contains("unknown ctx op"),
            "{op} is an unknown op for every guest: {result}"
        );
    }
    assert_eq!(
        result["session_start_served"], true,
        "the read-only hydration exception stays reachable: {result}"
    );
}

#[test]
fn a_sealed_loop_requires_the_exact_toolchain_and_stored_digest() {
    use brain::agentloop::AgentloopRegistry as _;
    use brain::journal::AgentloopSelectorDoc;
    let registry =
        brain_loophost::registry::LoophostRegistry::new(shared_loop_store(), guest_dir())
            .expect("registry");
    let wrong_toolchain = AgentloopSelectorDoc {
        source_bundle_sha256: "0".repeat(64),
        source_bundle_bytes: 1,
        toolchain: "other-toolchain".into(),
    };
    assert!(
        registry.resolve(&wrong_toolchain).is_err(),
        "a foreign toolchain must refuse"
    );
    let missing = AgentloopSelectorDoc {
        source_bundle_sha256: "0".repeat(64),
        source_bundle_bytes: 1,
        toolchain: brain_loophost::registry::LOOP_TOOLCHAIN.into(),
    };
    assert!(
        registry.resolve(&missing).is_err(),
        "an artifact absent from this composition must refuse"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reference_loop_closes_with_a_final_text_round_at_the_round_ceiling() {
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

    let brain = serve_brain_with(capped(), builtin_services(), script()).await;
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
    let brain = serve_brain(remote_services(client), tool_call_script()).await;
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
