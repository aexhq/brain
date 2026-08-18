//! The composability proof: a THIRD-PARTY hand adapter, written against nothing but the
//! public `brain::adapter` traits, composed into a running brain via `Brain::with_parts`,
//! driven over real HTTP. If this compiles and passes, anyone can bring their own substrate
//! (a k8s pod, an SSH box, a different cloud) without touching the core.
//!
//! The adapter here is deliberately tiny: an in-memory "echo" substrate that records every
//! call, answers `bash` with a canned transcript, and persists from an in-memory file map.

use brain::adapter::{
    ArtifactMeta, CallOutcome, CallRequest, HandAdapter, HandFactory, HandSpec, LostReport,
    OutputSink, SeedFile,
};
use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// The whole custom substrate: seeds + calls recorded in memory.
#[derive(Default)]
struct EchoHand {
    files: Mutex<HashMap<String, Vec<u8>>>,
    calls: Mutex<Vec<String>>,
    ready_count: Mutex<u32>,
}

#[async_trait::async_trait]
impl HandAdapter for EchoHand {
    async fn ensure_ready(&self) -> brain::Result<Option<LostReport>> {
        *self.ready_count.lock().unwrap() += 1;
        Ok(None)
    }

    async fn call(
        &self,
        req: CallRequest,
        _cancel: CancellationToken,
        sink: OutputSink,
    ) -> CallOutcome {
        self.calls.lock().unwrap().push(req.tool.clone());
        // Stream something so tool.output events flow end to end.
        sink("stdout", 0, "echo:".into());
        let text = format!(
            "echo:{}:{}",
            req.tool,
            req.input["command"].as_str().unwrap_or("?")
        );
        sink("stdout", 5, text[5..].to_string());
        CallOutcome {
            outcome: "completed".into(),
            content: text,
            is_error: false,
            exit_code: Some(0),
            duration_ms: 1,
            truncated: false,
        }
    }

    async fn release(&self) -> brain::Result<()> {
        Ok(())
    }

    async fn persist(
        &self,
        name: &str,
        path: &str,
        media_type: Option<&str>,
    ) -> brain::Result<ArtifactMeta> {
        let files = self.files.lock().unwrap();
        let bytes = files
            .get(path)
            .ok_or_else(|| brain::BrainError::Hand(format!("no such file {path}")))?;
        Ok(ArtifactMeta {
            bytes: bytes.len() as u64,
            sha256: "0".repeat(64),
            media_type: media_type.unwrap_or("application/octet-stream").into(),
            location: format!("echo://{name}"),
        })
    }

    fn hand_info(&self) -> aex_contracts::session::HandInfo {
        use aex_contracts::session::{HandInfo, HandShape, HandState};
        HandInfo {
            generation: Some(1),
            last_sync_at: None,
            live_jobs: Some(0),
            shape: HandShape::X1gb,
            started_at: None,
            state: HandState::Ready,
            wall_deadline_at: None,
        }
    }

    fn state(&self) -> Value {
        json!({"echo": true})
    }
}

struct EchoFactory {
    opened: Arc<EchoHand>,
}

#[async_trait::async_trait]
impl HandFactory for EchoFactory {
    async fn create(&self, _spec: &HandSpec, seeds: &[SeedFile<'_>]) -> brain::Result<Value> {
        let mut files = self.opened.files.lock().unwrap();
        for s in seeds {
            files.insert(s.path.to_string(), s.bytes.to_vec());
        }
        Ok(json!({"echo": "created"}))
    }

    async fn open(&self, _spec: &HandSpec, state: Value) -> brain::Result<Arc<dyn HandAdapter>> {
        // The state persisted at create round-trips back through the journal head.
        assert_eq!(
            state["echo"],
            json!("created"),
            "adapter state must round-trip"
        );
        Ok(self.opened.clone())
    }

    async fn purge(&self, _session_id: &str) -> brain::Result<()> {
        self.opened.files.lock().unwrap().clear();
        Ok(())
    }

    async fn artifact_url(&self, _session_id: &str, location: &str) -> Option<String> {
        Some(format!("https://example.invalid/{location}"))
    }
}

#[tokio::test]
async fn a_third_party_adapter_composes_via_public_api_only() {
    let echo = Arc::new(EchoHand::default());
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::tool("bash", json!({"command": "hello-from-custom-substrate"})),
        Scripted::Text("done".into()),
    ]);
    let factory_fake = fake.clone();
    let brain = Brain::with_parts(
        BrainConfig::default(),
        Journal::new_memory("brain-custom-test"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(EchoFactory {
            opened: echo.clone(),
        }),
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );

    let token = "custom-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();
    let r = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "files": [{"path": "seed.txt", "content_base64": "c2VlZGVk"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201, "{}", r.text().await.unwrap());
    let ses: Value = r.json().await.unwrap();
    let sid = ses["id"].as_str().unwrap().to_string();

    // The factory saw the seed at create.
    assert_eq!(
        echo.files
            .lock()
            .unwrap()
            .get("seed.txt")
            .map(|b| b.as_slice()),
        Some(b"seeded".as_slice())
    );

    let r = http
        .post(format!("{base}/v1/sessions/{sid}/messages"))
        .bearer_auth(&token)
        .json(&json!({"content": "run it"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);

    // Wait for the turn, replay-only.
    let deadline = Instant::now() + Duration::from_secs(20);
    let events = loop {
        let text = http
            .get(format!(
                "{base}/v1/sessions/{sid}/events?after=0&follow=false"
            ))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        if text.contains("event: turn.completed") {
            break text;
        }
        assert!(
            Instant::now() < deadline,
            "turn never completed; saw:\n{text}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(
        events.contains("echo:bash:hello-from-custom-substrate"),
        "the custom substrate's result must reach the event stream:\n{events}"
    );
    assert!(
        *echo.ready_count.lock().unwrap() >= 1,
        "ensure_ready went through the adapter"
    );
    assert_eq!(echo.calls.lock().unwrap().as_slice(), ["bash"]);

    // Persist through the adapter; the artifact URL comes from the factory.
    let r = http
        .post(format!("{base}/v1/sessions/{sid}/persist"))
        .bearer_auth(&token)
        .json(&json!({"name": "seed.txt", "path": "seed.txt"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201, "{}", r.text().await.unwrap());
    let art: Value = r.json().await.unwrap();
    assert_eq!(art["bytes"], 6);
    assert!(
        art["download_url"]
            .as_str()
            .unwrap()
            .starts_with("https://example.invalid/echo://"),
        "factory-minted url: {}",
        art["download_url"]
    );

    // Delete purges through the factory.
    let r = http
        .delete(format!("{base}/v1/sessions/{sid}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    assert!(
        echo.files.lock().unwrap().is_empty(),
        "purge reached the adapter"
    );
}
