//! The session API v1 over axum: the HTTP surface defined by
//! `contracts/session/v1/openapi.yaml`. Shapes come from `brain-protocol`; this
//! module only routes, authenticates and maps errors -- it never invents wire formats.
//!
//! SSE framing: `id: <seq>` / `event: <type>` / `data: <Event JSON>`. Replay comes from the
//! journal (`?after=` or `Last-Event-ID`); the live tail comes from the session's event hub.
//! The subscription starts BEFORE the replay read so nothing falls between them; duplicates
//! are dropped by seq.
//!
//! Create and message admission persist hashes of Idempotency-Key for replay. Raw keys never
//! enter the journal.

use crate::events::{event_is_ephemeral, event_seq, event_type};
use crate::journal::Record;
use crate::session::{Brain, TrustedPrincipal};
use crate::{BrainError, mint_id};
use axum::body::Bytes;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseFrame, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use brain_protocol::session::{
    self, ApiError, ApiErrorCode, ApiErrorResponse, CreateSessionRequest, MessageAccepted,
    MessageRequest, MessageRequestContent, SessionList,
};
use futures_util::stream::Stream;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::num::NonZeroU64;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub brain: Arc<Brain>,
    /// The dev-plane bearer token. Identity proper is slice 4.
    pub token: String,
}

/// Ordinary control requests contain only bounded identifiers, paths, cursors and page options.
const SMALL_JSON_BODY_LIMIT_BYTES: usize = 32 * 1024;
/// Inline file bodies carry at most 1 MiB decoded bytes plus base64 and JSON framing.
const INLINE_FILE_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

pub fn router(state: AppState) -> Router {
    state.brain.start_recovery_worker();
    let create = Router::new()
        .route(
            "/v1/sessions",
            post(create_session).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_CREATE_SESSION_REQUEST_BYTES,
            )),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            create_admission_before_body,
        ));
    let operator = Router::new()
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session).delete(delete_session))
        .route("/v1/sessions/{id}/deletion", get(get_deletion_status))
        .route(
            "/v1/sessions/{id}/messages",
            post(send_message).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_MESSAGE_REQUEST_BYTES,
            )),
        )
        .route("/v1/sessions/{id}/events", get(stream_events))
        .route("/v1/sessions/{id}/cancel", post(cancel_turn))
        .route("/v1/sessions/{id}/end", post(end_session))
        .route(
            "/v1/sessions/{id}/sandbox",
            get(get_default_sandbox).post(create_default_sandbox),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/list",
            post(sandbox_file_list).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/stat",
            post(sandbox_file_stat).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/read-inline",
            post(sandbox_file_read_inline)
                .layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/write-inline",
            post(sandbox_file_write_inline)
                .layer(DefaultBodyLimit::max(INLINE_FILE_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/downloads",
            post(sandbox_file_prepare_download)
                .layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/uploads",
            post(sandbox_file_prepare_upload)
                .layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/uploads/{transfer_id}/complete",
            post(sandbox_file_complete_upload),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/find",
            post(sandbox_file_find).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/sandbox/files/grep",
            post(sandbox_file_grep).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/children",
            post(create_child)
                .layer(DefaultBodyLimit::max(
                    brain_protocol::MAX_MESSAGE_REQUEST_BYTES,
                ))
                .get(list_children),
        )
        .route("/v1/sessions/{id}/children/{child_id}", get(get_child))
        .route(
            "/v1/sessions/{id}/children/{child_id}/messages",
            post(send_child_message).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_MESSAGE_REQUEST_BYTES,
            )),
        )
        .route(
            "/v1/sessions/{id}/children/{child_id}/follow-up",
            post(follow_up_child).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_MESSAGE_REQUEST_BYTES,
            )),
        )
        .route(
            "/v1/sessions/{id}/children/{child_id}/wait",
            post(wait_child).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/children/{child_id}/interrupt",
            post(interrupt_child),
        )
        .route("/v1/sessions/{id}/children/{child_id}/end", post(end_child))
        .route(
            "/v1/sessions/{id}/storage/list",
            post(storage_list).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/stat",
            post(storage_stat).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/read-inline",
            post(storage_read_inline).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/write-inline",
            post(storage_write_inline).layer(DefaultBodyLimit::max(INLINE_FILE_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/downloads",
            post(storage_prepare_download)
                .layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/uploads",
            post(storage_prepare_upload).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/uploads/{transfer_id}/complete",
            post(storage_complete_upload),
        )
        .route(
            "/v1/sessions/{id}/storage/delete",
            post(storage_delete).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/copy-from-sandbox",
            post(storage_copy_from_sandbox)
                .layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/sessions/{id}/storage/copy-to-sandbox",
            post(storage_copy_to_sandbox).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/v1/customer-hand/grants",
            post(customer_hand_grant).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/internal/v1/customer-hand/grants",
            post(customer_hand_grant).layer(DefaultBodyLimit::max(SMALL_JSON_BODY_LIMIT_BYTES)),
        )
        .route(
            "/internal/v1/customer-hand/gateway",
            post(customer_hand_gateway).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_CUSTOMER_WS_FRAME_BYTES,
            )),
        )
        .merge(create)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            operator_auth_before_body,
        ));

    let public_observation = Router::new()
        .route(
            "/v1/customer-hand/observations/{grant_id}",
            post(customer_hand_observation).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_CUSTOMER_OBSERVATION_BYTES,
            )),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            public_observation_auth_before_body,
        ));
    let internal_observation = Router::new()
        .route(
            "/internal/v1/customer-hand/observations/{grant_id}",
            post(internal_customer_hand_observation).layer(DefaultBodyLimit::max(
                brain_protocol::MAX_CUSTOMER_OBSERVATION_BYTES,
            )),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            internal_observation_auth_before_body,
        ));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/customer-hand/socket", get(customer_hand_socket))
        .merge(operator)
        .merge(public_observation)
        .merge(internal_observation)
        .with_state(state)
}

async fn operator_auth_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(failure) = auth(&state, request.headers()) {
        return failure.into_response();
    }
    next.run(request).await
}

async fn create_admission_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let permit = match state.brain.try_admit_create() {
        Ok(permit) => permit,
        Err(error) => return map_err(error).into_response(),
    };
    let response = next.run(request).await;
    drop(permit);
    response
}

fn observation_grant_id(request: &Request) -> Option<String> {
    request
        .uri()
        .path()
        .rsplit_once('/')
        .map(|(_, grant_id)| grant_id)
        .filter(|grant_id| !grant_id.is_empty())
        .map(str::to_owned)
}

async fn authorize_observation_before_body(
    state: &AppState,
    grant_id: Option<String>,
    token: Option<String>,
) -> Result<(), Failure> {
    let grant_id = grant_id.ok_or_else(invalid_observation_grant)?;
    let token = token.ok_or_else(invalid_observation_grant)?;
    let coordinator = state.brain.customer.as_ref().ok_or_else(|| {
        map_err(BrainError::HandUnavailable(
            "customer Hand is unavailable".into(),
        ))
    })?;
    coordinator
        .authorize_observation(&grant_id, &token)
        .await
        .map_err(|_| invalid_observation_grant())
}

async fn public_observation_auth_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let grant_id = observation_grant_id(&request);
    let token = bearer_token(request.headers()).map(str::to_owned);
    if let Err(failure) = authorize_observation_before_body(&state, grant_id, token).await {
        return failure.into_response();
    }
    next.run(request).await
}

async fn internal_observation_auth_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(failure) = auth(&state, request.headers()) {
        return failure.into_response();
    }
    let grant_id = observation_grant_id(&request);
    let token = observation_grant_header(request.headers()).map(str::to_owned);
    if let Err(failure) = authorize_observation_before_body(&state, grant_id, token).await {
        return failure.into_response();
    }
    next.run(request).await
}

