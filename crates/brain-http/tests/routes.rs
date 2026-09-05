use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use brain_http::{BrainApi, HostConnection, router, router_with_bearer};
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopIdentity, ApiError, CreateSessionRequest,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, Event, EventId, EventPage,
    HostCommand, HostEvent, HostEventAck, HostId, HostOperation, HostRegistration, HostResult,
    LiveEvent, MessageRequest, SessionId, SessionList, SessionStatus, SessionSummary,
    StreamingEvent, ToolAdmission, ToolAdmissionStatus, ToolIdentity, ToolInvocation,
};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct Api {
    /// Held so a test can push a record after the page has been served, which is what a
    /// turn does while a client is already streaming.
    live: Option<tokio::sync::broadcast::Sender<(SessionId, LiveEvent)>>,
    /// A finite journal for tests that page through it (the serve feed does); absent,
    /// `events` serves one synthetic record.
    journal: Option<Vec<Event>>,
    page_size: Option<usize>,
    status: Option<SessionStatus>,
}

#[async_trait]
impl BrainApi for Api {
    async fn register_host(&self) -> Result<HostRegistration, ApiError> {
        Ok(HostRegistration {
            host_id: HostId::new("host_12345678901234567890"),
            token: "host-token".into(),
        })
    }
    async fn connect_host(
        &self,
        host_id: HostId,
        token: String,
    ) -> Result<HostConnection, ApiError> {
        if host_id.as_str() != "host_12345678901234567890" || token != "host-token" {
            return Err(ApiError::unauthorized("invalid host credential"));
        }
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let (_disconnect, displaced) = tokio::sync::oneshot::channel();
        sender
            .send(HostCommand {
                session_id: SessionId::new("ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                sequence: 7,
                deadline_at_ms: 1_787_846_460_000,
                operation: HostOperation::InvokeTool {
                    invocation: ToolInvocation {
                        call_id: "call_1".into(),
                        name: "highlight_row".into(),
                        input: serde_json::json!({"row": 4}),
                    },
                },
            })
            .await
            .unwrap();
        Ok(HostConnection {
            commands: receiver,
            displaced,
            on_close: None,
        })
    }
    async fn resolve_host(
        &self,
        host_id: HostId,
        token: String,
        _: HostResult,
    ) -> Result<(), ApiError> {
        if host_id.as_str() != "host_12345678901234567890" || token != "host-token" {
            return Err(ApiError::unauthorized("invalid host credential"));
        }
        Ok(())
    }
    async fn emit_host_event(
        &self,
        host_id: HostId,
        token: String,
        _: HostEvent,
    ) -> Result<HostEventAck, ApiError> {
        if host_id.as_str() != "host_12345678901234567890" || token != "host-token" {
            return Err(ApiError::unauthorized("invalid host credential"));
        }
        Ok(HostEventAck { sequence: 8 })
    }
    async fn admit_agentloop(&self, _: String, _: Vec<u8>) -> Result<AgentloopAdmission, ApiError> {
        Ok(admission())
    }
    async fn admit_tool(&self, _: String, _: Vec<u8>) -> Result<ToolAdmission, ApiError> {
        Ok(ToolAdmission {
            identity: ToolIdentity::new("b".repeat(64)),
            status: ToolAdmissionStatus::Admitted,
            error: None,
        })
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
        let mut session = session();
        if let Some(status) = &self.status {
            session.status = status.clone();
        }
        Ok(session)
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
                .take(self.page_size.unwrap_or(usize::MAX))
                .cloned()
                .collect();
            let next_cursor = events.last().map_or(after, |event| event.sequence);
            return Ok(EventPage {
                events,
                next_cursor,
            });
        }
        let events = matches!(after, 0 | 7).then(|| Event {
            event_id: EventId::new("evt_test"),
            sequence: after + 1,
            recorded_at_ms: 1_787_846_400_000,
            event_type: "test_event".into(),
            data: serde_json::json!({"ok":true}),
        });
        Ok(EventPage {
            next_cursor: events.as_ref().map_or(after, |event| event.sequence),
            events: events.into_iter().collect(),
        })
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
    // and its implementation, an environment carrying sealed binding values.
    let create = serde_json::json!({
        "agentloop": {"identity": digest, "configuration": {}, "environment_id": "env_1"},
        "model": {"provider":"vercel-ai-gateway","name":"test/model","api_key":"test-key"},
        "tools": [{
            "name": "bash",
            "description": "Run a shell command.",
            "input_schema": {"type": "object"},
            "needs": ["process", "fs"],
            "binding_names": ["API_BASE"],
            "hosting": "provisioned",
            "implementation": {"kind": "test"},
            "environment_id": "env_1"
        }],
        "environments": [{
            "environment_id": "env_1",
            "configuration": {"driver": "test"},
            "bindings": {"API_BASE": "https://api.internal"}
        }]
    });
    let cases = vec![
        request("POST", "/v1/agentloops", Some(vec![1]), None),
        request("POST", "/v1/tools", Some(vec![1]), None),
        request("GET", &format!("/v1/agentloops/{digest}"), None, None),
        request("POST", "/v1/hosts", None, None),
        Request::builder()
            .uri("/v1/hosts/host_12345678901234567890/commands")
            .header("authorization", "Bearer host-token")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("POST")
            .uri("/v1/hosts/host_12345678901234567890/results")
            .header("authorization", "Bearer host-token")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"session_id":"ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sequence":7,"outcome":{"status":"ok","value":null}}"#))
            .unwrap(),
        Request::builder()
            .method("POST")
            .uri("/v1/hosts/host_12345678901234567890/events")
            .header("authorization", "Bearer host-token")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"session_id":"ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sequence":7,"event_type":"progress","data":null}"#))
            .unwrap(),
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
            "environment_id": "env_1"
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

#[tokio::test]
async fn the_event_stream_drains_every_history_page_before_following_live() {
    let mut journal: Vec<Event> = (1..=1_002)
        .map(|sequence| Event {
            event_id: EventId::new(format!("evt_{sequence}")),
            sequence,
            recorded_at_ms: 1_787_846_400_000 + sequence,
            event_type: "test_event".into(),
            data: serde_json::json!({"sequence": sequence}),
        })
        .collect();
    journal.last_mut().unwrap().event_type = brain_protocol::codes::event::SESSION_ENDED.into();
    let response = router(Api {
        journal: Some(journal),
        page_size: Some(1_000),
        status: Some(SessionStatus::Ended),
        ..Api::default()
    })
    .oneshot(
        Request::builder()
            .uri("/v1/sessions/ses_test/events")
            .header("accept", "text/event-stream")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    let body = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        axum::body::to_bytes(response.into_body(), 1024 * 1024),
    )
    .await
    .expect("an ended session stream must close")
    .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("id: 1001"));
    assert!(body.contains("id: 1002"));
    assert!(body.contains("event: session_ended"));
}

