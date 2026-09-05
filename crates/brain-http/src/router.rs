use std::collections::BTreeSet;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::HeaderMap,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response, Sse, sse::Event as SseEvent},
    routing::{MethodFilter, MethodRouter, on},
};
use brain_protocol::{
    AgentloopAdmission, AgentloopIdentity, CreateSessionRequest, EnvironmentCallRequest,
    EnvironmentCallResult, EnvironmentId, HostCommand, HostEvent, HostEventAck, HostId,
    HostRegistration, HostResult, MessageRequest, SessionId, SessionList, SessionSummary,
    ToolAdmission,
};
use utoipa::{OpenApi, openapi::HttpMethod};

use crate::{
    BrainApi, HttpError,
    openapi::{Package, contract, operations},
};

pub(crate) const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

/// The session API as one document. Every handler below is listed here and registered
/// through `routes!`; [`build`] checks that the two agree.
#[derive(OpenApi)]
#[openapi(
    info(title = "Brain HTTP API", version = "1.0.0"),
    paths(
        admit_agentloop,
        admit_tool,
        get_agentloop,
        register_host,
        host_commands,
        resolve_host,
        emit_host_event,
        create_session,
        list_sessions,
        get_session,
        transcript,
        delete_session,
        send_message,
        call_environment,
        events,
        cancel_session,
        end_session,
        live,
        ready,
    )
)]
pub(crate) struct ApiDoc;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    after: Option<u64>,
}

pub fn router<A: BrainApi>(api: A) -> Router {
    build(api, None)
}

pub fn router_with_bearer<A: BrainApi>(api: A, token: String) -> Router {
    build(api, Some(token))
}

fn build<A: BrainApi>(api: A, token: Option<String>) -> Router {
    let expected = token.map(|token| sha256(token.as_bytes()));
    let mut routed = BTreeSet::new();
    let mut protected = protected_routes(api.clone(), &mut routed);
    if let Some(expected) = expected {
        protected = protected.layer(middleware::from_fn(
            move |request: Request, next: Next| async move {
                if !bearer_matches(request.headers(), &expected) {
                    return unauthorized().into_response();
                }
                next.run(request).await
            },
        ));
    }
    let hosts = host_routes(api.clone(), &mut routed);
    let health = health_routes(api, &mut routed);
    // The published document is rendered from `ApiDoc`; the router from the same
    // annotations, through `documented`. A handler in one and not the other would ship
    // undocumented or unreachable.
    assert_eq!(
        routed,
        operations(&ApiDoc::openapi()),
        "brain-http: the routes and the OpenAPI document disagree"
    );
    protected.merge(hosts).merge(health)
}

/// A handler's route, taken from its `#[utoipa::path]` annotation: the path and the
/// methods it answers. `routed` collects what was registered, for [`build`] to check
/// against the document.
fn documented<P, H, T, S>(routed: &mut BTreeSet<String>, handler: H) -> (String, MethodRouter<S>)
where
    P: utoipa::Path,
    H: axum::handler::Handler<T, S>,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    let path = P::path();
    let mut filter = MethodFilter::GET;
    let mut first = true;
    for method in P::methods() {
        let (name, method_filter) = match method {
            HttpMethod::Get => ("GET", MethodFilter::GET),
            HttpMethod::Post => ("POST", MethodFilter::POST),
            HttpMethod::Put => ("PUT", MethodFilter::PUT),
            HttpMethod::Delete => ("DELETE", MethodFilter::DELETE),
            HttpMethod::Options => ("OPTIONS", MethodFilter::OPTIONS),
            HttpMethod::Head => ("HEAD", MethodFilter::HEAD),
            HttpMethod::Patch => ("PATCH", MethodFilter::PATCH),
            HttpMethod::Trace => ("TRACE", MethodFilter::TRACE),
        };
        routed.insert(format!("{name} {path}"));
        filter = if first {
            method_filter
        } else {
            filter.or(method_filter)
        };
        first = false;
    }
    assert!(!first, "{path} answers no method");
    (path, on(filter, handler))
}

