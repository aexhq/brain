use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use brain_http::{BrainApi, router, router_with_bearer};
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopIdentity, ApiError, CreateSessionRequest,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, Event, EventId, EventPage,
    MessageRequest, Session, SessionId, SessionList, SessionStatus,
};
use tower::ServiceExt;

#[derive(Clone)]
struct Api;

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
    ) -> Result<Session, ApiError> {
        Ok(session())
    }
    async fn get_session(&self, _: SessionId) -> Result<Session, ApiError> {
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
    ) -> Result<Session, ApiError> {
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
    async fn events(&self, _: SessionId, after: Option<u64>) -> Result<EventPage, ApiError> {
        Ok(EventPage {
            events: vec![Event {
                event_id: EventId::new("evt_test"),
                sequence: after.unwrap_or(0) + 1,
                recorded_at_ms: 1_787_846_400_000,
                event_type: "test_event".into(),
                data: serde_json::json!({"ok":true}),
            }],
            next_cursor: after.unwrap_or(0) + 1,
        })
    }
    async fn cancel_session(&self, _: SessionId, _: String) -> Result<(), ApiError> {
        Ok(())
    }
    async fn end_session(&self, _: SessionId, _: String) -> Result<Session, ApiError> {
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
    let create = serde_json::json!({
        "agentloop_identity": digest,
        "brain_configuration": {},
        "model": {"provider":"vercel-ai-gateway","name":"test/model","api_key":"test-key"},
        "presentation": {"system":"","tools":[]},
        "environments": [],
        "tool_bindings": []
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
            Some(br#"{"content":"hello"}"#.to_vec()),
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
        let response = router(Api).oneshot(request).await.unwrap();
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
    let response = router(Api).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn request_bodies_reject_unknown_fields() {
    let digest = "a".repeat(64);
    let create = serde_json::json!({
        "agentloop_identity": digest,
        "brain_configuration": {},
        "model": {"provider":"vercel-ai-gateway","name":"test/model","api_key":"test-key"},
        "presentation": {"system":"","tools":[]},
        "environments": [],
        "tool_bindings": [],
        "unknown": true
    });
    let response = router(Api)
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
    let unauthorized = router_with_bearer(Api, "secret".into())
        .oneshot(request("GET", "/v1/sessions", None, None))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = router_with_bearer(Api, "secret".into())
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

    let health = router_with_bearer(Api, "secret".into())
        .oneshot(request("GET", "/health/ready", None, None))
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn event_route_negotiates_a_finite_sse_page() {
    let response = router(Api)
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

fn session() -> Session {
    Session {
        session_id: SessionId::new("ses_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        journal_id: brain_protocol::JournalId::new("jrn_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        status: SessionStatus::Idle,
        through_sequence: 1,
        presentation_identity: brain_protocol::Identity::of(&"presentation").unwrap(),
    }
}