#[derive(Deserialize)]
struct CustomerGrantRequest {
    client_id: String,
}

#[derive(Serialize)]
struct CustomerGrantResponse {
    url: String,
    protocol: String,
    expires_at: brain_protocol::session::Timestamp,
    grant_id: String,
    observation_url: String,
    observation_token: String,
}

async fn customer_hand_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CustomerGrantRequest>,
) -> Result<Json<CustomerGrantResponse>, Failure> {
    let principal = auth(&state, &headers)?;
    let coordinator = state.brain.customer.as_ref().ok_or_else(|| {
        Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            api_code("service_unavailable"),
            "customer-app Tools are unavailable in this composition".into(),
        )
    })?;
    let grant = coordinator
        .grant(principal.as_str(), &request.client_id)
        .await
        .map_err(map_err)?;
    Ok(Json(CustomerGrantResponse {
        url: grant.url,
        protocol: grant.protocol,
        expires_at: crate::events::ts(grant.expires_at_ms),
        grant_id: grant.grant_id,
        observation_url: grant.observation_url,
        observation_token: grant.observation_token,
    }))
}

async fn customer_hand_socket(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, Failure> {
    let coordinator = state.brain.customer.clone().ok_or_else(|| {
        Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            api_code("service_unavailable"),
            "customer-app Tools are unavailable in this composition".into(),
        )
    })?;
    let protocol = customer_grant_subprotocol(&headers)?;
    let connection_id = mint_id("conn", 24);
    crate::customer::CustomerHandIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Connect,
            connection_id: connection_id.clone(),
            request_id: mint_id("req", 16),
            route_key: "$connect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: Some(protocol.clone()),
            body: None,
        },
    )
    .await
    .map_err(map_err)?;
    Ok(ws
        .protocols([protocol])
        .max_message_size(crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES)
        .max_frame_size(crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES)
        .on_upgrade(move |socket| serve_customer_hand_socket(coordinator, connection_id, socket))
        .into_response())
}

async fn serve_customer_hand_socket(
    coordinator: Arc<crate::customer::CustomerCoordinator>,
    connection_id: String,
    socket: WebSocket,
) {
    let (mut sink, mut source) = socket.split();
    let (sender, mut outbound) = tokio::sync::mpsc::channel(128);
    if coordinator
        .bind_local_sender(&connection_id, sender)
        .await
        .is_err()
    {
        let _ = sink.close().await;
        return;
    }
    loop {
        tokio::select! {
            frame = outbound.recv() => {
                let Some(frame) = frame else { break; };
                let Ok(bytes) = frame.to_frame() else { break; };
                let Ok(text) = String::from_utf8(bytes) else { break; };
                if sink.send(WsMessage::Text(text.into())).await.is_err() { break; }
            }
            frame = source.next() => {
                let Some(Ok(frame)) = frame else { break; };
                match frame {
                    WsMessage::Text(text) => {
                        let result = crate::customer::CustomerHandIngressPort::receive(
                            coordinator.as_ref(),
                            crate::customer::CustomerGatewayInput {
                                route: crate::customer::CustomerGatewayRoute::Message,
                                connection_id: connection_id.clone(),
                                request_id: mint_id("req", 16),
                                route_key: "$default".into(),
                                source_ip: "127.0.0.1".into(),
                                subprotocol: None,
                                body: Some(text.to_string()),
                            },
                        ).await;
                        if result.is_err() { break; }
                    }
                    WsMessage::Ping(bytes) => {
                        if sink.send(WsMessage::Pong(bytes)).await.is_err() { break; }
                    }
                    WsMessage::Close(_) => break,
                    WsMessage::Binary(_) | WsMessage::Pong(_) => {}
                }
            }
        }
    }
    let _ = crate::customer::CustomerHandIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Disconnect,
            connection_id,
            request_id: mint_id("req", 16),
            route_key: "$disconnect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: None,
        },
    )
    .await;
}

async fn customer_hand_observation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, Failure> {
    let observation_token = bearer_token(&headers).ok_or_else(invalid_observation_grant)?;
    apply_customer_hand_observation(&state, &grant_id, observation_token, &body).await
}

async fn internal_customer_hand_observation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, Failure> {
    // Internal callers authenticate as the operator/service with Authorization. The scoped
    // customer observation grant is deliberately carried in a separate header so the two
    // authorities cannot be confused or substituted for one another.
    auth(&state, &headers)?;
    let observation_token =
        observation_grant_header(&headers).ok_or_else(invalid_observation_grant)?;
    apply_customer_hand_observation(&state, &grant_id, observation_token, &body).await
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
}

fn observation_grant_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-brain-observation-grant")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

fn invalid_observation_grant() -> Failure {
    Failure(
        StatusCode::UNAUTHORIZED,
        api_code("unauthorized"),
        "invalid customer Hand observation grant".into(),
    )
}

async fn apply_customer_hand_observation(
    state: &AppState,
    grant_id: &str,
    observation_token: &str,
    body: &[u8],
) -> Result<StatusCode, Failure> {
    if body.len() > crate::customer::MAX_CUSTOMER_HTTP_OBSERVATION_BYTES {
        return Err(map_err(BrainError::FileTooLarge {
            limit: crate::customer::MAX_CUSTOMER_HTTP_OBSERVATION_BYTES,
        }));
    }
    let observation = serde_json::from_slice(body).map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("customer Hand observation: {error}"),
        )
    })?;
    state
        .brain
        .customer
        .as_ref()
        .ok_or_else(|| {
            map_err(BrainError::HandUnavailable(
                "customer Hand is unavailable".into(),
            ))
        })?
        .observation(grant_id, observation_token, observation)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn customer_hand_gateway(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Failure> {
    auth(&state, &headers)?;
    let connection_id = trusted_header(&headers, "x-brain-connection-id")?;
    let request_id = trusted_header(&headers, "x-brain-request-id")?;
    let route_key = trusted_header(&headers, "x-brain-route-key")?;
    let source_ip = trusted_header(&headers, "x-brain-source-ip")?;
    let route = match route_key.as_str() {
        "$connect" => crate::customer::CustomerGatewayRoute::Connect,
        "$disconnect" => crate::customer::CustomerGatewayRoute::Disconnect,
        "$default" => crate::customer::CustomerGatewayRoute::Message,
        _ => {
            return Err(Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "x-brain-route-key must be $connect, $disconnect, or $default".into(),
            ));
        }
    };
    if route == crate::customer::CustomerGatewayRoute::Message
        && body.len() > crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES
    {
        return Err(map_err(BrainError::FileTooLarge {
            limit: crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES,
        }));
    }
    let subprotocol = if route == crate::customer::CustomerGatewayRoute::Connect {
        Some(customer_grant_subprotocol(&headers)?)
    } else {
        None
    };
    let body = if route == crate::customer::CustomerGatewayRoute::Message {
        Some(String::from_utf8(body.to_vec()).map_err(|_| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "customer Hand WebSocket frame must be UTF-8 text".into(),
            )
        })?)
    } else {
        None
    };
    let coordinator = state.brain.customer.as_ref().ok_or_else(|| {
        map_err(BrainError::HandUnavailable(
            "customer Hand is unavailable".into(),
        ))
    })?;
    crate::customer::CustomerHandIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route,
            connection_id,
            request_id,
            route_key,
            source_ip,
            subprotocol: subprotocol.clone(),
            body,
        },
    )
    .await
    .map_err(map_err)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(protocol) = subprotocol {
        response.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_str(&protocol).map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "customer Hand grant protocol is invalid".into(),
                )
            })?,
        );
    }
    Ok(response)
}

