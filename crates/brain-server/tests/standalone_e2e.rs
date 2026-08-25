//! Canonical local-distribution gate: real HTTP and finite SSE over durable SQLite/storage, with
//! only the provider scripted. Managed Tool code runs in the actual Node subprocess runner. Two
//! interleaved sessions prove workspace, journal, provider-key, and managed-secret isolation.

use base64::Engine as _;
use brain::session::BrainConfig;
use brain_standalone::durable_local_parts;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const OPERATOR_TOKEN: &str = "standalone-e2e-operator";
const CONTRACT_DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ALPHA_KEY: &str = "sk-standalone-alpha-sentinel";
const BETA_KEY: &str = "sk-standalone-beta-sentinel";
const ALPHA_SECRET: &str = "managed-alpha-secret-sentinel";
const BETA_SECRET: &str = "managed-beta-secret-sentinel";
const ALPHA_VALUE: &str = "alpha-workspace-value";
const BETA_VALUE: &str = "beta-workspace-value";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "brain-standalone-e2e-{}",
            brain::mint_id("test", 16)
        ));
        std::fs::create_dir_all(&path).expect("create standalone test directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn runtime_bundle() -> (Vec<u8>, String) {
    let source = format!(
        r#"import {{ writeFile, readFile }} from "node:fs/promises";
export default {{
  kind: "tool-runtime/v1",
  name: "workspace_probe",
  description: "Write and read one session-local file.",
  contractDigest: "{CONTRACT_DIGEST}",
  requiredEnv: ["TENANT_SECRET"],
  execute: async (input, context) => {{
    const path = `${{context.workspace}}/shared.txt`;
    await writeFile(path, String(input.value), {{ encoding: "utf8", flag: "wx" }});
    return {{
      value: await readFile(path, "utf8"),
      session_id: context.sessionId,
      secret_configured: typeof process.env.TENANT_SECRET === "string" && process.env.TENANT_SECRET.length > 0
    }};
  }}
}};
"#
    );
    let bytes = source.into_bytes();
    let digest = hex::encode(Sha256::digest(&bytes));
    (bytes, digest)
}

fn create_body(
    provider: &str,
    model: &str,
    api_key: &str,
    managed_secret: &str,
    bundle: &[u8],
    bundle_digest: &str,
) -> Value {
    let loop_component = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../brain-component-host/guest/dist/agentloop.component.wasm"),
    )
    .expect("run npm run build:components before the standalone gate");
    let model_component = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../brain-component-host/guest/dist/model.component.wasm"),
    )
    .expect("run npm run build:components before the standalone gate");
    let loop_digest = hex::encode(Sha256::digest(&loop_component));
    let model_digest = hex::encode(Sha256::digest(&model_component));
    let manifest = json!({
        "profile":"computer/v1",
        "target":"linux-amd64",
        "execute_path":"/tool/runtime.mjs",
        "setup_path":null,
        "layers":[{
            "checksum":bundle_digest,
            "bytes":bundle.len(),
            "media_type":"application/javascript+esm",
            "mount_path":"/tool/runtime.mjs",
            "unpack":"file"
        }]
    });
    let manifest_digest = brain_protocol::contract::canonical_digest(&manifest).unwrap();
    json!({
        "component_artifacts": [
            {
                "component_digest": loop_digest,
                "component_base64": base64::engine::general_purpose::STANDARD.encode(&loop_component),
                "bytes": loop_component.len(),
            },
            {
                "component_digest": model_digest,
                "component_base64": base64::engine::general_purpose::STANDARD.encode(&model_component),
                "bytes": model_component.len(),
            }
        ],
        "model": {
            "component_digest": model_digest,
            "world": "aex:model/model@1.0.0",
            "config": {
                "toolName": "workspace_probe",
                "toolInput": {"value": if provider == "anthropic" { ALPHA_VALUE } else { BETA_VALUE }},
                "finalText": if provider == "anthropic" { "ALPHA_PUBLIC_RESULT" } else { "BETA_PUBLIC_RESULT" },
            },
            "provider":provider,
            "name":model,
            "api_key":api_key
        },
        "agentloop": {
            "component_digest": loop_digest,
            "world": "aex:agentloop/agentloop@1.0.0",
            "config": {"fixture":"sequential"},
        },
        "tools": {"items":[{
            "definition": {
                "name":"workspace_probe",
                "description":"Write and read one session-local file.",
                "contract_digest":CONTRACT_DIGEST,
                "input_schema":{
                    "type":"object",
                    "additionalProperties":false,
                    "properties":{"value":{"type":"string"}},
                    "required":["value"]
                },
                "output_schema":{
                    "type":"object",
                    "additionalProperties":false,
                    "properties":{
                        "value":{"type":"string"},
                        "session_id":{"type":"string"},
                        "secret_configured":{"type":"boolean"}
                    },
                    "required":["value","session_id","secret_configured"]
                }
            },
            "executor":{
                "kind":"environment",
                "environment":"workspace",
                "artifact_digest":manifest_digest,
                "requirements":{"env":["TENANT_SECRET"],"workspace":true}
            }
        }]},
        "tool_bundles":[{
            "checksum":manifest_digest,
            "bytes":bundle.len(),
            "target":manifest["target"],
            "execute_path":manifest["execute_path"],
            "setup_path":manifest["setup_path"],
            "layers":manifest["layers"]
        }],
        "tool_artifact_layers":[{
            "checksum":bundle_digest,
            "content_base64":base64::engine::general_purpose::STANDARD.encode(bundle),
            "bytes":bundle.len(),
            "media_type":"application/javascript+esm"
        }],
        "environments":{
            "workspace":{
                "extension":"brain.local",
                "protocol":"environment/v1",
                "profile":{
                    "kind":"computer",
                    "platform":"linux-amd64",
                    "network":"none",
                    "recovery":"retained"
                },
                "configuration":{}
            }
        },
        "secrets":{"TENANT_SECRET":managed_secret}
    })
}

