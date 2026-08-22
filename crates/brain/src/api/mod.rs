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
use crate::journal::{DeletionState, Record, SessionLifecycle};
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
    /// Hosted compositions require `x-brain-tenant-id` on every request; a single-tenant
    /// composition names its one implicit tenant explicitly — never a silent hardcoded default.
    pub tenancy: Tenancy,
}

/// How requests are booked to tenants.
#[derive(Debug, Clone)]
pub enum Tenancy {
    /// Every request must carry `x-brain-tenant-id`; a missing header is a request error.
    Required,
    /// Single-tenant composition: requests without the header book to this tenant.
    Implicit(String),
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

pub async fn serve(state: AppState, addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let brain = state.brain.clone();
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "brain listening");
    // Drain-before-stop: on SIGTERM/ctrl-c the Brain refuses new work while admitted turns run
    // to completion, and the HTTP server keeps serving so event followers stream those turns to
    // their durable terminals. Selecting (not axum graceful shutdown) is deliberate: open SSE
    // connections would otherwise hold shutdown forever; dropping the server resets them and
    // reconnecting followers replay durable records from the replacement instance.
    let serving = axum::serve(nodelay(listener), app);
    tokio::select! {
        result = serving => result?,
        () = drain_on_shutdown(brain) => {}
    }
    Ok(())
}

/// Wait for a shutdown signal, then drain: refuse new work and hold the process open until
/// every admitted turn finished or the drain timeout passed.
async fn drain_on_shutdown(brain: Arc<crate::session::Brain>) {
    shutdown_signal().await;
    brain.begin_drain();
    let deadline = std::time::Instant::now() + brain.cfg.drain_timeout;
    loop {
        let active = brain.active_turns();
        if active == 0 {
            tracing::info!("drain complete; shutting down");
            return;
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!(
                active_turns = active,
                "drain timeout reached; shutting down with turns still active"
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler installs");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
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

fn auth(state: &AppState, headers: &HeaderMap) -> Result<TrustedPrincipal, Failure> {
    if bearer_token(headers) == Some(state.token.as_str()) {
        let tenant_id = match (
            headers
                .get("x-brain-tenant-id")
                .and_then(|value| value.to_str().ok()),
            &state.tenancy,
        ) {
            (Some(tenant), _) => tenant,
            (None, Tenancy::Required) => {
                return Err(Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "x-brain-tenant-id header is required by this composition".into(),
                ));
            }
            (None, Tenancy::Implicit(tenant)) => tenant.as_str(),
        };
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
#[path = "../api_tests/error.rs"]
mod api_error_tests;

#[cfg(test)]
#[path = "../api_tests/observation_auth.rs"]
mod customer_observation_auth_tests;

#[cfg(test)]
#[path = "../api_tests/routes.rs"]
mod public_route_contract_tests;

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

mod children;
mod customer_ws;
mod error;
mod sandbox_files;
mod sessions;
mod sse;
mod storage;

use children::*;
use customer_ws::*;
use error::*;
use sandbox_files::*;
use sessions::*;
pub use sse::replay;
use sse::*;
use storage::*;