fn trusted_header(headers: &HeaderMap, name: &'static str) -> Result<String, Failure> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                format!("trusted gateway header {name} is required"),
            )
        })
}

fn customer_grant_subprotocol(headers: &HeaderMap) -> Result<String, Failure> {
    let invalid = || {
        Failure(
            StatusCode::UNAUTHORIZED,
            api_code("unauthorized"),
            "exactly one customer Hand grant subprotocol is required".into(),
        )
    };
    let mut protocol = None;
    for value in headers.get_all(header::SEC_WEBSOCKET_PROTOCOL) {
        let value = value.to_str().map_err(|_| invalid())?;
        for candidate in value.split(',').map(str::trim) {
            if candidate.len() <= "aex-grant.".len()
                || !candidate.starts_with("aex-grant.")
                || protocol.is_some()
            {
                return Err(invalid());
            }
            protocol = Some(candidate.to_owned());
        }
    }
    protocol.ok_or_else(invalid)
}

pub async fn serve(state: AppState, addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "brain listening");
    axum::serve(nodelay(listener), app).await?;
    Ok(())
}

/// TCP_NODELAY on every accepted connection. Load-bearing for the event stream: SSE frames are
/// small writes, and Nagle plus delayed ACK added a measured ~40 ms stall per turn on Linux.
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

#[derive(Debug)]
struct Failure {
    status: StatusCode,
    code: ApiErrorCode,
    message: String,
    request_id: String,
}

// Keep the compact constructor used by handlers while storing the request id in the failure at
// creation time. That gives the response and any structured server-side diagnostic one identity.
#[allow(non_snake_case)]
fn Failure(status: StatusCode, code: ApiErrorCode, message: String) -> Failure {
    Failure {
        status,
        code,
        message,
        request_id: mint_id("req", 16),
    }
}

fn api_code(value: &str) -> ApiErrorCode {
    value.parse().expect("static API error code")
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            error: ApiError {
                code: self.code,
                details: None,
                message: self.message,
                param: None,
                request_id: Some(self.request_id),
            },
        };
        (self.status, Json(body)).into_response()
    }
}

fn map_err(e: BrainError) -> Failure {
    use StatusCode as S;
    let (status, code, message, log_internal) = match &e {
        BrainError::PrefixSealed { .. } => (
            S::CONFLICT,
            "configuration_sealed",
            "session configuration is immutable",
            false,
        ),
        BrainError::NoSuchSession(_) | BrainError::FileNotFound(_) => {
            (S::NOT_FOUND, "not_found", "resource not found", false)
        }
        BrainError::TurnInFlight(_) => (
            S::CONFLICT,
            "session_busy",
            "the session already has a running turn",
            false,
        ),
        BrainError::IdempotencyConflict => (
            S::CONFLICT,
            "conflict",
            "the idempotency key was used for a different request",
            false,
        ),
        BrainError::SessionDeleted(_) => (
            S::GONE,
            "session_deleted",
            "the session has been deleted",
            false,
        ),
        BrainError::SessionFailed(_) => (
            S::CONFLICT,
            "session_failed",
            "the session is failed",
            false,
        ),
        BrainError::Invalid(_) => (
            S::BAD_REQUEST,
            "invalid_request",
            "the request is invalid",
            false,
        ),
        BrainError::FileTooLarge { .. } => (
            S::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "the file payload exceeds the route limit",
            false,
        ),
        BrainError::StorageObjectTooLarge { .. } => (
            S::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "the storage object exceeds the session limit",
            false,
        ),
        BrainError::SandboxNotMaterialized => (
            S::CONFLICT,
            "sandbox_not_materialized",
            "the default sandbox has not been materialized",
            false,
        ),
        BrainError::SandboxGone => (
            S::GONE,
            "sandbox_gone",
            "the requested sandbox generation is gone",
            false,
        ),
        BrainError::SandboxGenerationConflict => (
            S::CONFLICT,
            "generation_conflict",
            "the sandbox generation does not match",
            false,
        ),
        BrainError::SandboxResourceExhausted => (
            S::TOO_MANY_REQUESTS,
            "resource_exhausted",
            "sandbox capacity is exhausted",
            false,
        ),
        BrainError::StorageQuotaExceeded { .. } | BrainError::TenantStorageQuotaExceeded { .. } => {
            (
                S::PAYLOAD_TOO_LARGE,
                "storage_limit_exceeded",
                "the storage quota would be exceeded",
                false,
            )
        }
        BrainError::StorageUploadInProgress { .. } => (
            S::CONFLICT,
            "storage_upload_in_progress",
            "another storage upload is already in progress",
            false,
        ),
        BrainError::StorageUploadExpired(_) => (
            S::GONE,
            "storage_upload_expired",
            "the storage upload has expired",
            false,
        ),
        BrainError::SandboxTransferUnknown(_) => (
            S::NOT_FOUND,
            "sandbox_transfer_unknown",
            "the sandbox transfer is unknown; prepare a fresh transfer",
            false,
        ),
        BrainError::SandboxTransferExpired(_) => (
            S::GONE,
            "sandbox_transfer_expired",
            "the sandbox transfer expired; inspect the file and prepare a fresh transfer",
            false,
        ),
        BrainError::SandboxTransferAmbiguous => (
            S::CONFLICT,
            "sandbox_transfer_ambiguous",
            "the sandbox transfer outcome is ambiguous; inspect the file before retrying",
            false,
        ),
        BrainError::SessionJournalQuotaExceeded { .. }
        | BrainError::TenantJournalQuotaExceeded { .. }
        | BrainError::TenantRetainedSessionQuotaExceeded { .. } => (
            S::TOO_MANY_REQUESTS,
            "resource_exhausted",
            "the retained-session quota would be exceeded",
            false,
        ),
        BrainError::Overloaded => (
            S::TOO_MANY_REQUESTS,
            "rate_limited",
            "Brain is at capacity; retry with backoff",
            false,
        ),
        BrainError::UndeclaredTool { .. } => (
            S::BAD_REQUEST,
            "invalid_request",
            "the requested Tool is not declared for this session",
            false,
        ),
        BrainError::Tool { .. } => (S::BAD_GATEWAY, "tool_error", "Tool execution failed", false),
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => (
            S::BAD_GATEWAY,
            "provider_error",
            "the upstream provider request failed",
            false,
        ),
        BrainError::HandUnavailable(_) | BrainError::Hand(_) => (
            S::SERVICE_UNAVAILABLE,
            "hand_unavailable",
            "the execution runtime is unavailable",
            false,
        ),
        BrainError::Agentloop(_) => (
            S::INTERNAL_SERVER_ERROR,
            "agentloop_error",
            "the session's agentloop failed",
            true,
        ),
        BrainError::Journal(_) | BrainError::Fenced | BrainError::Custody(_) => (
            S::INTERNAL_SERVER_ERROR,
            "internal",
            "the request could not be completed",
            true,
        ),
        BrainError::RoundCap { .. } => (
            S::CONFLICT,
            "round_limit_exceeded",
            "the turn exceeded its Tool-round limit",
            false,
        ),
        BrainError::Cancelled => (
            S::CONFLICT,
            "cancelled",
            "the operation was cancelled",
            false,
        ),
        BrainError::Serde(_) => (
            S::BAD_REQUEST,
            "invalid_request",
            "the request body is invalid",
            false,
        ),
    };
    let request_id = mint_id("req", 16);
    if log_internal {
        let error_kind = match &e {
            BrainError::Journal(_) => "journal",
            BrainError::Fenced => "lease_fenced",
            BrainError::Custody(_) => "custody",
            _ => "internal",
        };
        tracing::error!(%request_id, error_kind, "internal Brain request failure");
    }
    Failure {
        status,
        code: api_code(code),
        message: message.into(),
        request_id,
    }
}

