use brain::adapter::DisabledToolExecutor;
use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig, BrainServices};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "brain-message-idempotency-{}-{}",
            std::process::id(),
            brain::wall_ms()
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

async fn events(http: &reqwest::Client, base: &str, token: &str, session_id: &str) -> Vec<Value> {
    let text = http
        .get(format!(
            "{base}/v1/sessions/{session_id}/events?after=0&follow=false"
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

#[tokio::test]
async fn message_key_replays_while_active_after_completion_and_after_cold_hydration() {
    let _temp = TempDir::new();
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::Text("one durable response".into())]);
    fake.tokens_per_second.store(5, Ordering::Relaxed);
    let factory_fake = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_millis(40),
            ..BrainConfig::default()
        },
        Journal::new_memory("message-idempotency-test"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );

    let token = "message-idempotency-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
        require_tenant: false,
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();
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
    let session_id = created["id"].as_str().unwrap();
    let body = json!({"content": "hello", "metadata": {"request": "one"}});

    let first = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(&token)
        .header("Idempotency-Key", "same-message-request")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 202);
    let first: Value = first.json().await.unwrap();

    // Admission returns before the paced provider finishes, so this exercises replay while the
    // resident state is parked in the running turn.
    let active_replay = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(&token)
        .header("Idempotency-Key", "same-message-request")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(active_replay.status(), 202);
    let active_replay: Value = active_replay.json().await.unwrap();
    assert_eq!(active_replay, first);

    let conflict = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(&token)
        .header("Idempotency-Key", "same-message-request")
        .json(&json!({"content": "different", "metadata": {"request": "one"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), 409);
    let conflict: Value = conflict.json().await.unwrap();
    assert_eq!(conflict["error"]["code"], "conflict");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if events(&http, &base, &token, session_id)
            .await
            .iter()
            .any(|event| event["type"] == "turn.completed")
        {
            break;
        }
        assert!(Instant::now() < deadline, "turn did not complete");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let completed_replay: Value = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(&token)
        .header("Idempotency-Key", "same-message-request")
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(completed_replay, first);

    // Let the actor discard, then prove the persisted hashes rebuild the same acceptance.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let cold_replay: Value = http
        .post(format!("{base}/v1/sessions/{session_id}/messages"))
        .bearer_auth(&token)
        .header("Idempotency-Key", "same-message-request")
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cold_replay, first);

    let replayed_events = events(&http, &base, &token, session_id).await;
    assert_eq!(
        replayed_events
            .iter()
            .filter(|event| event["type"] == "turn.started")
            .count(),
        1
    );
    assert_eq!(
        replayed_events
            .iter()
            .filter(|event| event["type"] == "turn.completed")
            .count(),
        1
    );
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
}
