use std::collections::HashMap;

use axum::{Json, Router, extract::{Path, Query, State}, http::HeaderMap, routing::{get, post}};
use brain_protocol::{CreateSessionRequest, EventPage, MessageRequest, Session, SessionId};

use crate::{BrainApi, HttpError};

pub fn router<A: BrainApi>(api: A) -> Router {
    Router::new()
        .route("/v1/sessions", post(create_session::<A>))
        .route("/v1/sessions/{session_id}", get(get_session::<A>))
        .route("/v1/sessions/{session_id}/messages", post(send_message::<A>))
        .route("/v1/sessions/{session_id}/events", get(events::<A>))
        .route("/health/live", get(live::<A>))
        .route("/health/ready", get(ready::<A>))
        .with_state(api)
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, HttpError> {
    headers.get("idempotency-key").and_then(|value| value.to_str().ok()).filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| HttpError(brain_protocol::ApiError::invalid_request("missing idempotency-key header")))
}

async fn create_session<A: BrainApi>(State(api): State<A>, headers: HeaderMap, Json(request): Json<CreateSessionRequest>) -> Result<Json<Session>, HttpError> {
    Ok(Json(api.create_session(idempotency_key(&headers)?, request).await.map_err(HttpError)?))
}

async fn get_session<A: BrainApi>(State(api): State<A>, Path(session_id): Path<SessionId>) -> Result<Json<Session>, HttpError> {
    Ok(Json(api.get_session(session_id).await.map_err(HttpError)?))
}

async fn send_message<A: BrainApi>(State(api): State<A>, Path(session_id): Path<SessionId>, headers: HeaderMap, Json(request): Json<MessageRequest>) -> Result<Json<Session>, HttpError> {
    Ok(Json(api.send_message(session_id, idempotency_key(&headers)?, request).await.map_err(HttpError)?))
}

async fn events<A: BrainApi>(State(api): State<A>, Path(session_id): Path<SessionId>, Query(query): Query<HashMap<String, u64>>) -> Result<Json<EventPage>, HttpError> {
    Ok(Json(api.events(session_id, query.get("after").copied()).await.map_err(HttpError)?))
}

async fn live<A: BrainApi>(State(api): State<A>) -> axum::http::StatusCode {
    if api.live().await { axum::http::StatusCode::NO_CONTENT } else { axum::http::StatusCode::SERVICE_UNAVAILABLE }
}

async fn ready<A: BrainApi>(State(api): State<A>) -> axum::http::StatusCode {
    if api.ready().await { axum::http::StatusCode::NO_CONTENT } else { axum::http::StatusCode::SERVICE_UNAVAILABLE }
}
