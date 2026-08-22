use super::*;

pub(super) async fn create_session(
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
pub(super) struct ListQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    cursor: Option<String>,
    state: Option<String>,
}
fn default_limit() -> usize {
    20
}

pub(super) async fn list_sessions(
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

pub(super) async fn get_session(
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

pub(super) async fn delete_session(
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
        if status.state == DeletionState::Succeeded {
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

pub(super) async fn get_deletion_status(
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
        state: status.state.as_str().to_string(),
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

pub(super) async fn send_message(
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
            seq: NonZeroU64::new(seq).expect("journal seqs start at 1; zero is corrupt"),
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

pub(super) async fn cancel_turn(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<session::Session>, Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(Json(state.brain.cancel(&id).await.map_err(map_err)?))
}

pub(super) async fn end_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<session::Session>), Failure> {
    authorize_session(&state, &headers, &id).await?;
    Ok(end_accepted(state.brain.end(&id).await.map_err(map_err)?))
}

pub(super) const END_ACCEPTED_STATUS: StatusCode = StatusCode::ACCEPTED;

pub(super) fn end_accepted(session: session::Session) -> (StatusCode, Json<session::Session>) {
    (END_ACCEPTED_STATUS, Json(session))
}

pub(super) async fn get_default_sandbox(
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

pub(super) async fn create_default_sandbox(
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
