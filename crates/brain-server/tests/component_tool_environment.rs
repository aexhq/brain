use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use brain::journal::Record;
use brain::session::BrainConfig;
use brain_environment_host::HttpEnvironmentCapabilities;
use brain_protocol::session::CreateSessionRequest;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "brain-component-tool-environment-{}",
            brain::mint_id("test", 16)
        ));
        std::fs::create_dir_all(&path).expect("create component test directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../brain-component-host/guest/dist")
            .join(format!("{name}.component.wasm")),
    )
    .unwrap_or_else(|error| panic!("read {name} fixture: {error}; run npm run build:components"))
}

fn artifact(bytes: &[u8]) -> serde_json::Value {
    json!({
        "component_digest": hex::encode(Sha256::digest(bytes)),
        "component_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        "bytes": bytes.len(),
    })
}

/// Serves the Environment dispatch endpoint the way a deployment does, so the identity a real
/// component run stamps is judged by the real `HttpEnvironmentCapabilities` guard rather than by a
/// hand-built call. A refused dispatch is invisible to a turn: it surfaces only as a session end
/// that retries forever, which is why only a live plane ever saw it.
async fn dispatch_endpoint() -> (String, Arc<Mutex<Vec<Value>>>) {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    let app = axum::Router::new().route(
        "/environment",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let recorder = recorder.clone();
            async move {
                recorder.lock().expect("recorded dispatches").push(body);
                axum::Json(json!({"dispatched": "ok"}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind dispatch endpoint");
    let address = listener.local_addr().expect("dispatch endpoint address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}/environment"), seen)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn component_tool_calls_component_environment_through_the_session_kernel() {
    let temp = TempDir::new();
    let loop_component = fixture("agentloop");
    let model_component = fixture("model");
    let tool_component = fixture("tool");
    let environment_component = fixture("environment");
    let loop_digest = hex::encode(Sha256::digest(&loop_component));
    let model_digest = hex::encode(Sha256::digest(&model_component));
    let tool_digest = hex::encode(Sha256::digest(&tool_component));
    let environment_digest = hex::encode(Sha256::digest(&environment_component));
    let bundle = b"export default async function invoke(input) { return input; }";
    let bundle_digest = hex::encode(Sha256::digest(bundle));
    let (endpoint, dispatched) = dispatch_endpoint().await;
    let dispatch = Arc::new(
        HttpEnvironmentCapabilities::new(endpoint, Some("token".into()), Duration::from_secs(30))
            .expect("Environment dispatch capabilities"),
    );

    let brain = brain_server::compose_local(brain_server::LocalOptions {
        data_dir: temp.0.clone(),
        cfg: BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        advertised_address: "127.0.0.1:1".into(),
        transport_urls: None,
        provider_factory: None,
        environment_capabilities: Some(dispatch.clone()),
        loophost: Some(brain_server::LoophostOptions {
            component_host: PathBuf::from(env!("CARGO_BIN_EXE_brain-component-host")),
            workers: 2,
        }),
    })
    .await
    .expect("compose component Brain");

    let request: CreateSessionRequest = serde_json::from_value(json!({
        "component_artifacts": [
            artifact(&loop_component),
            artifact(&model_component),
            artifact(&tool_component),
            artifact(&environment_component),
        ],
        "model": {
            "component_digest": model_digest,
            "world": "aex:model/model@1.0.0",
            "provider": "fixture",
            "name": "fixture",
            "api_key": "sk-fixture",
            "config": {
                "toolName": "environment_echo",
                "toolInput": {"message":"hello"},
                "finalText": "component environment completed"
            }
        },
        "agentloop": {
            "component_digest": loop_digest,
            "world": "aex:agentloop/agentloop@1.0.0",
            "config": {"fixture":"sequential"}
        },
        "tools": {"items":[{
            "definition": {
                "name": "environment_echo",
                "contract_digest": "a".repeat(64),
                "input_schema": {"type":"object"},
                "output_schema": {
                    "type":"object",
                    "required":["providerOperationId","value"]
                }
            },
            "executor": {
                "kind": "component",
                "component_digest": tool_digest,
                "world": "aex:tool/tool@1.0.0",
                "config": {"useEnvironment":true},
                "grants": ["environment"],
                "environment": "workspace",
                "bundle_digest": bundle_digest
            }
        }]},
        "tool_artifact_layers": [{
            "checksum": bundle_digest,
            "content_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
            "bytes": bundle.len(),
            "media_type": "application/javascript+esm"
        }],
        "environments": {
            "workspace": {
                "component_digest": environment_digest,
                "world": "aex:environment/environment@1.0.0",
                "config": {"dispatch": true}
            }
        }
    }))
    .expect("component session request");
    let created = brain
        .create_session(request, None)
        .await
        .expect("create session");
    let session_id = created.id.to_string();
    let (_, admitted_seq) = brain
        .message(
            &session_id,
            serde_json::from_value(json!("run the environment Tool")).unwrap(),
        )
        .await
        .expect("message session");
    for _ in 0..12_000 {
        let head = brain.journal.get_head(&session_id).await.unwrap();
        if head.doc.turn.is_none() && head.last_seq > admitted_seq {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let records = brain.journal.read_records(&session_id, 0).await.unwrap();
    assert!(
        records.iter().any(|entry| match &entry.record {
            Record::ToolResult {
                content,
                is_error: false,
                ..
            } => serde_json::from_str::<serde_json::Value>(content).is_ok_and(|value| {
                value["value"] == "environment-ok"
                    && value["providerOperationId"].as_str().is_some_and(|id| {
                        id.starts_with("ok:") && id.ends_with(&format!(":{}", bundle.len()))
                    })
            }),
            _ => false,
        }),
        "the sealed bundle and the dispatch response must reach the Environment: {records:#?}"
    );

    {
        let recorded = dispatched.lock().expect("recorded dispatches");
        let call = recorded
            .first()
            .unwrap_or_else(|| panic!("the Environment never reached its dispatch: {records:#?}"));
        assert_eq!(call["action"], "submit");
        assert!(call["request"]["operation_id"].is_string());
    }

    // The immutable bundle is the largest Tool payload; sealing it inline would put megabytes in
    // every session's CONFIG record, which is what the journal ceiling exists to refuse.
    let head = brain.journal.get_head(&session_id).await.unwrap();
    let sealed = serde_json::to_string(&head.doc.prefix.tools).unwrap();
    assert!(!sealed.contains(&base64::engine::general_purpose::STANDARD.encode(bundle)));
    assert!(sealed.contains(&bundle_digest));
    brain::journal::validate_config_doc(&head.doc).expect("the sealed configuration stays bounded");
}
