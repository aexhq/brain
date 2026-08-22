use super::*;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateChildRequest {
    prompt: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    fork_turns: Option<String>,
}

pub(super) async fn create_child(
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
pub(super) struct ChildListQuery {
    cursor: Option<String>,
    #[serde(default = "default_child_limit")]
    limit: usize,
}

fn default_child_limit() -> usize {
    20
}

pub(super) async fn list_children(
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

pub(super) async fn get_child(
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
pub(super) struct ChildMessageRequest {
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
        seq: NonZeroU64::new(seq).expect("journal seqs start at 1; zero is corrupt"),
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

pub(super) async fn send_child_message(
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

pub(super) async fn follow_up_child(
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
pub(super) struct WaitChildRequest {
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// The wait ceiling is 5 minutes; asking for more is rejected, never silently clamped.
fn wait_child_timeout_ms(requested: Option<u64>) -> Result<u64, Failure> {
    const WAIT_CHILD_MAX_MS: u64 = 300_000;
    match requested {
        None => Ok(30_000),
        Some(ms) if ms <= WAIT_CHILD_MAX_MS => Ok(ms),
        Some(ms) => Err(Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("timeout_ms {ms} exceeds the {WAIT_CHILD_MAX_MS}ms wait ceiling"),
        )),
    }
}

pub(super) async fn wait_child(
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
                std::time::Duration::from_millis(wait_child_timeout_ms(request.timeout_ms)?),
            )
            .await
            .map_err(map_err)?,
    ))
}

pub(super) async fn interrupt_child(
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

pub(super) async fn end_child(
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