fn protected_routes<A: BrainApi>(api: A, routed: &mut BTreeSet<String>) -> Router {
    let mut router = Router::new();
    for (path, method) in [
        documented::<__path_admit_agentloop, _, _, _>(routed, admit_agentloop::<A>),
        documented::<__path_admit_tool, _, _, _>(routed, admit_tool::<A>),
        documented::<__path_get_agentloop, _, _, _>(routed, get_agentloop::<A>),
        documented::<__path_register_host, _, _, _>(routed, register_host::<A>),
        documented::<__path_create_session, _, _, _>(routed, create_session::<A>),
        documented::<__path_list_sessions, _, _, _>(routed, list_sessions::<A>),
        documented::<__path_get_session, _, _, _>(routed, get_session::<A>),
        documented::<__path_transcript, _, _, _>(routed, transcript::<A>),
        documented::<__path_delete_session, _, _, _>(routed, delete_session::<A>),
        documented::<__path_send_message, _, _, _>(routed, send_message::<A>),
        documented::<__path_call_environment, _, _, _>(routed, call_environment::<A>),
        documented::<__path_events, _, _, _>(routed, events::<A>),
        documented::<__path_cancel_session, _, _, _>(routed, cancel_session::<A>),
        documented::<__path_end_session, _, _, _>(routed, end_session::<A>),
    ] {
        router = router.route(&path, method);
    }
    router
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(api)
}

/// Host commands use the scoped token returned by registration rather than the API
/// bearer, so these routes authenticate inside their handlers.
fn host_routes<A: BrainApi>(api: A, routed: &mut BTreeSet<String>) -> Router {
    let mut router = Router::new();
    for (path, method) in [
        documented::<__path_host_commands, _, _, _>(routed, host_commands::<A>),
        documented::<__path_resolve_host, _, _, _>(routed, resolve_host::<A>),
        documented::<__path_emit_host_event, _, _, _>(routed, emit_host_event::<A>),
    ] {
        router = router.route(&path, method);
    }
    router
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(api)
}

fn health_routes<A: BrainApi>(api: A, routed: &mut BTreeSet<String>) -> Router {
    let mut router = Router::new();
    for (path, method) in [
        documented::<__path_live, _, _, _>(routed, live::<A>),
        documented::<__path_ready, _, _, _>(routed, ready::<A>),
    ] {
        router = router.route(&path, method);
    }
    router.with_state(api)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).into()
}

fn bearer_matches(headers: &HeaderMap, expected: &[u8; 32]) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| constant_time_equal(&sha256(value.as_bytes()), expected))
}

fn constant_time_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn unauthorized() -> HttpError {
    HttpError(brain_protocol::ApiError::unauthorized(
        "a valid bearer token is required",
    ))
}

fn bearer(headers: &HeaderMap) -> Result<String, HttpError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(unauthorized)
}

