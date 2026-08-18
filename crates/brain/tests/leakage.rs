//! The cross-session leakage gate (slice 5, ARCHITECTURE-v1 §2.9): two tenants on one brain,
//! adversarially interleaved, and NOTHING crosses — not workspace files, not prompt or output
//! content, not model identity, not provider keys. This is the CI form of the P1 incident
//! class (leaked accounting across tenants) and it runs on every push.
//!
//! Method: one Brain, the real local substrate (real files on disk), two sessions on DIFFERENT
//! dialects so each gets its own scripted fake — provider traffic is attributable per session
//! by construction, and any bleed shows up as the wrong fake being asked, the wrong system
//! prompt length arriving, or the wrong content in a journal.

use brain::config::Dialect;
use brain::journal::Journal;
use brain::local::LocalFactory;
use brain::provider::Provider;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ALPHA_SYSTEM: &str = "You are agent ALPHA. MARKER_ALPHA_PROMPT.";
const BETA_SYSTEM: &str = "You are agent BETA with a longer prompt. MARKER_BETA_PROMPT!!";
const ALPHA_KEY: &str = "sk-alpha-secret-key-000001";
const BETA_KEY: &str = "sk-beta-secret-key-000002";

struct Tenant {
    sid: String,
    fake: Arc<FakeProvider>,
}

