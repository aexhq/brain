use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::HeaderMap,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response, Sse, sse::Event as SseEvent},
    routing::{get, post},
};
use brain_protocol::{
    AgentloopAdmission, AgentloopDigest, CreateSessionRequest, MessageRequest, Session, SessionId,
    SessionList,
};

use crate::{BrainApi, HttpError};

const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    after: Option<u64>,
}

pub fn router<A: BrainApi>(api: A) -> Router {
    protected_routes(api.clone()).merge(health_routes(api))
}

pub fn router_with_bearer<A: BrainApi>(api: A, token: String) -> Router {
    let expected = sha256(token.as_bytes());
    protected_routes(api.clone())
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let expected = expected;
            async move {
                let authorized = request
                    .headers()
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .is_some_and(|value| constant_time_equal(&sha256(value.as_bytes()), &expected));
                if !authorized {
                    return HttpError(brain_protocol::ApiError {
                        code: "unauthorized".into(),
                        message: "a valid bearer token is required".into(),
                        retryable: false,
                        details: None,
                    })
                    .into_response();
                }
                next.run(request).await
            }
        }))
        .merge(health_routes(api))
}

fn protected_routes<A: BrainApi>(api: A) -> Router {
    Router::new()
        .route("/v1/agentloops", post(admit_agentloop::<A>))
        .route("/v1/agentloops/{digest}", get(get_agentloop::<A>))
        .route(
            "/v1/sessions",
            post(create_session::<A>).get(list_sessions::<A>),
        )
        .route(
            "/v1/sessions/{session_id}",
            get(get_session::<A>).delete(delete_session::<A>),
        )
        .route(
            "/v1/sessions/{session_id}/messages",
            post(send_message::<A>),
        )
        .route("/v1/sessions/{session_id}/events", get(events::<A>))
        .route(
            "/v1/sessions/{session_id}/cancel",
            post(cancel_session::<A>),
        )
        .route("/v1/sessions/{session_id}/end", post(end_session::<A>))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(api)
}

fn health_routes<A: BrainApi>(api: A) -> Router {
    Router::new()
        .route("/health/live", get(live::<A>))
        .route("/health/ready", get(ready::<A>))
        .with_state(api)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).into()
}

fn constant_time_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

async fn admit_agentloop<A: BrainApi>(
    State(api): State<A>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AgentloopAdmission>, HttpError> {
    if body.is_empty() || body.len() > MAX_REQUEST_BYTES {
        return Err(invalid(
            "Agentloop package must be between 1 byte and 32 MiB",
        ));
    }
    Ok(Json(
        api.admit_agentloop(idempotency_key(&headers)?, body.to_vec())
            .await
            .map_err(HttpError)?,
    ))
}

async fn get_agentloop<A: BrainApi>(
    State(api): State<A>,
    Path(digest): Path<AgentloopDigest>,
) -> Result<Json<AgentloopAdmission>, HttpError> {
    Ok(Json(api.get_agentloop(digest).await.map_err(HttpError)?))
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, HttpError> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= MAX_IDEMPOTENCY_KEY_BYTES)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            HttpError(brain_protocol::ApiError::invalid_request(
                "idempotency-key must be between 1 and 256 bytes",
            ))
        })
}

async fn create_session<A: BrainApi>(
    State(api): State<A>,
    headers: HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<Session>, HttpError> {
    Ok(Json(
        api.create_session(idempotency_key(&headers)?, request)
            .await
            .map_err(HttpError)?,
    ))
}

async fn get_session<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<Session>, HttpError> {
    Ok(Json(api.get_session(session_id).await.map_err(HttpError)?))
}

async fn list_sessions<A: BrainApi>(State(api): State<A>) -> Result<Json<SessionList>, HttpError> {
    Ok(Json(api.list_sessions().await.map_err(HttpError)?))
}

async fn send_message<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    headers: HeaderMap,
    Json(request): Json<MessageRequest>,
) -> Result<Json<Session>, HttpError> {
    Ok(Json(
        api.send_message(session_id, idempotency_key(&headers)?, request)
            .await
            .map_err(HttpError)?,
    ))
}

async fn events<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let page = api
        .events(session_id, query.after)
        .await
        .map_err(HttpError)?;
    let wants_sse = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|media| media.trim().starts_with("text/event-stream"))
        });
    if !wants_sse {
        return Ok(Json(page).into_response());
    }
    let events = page.events.into_iter().map(|event| {
        let data = serde_json::to_string(&event.data).expect("JSON event payload is serializable");
        Ok::<_, std::convert::Infallible>(
            SseEvent::default()
                .id(event.sequence.to_string())
                .event(event.event_type)
                .data(data),
        )
    });
    Ok(Sse::new(futures_util::stream::iter(events)).into_response())
}

fn invalid(message: impl Into<String>) -> HttpError {
    HttpError(brain_protocol::ApiError::invalid_request(message))
}

async fn cancel_session<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    headers: HeaderMap,
) -> Result<StatusCode, HttpError> {
    api.cancel_session(session_id, idempotency_key(&headers)?)
        .await
        .map_err(HttpError)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn end_session<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    headers: HeaderMap,
) -> Result<Json<Session>, HttpError> {
    Ok(Json(
        api.end_session(session_id, idempotency_key(&headers)?)
            .await
            .map_err(HttpError)?,
    ))
}

async fn delete_session<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    headers: HeaderMap,
) -> Result<StatusCode, HttpError> {
    api.delete_session(session_id, idempotency_key(&headers)?)
        .await
        .map_err(HttpError)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn live<A: BrainApi>(State(api): State<A>) -> axum::http::StatusCode {
    if api.live().await {
        axum::http::StatusCode::NO_CONTENT
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn ready<A: BrainApi>(State(api): State<A>) -> axum::http::StatusCode {
    if api.ready().await {
        axum::http::StatusCode::NO_CONTENT
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
}