fn auth(state: &AppState, headers: &HeaderMap) -> Result<TrustedPrincipal, Failure> {
    if bearer_token(headers) == Some(state.token.as_str()) {
        let tenant_id = headers
            .get("x-brain-tenant-id")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("local");
        TrustedPrincipal::new(tenant_id).map_err(map_err)
    } else {
        Err(Failure(
            StatusCode::UNAUTHORIZED,
            api_code("unauthorized"),
            "missing or invalid bearer token".into(),
        ))
    }
}

#[cfg(test)]
mod api_error_tests {
    use super::map_err;
    use crate::BrainError;
    use axum::http::StatusCode;

    fn serde_error() -> serde_json::Error {
        serde_json::from_str::<serde_json::Value>("{").unwrap_err()
    }

    #[test]
    fn every_brain_error_has_an_explicit_safe_public_mapping() {
        const SECRET: &str = "dictionary-sentinel-secret";
        let cases = vec![
            (
                BrainError::PrefixSealed {
                    digest: SECRET.into(),
                    what: "model",
                },
                StatusCode::CONFLICT,
                "configuration_sealed",
            ),
            (
                BrainError::NoSuchSession(SECRET.into()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                BrainError::TurnInFlight(SECRET.into()),
                StatusCode::CONFLICT,
                "session_busy",
            ),
            (
                BrainError::IdempotencyConflict,
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                BrainError::SessionDeleted(SECRET.into()),
                StatusCode::GONE,
                "session_deleted",
            ),
            (
                BrainError::SessionFailed(SECRET.into()),
                StatusCode::CONFLICT,
                "session_failed",
            ),
            (
                BrainError::Invalid(SECRET.into()),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                BrainError::FileNotFound(SECRET.into()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                BrainError::FileTooLarge { limit: 1 },
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
            ),
            (
                BrainError::StorageObjectTooLarge { limit: 1 },
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
            ),
            (
                BrainError::SandboxNotMaterialized,
                StatusCode::CONFLICT,
                "sandbox_not_materialized",
            ),
            (BrainError::SandboxGone, StatusCode::GONE, "sandbox_gone"),
            (
                BrainError::SandboxGenerationConflict,
                StatusCode::CONFLICT,
                "generation_conflict",
            ),
            (
                BrainError::SandboxResourceExhausted,
                StatusCode::TOO_MANY_REQUESTS,
                "resource_exhausted",
            ),
            (
                BrainError::StorageQuotaExceeded {
                    published: 1,
                    reserved: 2,
                    requested: 3,
                    limit: 4,
                },
                StatusCode::PAYLOAD_TOO_LARGE,
                "storage_limit_exceeded",
            ),
            (
                BrainError::TenantStorageQuotaExceeded {
                    requested: 1,
                    limit: 2,
                },
                StatusCode::PAYLOAD_TOO_LARGE,
                "storage_limit_exceeded",
            ),
            (
                BrainError::SessionJournalQuotaExceeded {
                    requested: 1,
                    limit: 2,
                },
                StatusCode::TOO_MANY_REQUESTS,
                "resource_exhausted",
            ),
            (
                BrainError::TenantJournalQuotaExceeded {
                    requested: 1,
                    limit: 2,
                },
                StatusCode::TOO_MANY_REQUESTS,
                "resource_exhausted",
            ),
            (
                BrainError::TenantRetainedSessionQuotaExceeded { limit: 2 },
                StatusCode::TOO_MANY_REQUESTS,
                "resource_exhausted",
            ),
            (
                BrainError::StorageUploadInProgress {
                    transfer_id: SECRET.into(),
                },
                StatusCode::CONFLICT,
                "storage_upload_in_progress",
            ),
            (
                BrainError::StorageUploadExpired(SECRET.into()),
                StatusCode::GONE,
                "storage_upload_expired",
            ),
            (
                BrainError::SandboxTransferUnknown(SECRET.into()),
                StatusCode::NOT_FOUND,
                "sandbox_transfer_unknown",
            ),
            (
                BrainError::SandboxTransferExpired(SECRET.into()),
                StatusCode::GONE,
                "sandbox_transfer_expired",
            ),
            (
                BrainError::SandboxTransferAmbiguous,
                StatusCode::CONFLICT,
                "sandbox_transfer_ambiguous",
            ),
            (
                BrainError::Overloaded,
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
            ),
            (
                BrainError::UndeclaredTool {
                    name: SECRET.into(),
                },
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                BrainError::Tool {
                    name: SECRET.into(),
                    source: Box::new(std::io::Error::other(SECRET)),
                },
                StatusCode::BAD_GATEWAY,
                "tool_error",
            ),
            (
                BrainError::Transport(SECRET.into()),
                StatusCode::BAD_GATEWAY,
                "provider_error",
            ),
            (
                BrainError::Protocol(SECRET.into()),
                StatusCode::BAD_GATEWAY,
                "provider_error",
            ),
            (
                BrainError::ProviderStatus {
                    status: 500,
                    body: SECRET.into(),
                    retry_after_ms: None,
                },
                StatusCode::BAD_GATEWAY,
                "provider_error",
            ),
            (
                BrainError::HandUnavailable(SECRET.into()),
                StatusCode::SERVICE_UNAVAILABLE,
                "hand_unavailable",
            ),
            (
                BrainError::Hand(SECRET.into()),
                StatusCode::SERVICE_UNAVAILABLE,
                "hand_unavailable",
            ),
            (
                BrainError::Journal(SECRET.into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
            (
                BrainError::Fenced,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
            (
                BrainError::Custody(SECRET.into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
            (
                BrainError::RoundCap { cap: 1 },
                StatusCode::CONFLICT,
                "round_limit_exceeded",
            ),
            (BrainError::Cancelled, StatusCode::CONFLICT, "cancelled"),
            (
                BrainError::Serde(serde_error()),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
        ];

        for (error, expected_status, expected_code) in cases {
            let failure = map_err(error);
            assert_eq!(failure.status, expected_status);
            assert_eq!(failure.code.as_str(), expected_code);
            assert!(!failure.message.contains(SECRET));
            assert!(failure.request_id.starts_with("req_"));
        }
    }
}

#[cfg(test)]
mod customer_observation_auth_tests {
    use super::{bearer_token, customer_grant_subprotocol, observation_grant_header};
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn internal_observation_keeps_operator_and_scoped_grant_separate() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer operator-secret"),
        );
        headers.insert(
            "x-brain-observation-grant",
            HeaderValue::from_static("scoped-observation-secret"),
        );

        assert_eq!(bearer_token(&headers), Some("operator-secret"));
        assert_eq!(
            observation_grant_header(&headers),
            Some("scoped-observation-secret")
        );
    }

    #[test]
    fn missing_or_swapped_internal_observation_authorities_do_not_match() {
        let missing = HeaderMap::new();
        assert_eq!(bearer_token(&missing), None);
        assert_eq!(observation_grant_header(&missing), None);

        let mut swapped = HeaderMap::new();
        swapped.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer scoped-observation-secret"),
        );
        swapped.insert(
            "x-brain-observation-grant",
            HeaderValue::from_static("operator-secret"),
        );
        assert_ne!(bearer_token(&swapped), Some("operator-secret"));
        assert_ne!(
            observation_grant_header(&swapped),
            Some("scoped-observation-secret")
        );
    }

    #[test]
    fn customer_socket_accepts_exactly_one_grant_subprotocol() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("aex-grant.valid-token"),
        );
        assert_eq!(
            customer_grant_subprotocol(&headers).unwrap(),
            "aex-grant.valid-token"
        );
    }

    #[test]
    fn customer_socket_rejects_missing_extra_or_duplicate_subprotocols() {
        assert!(customer_grant_subprotocol(&HeaderMap::new()).is_err());

        let mut extra = HeaderMap::new();
        extra.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("aex-grant.valid-token, chat"),
        );
        assert!(customer_grant_subprotocol(&extra).is_err());

        let mut duplicate = HeaderMap::new();
        duplicate.append(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("aex-grant.first"),
        );
        duplicate.append(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("aex-grant.second"),
        );
        assert!(customer_grant_subprotocol(&duplicate).is_err());

        let mut empty_grant = HeaderMap::new();
        empty_grant.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("aex-grant."),
        );
        assert!(customer_grant_subprotocol(&empty_grant).is_err());
    }
}

#[cfg(test)]
mod public_route_contract_tests {
    use super::END_ACCEPTED_STATUS;
    use axum::http::StatusCode;
    use std::collections::BTreeSet;