#[tokio::test]
async fn a_terminal_cursor_and_a_failed_creation_close_the_event_stream() {
    for (after, event_type) in [
        (1, brain_protocol::codes::event::SESSION_ENDED),
        (0, brain_protocol::codes::event::SESSION_CREATION_FAILED),
    ] {
        let journal = vec![Event {
            event_id: EventId::new("evt_terminal"),
            sequence: 1,
            recorded_at_ms: 1_787_846_400_000,
            event_type: event_type.into(),
            data: serde_json::json!({}),
        }];
        let response = router(Api {
            journal: Some(journal),
            status: Some(if after == 1 {
                SessionStatus::Ended
            } else {
                SessionStatus::Failed
            }),
            ..Api::default()
        })
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/ses_test/events?after={after}"))
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let body = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            axum::body::to_bytes(response.into_body(), 64 * 1024),
        )
        .await
        .expect("a terminal stream must close")
        .unwrap();
        if after == 0 {
            assert!(
                String::from_utf8(body.to_vec())
                    .unwrap()
                    .contains(event_type)
            );
        } else {
            assert!(body.is_empty());
        }
    }
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
    }
}

#[tokio::test]
async fn the_host_token_opens_exactly_the_resident_surface() {
    let build = || router_with_bearer(Api::default(), "secret".into());
    let authed = |uri: &str, method: &str, bearer: &str, body: Option<&str>| {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {bearer}"));
        if method == "POST" {
            builder = builder.header("content-type", "application/json");
        }
        builder
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_owned())))
            .unwrap()
    };

    let commands = build()
        .oneshot(authed(
            "/v1/hosts/host_12345678901234567890/commands",
            "GET",
            "host-token",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(commands.status(), StatusCode::OK);

    let result = build()
        .oneshot(authed(
            "/v1/hosts/host_12345678901234567890/results",
            "POST",
            "host-token",
            Some(r#"{"session_id":"ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sequence":7,"outcome":{"status":"ok","value":null}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::NO_CONTENT);

    let wrong_key = build()
        .oneshot(authed(
            "/v1/hosts/host_12345678901234567890/commands",
            "GET",
            "wrong-token",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(wrong_key.status(), StatusCode::UNAUTHORIZED);

    let rest_of_api = build()
        .oneshot(authed("/v1/sessions", "GET", "host-token", None))
        .await
        .unwrap();
    assert_eq!(rest_of_api.status(), StatusCode::UNAUTHORIZED);

    let registration = build()
        .oneshot(authed("/v1/hosts", "POST", "secret", None))
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_host_stream_carries_typed_commands() {
    let response = router(Api::default())
        .oneshot(
            Request::builder()
                .uri("/v1/hosts/host_12345678901234567890/commands")
                .header("authorization", "Bearer host-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        body.contains("event: command"),
        "command event missing: {body}"
    );
    assert!(
        body.contains("invoke_tool"),
        "typed operation missing: {body}"
    );
    assert!(
        body.contains("highlight_row"),
        "Tool invocation missing: {body}"
    );
    assert!(
        !body.contains("id:"),
        "resident commands are not replay cursors: {body}"
    );
}

#[tokio::test]
async fn the_api_bearer_does_not_replace_a_host_token() {
    let response = router_with_bearer(Api::default(), "secret".into())
        .oneshot(request(
            "GET",
            "/v1/hosts/host_12345678901234567890/commands",
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
            event_type: "model_call_ended".into(),
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
