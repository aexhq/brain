//! CI-runnable MCP coverage: real servers (2026-07-28 stateless and the legacy
//! `initialize` + `Mcp-Session-Id` adapter) exercised over the real HTTP surface, with only
//! the model scripted (the fake provider).
//!
//! What a pass proves:
//! - `tools/list` runs at create, the tools are sealed as namespaced `server__tool` decls, and
//!   a scripted call dispatches through `McpRuntime` to the wire and back;
//! - parallel calls, `isError` results, and MRTR `input_required` all map to ordinary
//!   `tool.result` events;
//! - per-server headers reach the server but never the journal in plaintext (custody);
//! - `auto` negotiation falls back from the modern probe to the legacy adapter, and an
//!   allowlist that names a tool the server does not serve fails the create;
//! - the sealed prefix digest is stable across a discard + rehydrate boundary (same prefix,
//!   same digest -- no I/O at hydrate, no schema drift).

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use brain::config::Dialect;
use brain::journal::Journal;
use brain::local::LocalFactory;
use brain::provider::fake::{FakeProvider, Scripted};
use brain::session::{Brain, BrainConfig};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------
// A hand-rolled MCP server that speaks both the 2026-07-28 stateless revision and the legacy
// `initialize` + `Mcp-Session-Id` Streamable HTTP revision. Only what the client calls.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// 2026-07-28 stateless: no initialize, no session; `tools/list` and `tools/call` only.
    V2,
    /// 2025-06-18: `initialize` mints a session id; every later request needs it in the
    /// `Mcp-Session-Id` header or it gets a 400 (the legacy-shaped failure the client's
    /// backward-compat probe must fall back on).
    Legacy,
}

fn tool_list() -> Vec<Value> {
    vec![
        json!({"name": "echo", "description": "echo the message", "inputSchema": {"type":"object","properties":{"msg":{"type":"string"}},"required":["msg"]}}),
        json!({"name": "boom", "description": "always fails", "inputSchema": {"type":"object"}}),
        json!({"name": "ask", "description": "needs interactive input", "inputSchema": {"type":"object"}}),
    ]
}

struct McpState {
    mode: Mode,
    /// Legacy session id once minted (legacy mode only).
    session: Mutex<Option<String>>,
    /// `tools/call` params, in arrival order.
    calls: Mutex<Vec<Value>>,
    /// Every request's headers, in arrival order, lowercased.
    req_headers: Mutex<Vec<Vec<(String, String)>>>,
    init_count: AtomicU64,
}

impl McpState {
    fn record(&self, headers: &HeaderMap) {
        let mut hs: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_ascii_lowercase(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        hs.sort();
        self.req_headers.lock().unwrap().push(hs);
    }
    fn saw_header(&self, name: &str, value: &str) -> bool {
        self.req_headers
            .lock()
            .unwrap()
            .iter()
            .any(|hs| hs.iter().any(|(k, v)| k == name && v == value))
    }
    fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }
}

fn json_response(status: StatusCode, headers: Vec<(String, String)>, body: Value) -> Response {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        map.insert(
            axum::http::header::HeaderName::try_from(k).unwrap(),
            axum::http::header::HeaderValue::try_from(v).unwrap(),
        );
    }
    map.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::header::HeaderValue::from_static("application/json"),
    );
    (status, map, body.to_string()).into_response()
}

async fn mcp_handler(State(st): State<Arc<McpState>>, headers: HeaderMap, body: Bytes) -> Response {
    st.record(&headers);
    let req: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                vec![],
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}),
            );
        }
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let has_id = req.get("id").is_some();

    let result = |v: Value| json!({"jsonrpc":"2.0","id":id,"result":v});
    let error =
        |code: i64, msg: &str| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}});

    match (st.mode, method) {
        // ---- legacy initialize / initialized -----------------------------------------------
        (Mode::Legacy, "initialize") => {
            let version = req["params"]["protocolVersion"].as_str().unwrap_or("");
            if version != "2025-06-18" {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    vec![],
                    error(-32022, "unsupported protocol version"),
                );
            }
            let sid = "legacy-session-1".to_string();
            *st.session.lock().unwrap() = Some(sid.clone());
            st.init_count.fetch_add(1, Ordering::SeqCst);
            json_response(
                StatusCode::OK,
                vec![("mcp-session-id".into(), sid)],
                result(json!({"protocolVersion":"2025-06-18","capabilities":{}})),
            )
        }
        (Mode::Legacy, "notifications/initialized") => {
            json_response(StatusCode::ACCEPTED, vec![], Value::Null)
        }

        // ---- everything else needs the session in legacy mode ------------------------------
        (Mode::Legacy, "tools/list" | "tools/call") => {
            let got = headers
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let want = st.session.lock().unwrap().clone();
            if want.is_none() || got != want {
                return json_response(
                    StatusCode::BAD_REQUEST,
                    vec![],
                    error(-32000, "session required"),
                );
            }
            dispatch(&st, method, &req, has_id, result, error)
        }

        // ---- v2 stateless -------------------------------------------------------------------
        (Mode::V2, "tools/list" | "tools/call") => {
            dispatch(&st, method, &req, has_id, result, error)
        }

        // ---- anything else is a method-not-found -------------------------------------------
        _ => json_response(
            StatusCode::NOT_FOUND,
            vec![],
            error(-32601, "method not found"),
        ),
    }
}

