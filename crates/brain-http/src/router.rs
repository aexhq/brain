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
    AgentloopAdmission, AgentloopIdentity, CreateSessionRequest, EnvironmentCallRequest,
    EnvironmentCallResult, EnvironmentId, MessageRequest, OperationId, Outcome, Session, SessionId,
    SessionList,
};

use futures_util::StreamExt as _;

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
        .route("/v1/agentloops/{identity}", get(get_agentloop::<A>))
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
        .route(
            "/v1/sessions/{session_id}/environments/{environment_id}/calls/{name}",
            post(call_environment::<A>),
        )
        .route("/v1/sessions/{session_id}/events", get(events::<A>))
        .route(
            "/v1/sessions/{session_id}/tool-results/{operation_id}",
            post(resolve_tool_call::<A>),
        )
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
    Path(digest): Path<AgentloopIdentity>,
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

async fn resolve_tool_call<A: BrainApi>(
    State(api): State<A>,
    Path((session_id, operation_id)): Path<(SessionId, OperationId)>,
    headers: HeaderMap,
    Json(outcome): Json<Outcome>,
) -> Result<StatusCode, HttpError> {
    api.resolve_tool_call(
        session_id,
        operation_id,
        idempotency_key(&headers)?,
        outcome,
    )
    .await
    .map_err(HttpError)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn call_environment<A: BrainApi>(
    State(api): State<A>,
    Path((session_id, environment_id, name)): Path<(SessionId, EnvironmentId, String)>,
    headers: HeaderMap,
    Json(request): Json<EnvironmentCallRequest>,
) -> Result<Json<EnvironmentCallResult>, HttpError> {
    Ok(Json(
        api.call_environment(
            session_id,
            environment_id,
            name,
            idempotency_key(&headers)?,
            request,
        )
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
    let wants_sse = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|media| media.trim().starts_with("text/event-stream"))
        });

    // Subscribed before the page is read, so a record appended between the two arrives on
    // the subscription rather than falling into the gap. Only for a stream: a JSON reader
    // asked for a page and gets one.
    let live = wants_sse.then(|| api.subscribe());
    let page = api
        .events(session_id.clone(), query.after)
        .await
        .map_err(HttpError)?;
    let Some(live) = live else {
        return Ok(Json(page).into_response());
    };

    // Where the page ended. Everything at or below it was already sent; everything above
    // it is what this stream is for.
    let sent_through = page.next_cursor;
    // A session that has ended appends nothing more, so a stream that stayed open on one
    // would wait for records that cannot arrive. The page is the whole story there.
    let ended = page.events.iter().any(is_last);
    let backlog = futures_util::stream::iter(page.events.into_iter().map(sse));
    if ended {
        return Ok(Sse::new(backlog).into_response());
    }
    let following =
        futures_util::stream::unfold(Some((live, sent_through, session_id)), |state| async move {
            // `None` once the session has ended: nothing follows it, so holding the
            // connection open would leave a client waiting on a promise the journal
            // cannot keep.
            let (mut live, mut sent_through, session_id) = state?;
            loop {
                match live.recv().await {
                    Ok((session, brain_protocol::LiveEvent::Recorded(event)))
                        if session == session_id && event.sequence > sent_through =>
                    {
                        sent_through = event.sequence;
                        let carry = (!is_last(&event)).then_some((live, sent_through, session_id));
                        return Some((sse(event), carry));
                    }
                    // Model output, mid-turn. It has no sequence and is not in the journal,
                    // so it is passed straight through and leaves the cursor alone: the
                    // cursor is a position in the record, and this is not a record. A client
                    // that reconnects resumes from the last record it saw and is given the
                    // completed message rather than the tokens.
                    Ok((session, brain_protocol::LiveEvent::Streaming(streaming)))
                        if session == session_id =>
                    {
                        let carry = Some((live, sent_through, session_id));
                        return Some((streaming_sse(streaming), carry));
                    }
                    Ok(_) => continue,
                    // Lagged, or the kernel is gone. The stream ends rather than
                    // silently skipping records: a client reconnects with the cursor it
                    // last saw and the journal hands back exactly what it missed.
                    Err(_) => return None,
                }
            }
        });

    Ok(Sse::new(backlog.chain(following)).into_response())
}

/// Whether this record is the last a session can produce. A stream past it would wait
/// on an append that cannot happen.
fn is_last(event: &brain_protocol::Event) -> bool {
    event.event_type == "session_ended"
}

/// One journal record as it goes out on the wire. The id is the sequence, so a client
/// that reconnects with `Last-Event-ID` resumes from exactly what it saw.
fn sse(event: brain_protocol::Event) -> Result<SseEvent, std::convert::Infallible> {
    let data = serde_json::to_string(&event.data).expect("JSON event payload is serializable");
    Ok(SseEvent::default()
        .id(event.sequence.to_string())
        .event(event.event_type)
        .data(data))
}

/// Model output on the wire.
///
/// No `id`: the id is the resume cursor, and this is not something a client can resume to
/// -- it was never journalled. Giving it one would hand back a cursor that means nothing.
fn streaming_sse(
    streaming: brain_protocol::StreamingEvent,
) -> Result<SseEvent, std::convert::Infallible> {
    let data = serde_json::to_string(&streaming.data).expect("JSON event payload is serializable");
    Ok(SseEvent::default().event(streaming.event_type).data(data))
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
