//! Managed-web gate over real HTTP on both sides: session API -> fake model -> web runtime ->
//! deterministic search/content origins. Only the model and search corpus are scripted.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use brain::config::{Dialect, ProviderKey};
use brain::journal::Journal;
use brain::local::LocalFactory;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Default)]
struct SearchState {
    calls: AtomicU64,
}

async fn search(
    State(state): State<Arc<SearchState>>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    if headers.get("x-api-key").and_then(|v| v.to_str().ok()) != Some("test-search-key") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.calls.fetch_add(1, Ordering::SeqCst);
    axum::Json(json!({
        "organic": [
            {"title":"Alpha", "link":"https://example.com/a", "snippet":"first", "date":"Aug 19, 2026"},
            {"title":"Beta", "link":"https://example.com/b", "snippet":"second"}
        ],
        "echo": body
    }))
    .into_response()
}

async fn redirect() -> Response {
    (StatusCode::FOUND, [(header::LOCATION, "/page")]).into_response()
}

async fn page() -> Html<&'static str> {
    Html(
        "<html><head><title>Gate</title></head><body><h1>Readable</h1><p>web fetch works</p></body></html>",
    )
}

async fn slow() -> &'static str {
    tokio::time::sleep(Duration::from_secs(5)).await;
    "late"
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aex-web-e2e-{}-{}",
            std::process::id(),
            brain::mint_id("t", 8)
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

async fn listen(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn replay(http: &reqwest::Client, base: &str, token: &str, sid: &str) -> Vec<Value> {
    let text = http
        .get(format!(
            "{base}/v1/sessions/{sid}/events?after=0&follow=false"
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
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[tokio::test]
async fn search_fetch_redirect_cancel_and_secret_hygiene() {
    let search_state = Arc::new(SearchState::default());
    let search_base = listen(
        Router::new()
            .route("/search", post(search))
            .with_state(search_state.clone()),
    )
    .await;
    let content_base = listen(
        Router::new()
            .route("/page", get(page))
            .route("/redirect", get(redirect))
            .route("/slow", get(slow)),
    )
    .await;

    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::ToolCalls(vec![
            (
                "call_search".into(),
                "web_search".into(),
                json!({"query":"aex runtime", "num":2}),
            ),
            (
                "call_fetch".into(),
                "web_fetch".into(),
                json!({"url":format!("{content_base}/redirect"), "max_chars":1000}),
            ),
        ]),
        Scripted::Text("web tools complete".into()),
        Scripted::tool("web_fetch", json!({"url":format!("{content_base}/slow")})),
    ]);

    let temp = TempDir::new();
    let cfg = BrainConfig {
        outbound_allow_private: true,
        web_search_endpoint: format!("{search_base}/search"),
        web_search_api_key: Some(ProviderKey::new("test-search-key")),
        web_call_timeout: Duration::from_secs(10),
        idle_discard: Duration::from_secs(300),
        ..BrainConfig::default()
    };
    let provider = fake.clone();
    let brain = Brain::with_parts(
        cfg,
        Journal::new_memory("web-test"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(temp.0.clone())),
        Some(Arc::new(move |_| {
            provider.clone() as Arc<dyn brain::provider::Provider>
        })),
    );
    let token = "web-token".to_string();
    let base = listen(brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    }))
    .await;
    let http = reqwest::Client::new();

    let response = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&json!({
            "model":{"provider":"anthropic", "name":"scripted", "api_key":"fake"},
            "tools":{"builtin":["web_search", "web_fetch"]}
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.bytes().await.unwrap();
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));
    let session: Value = serde_json::from_slice(&body).unwrap();
    let sid = session["id"].as_str().unwrap();

    assert_eq!(
        http.post(format!("{base}/v1/sessions/{sid}/messages"))
            .bearer_auth(&token)
            .json(&json!({"content":"search and fetch"}))
            .send()
            .await
            .unwrap()
            .status(),
        202
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let events = loop {
        let events = replay(&http, &base, &token, sid).await;
        if events.iter().any(|event| event["type"] == "turn.completed") {
            break events;
        }
        assert!(Instant::now() < deadline, "web turn did not complete");
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let results: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "tool.result")
        .collect();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|event| event["outcome"] == "completed"));
    assert!(
        results
            .iter()
            .any(|event| event["output_preview"].as_str().unwrap().contains("Alpha"))
    );
    assert!(results.iter().any(|event| {
        event["output_preview"]
            .as_str()
            .unwrap()
            .contains("web fetch works")
    }));
    assert_eq!(search_state.calls.load(Ordering::SeqCst), 1);
    let journal = serde_json::to_string(&events).unwrap();
    assert!(!journal.contains("test-search-key"));

    // A running fetch drops promptly when the root turn is cancelled.
    http.post(format!("{base}/v1/sessions/{sid}/messages"))
        .bearer_auth(&token)
        .json(&json!({"content":"start slow fetch"}))
        .send()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    http.post(format!("{base}/v1/sessions/{sid}/cancel"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let events = replay(&http, &base, &token, sid).await;
        if events
            .iter()
            .filter(|event| event["type"] == "turn.completed")
            .count()
            >= 2
        {
            let slow = events
                .iter()
                .rev()
                .find(|event| event["type"] == "tool.result")
                .unwrap();
            assert_eq!(slow["outcome"], "cancelled");
            break;
        }
        assert!(Instant::now() < deadline, "cancel did not stop web fetch");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    fake.assert_drained(3, "web e2e").unwrap();
}

#[tokio::test]
async fn search_is_refused_at_create_without_plane_credential() {
    let temp = TempDir::new();
    let brain = Brain::with_parts(
        BrainConfig {
            web_search_api_key: None,
            outbound_allow_private: true,
            ..BrainConfig::default()
        },
        Journal::new_memory("web-no-key"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(temp.0.clone())),
        None,
    );
    let token = "web-no-key-token".to_string();
    let base = listen(brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    }))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(token)
        .json(&json!({
            "model":{"provider":"anthropic", "name":"scripted", "api_key":"fake"},
            "tools":{"builtin":["web_search"]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("requires the managed search credential")
    );
}