    fn normalized(path: &str) -> String {
        path.replace("{id}", "{session_id}")
    }

    #[test]
    fn every_public_router_path_is_documented_and_every_documented_path_is_live() {
        let source = include_str!("api.rs");
        let route = regex::Regex::new(r#"(?s)\.route\(\s*\"([^\"]+)\""#).unwrap();
        let router_paths = route
            .captures_iter(source)
            .map(|capture| capture[1].to_owned())
            .filter(|path| path.starts_with("/v1/") && !path.starts_with("/internal/"))
            .map(|path| normalized(&path))
            .collect::<BTreeSet<_>>();

        let contract = include_str!("../../../contracts/session/v1/openapi.yaml");
        let mut in_paths = false;
        let mut openapi_paths = BTreeSet::new();
        for line in contract.lines() {
            if line == "paths:" {
                in_paths = true;
                continue;
            }
            if line == "components:" {
                break;
            }
            if in_paths
                && let Some(path) = line
                    .strip_prefix("  /")
                    .and_then(|line| line.strip_suffix(':'))
            {
                openapi_paths.insert(format!("/{path}"));
            }
        }

        assert_eq!(router_paths.len(), 37, "public route inventory changed");
        assert_eq!(router_paths, openapi_paths);
    }

    #[test]
    fn root_and_child_end_share_the_async_acceptance_status() {
        assert_eq!(END_ACCEPTED_STATUS, StatusCode::ACCEPTED);
        let contract = include_str!("../../../contracts/session/v1/openapi.yaml");
        for path in [
            "/v1/sessions/{session_id}/end",
            "/v1/sessions/{session_id}/children/{child_id}/end",
        ] {
            let marker = format!("  {path}:");
            let tail = contract
                .split_once(&marker)
                .unwrap_or_else(|| panic!("missing OpenAPI path {path}"))
                .1;
            let operation = tail.split("\n  /").next().expect("path operation section");
            assert!(operation.contains("\n    post:"));
            assert!(operation.contains("\n        \"202\":"));
            assert!(!operation.contains("\n        \"200\":"));
        }
    }
}

async fn authorize_session(
    state: &AppState,
    headers: &HeaderMap,
    session_id: &str,
) -> Result<TrustedPrincipal, Failure> {
    let principal = auth(state, headers)?;
    state
        .brain
        .authorize(&principal, session_id)
        .await
        .map_err(map_err)?;
    Ok(principal)
}

// ---------------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------------

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<session::Session>), Failure> {
    let principal = auth(&state, &headers)?;
    let encoded_bytes = serde_json::to_vec(&req).map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("create request is not serializable: {error}"),
        )
    })?;
    if encoded_bytes.len() > brain_protocol::MAX_CREATE_SESSION_REQUEST_BYTES {
        return Err(Failure(
            StatusCode::PAYLOAD_TOO_LARGE,
            api_code("payload_too_large"),
            format!(
                "create request exceeds the {}-byte limit",
                brain_protocol::MAX_CREATE_SESSION_REQUEST_BYTES
            ),
        ));
    }
    let idempotency_key = headers
        .get("idempotency-key")
        .map(|value| {
            value.to_str().map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "Idempotency-Key must be valid ASCII".into(),
                )
            })
        })
        .transpose()?;
    let doc = state
        .brain
        .create_session_for_admitted(&principal, req, idempotency_key)
        .await
        .map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(doc)))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    cursor: Option<String>,
    state: Option<String>,
}
fn default_limit() -> usize {
    20
}

async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<SessionList>, Failure> {
    let principal = auth(&state, &headers)?;
    let (data, next_cursor) = state
        .brain
        .list_for(
            &principal,
            q.state.as_deref(),
            q.limit.clamp(1, 100),
            q.cursor.as_deref(),
        )
        .await
        .map_err(map_err)?;
    Ok(Json(SessionList {
        data,
        has_more: next_cursor.is_some(),
        next_cursor,
        object: session::SessionListObject::List,
    }))
}

async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<session::Session>, Failure> {
    let principal = auth(&state, &headers)?;
    Ok(Json(
        state
            .brain
            .get_for(&principal, &id)
            .await
            .map_err(map_err)?,
    ))
}

async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let principal = auth(&state, &headers)?;
    let existing = match state.brain.deletion_status(&id).await {
        Ok(status) => Some(status),
        Err(BrainError::NoSuchSession(_)) => None,
        Err(error) => return Err(map_err(error)),
    };
    if let Some(status) = &existing {
        if status.tenant_id != principal.as_str() {
            return Err(map_err(BrainError::NoSuchSession(id)));
        }
        if status.state == "succeeded" {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
    } else {
        state
            .brain
            .authorize(&principal, &id)
            .await
            .map_err(map_err)?;
    }
    state.brain.queue_delete(&id).await.map_err(map_err)?;
    let mut response = StatusCode::ACCEPTED.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&format!("/v1/sessions/{id}/deletion")).map_err(|_| {
            Failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                api_code("internal"),
                "deletion status location".into(),
            )
        })?,
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    Ok(response)
}

#[derive(Serialize)]
struct DeletionStatusResponse {
    object: &'static str,
    session_id: String,
    state: String,
    requested_at_ms: u64,
    updated_at_ms: u64,
    completed_at_ms: Option<u64>,
}

async fn get_deletion_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, Failure> {
    let principal = auth(&state, &headers)?;
    let status = state.brain.deletion_status(&id).await.map_err(map_err)?;
    if status.tenant_id != principal.as_str() {
        return Err(map_err(BrainError::NoSuchSession(id)));
    }
    let mut response = Json(DeletionStatusResponse {
        object: "session.deletion",
        session_id: status.session_id,
        state: status.state,
        requested_at_ms: status.requested_at_ms,
        updated_at_ms: status.updated_at_ms,
        completed_at_ms: status.completed_at_ms,
    })
    .into_response();
    if response.status() == StatusCode::OK {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    Ok(response)
}

async fn send_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<MessageRequest>,
) -> Result<(StatusCode, Json<MessageAccepted>), Failure> {
    let encoded_bytes = serde_json::to_vec(&req).map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("message request: {error}"),
        )
    })?;
    if encoded_bytes.len() > crate::journal::MAX_MESSAGE_REQUEST_BYTES {
        return Err(Failure(
            StatusCode::PAYLOAD_TOO_LARGE,
            api_code("payload_too_large"),
            format!(
                "message request is {} bytes; maximum is {}",
                encoded_bytes.len(),
                crate::journal::MAX_MESSAGE_REQUEST_BYTES
            ),
        ));
    }
    let principal = auth(&state, &headers)?;
    state
        .brain
        .authorize(&principal, &id)
        .await
        .map_err(map_err)?;
    let idempotency_key = headers
        .get("idempotency-key")
        .map(|value| {
            value.to_str().map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "Idempotency-Key must be valid ASCII".into(),
                )
            })
        })
        .transpose()?;
    let (turn_id, seq) = state
        .brain
        .message_with_metadata_idempotent(&id, req.content, req.metadata, idempotency_key)
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
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(state.brain.cancel(&id).await.map_err(map_err)?))
}