async fn create(
    http: &reqwest::Client,
    base: &str,
    token: &str,
    provider: &str,
    model: &str,
    key: &str,
    system: &str,
) -> String {
    let r = http
        .post(format!("{base}/v1/sessions"))
        .bearer_auth(token)
        .json(&json!({
            "model": {"provider": provider, "name": model, "api_key": key},
            "system_prompt": system,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 201, "{}", r.text().await.unwrap());
    let v: Value = r.json().await.unwrap();
    v["id"].as_str().unwrap().to_string()
}

async fn run_turn(http: &reqwest::Client, base: &str, token: &str, sid: &str, content: &str) {
    let r = http
        .post(format!("{base}/v1/sessions/{sid}/messages"))
        .bearer_auth(token)
        .json(&json!({"content": content}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 202, "{}", r.text().await.unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let text = replay(http, base, token, sid).await;
        if text.contains("event: turn.completed") {
            return;
        }
        assert!(
            !text.contains("event: turn.failed"),
            "turn failed on {sid}:\n{text}"
        );
        assert!(Instant::now() < deadline, "turn never completed on {sid}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn replay(http: &reqwest::Client, base: &str, token: &str, sid: &str) -> String {
    http.get(format!(
        "{base}/v1/sessions/{sid}/events?after=0&follow=false"
    ))
    .bearer_auth(token)
    .send()
    .await
    .unwrap()
    .text()
    .await
    .unwrap()
}

#[tokio::test]
async fn nothing_crosses_between_two_tenants_on_one_brain() {
    let data_dir = std::env::temp_dir().join(format!("brain-leakage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);

    // Dialect-split fakes: anthropic traffic -> alpha's fake, openai traffic -> beta's fake.
    let fake_alpha = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let fake_beta = Arc::new(FakeProvider::new(Dialect::OpenAiChat));
    fake_alpha.script([
        Scripted::tool(
            "write",
            json!({"path": "/workspace/alpha.txt", "content": "SECRET_ALPHA_FILE"}),
        ),
        Scripted::Text("alpha wrote its file. MARKER_ALPHA_OUT".into()),
        // The adversarial probe: alpha reaches for beta's file BY THE SAME PATH it would have
        // inside beta's workspace. Isolation means this must not find it.
        Scripted::tool("read", json!({"path": "/workspace/beta.txt"})),
        Scripted::Text("alpha probed for beta's file. MARKER_ALPHA_PROBE_DONE".into()),
    ]);
    fake_beta.script([
        Scripted::tool(
            "write",
            json!({"path": "/workspace/beta.txt", "content": "SECRET_BETA_FILE"}),
        ),
        Scripted::Text("beta wrote its file. MARKER_BETA_OUT".into()),
    ]);
    let (fa, fb) = (fake_alpha.clone(), fake_beta.clone());
    let brain = Brain::with_parts(
        BrainConfig::default(),
        Journal::new_memory("leakage-test"),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(LocalFactory::new(&data_dir)),
        Some(Arc::new(move |d| match d {
            Dialect::AnthropicMessages => fa.clone() as Arc<dyn Provider>,
            Dialect::OpenAiChat => fb.clone() as Arc<dyn Provider>,
        })),
    );
    let token = "leakage-token".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = brain::api::router(brain::api::AppState {
        brain,
        token: token.clone(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();

    let alpha = Tenant {
        sid: create(
            &http,
            &base,
            &token,
            "anthropic",
            "model-alpha",
            ALPHA_KEY,
            ALPHA_SYSTEM,
        )
        .await,
        fake: fake_alpha,
    };
    let beta = Tenant {
        sid: create(
            &http,
            &base,
            &token,
            "openai",
            "model-beta",
            BETA_KEY,
            BETA_SYSTEM,
        )
        .await,
        fake: fake_beta,
    };

    // First turns run CONCURRENTLY — interleaving is the point.
    tokio::join!(
        run_turn(&http, &base, &token, &alpha.sid, "alpha: write your file"),
        run_turn(&http, &base, &token, &beta.sid, "beta: write your file"),
    );
    // Then alpha probes for beta's file by path.
    run_turn(&http, &base, &token, &alpha.sid, "alpha: probe").await;

    // 1. Workspaces are disjoint ON DISK: each session's file exists only in its own tree.
    let ws = |sid: &str, name: &str| data_dir.join(sid).join("workspace").join(name);
    assert!(ws(&alpha.sid, "alpha.txt").exists());
    assert!(ws(&beta.sid, "beta.txt").exists());
    assert!(
        !ws(&alpha.sid, "beta.txt").exists(),
        "beta's file must not exist in alpha's workspace"
    );
    assert!(!ws(&beta.sid, "alpha.txt").exists());

    // 2. The probe failed: alpha's read of /workspace/beta.txt found nothing, and beta's
    //    content never entered alpha's journal.
    let alpha_events = replay(&http, &base, &token, &alpha.sid).await;
    let beta_events = replay(&http, &base, &token, &beta.sid).await;
    for (events, foreign) in [
        (
            &alpha_events,
            [
                "SECRET_BETA",
                "MARKER_BETA",
                "model-beta",
                beta.sid.as_str(),
            ],
        ),
        (
            &beta_events,
            [
                "SECRET_ALPHA",
                "MARKER_ALPHA",
                "model-alpha",
                alpha.sid.as_str(),
            ],
        ),
    ] {
        for marker in foreign {
            assert!(
                !events.contains(marker),
                "foreign marker {marker:?} leaked into a journal:\n{events}"
            );
        }
    }

    // 3. Provider keys appear in NO journal, not even the session's own (BYOK custody:
    //    encrypted at create, decrypted per call, never journaled, never echoed).
    for events in [&alpha_events, &beta_events] {
        assert!(!events.contains(ALPHA_KEY) && !events.contains(BETA_KEY));
    }

    // 4. Every journaled event names its own session, nothing else's.
    for (events, sid) in [(&alpha_events, &alpha.sid), (&beta_events, &beta.sid)] {
        for line in events.lines().filter(|l| l.starts_with("data:")) {
            let v: Value = serde_json::from_str(line.trim_start_matches("data:").trim()).unwrap();
            assert_eq!(v["session_id"].as_str().unwrap(), sid.as_str());
        }
    }

    // 5. Provider traffic is attributable and sealed: each fake served exactly its own
    //    session's rounds, and every request carried exactly that session's system prompt.
    assert_eq!(
        alpha
            .fake
            .call_count
            .load(std::sync::atomic::Ordering::SeqCst),
        4
    );
    assert_eq!(
        beta.fake
            .call_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    for (t, system) in [(&alpha, ALPHA_SYSTEM), (&beta, BETA_SYSTEM)] {
        let arrivals = t.fake.arrivals.lock().unwrap();
        assert!(!arrivals.is_empty());
        for a in arrivals.iter() {
            assert_eq!(
                a.system_chars,
                system.len(),
                "a request on {}'s wire carried a foreign system prompt",
                t.sid
            );
        }
    }

    let _ = std::fs::remove_dir_all(&data_dir);
}
