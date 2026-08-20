//! The session API v1 over axum: the HTTP surface defined by
//! `contracts/session/v1/openapi.yaml`. Shapes come from `brain-protocol`; this
//! module only routes, authenticates and maps errors -- it never invents wire formats.
//!
//! SSE framing: `id: <seq>` / `event: <type>` / `data: <Event JSON>`. Replay comes from the
//! journal (`?after=` or `Last-Event-ID`); the live tail comes from the session's event hub.
//! The subscription starts BEFORE the replay read so nothing falls between them; duplicates
//! are dropped by seq.
//!
//! Create and output admission persist hashes of Idempotency-Key for replay. Raw keys never enter
//! the journal.

use crate::events::{event_seq, event_type};
use crate::journal::Record;
use crate::session::Brain;
use crate::{BrainError, mint_id};
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event as SseFrame, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use brain_protocol::session::{
    self, ApiError, ApiErrorCode, ApiErrorResponse, Artifact, ArtifactList, CreateSessionRequest,
    FileList, FileListObject, MessageAccepted, MessageRequest, PersistRequest, SessionList,
};
use futures_util::stream::Stream;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashSet;
use std::convert::Infallible;
use std::num::NonZeroU64;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub brain: Arc<Brain>,
    /// The dev-plane bearer token. Identity proper is slice 4.
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/sessions", post(create_session).get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session).delete(delete_session))
        .route("/v1/sessions/{id}/messages", post(send_message))
        .route("/v1/sessions/{id}/events", get(stream_events))
        .route("/v1/sessions/{id}/attached", get(attached_worker))
        .route("/v1/sessions/{id}/cancel", post(cancel_turn))
        .route("/v1/sessions/{id}/end", post(end_session))
        .route("/v1/sessions/{id}/files", get(list_files))
        .route(
            "/v1/sessions/{id}/files/{*path}",
            get(download_file).put(upload_file),
        )
        .route("/v1/sessions/{id}/persist", post(persist_artifact))
        .route("/v1/sessions/{id}/artifacts", get(list_artifacts))
        .route("/v1/sessions/{id}/artifacts/{name}", get(get_artifact))
        .with_state(state)
}

async fn attached_worker(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.max_message_size(128 * 1024)
        .max_frame_size(128 * 1024)
        .on_upgrade(move |socket| serve_attached_worker(state, id, socket))
        .into_response()
}

async fn serve_attached_worker(state: AppState, session_id: String, socket: WebSocket) {
    let (mut sink, mut source) = socket.split();
    let hello = tokio::time::timeout(std::time::Duration::from_secs(5), source.next()).await;
    let Some(Ok(WsMessage::Text(text))) = hello.ok().flatten() else {
        let _ = sink
            .send(WsMessage::Text(
                serde_json::to_string(&crate::attached::ServerFrame::Error {
                    message: "attached worker hello was not received".into(),
                })
                .unwrap_or_default()
                .into(),
            ))
            .await;
        return;
    };
    let frame = serde_json::from_str::<crate::attached::ClientFrame>(&text);
    let (token, callbacks) = match frame {
        Ok(crate::attached::ClientFrame::Hello { token, callbacks }) => (token, callbacks),
        _ => {
            let _ = sink.close().await;
            return;
        }
    };
    if token != state.token {
        let _ = sink
            .send(WsMessage::Text(
                serde_json::to_string(&crate::attached::ServerFrame::Error {
                    message: "invalid bearer token".into(),
                })
                .unwrap_or_default()
                .into(),
            ))
            .await;
        return;
    }
    let expected = match state.brain.attached_callbacks(&session_id).await {
        Ok(expected) if !expected.is_empty() => expected,
        _ => {
            let _ = sink.close().await;
            return;
        }
    };
    let callbacks: HashSet<_> = callbacks.into_iter().collect();
    let mut registration = match state
        .brain
        .attached
        .register(&session_id, callbacks, &expected)
    {
        Ok(registration) => registration,
        Err(error) => {
            let _ = sink
                .send(WsMessage::Text(
                    serde_json::to_string(&crate::attached::ServerFrame::Error {
                        message: error.to_string(),
                    })
                    .unwrap_or_default()
                    .into(),
                ))
                .await;
            return;
        }
    };
    let ready = serde_json::to_string(&crate::attached::ServerFrame::Ready).unwrap_or_default();
    if sink.send(WsMessage::Text(ready.into())).await.is_err() {
        registration.disconnect();
        return;
    }
    loop {
        tokio::select! {
            outbound = registration.outbound.recv() => {
                let Some(frame) = outbound else { break };
                let Ok(text) = serde_json::to_string(&frame) else { break };
                if sink.send(WsMessage::Text(text.into())).await.is_err() { break; }
            }
            inbound = source.next() => {
                let Some(Ok(message)) = inbound else { break };
                match message {
                    WsMessage::Text(text) => {
                        if let Ok(crate::attached::ClientFrame::Result { call_id, ok, output, error }) =
                            serde_json::from_str::<crate::attached::ClientFrame>(&text)
                        {
                            state.brain.attached.complete(
                                &session_id,
                                &call_id,
                                crate::attached::AttachedResult { ok, output, error },
                            );
                        }
                    }
                    WsMessage::Close(_) => break,
                    WsMessage::Ping(bytes) => {
                        if sink.send(WsMessage::Pong(bytes)).await.is_err() { break; }
                    }
                    WsMessage::Pong(_) | WsMessage::Binary(_) => {}
                }
            }
        }
    }
    registration.disconnect();
}

