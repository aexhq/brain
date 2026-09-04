use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex as StdMutex},
};

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
    AgentloopAdmission, AgentloopIdentity, CreateEnvironmentRequest, CreateSessionRequest,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, EnvironmentList,
    EnvironmentSummary, MessageRequest, Outcome, SessionId, SessionList, SessionSummary,
};
use futures_util::StreamExt as _;
use utoipa::{OpenApi, openapi::HttpMethod};

use crate::{
    BrainApi, HttpError,
    openapi::{Package, contract, operations},
};

pub(crate) const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_SERVED_TOOLS: usize = 128;

/// The session API as one document. Every handler below is listed here and registered
/// through `routes!`; [`build`] checks that the two agree.
#[derive(OpenApi)]
#[openapi(
    info(title = "Brain HTTP API", version = "1.0.0"),
    paths(
        admit_agentloop,
        get_agentloop,
        create_environment,
        list_environments,
        get_environment,
        delete_environment,
        create_session,
        list_sessions,
        get_session,
        delete_session,
        send_message,
        call_environment,
        events,
        cancel_session,
        end_session,
        serve_feed,
        resolve_tool_call,
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ServeQuery {
    after: Option<u64>,
    tools: String,
}

pub fn router<A: BrainApi>(api: A) -> Router {
    build(api, None)
}

pub fn router_with_bearer<A: BrainApi>(api: A, token: String) -> Router {
    build(api, Some(token))
}

fn build<A: BrainApi>(api: A, token: Option<String>) -> Router {
    let expected = token.map(|token| sha256(token.as_bytes()));
    let access = Access {
        api: api.clone(),
        expected,
        serves: Arc::new(ServeRegistry::new()),
    };
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
    let serve = serve_routes(access, &mut routed);
    let health = health_routes(api, &mut routed);
    // The published document is rendered from `ApiDoc`; the router from the same
    // annotations, through `documented`. A handler in one and not the other would ship
    // undocumented or unreachable.
    assert_eq!(
        routed,
        operations(&ApiDoc::openapi()),
        "brain-http: the routes and the OpenAPI document disagree"
    );
    protected.merge(serve).merge(health)
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

/// The credential surface of the serve group: requests authorized by the API token
/// or, per session, by that session's share key.
#[derive(Clone)]
struct Access<A> {
    api: A,
    expected: Option<[u8; 32]>,
    serves: Arc<ServeRegistry>,
}

impl<A: BrainApi> Access<A> {
    /// Whether this request may act on the session: open mode admits everything, and
    /// otherwise the bearer must be the API token or the session's share key. Both
    /// comparisons go through a digest, so neither is timing-sensitive.
    fn authorized(&self, headers: &HeaderMap, session_id: &SessionId) -> bool {
        let Some(expected) = &self.expected else {
            return true;
        };
        if bearer_matches(headers, expected) {
            return true;
        }
        let share = sha256(self.api.share_key(session_id).as_bytes());
        bearer_matches(headers, &share)
    }
}

/// Last-connection-wins seats: at most one live serve stream per (session, tool). A
/// new claim bumps the seat's generation and announces it; the stream holding the
/// older generation ends itself.
struct ServeRegistry {
    seats: StdMutex<HashMap<(String, String), u64>>,
    bus: tokio::sync::broadcast::Sender<Claim>,
}

#[derive(Clone)]
struct Claim {
    session: String,
    tool: String,
    generation: u64,
}

impl ServeRegistry {
    fn new() -> Self {
        let (bus, _) = tokio::sync::broadcast::channel(1024);
        Self {
            seats: StdMutex::new(HashMap::new()),
            bus,
        }
    }

    fn claim(&self, session: &str, tools: &[String]) -> HashMap<String, u64> {
        let mut seats = self.seats.lock().expect("serve seat table is poisoned");
        let mut mine = HashMap::with_capacity(tools.len());
        for tool in tools {
            let seat = seats.entry((session.to_owned(), tool.clone())).or_insert(0);
            *seat += 1;
            mine.insert(tool.clone(), *seat);
            let _ = self.bus.send(Claim {
                session: session.to_owned(),
                tool: tool.clone(),
                generation: *seat,
            });
        }
        mine
    }
}

fn protected_routes<A: BrainApi>(api: A, routed: &mut BTreeSet<String>) -> Router {
    let mut router = Router::new();
    for (path, method) in [
        documented::<__path_admit_agentloop, _, _, _>(routed, admit_agentloop::<A>),
        documented::<__path_get_agentloop, _, _, _>(routed, get_agentloop::<A>),
        documented::<__path_create_environment, _, _, _>(routed, create_environment::<A>),
        documented::<__path_list_environments, _, _, _>(routed, list_environments::<A>),
        documented::<__path_get_environment, _, _, _>(routed, get_environment::<A>),
        documented::<__path_delete_environment, _, _, _>(routed, delete_environment::<A>),
        documented::<__path_create_session, _, _, _>(routed, create_session::<A>),
        documented::<__path_list_sessions, _, _, _>(routed, list_sessions::<A>),
        documented::<__path_get_session, _, _, _>(routed, get_session::<A>),
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

/// Routes a share key can reach: the serve feed and the tool-results answer. Their
/// authorization is per session, so it happens in the handler rather than in a
/// router-wide layer.
fn serve_routes<A: BrainApi>(access: Access<A>, routed: &mut BTreeSet<String>) -> Router {
    let mut router = Router::new();
    for (path, method) in [
        documented::<__path_serve_feed, _, _, _>(routed, serve_feed::<A>),
        documented::<__path_resolve_tool_call, _, _, _>(routed, resolve_tool_call::<A>),
    ] {
        router = router.route(&path, method);
    }
    router
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(access)
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

#[utoipa::path(
    post,
    path = "/v1/environments",
    operation_id = "createEnvironment",
    description = "Creates an environment: Brain runs its setup and keeps what it declared it \
executes and offers. Sessions attach to it by id. A managed environment is closed by Brain \
once no session has been attached to it for its idle TTL; an unmanaged one lives until it \
is deleted.",
    params(("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    request_body = contract::CreateEnvironmentRequest,
    responses(
        (status = 200, description = "Created environment", body = contract::EnvironmentSummary),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn create_environment<A: BrainApi>(
    State(api): State<A>,
    headers: HeaderMap,
    Json(request): Json<CreateEnvironmentRequest>,
) -> Result<Json<EnvironmentSummary>, HttpError> {
    Ok(Json(
        api.create_environment(idempotency_key(&headers)?, request)
            .await
            .map_err(HttpError)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/environments/{environment_id}",
    operation_id = "getEnvironment",
    params(("environment_id" = contract::EnvironmentId, Path)),
    responses(
        (status = 200, description = "Environment state", body = contract::EnvironmentSummary),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn get_environment<A: BrainApi>(
    State(api): State<A>,
    Path(environment_id): Path<EnvironmentId>,
) -> Result<Json<EnvironmentSummary>, HttpError> {
    Ok(Json(
        api.get_environment(environment_id)
            .await
            .map_err(HttpError)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/environments",
    operation_id = "listEnvironments",
    responses(
        (status = 200, description = "Environments", body = contract::EnvironmentList),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn list_environments<A: BrainApi>(
    State(api): State<A>,
) -> Result<Json<EnvironmentList>, HttpError> {
    Ok(Json(api.list_environments().await.map_err(HttpError)?))
}

#[utoipa::path(
    delete,
    path = "/v1/environments/{environment_id}",
    operation_id = "deleteEnvironment",
    description = "Tears the environment down. Refused with `conflict` while a session is \
still attached; every session that was ever attached sees `environment_closed` on its \
events.",
    params(("environment_id" = contract::EnvironmentId, Path), ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    responses((status = 204, description = "Closed"), (status = "default", description = "Structured error", body = contract::ApiError))
)]
async fn delete_environment<A: BrainApi>(
    State(api): State<A>,
    Path(environment_id): Path<EnvironmentId>,
    headers: HeaderMap,
) -> Result<StatusCode, HttpError> {
    api.delete_environment(environment_id, idempotency_key(&headers)?)
        .await
        .map_err(HttpError)?;
    Ok(StatusCode::NO_CONTENT)
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
    path = "/v1/sessions/{session_id}/tool-results/{sequence}",
    operation_id = "resolveToolCall",
    description = "Answers a client-hosted tool call. The call is named by the sequence of \
its `tool_call_started` record on the event feed; the body is the call's outcome. \
Idempotent per call: a retry with the same key replays the first answer, and a call that \
is no longer pending is a conflict. Authorized by the API token or by the session's share \
key.",
    params(("session_id" = contract::SessionId, Path), ("sequence" = u64, Path, minimum = 1), ("Idempotency-Key" = String, Header, min_length = 1, max_length = 256)),
    request_body = contract::Outcome,
    responses((status = 204, description = "Outcome recorded"), (status = "default", description = "Structured error", body = contract::ApiError))
)]
async fn resolve_tool_call<A: BrainApi>(
    State(access): State<Access<A>>,
    Path((session_id, sequence)): Path<(SessionId, u64)>,
    headers: HeaderMap,
    Json(outcome): Json<Outcome>,
) -> Result<StatusCode, HttpError> {
    if !access.authorized(&headers, &session_id) {
        return Err(unauthorized());
    }
    access
        .api
        .resolve_tool_call(session_id, sequence, idempotency_key(&headers)?, outcome)
        .await
        .map_err(HttpError)?;
    Ok(StatusCode::NO_CONTENT)
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
                    // Lagged, or the session is gone. The stream ends rather than
                    // silently skipping records: a client reconnects with the cursor it
                    // last saw and the journal hands back exactly what it missed.
                    Err(_) => return None,
                }
            }
        });

    Ok(Sse::new(backlog.chain(following)).into_response())
}

/// The serve feed: pending client-hosted `tool_call_started` records for the claimed
/// tools, then matching records as they are appended. Always SSE. See the OpenAPI
/// description for the full contract.
#[utoipa::path(
    get,
    path = "/v1/sessions/{session_id}/serve",
    operation_id = "serveSessionTools",
    description = "The serve feed: an SSE stream of this session's client-hosted \
`tool_call_started` and `tool_cancel_started` records, filtered to the tools named in \
`tools`, plus `session_ended`. It opens with the still-pending backlog (calls with no \
finished record) and then carries records as they are appended. Authorized by the \
session's share key as a bearer token (the API token also works). One live consumer per \
tool: a new connection claiming a tool displaces the stream that held it, so a \
reconnecting client replaces its own dead connection instead of racing it.",
    params(
        ("session_id" = contract::SessionId, Path),
        (
            "tools" = String,
            Query,
            description = "Comma-separated client-hosted tool names this connection serves.",
            min_length = 1,
            max_length = 4096
        ),
        (
            "after" = Option<u64>,
            Query,
            description = "Resume cursor. Absent, the stream opens with the pending backlog; \
set, it replays every matching record after this sequence instead.",
            minimum = 0
        )
    ),
    responses(
        (status = 200, description = "Live serve stream", body = String, content_type = "text/event-stream"),
        (status = "default", description = "Structured error", body = contract::ApiError)
    )
)]
async fn serve_feed<A: BrainApi>(
    State(access): State<Access<A>>,
    Path(session_id): Path<SessionId>,
    Query(query): Query<ServeQuery>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    if !access.authorized(&headers, &session_id) {
        return Err(unauthorized());
    }
    let tools: Vec<String> = query
        .tools
        .split(',')
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if tools.is_empty() || tools.len() > MAX_SERVED_TOOLS {
        return Err(invalid("tools must name between 1 and 128 tools"));
    }
    let declared: HashSet<String> = access
        .api
        .client_tool_names(session_id.clone())
        .await
        .map_err(HttpError)?
        .into_iter()
        .collect();
    if let Some(unknown) = tools.iter().find(|name| !declared.contains(*name)) {
        return Err(invalid(format!(
            "{unknown} is not a client-hosted tool of this session"
        )));
    }
    let served: Arc<HashSet<String>> = Arc::new(tools.iter().cloned().collect());

    // Order matters: subscribe to both feeds before claiming the seats and before
    // reading the backlog, so nothing lands in a gap. Our own claims echo back on the
    // bus and are skipped by generation.
    let live = access.api.subscribe();
    let displaced = access.serves.bus.subscribe();
    let mine = Arc::new(access.serves.claim(session_id.as_str(), &tools));

    let (backlog_events, sent_through, ended) =
        serve_backlog(&access.api, &session_id, query.after, &served).await?;
    let backlog = futures_util::stream::iter(backlog_events.into_iter().map(sse));
    if ended {
        return Ok(Sse::new(backlog).into_response());
    }

    struct FollowState<L> {
        live: L,
        displaced: tokio::sync::broadcast::Receiver<Claim>,
        sent_through: u64,
        session_id: SessionId,
        served: Arc<HashSet<String>>,
        mine: Arc<HashMap<String, u64>>,
    }
    let state = FollowState {
        live,
        displaced,
        sent_through,
        session_id,
        served,
        mine,
    };
    let following = futures_util::stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        loop {
            tokio::select! {
                claim = state.displaced.recv() => {
                    match claim {
                        Ok(claim) => {
                            let superseded = claim.session == state.session_id.as_str()
                                && state
                                    .mine
                                    .get(&claim.tool)
                                    .is_some_and(|generation| claim.generation > *generation);
                            // A newer connection took one of these seats; this stream is
                            // the half-dead socket it displaces.
                            if superseded {
                                return None;
                            }
                        }
                        // A lagged displacement bus cannot say who holds the seat; end
                        // and let the client reconnect into a clean claim.
                        Err(_) => return None,
                    }
                }
                event = state.live.recv() => {
                    match event {
                        Ok((session, brain_protocol::LiveEvent::Recorded(event)))
                            if session == state.session_id
                                && event.sequence > state.sent_through
                                && serves_event(&event, &state.served) =>
                        {
                            state.sent_through = event.sequence;
                            let ended = is_last(&event);
                            let frame = sse(event);
                            return Some((frame, (!ended).then_some(state)));
                        }
                        Ok(_) => continue,
                        Err(_) => return None,
                    }
                }
            }
        }
    });
    Ok(Sse::new(backlog.chain(following)).into_response())
}

/// What the serve stream opens with. Without a cursor: the still-pending calls —
/// every matching `tool_call_started` with no `tool_call_ended` yet whose deadline
/// has not already passed (the session would have timed those out; replaying them
/// would run side effects nothing is waiting for). With a cursor: an exact replay of
/// matching records after it, the same resume contract as the events feed.
async fn serve_backlog<A: BrainApi>(
    api: &A,
    session_id: &SessionId,
    after: Option<u64>,
    served: &HashSet<String>,
) -> Result<(Vec<brain_protocol::Event>, u64, bool), HttpError> {
    let replay_all = after.is_some();
    let mut cursor = after.unwrap_or(0);
    let mut kept: Vec<brain_protocol::Event> = Vec::new();
    let mut pending: HashMap<u64, usize> = HashMap::new();
    let mut ended = false;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    loop {
        let page = api
            .events(session_id.clone(), Some(cursor))
            .await
            .map_err(HttpError)?;
        for event in page.events {
            ended = ended || is_last(&event);
            if serves_event(&event, served) {
                if replay_all || is_last(&event) {
                    kept.push(event);
                    continue;
                }
                if event.event_type == "tool_call_started" {
                    let alive = event
                        .data
                        .get("deadline_ms")
                        .and_then(serde_json::Value::as_u64)
                        .is_none_or(|deadline| event.recorded_at_ms + deadline > now_ms);
                    if alive {
                        pending.insert(event.sequence, kept.len());
                        kept.push(event);
                    }
                }
                // Backlog cancellations target calls that resolve moments later; only
                // the live tail needs them.
            } else if !replay_all
                && event.event_type == "tool_call_ended"
                && let Some(started) = event
                    .data
                    .get("sequence")
                    .and_then(serde_json::Value::as_u64)
                && let Some(index) = pending.remove(&started)
            {
                kept[index].sequence = 0; // answered: marked for removal below
            }
        }
        if page.next_cursor == cursor {
            break;
        }
        cursor = page.next_cursor;
    }
    if !replay_all {
        kept.retain(|event| event.sequence != 0);
    }
    Ok((kept, cursor, ended))
}

/// Whether a record belongs on a serve stream claiming these tools: the session's
/// end, and client-hosted calls (and their cancellations) for the claimed names.
fn serves_event(event: &brain_protocol::Event, served: &HashSet<String>) -> bool {
    if event.event_type == "session_ended" {
        return true;
    }
    if event.event_type != "tool_call_started" && event.event_type != "tool_cancel_started" {
        return false;
    }
    let Some(binding) = event.data.get("binding") else {
        return false;
    };
    binding.get("hosting").and_then(serde_json::Value::as_str) == Some("client")
        && binding
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| served.contains(name))
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