async fn end_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<session::Session>), Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(end_accepted(state.brain.end(&id).await.map_err(map_err)?))
}

const END_ACCEPTED_STATUS: StatusCode = StatusCode::ACCEPTED;

fn end_accepted(session: session::Session) -> (StatusCode, Json<session::Session>) {
    (END_ACCEPTED_STATUS, Json(session))
}

async fn get_default_sandbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<brain_protocol::hand::SandboxStatus>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .default_sandbox_status(&id)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn create_default_sandbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<brain_protocol::hand::SandboxStatus>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .materialize_default_sandbox(&id)
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxFilePathRequest {
    path: String,
    generation: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxFileListRequest {
    path: String,
    generation: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_storage_limit")]
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxFileReadRequest {
    path: String,
    generation: String,
    #[serde(default = "default_inline_storage_limit")]
    max_bytes: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SandboxFileWriteRequest {
    path: String,
    generation: String,
    content_base64: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxFileUploadRequest {
    path: String,
    generation: String,
    bytes: u64,
    sha256: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxFileSearchRequest {
    path: String,
    generation: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_storage_limit")]
    limit: u32,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Serialize)]
struct SandboxFileListResponse {
    data: Vec<brain_protocol::hand::FileEntry>,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    generation: String,
}

fn sandbox_file_page(
    page: crate::hand::SandboxFileList,
    generation: String,
) -> SandboxFileListResponse {
    SandboxFileListResponse {
        data: page.entries,
        has_more: page.next_cursor.is_some(),
        next_cursor: page.next_cursor,
        generation,
    }
}

async fn sandbox_file_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileListRequest>,
) -> Result<Json<SandboxFileListResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let page = state
        .brain
        .sandbox_file_list(
            &id,
            &request.generation,
            &request.path,
            request.cursor.as_deref(),
            request.limit,
        )
        .await
        .map_err(map_err)?;
    Ok(Json(sandbox_file_page(page, request.generation)))
}

async fn sandbox_file_stat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFilePathRequest>,
) -> Result<Json<brain_protocol::hand::FileEntry>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .sandbox_file_stat(&id, &request.generation, &request.path)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn sandbox_file_read_inline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileReadRequest>,
) -> Result<Json<crate::hand::SandboxFileContent>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .sandbox_file_read_inline(&id, &request.generation, &request.path, request.max_bytes)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn sandbox_file_write_inline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileWriteRequest>,
) -> Result<Json<brain_protocol::hand::FileEntry>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let idempotency_key = headers
        .get("idempotency-key")
        .ok_or_else(|| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "Idempotency-Key is required for sandbox file writes".into(),
            )
        })?
        .to_str()
        .map_err(|_| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "Idempotency-Key must be valid ASCII".into(),
            )
        })?;
    state
        .brain
        .sandbox_file_write_inline(
            &id,
            request.generation,
            request.path,
            request.content_base64,
            request.overwrite,
            idempotency_key,
        )
        .await
        .map(Json)
        .map_err(map_err)
}

async fn sandbox_file_prepare_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFilePathRequest>,
) -> Result<Json<StorageTransferResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .sandbox_file_prepare_download(&id, request.generation, request.path)
        .await
        .map(storage_ticket)
        .map(Json)
        .map_err(map_err)
}

async fn sandbox_file_prepare_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileUploadRequest>,
) -> Result<Json<StorageTransferResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .sandbox_file_prepare_upload(
            &id,
            request.generation,
            request.path,
            request.bytes,
            request.sha256,
            request.overwrite,
        )
        .await
        .map(storage_ticket)
        .map(Json)
        .map_err(map_err)
}

async fn sandbox_file_complete_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, transfer_id)): Path<(String, String)>,
) -> Result<Json<brain_protocol::hand::FileEntry>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .sandbox_file_complete_upload(&id, &transfer_id)
        .await
        .map(Json)
        .map_err(map_err)
}

async fn sandbox_file_search(
    state: AppState,
    headers: HeaderMap,
    id: String,
    request: SandboxFileSearchRequest,
    grep: bool,
) -> Result<Json<SandboxFileListResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let expression = if grep {
        request.query.as_deref().ok_or_else(|| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "sandbox grep query is required".into(),
            )
        })?
    } else {
        request.glob.as_deref().ok_or_else(|| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "sandbox find glob is required".into(),
            )
        })?
    };
    if (grep && request.glob.is_some()) || (!grep && request.query.is_some()) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            "sandbox search request contains a field for the wrong operation".into(),
        ));
    }
    let page = state
        .brain
        .sandbox_file_search(
            &id,
            &request.generation,
            &request.path,
            expression,
            request.cursor.as_deref(),
            request.limit,
            grep,
        )
        .await
        .map_err(map_err)?;
    Ok(Json(sandbox_file_page(page, request.generation)))
}

async fn sandbox_file_find(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileSearchRequest>,
) -> Result<Json<SandboxFileListResponse>, Failure> {
    sandbox_file_search(state, headers, id, request, false).await
}

async fn sandbox_file_grep(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileSearchRequest>,
) -> Result<Json<SandboxFileListResponse>, Failure> {
    sandbox_file_search(state, headers, id, request, true).await
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateChildRequest {
    prompt: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    fork_turns: Option<String>,
}

async fn create_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreateChildRequest>,
) -> Result<(StatusCode, Json<session::Session>), Failure> {
    authorize_session(&state, &headers, &id).await?;
    ensure_child_request_bound(&request)?;
    if request.name.as_ref().is_some_and(|name| name.len() > 128) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            "child name exceeds 128 bytes".into(),
        ));
    }
    let idempotency_key = headers
        .get("idempotency-key")
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "Idempotency-Key must be valid ASCII".into(),
            )
        })?;
    let child = state
        .brain
        .create_child(
            &id,
            request.prompt,
            request.name,
            request.fork_turns,
            idempotency_key,
        )
        .await
        .map_err(map_err)?;
    Ok((StatusCode::CREATED, Json(child)))
}

#[derive(Default, Deserialize)]
struct ChildListQuery {
    cursor: Option<String>,
    #[serde(default = "default_child_limit")]
    limit: usize,
}

fn default_child_limit() -> usize {
    20
}

async fn list_children(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ChildListQuery>,
) -> Result<Json<SessionList>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let (data, next_cursor) = state
        .brain
        .list_children(&id, query.cursor.as_deref(), query.limit)
        .await
        .map_err(map_err)?;
    Ok(Json(SessionList {
        data,
        has_more: next_cursor.is_some(),
        next_cursor,
        object: session::SessionListObject::List,
    }))
}

async fn get_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, child_id)): Path<(String, String)>,
) -> Result<Json<session::Session>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(
        state
            .brain
            .get_child(&id, &child_id)
            .await
            .map_err(map_err)?,
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChildMessageRequest {
    message: String,
}

fn ensure_child_request_bound(request: &impl Serialize) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(request).map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("child request: {error}"),
        )
    })?;
    if bytes.len() > brain_protocol::MAX_MESSAGE_REQUEST_BYTES {
        return Err(Failure(
            StatusCode::PAYLOAD_TOO_LARGE,
            api_code("payload_too_large"),
            format!(
                "child request is {} bytes; maximum is {}",
                bytes.len(),
                brain_protocol::MAX_MESSAGE_REQUEST_BYTES
            ),
        ));
    }
    Ok(())
}