pub async fn serve(state: AppState, addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "brain listening");
    axum::serve(nodelay(listener), app).await?;
    Ok(())
}

/// TCP_NODELAY on every accepted connection. Load-bearing for the event stream: SSE frames are
/// small writes, and Nagle + delayed ACK turns each one into a ~40 ms stall on Linux — the
/// slice-5 TTFT gate measured a hard 46 ms floor per turn before this (1 ms after).
pub fn nodelay(
    listener: tokio::net::TcpListener,
) -> impl axum::serve::Listener<Io = tokio::net::TcpStream, Addr = std::net::SocketAddr> {
    use axum::serve::ListenerExt as _;
    listener.tap_io(|io| {
        let _ = io.set_nodelay(true);
    })
}

// ---------------------------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------------------------

struct Failure(StatusCode, ApiErrorCode, String);

fn api_code(value: &str) -> ApiErrorCode {
    value.parse().expect("static API error code")
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            error: ApiError {
                code: self.1,
                details: None,
                message: self.2,
                param: None,
                request_id: Some(mint_id("req", 16)),
            },
        };
        (self.0, Json(body)).into_response()
    }
}

fn map_err(e: BrainError) -> Failure {
    use StatusCode as S;
    let (status, code) = match &e {
        BrainError::Invalid(_) | BrainError::PrefixSealed { .. } | BrainError::Serde(_) => {
            (S::BAD_REQUEST, "invalid_request")
        }
        BrainError::NoSuchSession(_) => (S::NOT_FOUND, "not_found"),
        BrainError::FileNotFound(_) => (S::NOT_FOUND, "not_found"),
        BrainError::FileTooLarge { .. } => (S::PAYLOAD_TOO_LARGE, "invalid_request"),
        BrainError::TurnInFlight(_) => (S::CONFLICT, "session_busy"),
        BrainError::IdempotencyConflict => (S::CONFLICT, "conflict"),
        BrainError::SessionDeleted(_) => (S::GONE, "session_deleted"),
        BrainError::SessionFailed(_) => (S::CONFLICT, "session_failed"),
        BrainError::Overloaded => (S::TOO_MANY_REQUESTS, "rate_limited"),
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => {
            (S::BAD_GATEWAY, "provider_error")
        }
        BrainError::Cancelled => (S::CONFLICT, "cancelled"),
        BrainError::HandUnavailable(_) | BrainError::Hand(_) => {
            (S::SERVICE_UNAVAILABLE, "hand_unavailable")
        }
        _ => (S::INTERNAL_SERVER_ERROR, "internal"),
    };
    // The message is safe to surface: BrainError never carries credentials (ProviderKey's
    // Debug redacts, and no variant embeds request bodies).
    Failure(status, api_code(code), e.to_string())
}

