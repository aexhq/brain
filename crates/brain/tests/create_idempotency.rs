use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::path::PathBuf;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aex-create-idempotency-{}-{}",
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

#[tokio::test]
async fn create_replays_one_session_and_rejects_key_reuse_with_another_body() {
    let temp = TempDir::new();
    let brain = Brain::local(temp.0.clone(), BrainConfig::default()).unwrap();
    let token = "create-idempotency-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();
    let body = json!({
        "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
        "metadata": {"test": "create-idempotency"}
    });

    let create = || {
        http.post(format!("{base}/v1/sessions"))
            .bearer_auth(&token)
            .header("Idempotency-Key", "same-create-request")
            .json(&body)
            .send()
    };
    let (first, concurrent) = tokio::join!(create(), create());
    let first = first.unwrap();
    let concurrent = concurrent.unwrap();
    assert_eq!(first.status(), 201);
    assert_eq!(concurrent.status(), 201);
    let first: Value = first.json().await.unwrap();
    let concurrent: Value = concurrent.json().await.unwrap();
    assert_eq!(first["id"], concurrent["id"]);

    let replay: Value = create().await.unwrap().json().await.unwrap();
    assert_eq!(first["id"], replay["id"]);

    let conflict = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .header("Idempotency-Key", "same-create-request")
        .json(&json!({
            "model": {"provider": "anthropic", "name": "different", "api_key": "sk-fake"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), 409);
    let conflict: Value = conflict.json().await.unwrap();
    assert_eq!(conflict["error"]["code"], "conflict");

    let unkeyed_one: Value = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let unkeyed_two: Value = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_ne!(unkeyed_one["id"], unkeyed_two["id"]);

    let list: Value = http
        .get(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["data"].as_array().unwrap().len(), 3);
}