async fn create_session(http: &Client, base: &str, body: Value, key: &str) -> String {
    let response = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(OPERATOR_TOKEN)
        .header("Idempotency-Key", key)
        .json(&body)
        .send()
        .await
        .expect("create request");
    let status = response.status();
    let bytes = response.bytes().await.expect("create body");
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let value: Value = serde_json::from_slice(&bytes).expect("create response JSON");
    let rendered = String::from_utf8_lossy(&bytes);
    for secret in [ALPHA_KEY, BETA_KEY, ALPHA_SECRET, BETA_SECRET] {
        assert!(
            !rendered.contains(secret),
            "create response leaked a credential"
        );
    }
    value["id"].as_str().expect("session id").to_owned()
}

async fn start_turn(http: &Client, base: &str, session_id: &str, value: &str) {
    let response = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(OPERATOR_TOKEN)
        .header("Idempotency-Key", format!("message-{session_id}"))
        .json(&json!({"content":format!("write {value}")}))
        .send()
        .await
        .expect("message request");
    let status = response.status();
    let body = response.text().await.expect("message response");
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

async fn finite_replay(http: &Client, base: &str, session_id: &str) -> String {
    let response = http
        .get(format!(
            "{base}/v1/sessions/{session_id}/events?after=0&follow=false"
        ))
        .bearer_auth(OPERATOR_TOKEN)
        .send()
        .await
        .expect("finite replay request");
    assert_eq!(response.status(), StatusCode::OK);
    response.text().await.expect("finite replay body")
}

async fn wait_for_turn(http: &Client, base: &str, session_id: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let replay = finite_replay(http, base, session_id).await;
        assert!(!replay.contains("event: turn.failed"), "{replay}");
        if replay.contains("event: turn.completed") {
            assert!(replay.contains("event: replay.complete"));
            return replay;
        }
        assert!(Instant::now() < deadline, "turn timed out: {replay}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn assert_replay_identity(replay: &str, expected_session: &str) {
    for line in replay
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
    {
        let value: Value = serde_json::from_str(line).expect("SSE data JSON");
        assert_eq!(
            value["session_id"].as_str(),
            Some(expected_session),
            "finite replay crossed a session boundary: {value}"
        );
    }
}

async fn environment_generation(http: &Client, base: &str, session_id: &str) -> String {
    let value: Value = http
        .get(format!(
            "{base}/v1/sessions/{session_id}/environments/workspace"
        ))
        .bearer_auth(OPERATOR_TOKEN)
        .send()
        .await
        .expect("environment status")
        .error_for_status()
        .expect("environment status success")
        .json()
        .await
        .expect("environment status JSON");
    assert_eq!(value["state"], "running");
    value["generation"]
        .as_str()
        .expect("environment generation")
        .to_owned()
}

async fn read_environment_file(
    http: &Client,
    base: &str,
    session_id: &str,
    generation: &str,
) -> Vec<u8> {
    let response = http
        .post(format!(
            "{base}/v1/sessions/{session_id}/environments/workspace/files/read-inline"
        ))
        .bearer_auth(OPERATOR_TOKEN)
        .json(&json!({
            "path":"/workspace/shared.txt",
            "generation":generation,
            "max_bytes":1024
        }))
        .send()
        .await
        .expect("environment read");
    let status = response.status();
    let body = response.text().await.expect("environment read body");
    assert_eq!(status, StatusCode::OK, "{body}");
    let value: Value = serde_json::from_str(&body).expect("environment read JSON");
    base64::engine::general_purpose::STANDARD
        .decode(
            value["content_base64"]
                .as_str()
                .expect("environment content"),
        )
        .expect("environment content base64")
}

async fn write_and_read_storage(http: &Client, base: &str, session_id: &str, value: &str) {
    let content = base64::engine::general_purpose::STANDARD.encode(value);
    let response = http
        .post(format!(
            "{base}/v1/sessions/{session_id}/storage/write-inline"
        ))
        .bearer_auth(OPERATOR_TOKEN)
        .header("Idempotency-Key", format!("storage-{session_id}"))
        .json(&json!({
            "key":"shared/private.txt",
            "content_base64":content,
            "content_type":"text/plain",
            "overwrite":false
        }))
        .send()
        .await
        .expect("storage write");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    let read: Value = http
        .post(format!(
            "{base}/v1/sessions/{session_id}/storage/read-inline"
        ))
        .bearer_auth(OPERATOR_TOKEN)
        .json(&json!({"key":"shared/private.txt","max_bytes":1024}))
        .send()
        .await
        .expect("storage read")
        .error_for_status()
        .expect("storage read success")
        .json()
        .await
        .expect("storage read JSON");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(read["content_base64"].as_str().expect("storage content"))
        .expect("storage content base64");
    assert_eq!(bytes, value.as_bytes());
}

fn assert_no_plaintext_secrets(root: &Path) {
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.expect("walk durable local state");
        if !entry.file_type().is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path()).expect("read durable local state");
        for secret in [ALPHA_KEY, BETA_KEY, ALPHA_SECRET, BETA_SECRET] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "plaintext credential leaked to {}",
                entry.path().display()
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_sse_journal_storage_and_node_tools_are_durable_and_isolated() {
    let temp = TempDir::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local HTTP");
    let address = listener.local_addr().unwrap();
    let base = format!("http://{address}");
    // The REAL shipped composition: the same compose_local the brain-server binary runs.
    let brain = brain_server::compose_local(brain_server::LocalOptions {
        data_dir: temp.0.clone(),
        cfg: BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        advertised_address: address.to_string(),
        transport_urls: None,
        provider_factory: None,
        loophost: Some(brain_server::LoophostOptions {
            component_host: PathBuf::from(env!("CARGO_BIN_EXE_brain-component-host")),
            workers: 2,
        }),
    })
    .await
    .expect("open the durable local composition");
    let journal = brain.journal.clone();
    let app = brain_server::api::router(brain_server::api::AppState {
        brain: brain.clone(),
        token: OPERATOR_TOKEN.into(),
        tenancy: brain_server::api::Tenancy::Implicit("local".into()),
    });
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let http = Client::new();

    let unauthorized = http
        .get(format!("{base}/v1/sessions"))
        .send()
        .await
        .expect("unauthorized request");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let (bundle, digest) = runtime_bundle();
    let (alpha_id, beta_id) = tokio::join!(
        create_session(
            &http,
            &base,
            create_body(
                "anthropic",
                "standalone-alpha",
                ALPHA_KEY,
                ALPHA_SECRET,
                &bundle,
                &digest,
            ),
            "create-alpha",
        ),
        create_session(
            &http,
            &base,
            create_body(
                "openai",
                "standalone-beta",
                BETA_KEY,
                BETA_SECRET,
                &bundle,
                &digest,
            ),
            "create-beta",
        )
    );
    tokio::join!(
        start_turn(&http, &base, &alpha_id, ALPHA_VALUE),
        start_turn(&http, &base, &beta_id, BETA_VALUE),
    );
    let (alpha_replay, beta_replay) = tokio::join!(
        wait_for_turn(&http, &base, &alpha_id),
        wait_for_turn(&http, &base, &beta_id),
    );
    assert_replay_identity(&alpha_replay, &alpha_id);
    assert_replay_identity(&beta_replay, &beta_id);
    assert!(alpha_replay.contains(ALPHA_VALUE));
    assert!(alpha_replay.contains("ALPHA_PUBLIC_RESULT"));
    assert!(!alpha_replay.contains(BETA_VALUE));
    assert!(!alpha_replay.contains("BETA_PUBLIC_RESULT"));
    assert!(beta_replay.contains(BETA_VALUE));
    assert!(beta_replay.contains("BETA_PUBLIC_RESULT"));
    assert!(!beta_replay.contains(ALPHA_VALUE));
    assert!(!beta_replay.contains("ALPHA_PUBLIC_RESULT"));
    for replay in [&alpha_replay, &beta_replay] {
        for secret in [ALPHA_KEY, BETA_KEY, ALPHA_SECRET, BETA_SECRET] {
            assert!(!replay.contains(secret), "SSE replay leaked a credential");
        }
    }

    let (alpha_generation, beta_generation) = tokio::join!(
        environment_generation(&http, &base, &alpha_id),
        environment_generation(&http, &base, &beta_id),
    );
    let (alpha_file, beta_file) = tokio::join!(
        read_environment_file(&http, &base, &alpha_id, &alpha_generation),
        read_environment_file(&http, &base, &beta_id, &beta_generation),
    );
    assert_eq!(alpha_file, ALPHA_VALUE.as_bytes());
    assert_eq!(beta_file, BETA_VALUE.as_bytes());

    tokio::join!(
        write_and_read_storage(&http, &base, &alpha_id, ALPHA_VALUE),
        write_and_read_storage(&http, &base, &beta_id, BETA_VALUE),
    );

    for session_id in [&alpha_id, &beta_id] {
        let records = journal.read_records(session_id, 0).await.unwrap();
        let kinds = records
            .iter()
            .map(|entry| entry.record.kind_name())
            .collect::<Vec<_>>();
        let intent = kinds
            .iter()
            .position(|kind| *kind == "managed_call_intent")
            .expect("managed intent");
        let accepted = kinds
            .iter()
            .position(|kind| *kind == "managed_call_accepted")
            .expect("managed receipt");
        let result = kinds
            .iter()
            .position(|kind| *kind == "tool_result")
            .expect("tool result");
        assert!(intent < accepted && accepted < result);
        let durable = format!(
            "{}{}",
            serde_json::to_string(&journal.get_head(session_id).await.unwrap().doc).unwrap(),
            records
                .iter()
                .map(|entry| serde_json::to_string(&entry.record).unwrap())
                .collect::<String>()
        );
        for secret in [ALPHA_KEY, BETA_KEY, ALPHA_SECRET, BETA_SECRET] {
            assert!(
                !durable.contains(secret),
                "journal leaked a plaintext credential"
            );
        }
    }

    // The Node runner, not the test process, created both files in distinct physical workspaces.
    let workspace_values = walkdir::WalkDir::new(temp.0.join("local-environment/workspaces"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() == "shared.txt")
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        workspace_values,
        std::collections::HashSet::from([ALPHA_VALUE.into(), BETA_VALUE.into()])
    );

    server.abort();
    let _ = server.await;
    drop(http);
    drop(brain);
    assert_no_plaintext_secrets(&temp.0);

    // Reopening both durable adapters proves the evidence was not merely resident memory.
    let reopened = durable_local_parts(&temp.0).expect("reopen durable local composition");
    let sessions = reopened.journal.list_sessions(10).await.unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        reopened
            .session_storage
            .read(&alpha_id, "shared/private.txt", 1024)
            .await
            .unwrap(),
        ALPHA_VALUE.as_bytes()
    );
    assert_eq!(
        reopened
            .session_storage
            .read(&beta_id, "shared/private.txt", 1024)
            .await
            .unwrap(),
        BETA_VALUE.as_bytes()
    );
}