fn auth(state: &AppState, headers: &HeaderMap) -> Result<(), Failure> {
    let got = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if got == Some(state.token.as_str()) {
        Ok(())
    } else {
        Err(Failure(
            StatusCode::UNAUTHORIZED,
            api_code("unauthorized"),
            "missing or invalid bearer token".into(),
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------------

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<session::Session>), Failure> {
    auth(&state, &headers)?;
    let idempotency_key = headers
        .get("idempotency-key")
        .map(|value| {
            value.to_str().map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    ApiErrorCode::InvalidRequest,
                    "Idempotency-Key must be valid ASCII".into(),
                )
            })
        })
        .transpose()?;
    let doc = state
        .brain
        .create_session(req, idempotency_key)
        .await
        .map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(doc)))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    20
}

async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<SessionList>, Failure> {
    auth(&state, &headers)?;
    let data = state
        .brain
        .list(q.limit.clamp(1, 100))
        .await
        .map_err(map_err)?;
    Ok(Json(SessionList {
        data,
        has_more: false,
        next_cursor: None,
        object: session::SessionListObject::List,
    }))
}

async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<session::Session>, Failure> {
    auth(&state, &headers)?;
    Ok(Json(state.brain.get(&id).await.map_err(map_err)?))
}

async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Failure> {
    auth(&state, &headers)?;
    state.brain.delete(&id).await.map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn send_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<MessageRequest>,
) -> Result<(StatusCode, Json<MessageAccepted>), Failure> {
    auth(&state, &headers)?;
    let (turn_id, seq) = state
        .brain
        .message_with_metadata(&id, req.content, req.metadata)
        .await
        .map_err(map_err)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(MessageAccepted {
            seq: NonZeroU64::new(seq.max(1)).expect("nonzero"),
            session_id: id.parse().map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "session id".into(),
                )
            })?,
            turn_id: turn_id.parse().map_err(|_| {
                Failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_code("internal"),
                    "turn id".into(),
                )
            })?,
        }),
    ))
}

async fn cancel_turn(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<session::Session>, Failure> {
    auth(&state, &headers)?;
    Ok(Json(state.brain.cancel(&id).await.map_err(map_err)?))
}

async fn end_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<session::Session>, Failure> {
    auth(&state, &headers)?;
    Ok(Json(state.brain.end(&id).await.map_err(map_err)?))
}

#[derive(Deserialize)]
struct FilesQuery {
    #[serde(default = "default_files_path")]
    path: String,
    #[serde(default)]
    recursive: bool,
}

fn default_files_path() -> String {
    "/workspace".into()
}

async fn list_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<FilesQuery>,
) -> Result<Json<FileList>, Failure> {
    auth(&state, &headers)?;
    let listing = state
        .brain
        .list_files(&id, q.path, q.recursive)
        .await
        .map_err(map_err)?;
    Ok(Json(FileList {
        data: listing.entries,
        object: FileListObject::List,
        source: listing.source,
        synced_at: listing.synced_ms.map(|ms| crate::events::ts(ms).0),
    }))
}

async fn download_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, Failure> {
    auth(&state, &headers)?;
    let file = state.brain.read_file(&id, path).await.map_err(map_err)?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        Bytes::from(file.bytes),
    )
        .into_response())
}

async fn upload_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, path)): Path<(String, String)>,
    body: Body,
) -> Result<Json<session::FileEntry>, Failure> {
    auth(&state, &headers)?;
    let bytes = to_bytes(body, state.brain.cfg.max_file_bytes)
        .await
        .map_err(|_| {
            map_err(BrainError::FileTooLarge {
                limit: state.brain.cfg.max_file_bytes,
            })
        })?;
    Ok(Json(
        state
            .brain
            .write_file(&id, path, bytes.to_vec())
            .await
            .map_err(map_err)?,
    ))
}

async fn persist_artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PersistRequest>,
) -> Result<(StatusCode, Json<Artifact>), Failure> {
    auth(&state, &headers)?;
    let doc = state
        .brain
        .persist(&id, req.name.to_string(), req.path, req.media_type)
        .await
        .map_err(map_err)?;
    let a = artifact_of(&state, &id, &doc).await;
    Ok((StatusCode::CREATED, Json(a)))
}

async fn list_artifacts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ArtifactList>, Failure> {
    auth(&state, &headers)?;
    let head = state.brain.head(&id).await.map_err(map_err)?;
    let mut data = Vec::with_capacity(head.doc.artifacts.len());
    for doc in &head.doc.artifacts {
        data.push(artifact_of(&state, &id, doc).await);
    }
    Ok(Json(ArtifactList {
        data,
        object: session::ArtifactListObject::List,
    }))
}