fn child_idempotency_key(headers: &HeaderMap) -> Result<Option<&str>, Failure> {
    headers
        .get("idempotency-key")
        .map(|value| {
            value.to_str().map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "Idempotency-Key must be valid ASCII".into(),
                )
            })
        })
        .transpose()
}

async fn admit_child_message(
    state: &AppState,
    parent_id: &str,
    child_id: &str,
    message: String,
    idempotency_key: Option<&str>,
) -> Result<MessageAccepted, Failure> {
    state
        .brain
        .get_child(parent_id, child_id)
        .await
        .map_err(map_err)?;
    let content = MessageRequestContent::String(message.parse().map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("child message: {error}"),
        )
    })?);
    let (turn_id, seq) = state
        .brain
        .message_with_metadata_idempotent(
            child_id,
            content,
            std::collections::HashMap::new(),
            idempotency_key,
        )
        .await
        .map_err(map_err)?;
    Ok(MessageAccepted {
        seq: NonZeroU64::new(seq.max(1)).expect("nonzero"),
        session_id: child_id.parse().map_err(|_| {
            Failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                api_code("internal"),
                "child session id".into(),
            )
        })?,
        turn_id: turn_id.parse().map_err(|_| {
            Failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                api_code("internal"),
                "child turn id".into(),
            )
        })?,
    })
}

async fn send_child_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, child_id)): Path<(String, String)>,
    Json(request): Json<ChildMessageRequest>,
) -> Result<(StatusCode, Json<MessageAccepted>), Failure> {
    authorize_session(&state, &headers, &id).await?;
    ensure_child_request_bound(&request)?;
    let accepted = admit_child_message(
        &state,
        &id,
        &child_id,
        request.message,
        child_idempotency_key(&headers)?,
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(accepted)))
}

async fn follow_up_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, child_id)): Path<(String, String)>,
    Json(request): Json<ChildMessageRequest>,
) -> Result<Json<session::Session>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    ensure_child_request_bound(&request)?;
    admit_child_message(
        &state,
        &id,
        &child_id,
        request.message,
        child_idempotency_key(&headers)?,
    )
    .await?;
    Ok(Json(
        state
            .brain
            .get_child(&id, &child_id)
            .await
            .map_err(map_err)?,
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitChildRequest {
    #[serde(default)]
    timeout_ms: Option<u64>,
}

async fn wait_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, child_id)): Path<(String, String)>,
    Json(request): Json<WaitChildRequest>,
) -> Result<Json<session::Session>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(
        state
            .brain
            .wait_child(
                &id,
                &child_id,
                std::time::Duration::from_millis(request.timeout_ms.unwrap_or(30_000).min(300_000)),
            )
            .await
            .map_err(map_err)?,
    ))
}

async fn interrupt_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, child_id)): Path<(String, String)>,
) -> Result<Json<session::Session>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .get_child(&id, &child_id)
        .await
        .map_err(map_err)?;
    Ok(Json(state.brain.cancel(&child_id).await.map_err(map_err)?))
}

async fn end_child(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, child_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<session::Session>), Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .get_child(&id, &child_id)
        .await
        .map_err(map_err)?;
    Ok(end_accepted(
        state.brain.end(&child_id).await.map_err(map_err)?,
    ))
}

#[derive(Deserialize)]
struct StorageListRequest {
    prefix: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_storage_limit")]
    limit: u32,
}

fn default_storage_limit() -> u32 {
    100
}

#[derive(Deserialize)]
struct StorageKeyRequest {
    key: String,
}

#[derive(Deserialize)]
struct StorageReadRequest {
    key: String,
    #[serde(default = "default_inline_storage_limit")]
    max_bytes: u64,
}

fn default_inline_storage_limit() -> u64 {
    1024 * 1024
}

#[derive(Deserialize)]
struct StorageWriteInlineRequest {
    key: String,
    content_base64: String,
    content_type: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
struct StorageUploadIntentRequest {
    key: String,
    bytes: u64,
    sha256: String,
    content_type: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageSandboxCopyRequest {
    key: String,
    path: String,
    sandbox_generation: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Serialize)]
struct StorageObjectResponse {
    key: String,
    bytes: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    created_at: brain_protocol::session::Timestamp,
    updated_at: brain_protocol::session::Timestamp,
}

#[derive(Serialize)]
struct StorageListResponse {
    data: Vec<StorageObjectResponse>,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct StorageReadInlineResponse {
    object: StorageObjectResponse,
    content_base64: String,
}

#[derive(Serialize)]
struct StorageTransferResponse {
    transfer_id: String,
    method: String,
    url: String,
    headers: std::collections::HashMap<String, String>,
    expires_at: brain_protocol::session::Timestamp,
    max_bytes: u64,
}

fn storage_object(object: crate::storage::StorageObject) -> StorageObjectResponse {
    StorageObjectResponse {
        key: object.key,
        bytes: object.bytes,
        sha256: object.sha256,
        content_type: object.content_type,
        created_at: crate::events::ts(object.created_at_ms),
        updated_at: crate::events::ts(object.updated_at_ms),
    }
}

fn storage_ticket(ticket: crate::storage::StorageTransferTicket) -> StorageTransferResponse {
    StorageTransferResponse {
        transfer_id: ticket.transfer_id,
        method: ticket.method,
        url: ticket.url,
        headers: ticket.headers,
        expires_at: crate::events::ts(ticket.expires_at_ms),
        max_bytes: ticket.max_bytes,
    }
}

async fn storage_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageListRequest>,
) -> Result<Json<StorageListResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let page = state
        .brain
        .storage_list(
            &id,
            request.prefix.as_deref(),
            request.cursor.as_deref(),
            request.limit,
        )
        .await
        .map_err(map_err)?;
    Ok(Json(StorageListResponse {
        data: page.objects.into_iter().map(storage_object).collect(),
        has_more: page.next_cursor.is_some(),
        next_cursor: page.next_cursor,
    }))
}

async fn storage_stat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageKeyRequest>,
) -> Result<Json<StorageObjectResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(storage_object(
        state
            .brain
            .storage_stat(&id, &request.key)
            .await
            .map_err(map_err)?,
    )))
}

async fn storage_read_inline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageReadRequest>,
) -> Result<Json<StorageReadInlineResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let (object, bytes) = state
        .brain
        .storage_read_inline(&id, &request.key, request.max_bytes)
        .await
        .map_err(map_err)?;
    Ok(Json(StorageReadInlineResponse {
        object: storage_object(object),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }))
}

async fn storage_write_inline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageWriteInlineRequest>,
) -> Result<Json<StorageObjectResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(storage_object(
        state
            .brain
            .storage_write_inline(
                &id,
                request.key,
                request.content_base64,
                request.content_type,
                request.overwrite,
            )
            .await
            .map_err(map_err)?,
    )))
}

async fn storage_prepare_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageKeyRequest>,
) -> Result<Json<StorageTransferResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(storage_ticket(
        state
            .brain
            .storage_prepare_download(&id, &request.key)
            .await
            .map_err(map_err)?,
    )))
}

async fn storage_prepare_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageUploadIntentRequest>,
) -> Result<Json<StorageTransferResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(storage_ticket(
        state
            .brain
            .storage_prepare_upload(
                &id,
                crate::storage::StorageUploadIntent {
                    key: request.key,
                    bytes: request.bytes,
                    sha256: Some(request.sha256),
                    content_type: request.content_type,
                    overwrite: request.overwrite,
                },
            )
            .await
            .map_err(map_err)?,
    )))
}