fn dispatch(
    st: &Arc<McpState>,
    method: &str,
    req: &Value,
    has_id: bool,
    result: impl Fn(Value) -> Value,
    error: impl Fn(i64, &str) -> Value,
) -> Response {
    if !has_id {
        // A notification other than initialized: the client declares zero capabilities, so a
        // server-initiated request is refused; a bare notification needs no reply body.
        return json_response(StatusCode::ACCEPTED, vec![], Value::Null);
    }
    match method {
        "tools/list" => json_response(
            StatusCode::OK,
            vec![],
            result(json!({"tools": tool_list()})),
        ),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(Value::Null);
            st.calls.lock().unwrap().push(params.clone());
            let name = params["name"].as_str().unwrap_or("");
            let out = match name {
                "echo" => {
                    let msg = params["arguments"]["msg"].as_str().unwrap_or("");
                    json!({"content":[{"type":"text","text":format!("echo:{msg}")}]})
                }
                "boom" => json!({"isError":true,"content":[{"type":"text","text":"kaput"}]}),
                "ask" => {
                    json!({"resultType":"input_required","inputRequests":[{"type":"message","message":"need more"}]})
                }
                other => {
                    return json_response(
                        StatusCode::OK,
                        vec![],
                        error(-32602, &format!("unknown tool {other}")),
                    );
                }
            };
            json_response(StatusCode::OK, vec![], result(out))
        }
        _ => json_response(
            StatusCode::NOT_FOUND,
            vec![],
            error(-32601, "method not found"),
        ),
    }
}