async fn get_artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<Artifact>, Failure> {
    auth(&state, &headers)?;
    let head = state.brain.head(&id).await.map_err(map_err)?;
    let doc = head
        .doc
        .artifacts
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| {
            Failure(
                StatusCode::NOT_FOUND,
                api_code("not_found"),
                format!("no artifact {name}"),
            )
        })?;
    Ok(Json(artifact_of(&state, &id, doc).await))
}

async fn artifact_of(
    state: &AppState,
    session_id: &str,
    doc: &crate::journal::ArtifactDoc,
) -> Artifact {
    let url = state.brain.artifact_url(session_id, doc).await;
    Artifact {
        bytes: doc.bytes,
        created_at: crate::events::ts(doc.created_ms),
        download_url_expires_at: url
            .is_some()
            .then(|| crate::events::ts(crate::wall_ms() + 900 * 1000)),
        download_url: url,
        media_type: doc.media_type.clone(),
        name: doc.name.clone(),
        object: session::ArtifactObject::Artifact,
        session_id: session_id
            .parse()
            .unwrap_or_else(|_| "ses_00000000000000000000".parse().expect("id")),
        sha256: doc
            .sha256
            .parse()
            .unwrap_or_else(|_| "0".repeat(64).parse().expect("hex")),
    }
}

// ---------------------------------------------------------------------------------------------
// SSE
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_follow")]
    follow: bool,
}
fn default_follow() -> bool {
    true
}

async fn stream_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<SseFrame, Infallible>>>, Failure> {
    auth(&state, &headers)?;
    // Last-Event-ID (reconnect) wins over ?after.
    let after = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(q.after);

    // Existence check first: a stream for a missing session must 404, not hang.
    let head = state.brain.head(&id).await.map_err(map_err)?;
    if head.doc.state == "deleted" {
        return Err(Failure(
            StatusCode::NOT_FOUND,
            api_code("not_found"),
            "session deleted".into(),
        ));
    }

    let brain = state.brain.clone();
    let follow = q.follow;
    // `session.updated` replay renders the hand as it is NOW (session facts live on HEAD, not
    // in the record); historical hand snapshots are not kept.
    let info = head.doc.hand_info.clone();
    let stream = async_stream::stream! {
        // Subscribe BEFORE replaying so no event falls between journal and live tail.
        let mut rx = brain.hub.subscribe(&id);
        let mut last = after;

        match brain.journal.read_records(&id, after).await {
            Ok(entries) => {
                for e in &entries {
                    if let Some(ev) = crate::events::derive(&id, e.seq, e.ts_ms, &e.record, &info) {
                        last = e.seq;
                        yield Ok(frame(&ev));
                    }
                    // Even recordless seqs advance the cursor so the live tail dedupes right.
                    last = last.max(e.seq);
                }
            }
            Err(e) => {
                tracing::warn!(session = %id, error = %e, "replay failed");
            }
        }

        if !follow {
            return;
        }
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let seq = event_seq(&ev);
                    if seq <= last {
                        continue; // replayed already
                    }
                    last = seq;
                    let stop = matches!(&*ev, session::Event::SessionUpdated { state, .. }
                        if *state == session::SessionState::Deleted);
                    yield Ok(frame(&ev));
                    if stop {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(session = %id, lagged = n, "sse consumer lagged; events skipped");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn frame(ev: &session::Event) -> SseFrame {
    SseFrame::default()
        .id(event_seq(ev).to_string())
        .event(event_type(ev))
        .data(serde_json::to_string(ev).unwrap_or_else(|_| "{}".into()))
}

// Re-exported for the M0 gate binary: build one deterministic replay of a session's durable
// events (no follow), used to assert byte-stable replay.
pub async fn replay(
    brain: &Arc<Brain>,
    session_id: &str,
    after: u64,
) -> crate::Result<Vec<session::Event>> {
    let head = brain.journal.get_head(session_id).await?;
    let info = head.doc.hand_info.clone();
    let entries = brain.journal.read_records(session_id, after).await?;
    let mut out = Vec::new();
    for e in &entries {
        if let Some(ev) = crate::events::derive(session_id, e.seq, e.ts_ms, &e.record, &info) {
            out.push(ev);
        }
    }
    let _ = Record::TurnStarted {
        turn: String::new(),
    }; // keep the import honest
    Ok(out)
}
