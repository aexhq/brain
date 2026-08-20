//! The local-mode gate, CI-runnable: the WHOLE brain loop -- session API over real HTTP,
//! SSE events, the turn loop, the journal, local tool execution in real subprocesses --
//! with only the model scripted (the fake provider). Zero cloud dependencies.
//!
//! What a pass proves: create -> message -> three model rounds -> `write` + `bash` executed
//! against the session's workspace directory -> streamed events -> persist -> replay ->
//! delete purges everything. If this is green, `cargo run --bin brain` gives a working
//! session API on localhost.

mod support;

use brain::config::Dialect;
use brain::journal::Journal;
use brain::local::LocalFactory;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("brain-local-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn wait_for<F: Fn(&[(String, Value)]) -> bool>(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    sid: &str,
    what: &str,
    pred: F,
) -> Vec<(String, Value)> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // Replay-only read each poll: also exercises journal-backed SSE continuously.
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
        let mut events = Vec::new();
        let mut kind = String::new();
        for line in text.lines() {
            if let Some(k) = line.strip_prefix("event: ") {
                kind = k.to_string();
            } else if let Some(d) = line.strip_prefix("data: ")
                && let Ok(v) = serde_json::from_str::<Value>(d)
            {
                events.push((kind.clone(), v));
            }
        }
        if pred(&events) {
            return events;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; saw {events:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "requires the bash executable used by the development adapter"
)]
async fn the_whole_local_loop_over_real_http() {
    let tmp = TempDir::new();
    let cfg = BrainConfig {
        max_concurrent_model_rounds: 8,
        max_concurrent_turns: 8,
        idle_discard: Duration::from_secs(300),
        history_budget_bytes: 1 << 20,
        max_file_bytes: 8,
        ..BrainConfig::default()
    };

    // The model, scripted: write a file, run bash over it, then finish. Everything else in
    // the loop is real.
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::tool(
            "write",
            json!({"path": "hello.txt", "content": "local-e2e-ok"}),
        ),
        Scripted::tool(
            "bash",
            json!({"command": "cat hello.txt && echo from-bash"}),
        ),
        Scripted::Text("all done".into()),
    ]);
    let factory_fake = fake.clone();
    let brain = Brain::with_parts(
        cfg,
        Journal::new_memory("brain-test"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(tmp.0.clone())),
        Some(Arc::new(move |_| {
            factory_fake.clone() as Arc<dyn brain::provider::Provider>
        })),
    );

    let token = "e2e-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();

    // Auth is enforced.
    let unauth = http
        .get(format!("{base}/v1/sessions"))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);

    // Create.
    let r = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(&token)
        .json(&json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "system_prompt": "test agent",
            "tools": {"items": [support::hand_tool("write"), support::hand_tool("bash")]}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201, "{}", r.text().await.unwrap());
    let ses: Value = r.json().await.unwrap();
    let sid = ses["id"].as_str().unwrap().to_string();

    // Message -> 202 with a turn id.
    let r = http
        .post(format!("{base}/v1/sessions/{sid}/messages"))
        .bearer_auth(&token)
        .json(&json!({"content": "write the file then cat it"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);

    // The turn completes; the REAL bash subprocess saw the REAL file the write tool made.
    let events = wait_for(&http, &base, &token, &sid, "turn.completed", |evs| {
        evs.iter().any(|(k, _)| k == "turn.completed")
    })
    .await;
    let results: Vec<&Value> = events
        .iter()
        .filter(|(k, _)| k == "tool.result")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(results.len(), 2, "write + bash");
    assert!(
        results[1]["output_preview"]
            .as_str()
            .unwrap()
            .contains("local-e2e-ok"),
        "bash must read the file write created: {}",
        results[1]["output_preview"]
    );
    assert!(
        results[1]["output_preview"]
            .as_str()
            .unwrap()
            .contains("from-bash")
    );
    assert!(events.iter().any(|(k, _)| k == "assistant.message"));
    let seqs: Vec<u64> = events
        .iter()
        .filter_map(|(_, v)| v["seq"].as_u64())
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "replay seqs strictly increase: {seqs:?}"
    );
    fake.assert_drained(3, "local e2e").unwrap();

    // The workspace really is on disk where the operator can see it.
    let on_disk = tmp.0.join(&sid).join("workspace").join("hello.txt");
    assert_eq!(std::fs::read_to_string(&on_disk).unwrap(), "local-e2e-ok");

    // Public files surface: binary-safe overwrite, deterministic live listing, exact
    // download, path confinement, and the configured request ceiling.
    let file_url = format!("{base}/v1/sessions/{sid}/files/%2Fworkspace%2Fupload.bin");
    let payload = vec![0, 1, 2, 0xff];
    let r = http
        .put(&file_url)
        .bearer_auth(&token)
        .header("content-type", "application/octet-stream")
        .body(payload.clone())
        .send()
        .await
        .unwrap();
    let status = r.status();
    let body = r.bytes().await.unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let entry: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(entry["path"], "/workspace/upload.bin");
    assert_eq!(entry["size"], payload.len());

    let listing: Value = http
        .get(format!(
            "{base}/v1/sessions/{sid}/files?path=%2Fworkspace&recursive=true"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listing["source"], "hand");
    let paths: Vec<_> = listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(paths.contains(&"/workspace/hello.txt"));
    assert!(paths.contains(&"/workspace/upload.bin"));

    let r = http
        .get(&file_url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.bytes().await.unwrap().as_ref(), payload.as_slice());

    let r = http
        .get(format!(
            "{base}/v1/sessions/{sid}/files?path=%2Fworkspace%2F..%2Fescape"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let r = http
        .put(&file_url)
        .bearer_auth(&token)
        .body(vec![0u8; 9])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 413);

    // Persist -> artifact metadata (no download URL in local mode; the file is local).
    let r = http
        .post(format!("{base}/v1/sessions/{sid}/persist"))
        .bearer_auth(&token)
        .json(&json!({"name": "hello.txt", "path": "hello.txt"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201, "{}", r.text().await.unwrap());
    let art: Value = r.json().await.unwrap();
    assert_eq!(art["bytes"], 12);
    let r = http
        .get(format!("{base}/v1/sessions/{sid}/artifacts"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let list: Value = r.json().await.unwrap();
    assert_eq!(list["data"][0]["name"], "hello.txt");

    // Busy interlock: a second message while idle is fine, while busy is 409 -- prove the
    // level by sending one that completes instantly.
    fake.script([Scripted::Text("second turn".into())]);
    let r = http
        .post(format!("{base}/v1/sessions/{sid}/messages"))
        .bearer_auth(&token)
        .json(&json!({"content": "again"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    wait_for(&http, &base, &token, &sid, "second turn.completed", |evs| {
        evs.iter().filter(|(k, _)| k == "turn.completed").count() >= 2
    })
    .await;

    // Delete purges the journal AND the workspace directory.
    let r = http
        .delete(format!("{base}/v1/sessions/{sid}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    let r = http
        .get(format!("{base}/v1/sessions/{sid}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    assert!(
        !tmp.0.join(&sid).exists(),
        "workspace directory must be purged on delete"
    );
}
