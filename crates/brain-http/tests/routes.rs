use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use brain_http::{BrainApi, router, router_with_bearer};
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopIdentity, ApiError, CreateSessionRequest,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, Event, EventId, EventPage,
    LiveEvent, MessageRequest, SessionId, SessionList, SessionStatus, SessionSummary,
    StreamingEvent,
};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct Api {
    /// Held so a test can push a record after the page has been served, which is what a
    /// turn does while a client is already streaming.
    live: Option<tokio::sync::broadcast::Sender<(SessionId, LiveEvent)>>,
    /// A finite journal for tests that page through it (the serve feed does); absent,
    /// `events` serves one synthetic record per page forever.
    journal: Option<Vec<Event>>,
}

#[async_trait]
impl BrainApi for Api {
    async fn admit_agentloop(&self, _: String, _: Vec<u8>) -> Result<AgentloopAdmission, ApiError> {
        Ok(admission())
    }
    async fn get_agentloop(&self, _: AgentloopIdentity) -> Result<AgentloopAdmission, ApiError> {
        Ok(admission())
    }
    async fn create_session(
        &self,
        _: String,
        _: CreateSessionRequest,
    ) -> Result<SessionSummary, ApiError> {
        Ok(session())
    }
    async fn get_session(&self, _: SessionId) -> Result<SessionSummary, ApiError> {
        Ok(session())
    }
    async fn list_sessions(&self) -> Result<SessionList, ApiError> {
        Ok(SessionList {
            sessions: vec![session()],
        })
    }
    async fn send_message(
        &self,
        _: SessionId,
        _: String,
        _: MessageRequest,
    ) -> Result<SessionSummary, ApiError> {
        Ok(session())
    }
    async fn call_environment(
        &self,
        _: SessionId,
        _: EnvironmentId,
        _: String,
        _: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError> {
        Ok(EnvironmentCallResult {
            output: request.input,
        })
    }
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(SessionId, LiveEvent)> {
        match &self.live {
            Some(live) => live.subscribe(),
            None => tokio::sync::broadcast::Sender::new(8).subscribe(),
        }
    }
    async fn events(&self, _: SessionId, after: Option<u64>) -> Result<EventPage, ApiError> {
        let after = after.unwrap_or(0);
        if let Some(journal) = &self.journal {
            let events: Vec<Event> = journal
                .iter()
                .filter(|event| event.sequence > after)
                .cloned()
                .collect();
            let next_cursor = events.last().map_or(after, |event| event.sequence);
            return Ok(EventPage {
                events,
                next_cursor,
            });
        }
        Ok(EventPage {
            events: vec![Event {
                event_id: EventId::new("evt_test"),
                sequence: after + 1,
                recorded_at_ms: 1_787_846_400_000,
                event_type: "test_event".into(),
                data: serde_json::json!({"ok":true}),
            }],
            next_cursor: after + 1,
        })
    }
    fn share_key(&self, session_id: &SessionId) -> String {
        format!("sk.{session_id}.{}", "b".repeat(64))
    }
    async fn client_tool_names(&self, _: SessionId) -> Result<Vec<String>, ApiError> {
        Ok(vec!["highlight_row".into(), "pick_file".into()])
    }
    async fn resolve_tool_call(
        &self,
        _: SessionId,
        _: u64,
        _: String,
        _: brain_protocol::Outcome,
    ) -> Result<(), ApiError> {
        Ok(())
    }
    async fn cancel_session(&self, _: SessionId, _: String) -> Result<(), ApiError> {
        Ok(())
    }
    async fn end_session(&self, _: SessionId, _: String) -> Result<SessionSummary, ApiError> {
        Ok(session())
    }
    async fn delete_session(&self, _: SessionId, _: String) -> Result<(), ApiError> {
        Ok(())
    }
    async fn live(&self) -> bool {
        true
    }
    async fn ready(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn exposes_every_v1_route_with_its_contract_status() {
    let digest = "a".repeat(64);
    let id = "ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    // A create request in the execution shape: a tool declaring `needs`, `binding_names`,
    // and its program, an environment carrying sealed binding values.
    let create = serde_json::json!({
        "agentloop": {"identity": digest, "configuration": {}},
        "model": {"provider":"vercel-ai-gateway","name":"test/model","api_key":"test-key"},
        "tools": [{
            "name": "bash",
            "description": "Run a shell command.",
            "input_schema": {"type": "object"},
            "needs": ["process", "fs"],
            "binding_names": ["API_BASE"],
            "hosting": "provisioned",
            "program": {"kind": "esm", "identity": "d".repeat(64)},
            "environment_id": "env_1"
        }],
        "environments": [{
            "environment_id": "env_1",
            "configuration": {},
            "lifecycle_policy": "session",
            "bindings": {"API_BASE": "https://api.internal"}
        }]
    });
    let cases = vec![
        request("POST", "/v1/agentloops", Some(vec![1]), None),
        request("GET", &format!("/v1/agentloops/{digest}"), None, None),
        request(
            "POST",
            "/v1/sessions",
            Some(serde_json::to_vec(&create).unwrap()),
            Some("application/json"),
        ),
        request("GET", "/v1/sessions", None, None),
        request("GET", &format!("/v1/sessions/{id}"), None, None),
        request(
            "POST",
            &format!("/v1/sessions/{id}/messages"),
            Some(br#"{"input":{"message":"hello"}}"#.to_vec()),
            Some("application/json"),
        ),
        request(
            "POST",
            &format!("/v1/sessions/{id}/environments/env_1/calls/suspend"),
            Some(br#"{"input":null}"#.to_vec()),
            Some("application/json"),
        ),
        request(
            "POST",
            &format!("/v1/sessions/{id}/tool-results/7"),
            Some(br#"{"status":"ok","value":{"content":"done"}}"#.to_vec()),
            Some("application/json"),
        ),
        request("POST", &format!("/v1/sessions/{id}/cancel"), None, None),
        request(
            "GET",
            &format!("/v1/sessions/{id}/events?after=7"),
            None,
            None,
        ),
        request("POST", &format!("/v1/sessions/{id}/end"), None, None),
        request("DELETE", &format!("/v1/sessions/{id}"), None, None),
        request("GET", "/health/live", None, None),
        request("GET", "/health/ready", None, None),
    ];
    for request in cases {
        let response = router(Api::default()).oneshot(request).await.unwrap();
        assert!(response.status().is_success(), "{}", response.status());
    }
}

#[tokio::test]
async fn mutating_routes_fail_fast_without_an_idempotency_key() {
    let request = Request::builder()
        .method("POST")
        .uri("/v1/agentloops")
        .body(Body::from(vec![1]))
        .unwrap();
    let response = router(Api::default()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn request_bodies_reject_unknown_fields() {
    let digest = "a".repeat(64);
    // `grant`, `configuration`, and `remote_tool_id` on a tool are the deleted v1
    // fields; a client still sending them is told so instead of silently ignored.
    let create = serde_json::json!({
        "agentloop": {"identity": digest, "configuration": {}},
        "model": {"provider":"vercel-ai-gateway","name":"test/model","api_key":"test-key"},
        "tools": [{
            "name": "bash",
            "description": "Run a shell command.",
            "input_schema": {"type": "object"},
            "needs": [],
            "binding_names": [],
            "environment_id": "env_1",
            "remote_tool_id": "bash",
            "configuration": {},
            "grant": {}
        }],
        "environments": [{
            "environment_id": "env_1",
            "configuration": {},
            "lifecycle_policy": "session"
        }]
    });
    let response = router(Api::default())
        .oneshot(request(
            "POST",
            "/v1/sessions",
            Some(serde_json::to_vec(&create).unwrap()),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn bearer_auth_protects_api_routes_but_not_health() {
    let unauthorized = router_with_bearer(Api::default(), "secret".into())
        .oneshot(request("GET", "/v1/sessions", None, None))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = router_with_bearer(Api::default(), "secret".into())
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);

    let health = router_with_bearer(Api::default(), "secret".into())
        .oneshot(request("GET", "/health/ready", None, None))
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::NO_CONTENT);
}

/// A stream still starts with the page `after` names, and still ends when there is
/// nothing live behind it — a client reading history gets history and a close, not a
/// connection held open forever.
#[tokio::test]
async fn the_event_stream_starts_with_the_page_the_cursor_names() {
    let response = router(Api::default())
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/events?after=7")
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("id: 8"));
    assert!(body.contains("event: test_event"));
    assert!(body.contains("data: {\"ok\":true}"));
}

fn request(
    method: &str,
    uri: &str,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !matches!(method, "GET") {
        builder = builder.header("idempotency-key", "test-key");
    }
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder
        .body(body.map_or_else(Body::empty, Body::from))
        .unwrap()
}

fn admission() -> AgentloopAdmission {
    AgentloopAdmission {
        identity: AgentloopIdentity::new("a".repeat(64)),
        status: AdmissionStatus::Admitted,
        error: None,
    }
}

fn session() -> SessionSummary {
    SessionSummary {
        session_id: SessionId::new("ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        status: SessionStatus::Idle,
        last_sequence: 1,
        share_key: String::new(),
    }
}

fn started(sequence: u64, tool: &str, call_id: &str) -> Event {
    Event {
        event_id: EventId::new(format!("evt_{sequence}")),
        sequence,
        // Freshly recorded, so the pending filter cannot see it as past its deadline.
        recorded_at_ms: now_ms(),
        event_type: "tool_call_started".into(),
        data: serde_json::json!({
            "sequence": sequence,
            "deadline_ms": 60_000,
            "binding": {"name": tool, "hosting": "client"},
            "invocation": {"call_id": call_id, "name": tool, "input": {}},
        }),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The serve feed is the share key's whole world: the key opens it and the
/// tool-results answer, while the rest of the API still demands the bearer token.
#[tokio::test]
async fn the_share_key_opens_exactly_the_serve_surface() {
    let id = "ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let key = format!("sk.{id}.{}", "b".repeat(64));
    let journal = vec![started(1, "highlight_row", "op_1")];
    let build = || {
        router_with_bearer(
            Api {
                journal: Some(journal.clone()),
                ..Api::default()
            },
            "secret".into(),
        )
    };
    let authed = |uri: &str, method: &str, bearer: &str, body: Option<&str>| {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"));
        if method == "POST" {
            builder = builder
                .header("idempotency-key", "test-key")
                .header("content-type", "application/json");
        }
        builder
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_owned())))
            .unwrap()
    };

    let serve = build()
        .oneshot(authed(
            &format!("/v1/sessions/{id}/serve?tools=highlight_row"),
            "GET",
            &key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(serve.status(), StatusCode::OK);

    let answer = build()
        .oneshot(authed(
            &format!("/v1/sessions/{id}/tool-results/7"),
            "POST",
            &key,
            Some(r#"{"status":"ok","value":null}"#),
        ))
        .await
        .unwrap();
    assert_eq!(answer.status(), StatusCode::NO_CONTENT);

    let wrong_key = build()
        .oneshot(authed(
            &format!("/v1/sessions/{id}/serve?tools=highlight_row"),
            "GET",
            &format!("sk.{id}.{}", "c".repeat(64)),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(wrong_key.status(), StatusCode::UNAUTHORIZED);

    let rest_of_api = build()
        .oneshot(authed(&format!("/v1/sessions/{id}"), "GET", &key, None))
        .await
        .unwrap();
    assert_eq!(rest_of_api.status(), StatusCode::UNAUTHORIZED);

    let api_token_serves = build()
        .oneshot(authed(
            &format!("/v1/sessions/{id}/serve?tools=highlight_row"),
            "GET",
            "secret",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(api_token_serves.status(), StatusCode::OK);
}

/// The serve backlog is the still-pending work: an intent already answered by a
/// `tool_result` is not replayed, another tool's intent is not this stream's, and a
/// tool the session never declared is refused at the door.
#[tokio::test]
async fn the_serve_feed_opens_with_pending_intents_only() {
    let id = "ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let journal = vec![
        started(1, "highlight_row", "call_answered"),
        Event {
            event_id: EventId::new("evt_2"),
            sequence: 2,
            recorded_at_ms: now_ms(),
            event_type: "tool_call_finished".into(),
            data: serde_json::json!({"sequence": 1, "result": {}}),
        },
        started(3, "highlight_row", "call_pending"),
        started(4, "pick_file", "call_other_tool"),
        Event {
            event_id: EventId::new("evt_5"),
            sequence: 5,
            recorded_at_ms: now_ms(),
            event_type: "session_ended".into(),
            data: serde_json::json!({}),
        },
    ];
    let api = Api {
        journal: Some(journal),
        ..Api::default()
    };
    let response = router(api.clone())
        .oneshot(request(
            "GET",
            &format!("/v1/sessions/{id}/serve?tools=highlight_row"),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        body.contains("call_pending"),
        "pending call missing: {body}"
    );
    assert!(
        !body.contains("call_answered"),
        "an answered call was replayed: {body}"
    );
    assert!(
        !body.contains("call_other_tool"),
        "another tool's call leaked onto this stream: {body}"
    );
    assert!(body.contains("session_ended"), "no end marker: {body}");

    let undeclared = router(api)
        .oneshot(request(
            "GET",
            &format!("/v1/sessions/{id}/serve?tools=made_up"),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(undeclared.status(), StatusCode::BAD_REQUEST);
}

/// One live consumer per tool: a second connection claiming the same tool displaces
/// the first, which ends instead of racing it for side effects.
#[tokio::test]
async fn a_new_serve_connection_displaces_the_seat_holder() {
    let id = "ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let live = tokio::sync::broadcast::Sender::new(8);
    let api = Api {
        live: Some(live.clone()),
        journal: Some(vec![started(1, "highlight_row", "op_1")]),
    };
    // One router, two connections: the seats live in the router's serve registry.
    let router = router(api);
    let first = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/sessions/{id}/serve?tools=highlight_row"),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let drained = tokio::spawn(axum::body::to_bytes(first.into_body(), 64 * 1024));

    let second = router
        .oneshot(request(
            "GET",
            &format!("/v1/sessions/{id}/serve?tools=highlight_row"),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);

    // The first stream must end because the second claimed its seat; a held stream
    // would make this join hang and the test time out.
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), drained)
        .await
        .expect("the displaced stream must end")
        .unwrap()
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        body.contains("op_1"),
        "backlog missing before the end: {body}"
    );
}

/// The event stream must carry what happens *after* it is opened.
///
/// It served one finite page of the journal and closed, so a client that opened it and
/// then sent a message never saw the turn it was waiting for. Nothing pushed, which made
/// a first-token measurement impossible to distinguish from a whole-turn one: the
/// benchmark's `ttfb` probe could only ever return its `round_trip`.
#[tokio::test]
async fn the_event_stream_carries_records_appended_after_it_opened() {
    let live = tokio::sync::broadcast::Sender::new(8);
    let api = Api {
        live: Some(live.clone()),
        ..Api::default()
    };

    let request = Request::builder()
        .uri("/v1/sessions/ses_test/events")
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let response = router(api).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Appended after the page was served, the way a turn appends while a client streams.
    live.send((
        SessionId::new("ses_test"),
        LiveEvent::Recorded(Event {
            event_id: EventId::new("evt_live"),
            sequence: 2,
            recorded_at_ms: 1_787_846_400_001,
            event_type: "assistant_delta".into(),
            data: serde_json::json!({"delta": "hello"}),
        }),
    ))
    .unwrap();
    // A record for another session must not reach this stream.
    live.send((
        SessionId::new("ses_other"),
        LiveEvent::Recorded(Event {
            event_id: EventId::new("evt_other"),
            sequence: 3,
            recorded_at_ms: 1_787_846_400_002,
            event_type: "assistant_delta".into(),
            data: serde_json::json!({"delta": "not yours"}),
        }),
    ))
    .unwrap();
    drop(live);

    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        body.contains("test_event"),
        "the page the stream starts with is missing: {body}"
    );
    assert!(
        body.contains("assistant_delta") && body.contains("hello"),
        "a record appended after the stream opened never arrived: {body}"
    );
    assert!(
        !body.contains("not yours"),
        "another session's record reached this stream: {body}"
    );
}

/// Model output has to reach a watching client while the turn is still running.
///
/// Brain streams from the model but used to keep what it received to itself: the deltas
/// went into a buffer in the session actor and the client saw nothing until the turn
/// finished and its record was appended. There was no first token to wait for, which is
/// why the benchmark's `ttfb` probe could only ever time out.
///
/// A streaming event is not a journal record, so it must not move the resume cursor: the
/// cursor is a position in the record and this was never in it. The recorded event sent
/// after it here would be dropped as already-seen if the delta had advanced the cursor.
#[tokio::test]
async fn the_event_stream_carries_model_output_before_the_turn_finishes() {
    let live = tokio::sync::broadcast::Sender::new(8);
    let api = Api {
        live: Some(live.clone()),
        ..Api::default()
    };

    let request = Request::builder()
        .uri("/v1/sessions/ses_test/events")
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let response = router(api).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    live.send((
        SessionId::new("ses_test"),
        LiveEvent::Streaming(StreamingEvent {
            sequence: 1,
            event_type: "assistant_delta".into(),
            data: serde_json::json!({ "text": "half a thought" }),
        }),
    ))
    .unwrap();
    // Another session's output must not reach this stream either.
    live.send((
        SessionId::new("ses_other"),
        LiveEvent::Streaming(StreamingEvent {
            sequence: 1,
            event_type: "assistant_delta".into(),
            data: serde_json::json!({ "text": "not yours" }),
        }),
    ))
    .unwrap();
    // The record the turn eventually appends, at the sequence right after the page.
    live.send((
        SessionId::new("ses_test"),
        LiveEvent::Recorded(Event {
            event_id: EventId::new("evt_done"),
            sequence: 2,
            recorded_at_ms: 1_787_846_400_003,
            event_type: "model_call_finished".into(),
            data: serde_json::json!({ "result": "whole thought" }),
        }),
    ))
    .unwrap();
    drop(live);

    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        body.contains("half a thought"),
        "model output never reached a client watching the turn: {body}"
    );
    assert!(
        !body.contains("not yours"),
        "another session's model output reached this stream: {body}"
    );
    assert!(
        body.contains("whole thought"),
        "the record appended after the deltas never arrived, so the delta consumed the \
         resume cursor it has no business touching: {body}"
    );
    // The id is the resume cursor, and a delta is not somewhere a client can resume to.
    let delta_block = body
        .split("\n\n")
        .find(|block| block.contains("half a thought"))
        .expect("the delta is on the stream");
    assert!(
        !delta_block.contains("id:"),
        "a delta carried a resume id, which points at nothing in the journal: {delta_block}"
    );
}
