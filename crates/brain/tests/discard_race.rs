//! Regression: a message arriving exactly when the idle timer discards the session's actor
//! must succeed, not 500. The density benchmark exposed this as "actor dropped the reply"
//! (the idle branch could win a select round against an already-buffered command and exit
//! without answering it). The fix is close-then-drain in the actor plus a one-shot send retry
//! in the callers; this test forces the collision hundreds of times.

use brain::config::Dialect;
use brain::journal::Journal;
use brain::provider::Provider;
use brain::provider::fake::{FakeMode, FakeProvider};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn a_message_racing_the_idle_discard_always_lands() {
    let data_dir = std::env::temp_dir().join(format!("brain-discard-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.set_mode(FakeMode::Policy {
        tool_rounds: 0,
        parallel: 1,
        tool: "bash".into(),
        text_bytes: 16,
    });
    let f = fake.clone();
    let brain = Brain::with_parts(
        BrainConfig {
            // Aggressive enough that every inter-message pause crosses it.
            idle_discard: Duration::from_millis(2),
            ..BrainConfig::default()
        },
        Journal::new_memory("discard-race"),
        Arc::new(brain::keys::PlainCustody),
        Some(Arc::new(move |_| f.clone() as Arc<dyn Provider>)),
    );
    let token = "race-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
        require_tenant: false,
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();

    let r = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&json!({"model": {"provider": "anthropic", "name": "race", "api_key": "sk-x"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 201, "{}", r.text().await.unwrap());
    let sid = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Every message lands in a fresh discard window; before the fix a run of 200 reliably
    // produced several "actor dropped the reply" 500s.
    for i in 0..200 {
        tokio::time::sleep(Duration::from_millis(if i % 2 == 0 { 2 } else { 3 })).await;
        let r = http
            .post(format!("{base}/v1/sessions/{sid}/messages"))
            .bearer_auth(&token)
            .json(&json!({"content": format!("m{i}")}))
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let body = r.text().await.unwrap();
        assert!(
            status == 202 || status == 409,
            "message {i}: {status} {body}"
        );
        // 409 (turn still running) means the previous turn hasn't finished; wait it out.
        if status == 409 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    let _ = std::fs::remove_dir_all(&data_dir);
}
