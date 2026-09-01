use std::{
    collections::{HashMap, HashSet},
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
const MAX_SERVED_TOOLS: usize = 128;

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
    let mut protected = protected_routes(api.clone());
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
    protected
        .merge(serve_routes(access))
        .merge(health_routes(api))
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
            "/v1/sessions/{session_id}/cancel",
            post(cancel_session::<A>),
        )
        .route("/v1/sessions/{session_id}/end", post(end_session::<A>))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(api)
}

/// Routes a share key can reach: the serve feed and the tool-results answer. Their
/// authorization is per session, so it happens in the handler rather than in a
/// router-wide layer.
fn serve_routes<A: BrainApi>(access: Access<A>) -> Router {
    Router::new()
        .route("/v1/sessions/{session_id}/serve", get(serve_feed::<A>))
        .route(
            "/v1/sessions/{session_id}/tool-results/{operation_id}",
            post(resolve_tool_call::<A>),
        )
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(access)
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
    HttpError(brain_protocol::ApiError {
        code: "unauthorized".into(),
        message: "a valid bearer token is required".into(),
        retryable: false,
        details: None,
    })
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
    State(access): State<Access<A>>,
    Path((session_id, operation_id)): Path<(SessionId, OperationId)>,
    headers: HeaderMap,
    Json(outcome): Json<Outcome>,
) -> Result<StatusCode, HttpError> {
    if !access.authorized(&headers, &session_id) {
        return Err(unauthorized());
    }
    access
        .api
        .resolve_tool_call(
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

/// The serve feed: pending client-hosted `tool_intent` records for the claimed
/// tools, then matching records as they are appended. Always SSE. See the OpenAPI
/// description for the full contract.
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

/// What the serve stream opens with. Without a cursor: the still-pending intents —
/// every matching `tool_intent` with no `tool_result` yet whose deadline has not
/// already passed (the kernel would have timed those out; replaying them would run
/// side effects nothing is waiting for). With a cursor: an exact replay of matching
/// records after it, the same resume contract as the events feed.
async fn serve_backlog<A: BrainApi>(
    api: &A,
    session_id: &SessionId,
    after: Option<u64>,
    served: &HashSet<String>,
) -> Result<(Vec<brain_protocol::Event>, u64, bool), HttpError> {
    let replay_all = after.is_some();
    let mut cursor = after.unwrap_or(0);
    let mut kept: Vec<brain_protocol::Event> = Vec::new();
    let mut pending: HashMap<String, usize> = HashMap::new();
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
                if event.event_type == "tool_intent" {
                    let alive = event
                        .data
                        .get("deadline_ms")
                        .and_then(serde_json::Value::as_u64)
                        .is_none_or(|deadline| event.recorded_at_ms + deadline > now_ms);
                    if let Some(operation) = operation_id_of(&event)
                        && alive
                    {
                        pending.insert(operation, kept.len());
                        kept.push(event);
                    }
                }
                // Backlog cancel intents target calls that resolve moments later; only
                // the live tail needs them.
            } else if !replay_all
                && event.event_type == "tool_result"
                && let Some(operation) = event
                    .data
                    .get("operation_id")
                    .and_then(serde_json::Value::as_str)
                && let Some(index) = pending.remove(operation)
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

fn operation_id_of(event: &brain_protocol::Event) -> Option<String> {
    event
        .data
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Whether a record belongs on a serve stream claiming these tools: the session's
/// end, and client-hosted intents (and their cancellations) for the claimed names.
fn serves_event(event: &brain_protocol::Event, served: &HashSet<String>) -> bool {
    if event.event_type == "session_ended" {
        return true;
    }
    if event.event_type != "tool_intent" && event.event_type != "tool_cancel_intent" {
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