#[utoipa::path(
    post,
    path = "/v1/hosts",
    operation_id = "registerHost",
    responses(
        (status = 200, description = "Registered resident extension host", body = contract::HostRegistration),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn register_host<A: BrainApi>(
    State(api): State<A>,
) -> Result<Json<HostRegistration>, HttpError> {
    Ok(Json(api.register_host().await.map_err(HttpError)?))
}

#[utoipa::path(
    get,
    path = "/v1/hosts/{host_id}/commands",
    operation_id = "hostCommands",
    params(("host_id" = contract::HostId, Path)),
    responses(
        (status = 200, description = "Bounded send-once resident command stream", body = String, content_type = "text/event-stream"),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn host_commands<A: BrainApi>(
    State(api): State<A>,
    Path(host_id): Path<HostId>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let connection = api
        .connect_host(host_id, bearer(&headers)?)
        .await
        .map_err(HttpError)?;
    let stream = futures_util::stream::unfold(Some(connection), |connection| async move {
        let mut connection = connection?;
        tokio::select! {
            biased;
            command = connection.commands.recv() => command.map(|command| {
                let frame = host_sse(command);
                (frame, Some(connection))
            }),
            _ = &mut connection.displaced => None,
        }
    });
    Ok(Sse::new(stream).into_response())
}

#[utoipa::path(
    post,
    path = "/v1/hosts/{host_id}/results",
    operation_id = "resolveHostCommand",
    params(("host_id" = contract::HostId, Path)),
    request_body = contract::HostResult,
    responses(
        (status = 204, description = "Resident command result accepted"),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn resolve_host<A: BrainApi>(
    State(api): State<A>,
    Path(host_id): Path<HostId>,
    headers: HeaderMap,
    Json(result): Json<HostResult>,
) -> Result<StatusCode, HttpError> {
    api.resolve_host(host_id, bearer(&headers)?, result)
        .await
        .map_err(HttpError)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/v1/hosts/{host_id}/events",
    operation_id = "emitHostEvent",
    params(("host_id" = contract::HostId, Path)),
    request_body = contract::HostEvent,
    responses(
        (status = 200, description = "Resident extension Event committed", body = contract::HostEventAck),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn emit_host_event<A: BrainApi>(
    State(api): State<A>,
    Path(host_id): Path<HostId>,
    headers: HeaderMap,
    Json(event): Json<HostEvent>,
) -> Result<Json<HostEventAck>, HttpError> {
    Ok(Json(
        api.emit_host_event(host_id, bearer(&headers)?, event)
            .await
            .map_err(HttpError)?,
    ))
}

fn host_sse(command: HostCommand) -> Result<SseEvent, std::convert::Infallible> {
    let data = serde_json::to_string(&command).expect("Host command is serializable");
    Ok(SseEvent::default().event("command").data(data))
}

#[utoipa::path(
    post,
    path = "/v1/agentloops",
    operation_id = "admitAgentloop",
    params(("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    request_body(content = inline(Package), content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "Agentloop admitted", body = contract::AgentloopAdmission),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
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

#[utoipa::path(
    post,
    path = "/v1/tools",
    operation_id = "admitTool",
    params(("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    request_body(content = inline(Package), content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "Tool Component admitted", body = contract::ToolAdmission),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn admit_tool<A: BrainApi>(
    State(api): State<A>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolAdmission>, HttpError> {
    if body.is_empty() || body.len() > MAX_REQUEST_BYTES {
        return Err(invalid("Tool Component must be between 1 byte and 32 MiB"));
    }
    Ok(Json(
        api.admit_tool(idempotency_key(&headers)?, body.to_vec())
            .await
            .map_err(HttpError)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/agentloops/{identity}",
    operation_id = "getAgentloop",
    params(("identity" = contract::AgentloopIdentity, Path)),
    responses(
        (status = 200, description = "Admission status", body = contract::AgentloopAdmission),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
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

#[utoipa::path(
    post,
    path = "/v1/sessions",
    operation_id = "createSession",
    params(("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    request_body = contract::CreateSessionRequest,
    responses(
        (status = 200, description = "Created session", body = contract::SessionSummary),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn create_session<A: BrainApi>(
    State(api): State<A>,
    headers: HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<SessionSummary>, HttpError> {
    Ok(Json(
        api.create_session(idempotency_key(&headers)?, request)
            .await
            .map_err(HttpError)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/sessions/{session_id}",
    operation_id = "getSession",
    params(("session_id" = contract::SessionId, Path)),
    responses(
        (status = 200, description = "Session state", body = contract::SessionSummary),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn get_session<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<SessionSummary>, HttpError> {
    Ok(Json(api.get_session(session_id).await.map_err(HttpError)?))
}

#[utoipa::path(
    get, path = "/v1/sessions/{session_id}/transcript",
    params(("session_id" = String, Path, description = "Session id")),
    responses(
        (status = 200, description = "Committed transcript, including suspended sessions", body = contract::SessionTranscript),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn transcript<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<brain_protocol::SessionTranscript>, HttpError> {
    Ok(Json(api.transcript(session_id).await.map_err(HttpError)?))
}

#[utoipa::path(
    get,
    path = "/v1/sessions",
    operation_id = "listSessions",
    responses(
        (status = 200, description = "Sessions", body = contract::SessionList),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn list_sessions<A: BrainApi>(State(api): State<A>) -> Result<Json<SessionList>, HttpError> {
    Ok(Json(api.list_sessions().await.map_err(HttpError)?))
}

#[utoipa::path(
    post,
    path = "/v1/sessions/{session_id}/messages",
    operation_id = "sendMessage",
    params(("session_id" = contract::SessionId, Path), ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    request_body = contract::MessageRequest,
    responses(
        (status = 200, description = "Updated session", body = contract::SessionSummary),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn send_message<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    headers: HeaderMap,
    Json(request): Json<MessageRequest>,
) -> Result<Json<SessionSummary>, HttpError> {
    Ok(Json(
        api.send_message(session_id, idempotency_key(&headers)?, request)
            .await
            .map_err(HttpError)?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/sessions/{session_id}/environments/{environment_id}/calls/{name}",
    operation_id = "callEnvironment",
    params(
        ("session_id" = contract::SessionId, Path),
        ("environment_id" = contract::EnvironmentId, Path),
        ("name" = String, Path, pattern = r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$"),
        ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)
    ),
    request_body = contract::EnvironmentCallRequest,
    responses(
        (status = 200, description = "Environment method result", body = contract::EnvironmentCallResult),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
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

#[utoipa::path(
    get,
    path = "/v1/sessions/{session_id}/events",
    operation_id = "readSessionEvents",
    params(("session_id" = contract::SessionId, Path), ("after" = Option<u64>, Query, minimum = 0)),
    responses(
        (
            status = 200,
            description = "A finite event page for application/json, or a live SSE stream \
for text/event-stream. The stream begins with the page `after` names and then carries \
records as they are appended, so a client that opens it before sending a message sees \
that turn. It ends if the subscriber falls too far behind: reconnect with `after` set to \
the last id seen, and the journal hands back exactly what was missed.",
            content(
                (contract::EventPage = "application/json"),
                (String = "text/event-stream")
            )
        ),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
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
    let live = wants_sse.then(|| api.subscribe(&session_id));
    let page = api
        .events(session_id.clone(), query.after)
        .await
        .map_err(HttpError)?;
    let Some(live) = live else {
        return Ok(Json(page).into_response());
    };

    let mut sent_through = query.after.unwrap_or(0);
    let stream = async_stream::stream! {
        let mut page = page;
        loop {
            let empty = page.events.is_empty();
            for event in page.events {
                sent_through = event.sequence;
                yield sse(event);
            }
            if empty {
                let terminal = match api.get_session(session_id.clone()).await {
                    Ok(session) => matches!(session.status, brain_protocol::SessionStatus::Ended | brain_protocol::SessionStatus::Failed),
                    Err(_) => return,
                };
                if !terminal {
                    break;
                }
            }
            page = match api.events(session_id.clone(), Some(sent_through)).await {
                Ok(page) => page,
                Err(_) => return,
            };
            if empty && page.events.is_empty() {
                return;
            }
        }

        drop(api);
        let mut live = live;
        loop {
            match live.recv().await {
                    Ok((session, brain_protocol::LiveEvent::Recorded(event)))
                        if session == session_id && event.sequence > sent_through =>
                    {
                        sent_through = event.sequence;
                        let terminal = is_last(&event);
                        yield sse(event);
                        if terminal {
                            return;
                        }
                    }
                    // Model output, mid-turn. It has no sequence and is not in the journal,
                    // so it is passed straight through and leaves the cursor alone: the
                    // cursor is a position in the record, and this is not a record. A client
                    // that reconnects resumes from the last record it saw and is given the
                    // completed message rather than the tokens.
                    Ok((session, brain_protocol::LiveEvent::Streaming(streaming)))
                        if session == session_id =>
                    {
                        yield streaming_sse(streaming);
                    }
                    Ok(_) => continue,
                    // Lagged, or the session is gone. The stream ends rather than
                    // silently skipping records: a client reconnects with the cursor it
                    // last saw and the journal hands back exactly what it missed.
                Err(_) => return,
            }
        }
    };

    Ok(Sse::new(stream).into_response())
}

/// Whether this record is the last a session can produce. A stream past it would wait
/// on an append that cannot happen.
fn is_last(event: &brain_protocol::Event) -> bool {
    matches!(
        event.event_type.as_str(),
        brain_protocol::codes::event::SESSION_ENDED
            | brain_protocol::codes::event::SESSION_CREATION_FAILED
    )
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

#[utoipa::path(
    post,
    path = "/v1/sessions/{session_id}/cancel",
    operation_id = "cancelSession",
    params(("session_id" = contract::SessionId, Path), ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    responses((status = 204, description = "Cancellation requested"), (status = "default", description = "Structured error", body = contract::ApiError))
)]
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

#[utoipa::path(
    post,
    path = "/v1/sessions/{session_id}/end",
    operation_id = "endSession",
    params(("session_id" = contract::SessionId, Path), ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    responses(
        (status = 200, description = "Ended session", body = contract::SessionSummary),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn end_session<A: BrainApi>(
    State(api): State<A>,
    Path(session_id): Path<SessionId>,
    headers: HeaderMap,
) -> Result<Json<SessionSummary>, HttpError> {
    Ok(Json(
        api.end_session(session_id, idempotency_key(&headers)?)
            .await
            .map_err(HttpError)?,
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/sessions/{session_id}",
    operation_id = "deleteSession",
    params(("session_id" = contract::SessionId, Path), ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    responses((status = 204, description = "Deleted"), (status = "default", description = "Structured error", body = contract::ApiError))
)]
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

#[utoipa::path(
    get,
    path = "/health/live",
    operation_id = "live",
    responses((status = 204, description = "Process is live"))
)]
async fn live<A: BrainApi>(State(api): State<A>) -> axum::http::StatusCode {
    if api.live().await {
        axum::http::StatusCode::NO_CONTENT
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
}

#[utoipa::path(
    get,
    path = "/health/ready",
    operation_id = "ready",
    responses(
        (status = 204, description = "Process is ready"),
        (status = 503, description = "Required dependency is unavailable")
    )
)]
async fn ready<A: BrainApi>(State(api): State<A>) -> axum::http::StatusCode {
    if api.ready().await {
        axum::http::StatusCode::NO_CONTENT
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
}