async fn serve(mode: Mode) -> (String, Arc<McpState>) {
    let st = Arc::new(McpState {
        mode,
        session: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
        req_headers: Mutex::new(Vec::new()),
        init_count: AtomicU64::new(0),
    });
    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/mcp"), st)
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        // Unique per harness: the tests in this binary run concurrently in ONE process, so a
        // pid-keyed dir is shared -- and the remove_dir_all here (and in Drop) would delete a
        // sibling test's live session workspace mid-create.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "brain-mcp-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
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
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A brain in permissive-outbound mode, scripted fake provider, real HTTP API.
struct Harness {
    brain: Arc<Brain>,
    fake: Arc<FakeProvider>,
    base: String,
    http: reqwest::Client,
    token: String,
    _tmp: TempDir,
}

impl Harness {
    async fn new() -> Self {
        let tmp = TempDir::new();
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        let f = fake.clone();
        let brain = Brain::with_parts(
            BrainConfig {
                idle_discard: Duration::from_millis(200),
                outbound_allow_private: true, // a developer's MCP server is on 127.0.0.1
                ..BrainConfig::default()
            },
            Journal::new_memory("mcp-e2e"),
            Arc::new(brain::keys::PlainCustody),
            Arc::new(LocalFactory::new(tmp.0.clone())),
            Some(Arc::new(move |_| {
                f.clone() as Arc<dyn brain::provider::Provider>
            })),
        );
        let token = "mcp-token".to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = brain::api::router(brain::api::AppState {
            brain: brain.clone(),
            token: token.clone(),
        });
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = reqwest::Client::new();
        Harness {
            brain,
            fake,
            base,
            http,
            token,
            _tmp: tmp,
        }
    }

    async fn create(&self, body: Value) -> (reqwest::StatusCode, Value) {
        let r = self
            .http
            .post(format!("{}/v1/sessions", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let v = r.json::<Value>().await.unwrap();
        (status, v)
    }

    /// Creates a session and asserts the API accepted it; a failure carries the body.
    async fn create_ok(&self, body: Value) -> Value {
        let (status, v) = self.create(body).await;
        assert_eq!(status, reqwest::StatusCode::CREATED, "create failed: {v}");
        v
    }

    async fn send(&self, sid: &str, content: &str) -> reqwest::StatusCode {
        self.http
            .post(format!("{}/v1/sessions/{sid}/messages", self.base))
            .bearer_auth(&self.token)
            .json(&json!({"content": content}))
            .send()
            .await
            .unwrap()
            .status()
    }
}

// ---------------------------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn mcp_tools_dispatch_over_the_real_http_surface() {
    let (mcp_url, mcp_st) = serve(Mode::V2).await;
    let h = Harness::new().await;

    // Create with an MCP server carrying a credential header; the sealed tools must be
    // namespaced `svc__echo` etc.
    let (status, created) = h
        .create(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "system_prompt": "mcp agent",
            "tools": {
                "mcp": [{
                    "name": "svc",
                    "url": mcp_url,
                    "protocol": "2026-07",
                    "headers": {"authorization": "Bearer topsecret"}
                }]
            }
        }))
        .await;
    assert_eq!(status, reqwest::StatusCode::CREATED, "{created}");
    let sid = created["id"].as_str().unwrap().to_string();

    // The credential header reached the MCP server (every request, v2 stateless) ...
    assert!(mcp_st.saw_header("authorization", "Bearer topsecret"));

    // ... but never the journal in plaintext: custody carries it, the HEAD carries a blob.
    let head = h.brain.journal.get_head(&sid).await.unwrap();
    let head_json = serde_json::to_value(&head.doc).unwrap();
    let head_text = head_json.to_string();
    assert!(
        !head_text.contains("topsecret"),
        "credential leaked into the journal HEAD"
    );
    assert_eq!(head.doc.prefix.mcp.len(), 1, "one sealed server");
    assert_eq!(head.doc.prefix.mcp[0].spec_version, "2026-07-28");
    let sealed: Vec<String> = head
        .doc
        .prefix
        .mcp_tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(sealed, vec!["svc__echo", "svc__boom", "svc__ask"]);

    // One scripted turn: two MCP calls (echo + boom) in one message (parallel).
    h.fake.script([
        Scripted::ToolCalls(vec![
            ("c1".into(), "svc__echo".into(), json!({"msg": "hi"})),
            ("c2".into(), "svc__boom".into(), json!({})),
        ]),
        Scripted::Text("done".into()),
    ]);
    assert_eq!(
        h.send(&sid, "run the tools").await,
        reqwest::StatusCode::ACCEPTED
    );

    let events = wait_for(&h.http, &h.base, &h.token, &sid, "turn.completed", |evs| {
        evs.iter().any(|(k, _)| k == "turn.completed")
    })
    .await;

    // The MCP server actually received the two calls, with the right remote names and args.
    // They were dispatched concurrently, so arrival order is not asserted.
    let calls = mcp_st.calls();
    assert_eq!(calls.len(), 2, "echo + boom reached the server: {calls:?}");
    let echo = calls.iter().find(|c| c["name"] == "echo").unwrap();
    assert_eq!(echo["arguments"]["msg"], "hi");
    assert!(calls.iter().any(|c| c["name"] == "boom"));

    // Each maps to a tool.result: echo completed with content, boom failed (is_error).
    let results: Vec<&Value> = events
        .iter()
        .filter(|(k, _)| k == "tool.result")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(results.len(), 2, "echo + boom: {results:?}");
    let echo = results.iter().find(|r| r["name"] == "svc__echo").unwrap();
    assert_eq!(echo["outcome"], "completed");
    assert!(echo["output_preview"].as_str().unwrap().contains("echo:hi"));
    let boom = results.iter().find(|r| r["name"] == "svc__boom").unwrap();
    assert_eq!(boom["outcome"], "failed");
    // `error` on a failed tool.result is the preview string (present, non-empty); the wire
    // `is_error` flag becomes the outcome, never a bool field on the event.
    assert!(
        boom["error"].as_str().is_some_and(|s| s.contains("kaput")),
        "boom must surface its error text: {boom}"
    );

    h.fake.assert_drained(2, "mcp e2e v2").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn input_required_maps_to_a_structured_tool_failure() {
    let (mcp_url, mcp_st) = serve(Mode::V2).await;
    let h = Harness::new().await;
    let created = h
        .create_ok(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"mcp": [{"name": "svc", "url": mcp_url}]}
        }))
        .await;
    let sid = created["id"].as_str().unwrap().to_string();

    h.fake.script([
        Scripted::tool("svc__ask", json!({})),
        Scripted::Text("done".into()),
    ]);
    assert_eq!(h.send(&sid, "ask it").await, reqwest::StatusCode::ACCEPTED);
    let events = wait_for(&h.http, &h.base, &h.token, &sid, "turn.completed", |evs| {
        evs.iter().any(|(k, _)| k == "turn.completed")
    })
    .await;
    let ask = events
        .iter()
        .filter(|(k, _)| k == "tool.result")
        .map(|(_, v)| v)
        .find(|r| r["name"] == "svc__ask")
        .unwrap();
    assert_eq!(ask["outcome"], "failed");
    assert!(
        ask["output_preview"]
            .as_str()
            .unwrap()
            .contains("interactive input"),
        "input_required must surface as a structured failure"
    );
    assert_eq!(mcp_st.calls().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_negotiation_falls_back_to_legacy_and_handles_is_error() {
    // A legacy server: the modern probe gets a 400 "session required", so `auto` must fall
    // back to initialize + session id.
    let (mcp_url, mcp_st) = serve(Mode::Legacy).await;
    let h = Harness::new().await;
    let (status, created) = h
        .create(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"mcp": [{"name": "old", "url": mcp_url, "protocol": "auto"}]}
        }))
        .await;
    assert_eq!(status, reqwest::StatusCode::CREATED, "{created}");
    let sid = created["id"].as_str().unwrap().to_string();

    // Negotiation ran the legacy handshake exactly once.
    assert_eq!(mcp_st.init_count.load(Ordering::SeqCst), 1);
    let head = h.brain.journal.get_head(&sid).await.unwrap();
    assert_eq!(head.doc.prefix.mcp[0].spec_version, "2025-06-18");

    h.fake.script([
        Scripted::tool("old__echo", json!({"msg": "legacy"})),
        Scripted::Text("done".into()),
    ]);
    assert_eq!(h.send(&sid, "go").await, reqwest::StatusCode::ACCEPTED);
    let events = wait_for(&h.http, &h.base, &h.token, &sid, "turn.completed", |evs| {
        evs.iter().any(|(k, _)| k == "turn.completed")
    })
    .await;
    let echo = events
        .iter()
        .filter(|(k, _)| k == "tool.result")
        .map(|(_, v)| v)
        .find(|r| r["name"] == "old__echo")
        .unwrap();
    assert_eq!(echo["outcome"], "completed");
    assert!(
        echo["output_preview"]
            .as_str()
            .unwrap()
            .contains("echo:legacy")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowlist_naming_an_unserved_tool_fails_the_create() {
    let (mcp_url, _) = serve(Mode::V2).await;
    let h = Harness::new().await;
    let (status, body) = h
        .create(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"mcp": [{
                "name": "svc",
                "url": mcp_url,
                "allowed_tools": ["echo", "not_really_there"]
            }]}
        }))
        .await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not served by tools/list"),
        "missing allowlisted tool must fail create: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowlist_filters_and_a_bad_server_fails_the_create() {
    let (mcp_url, _) = serve(Mode::V2).await;
    let h = Harness::new().await;
    // Only echo is allowlisted: the sealed set must be exactly `svc__echo`.
    let created = h
        .create_ok(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"mcp": [{
                "name": "svc",
                "url": mcp_url,
                "allowed_tools": ["echo"]
            }]}
        }))
        .await;
    let sid = created["id"].as_str().unwrap().to_string();
    let head = h.brain.journal.get_head(&sid).await.unwrap();
    let sealed: Vec<String> = head
        .doc
        .prefix
        .mcp_tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(sealed, vec!["svc__echo"]);

    // An unreachable server is strict: the create fails, no silent shrink.
    let (status, body) = h
        .create(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"mcp": [{
                "name": "gone",
                "url": "http://127.0.0.1:1/nope",
                "protocol": "2026-07"
            }]}
        }))
        .await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("tools.mcp server")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sealed_digest_survives_discard_and_rehydrate() {
    let (mcp_url, _) = serve(Mode::V2).await;
    let h = Harness::new().await;
    let created = h
        .create_ok(json!({
            "model": {"provider": "anthropic", "name": "scripted", "api_key": "sk-fake"},
            "tools": {"mcp": [{"name": "svc", "url": mcp_url}]}
        }))
        .await;
    let sid = created["id"].as_str().unwrap().to_string();
    let digest_before = h
        .brain
        .journal
        .get_head(&sid)
        .await
        .unwrap()
        .doc
        .manifest_digest;

    // Force a discard: the actor idles out (200ms), dropping the resident fold and the MCP
    // runtime. The next message rehydrates from the journal -- and the sealed prefix, rebuilt
    // with zero MCP network I/O, must digest identically.
    tokio::time::sleep(Duration::from_millis(400)).await;

    h.fake.script([
        Scripted::tool("svc__echo", json!({"msg": "after-hydrate"})),
        Scripted::Text("done".into()),
    ]);
    assert_eq!(h.send(&sid, "again").await, reqwest::StatusCode::ACCEPTED);
    wait_for(&h.http, &h.base, &h.token, &sid, "turn.completed", |evs| {
        evs.iter().any(|(k, _)| k == "turn.completed")
    })
    .await;

    let digest_after = h
        .brain
        .journal
        .get_head(&sid)
        .await
        .unwrap()
        .doc
        .manifest_digest;
    assert_eq!(
        digest_before, digest_after,
        "digest must be a pure function of the doc"
    );
    h.fake.assert_drained(2, "mcp e2e hydrate").unwrap();
}