async fn storage_complete_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, transfer_id)): Path<(String, String)>,
) -> Result<Json<StorageObjectResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(storage_object(
        state
            .brain
            .storage_complete_upload(&id, &transfer_id)
            .await
            .map_err(map_err)?,
    )))
}

async fn storage_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageKeyRequest>,
) -> Result<StatusCode, Failure> {
    authorize_session(&state, &headers, &id).await?;
    state
        .brain
        .storage_delete(&id, request.key)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

fn required_effect_idempotency_key<'a>(
    headers: &'a HeaderMap,
    operation: &str,
) -> Result<&'a str, Failure> {
    headers
        .get("idempotency-key")
        .ok_or_else(|| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                format!("Idempotency-Key is required for {operation}"),
            )
        })?
        .to_str()
        .map_err(|_| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "Idempotency-Key must be valid ASCII".into(),
            )
        })
}

async fn storage_copy_from_sandbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageSandboxCopyRequest>,
) -> Result<Json<StorageObjectResponse>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let idempotency_key = required_effect_idempotency_key(&headers, "sandbox storage copies")?;
    state
        .brain
        .storage_copy_from_sandbox(
            &id,
            request.key,
            request.path,
            request.sandbox_generation,
            request.overwrite,
            idempotency_key,
        )
        .await
        .map(storage_object)
        .map(Json)
        .map_err(map_err)
}

async fn storage_copy_to_sandbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StorageSandboxCopyRequest>,
) -> Result<Json<brain_protocol::hand::FileEntry>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    let idempotency_key = required_effect_idempotency_key(&headers, "sandbox storage copies")?;
    state
        .brain
        .storage_copy_to_sandbox(
            &id,
            request.key,
            request.path,
            request.sandbox_generation,
            request.overwrite,
            idempotency_key,
        )
        .await
        .map(Json)
        .map_err(map_err)
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
    /// Optional exact strong replay boundary. Finite billing/audit consumers capture this from
    /// GET Session and require the matching replay.complete proof before installing it.
    through: Option<u64>,
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
    authorize_session(&state, &headers, &id).await?;
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
    if q.follow && q.through.is_some() {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            "through is valid only when follow=false".into(),
        ));
    }
    if q.through.is_some_and(|through| through > head.last_seq) {
        return Err(Failure(
            StatusCode::CONFLICT,
            api_code("conflict"),
            "requested event replay boundary is ahead of the authoritative session high-water"
                .into(),
        ));
    }
    if q.through.is_some_and(|through| after > through) {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            "event replay cursor is ahead of the requested boundary".into(),
        ));
    }

    let brain = state.brain.clone();
    let follow = q.follow;
    let requested_through = q.through;
    // Admission happens before response headers are committed. A finite replay needs no live
    // ring; a followed stream holds its process-wide permit until the response body is dropped.
    let subscription = if follow {
        Some(
            brain
                .hub
                .subscribe(&id)
                .map_err(|_| map_err(BrainError::Overloaded))?,
        )
    } else {
        None
    };
    let stream = async_stream::stream! {
        // Subscribe BEFORE capturing the fixed replay high-water so no event falls between the
        // strong HEAD read and the live tail.
        let snapshot = match brain.journal.get_head(&id).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(session = %id, error = %error, "event replay snapshot failed");
                return;
            }
        };
        let through_seq = requested_through.unwrap_or(snapshot.last_seq);
        let mut last = after;

        while last < through_seq {
            let page = match brain.journal.read_record_page(&crate::journal::RecordPageQuery {
                session_id: &id,
                after: last,
                through_seq,
                limit: crate::journal::DEFAULT_RECORD_PAGE_ITEMS,
                max_bytes: crate::journal::DEFAULT_RECORD_PAGE_BYTES,
            }).await {
                Ok(page) => page,
                Err(error) => {
                    tracing::warn!(session = %id, error = %error, "replay page failed");
                    // Never tail live after an incomplete durable replay. Ending without
                    // advancing Last-Event-ID makes EventSource retry from the last confirmed
                    // record.
                    return;
                }
            };
            for entry in &page.entries {
                if let Some(event) =
                    crate::events::derive(&id, entry.seq, entry.ts_ms, &entry.record)
                {
                    let Some(frame) = frame(&event, true) else {
                        return;
                    };
                    yield Ok(frame);
                }
                // Internal-only records still advance the page cursor; only emitted durable
                // events update a client's Last-Event-ID.
                last = last.max(entry.seq);
            }
            let Some(next) = page.next_after else {
                break;
            };
            last = next;
        }
        // Sequence gaps through the snapshot are live-only provisional events. They are not
        // replayable, but queued live frames at or below the snapshot must be deduplicated.
        last = last.max(through_seq);

        let completion = session::Event::ReplayComplete {
            session_id: match id.parse() {
                Ok(session_id) => session_id,
                Err(_) => return,
            },
            through_seq,
        };
        let Some(completion) = frame(&completion, false) else {
            return;
        };
        yield Ok(completion);

        if !follow {
            return;
        }
        let Some(mut rx) = subscription else {
            return;
        };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let seq = event_seq(&ev);
                    if seq <= last {
                        continue; // replayed already
                    }
                    let durable = !event_is_ephemeral(&ev);
                    if durable {
                        last = seq;
                    }
                    let stop = matches!(&*ev, session::Event::SessionUpdated { state, .. }
                        if *state == session::SessionState::Deleted);
                    let Some(frame) = frame(&ev, durable) else {
                        return;
                    };
                    yield Ok(frame);
                    if stop {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(session = %id, lagged = n, "sse consumer lagged; events skipped");
                    // The skipped range may contain durable records. End the stream without
                    // advancing its cursor; EventSource reconnects from the last delivered
                    // durable id and journal replay fills the exact gap.
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn frame(ev: &session::Event, durable: bool) -> Option<SseFrame> {
    let data = serde_json::to_string(ev).ok()?;
    if data.len() > brain_protocol::MAX_PUBLIC_EVENT_BYTES {
        tracing::error!(
            event = event_type(ev),
            bytes = data.len(),
            limit = brain_protocol::MAX_PUBLIC_EVENT_BYTES,
            "public event exceeded its canonical byte ceiling"
        );
        return None;
    }
    // serde_json escapes embedded newlines, so every public event is exactly one `data:` line.
    let frame = SseFrame::default().event(event_type(ev)).data(data);
    if durable {
        Some(frame.id(event_seq(ev).to_string()))
    } else {
        Some(frame)
    }
}

// Re-exported for the M0 gate binary: build one deterministic replay of a session's durable
// events (no follow), used to assert byte-stable replay.
pub async fn replay(
    brain: &Arc<Brain>,
    session_id: &str,
    after: u64,
) -> crate::Result<Vec<session::Event>> {
    let head = brain.journal.get_head(session_id).await?;
    let mut out = Vec::new();
    let mut cursor = after;
    while cursor < head.last_seq {
        let page = brain
            .journal
            .read_record_page(&crate::journal::RecordPageQuery {
                session_id,
                after: cursor,
                through_seq: head.last_seq,
                limit: crate::journal::DEFAULT_RECORD_PAGE_ITEMS,
                max_bytes: crate::journal::DEFAULT_RECORD_PAGE_BYTES,
            })
            .await?;
        for entry in &page.entries {
            if let Some(event) =
                crate::events::derive(session_id, entry.seq, entry.ts_ms, &entry.record)
            {
                out.push(event);
            }
            cursor = cursor.max(entry.seq);
        }
        let Some(next) = page.next_after else {
            break;
        };
        cursor = next;
    }
    let _ = Record::TurnStarted {
        turn: String::new(),
    }; // keep the import honest
    Ok(out)
}
