use super::*;

#[derive(Deserialize)]
pub(super) struct StorageListRequest {
    prefix: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_storage_limit")]
    limit: u32,
}

pub(super) fn default_storage_limit() -> u32 {
    100
}

#[derive(Deserialize)]
pub(super) struct StorageKeyRequest {
    key: String,
}

#[derive(Deserialize)]
pub(super) struct StorageReadRequest {
    key: String,
    #[serde(default = "default_inline_storage_limit")]
    max_bytes: u64,
}

pub(super) fn default_inline_storage_limit() -> u64 {
    1024 * 1024
}

#[derive(Deserialize)]
pub(super) struct StorageWriteInlineRequest {
    key: String,
    content_base64: String,
    content_type: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
pub(super) struct StorageUploadIntentRequest {
    key: String,
    bytes: u64,
    sha256: String,
    content_type: Option<String>,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageSandboxCopyRequest {
    key: String,
    path: String,
    sandbox_generation: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Serialize)]
pub(super) struct StorageObjectResponse {
    key: String,
    bytes: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    created_at: brain_protocol::session::Timestamp,
    updated_at: brain_protocol::session::Timestamp,
}

#[derive(Serialize)]
pub(super) struct StorageListResponse {
    data: Vec<StorageObjectResponse>,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Serialize)]
pub(super) struct StorageReadInlineResponse {
    object: StorageObjectResponse,
    content_base64: String,
}

#[derive(Serialize)]
pub(super) struct StorageTransferResponse {
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

pub(super) fn storage_ticket(
    ticket: crate::storage::StorageTransferTicket,
) -> StorageTransferResponse {
    StorageTransferResponse {
        transfer_id: ticket.transfer_id,
        method: ticket.method,
        url: ticket.url,
        headers: ticket.headers,
        expires_at: crate::events::ts(ticket.expires_at_ms),
        max_bytes: ticket.max_bytes,
    }
}

pub(super) async fn storage_list(
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

pub(super) async fn storage_stat(
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

pub(super) async fn storage_read_inline(
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

pub(super) async fn storage_write_inline(
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

pub(super) async fn storage_prepare_download(
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

pub(super) async fn storage_prepare_upload(
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

pub(super) async fn storage_complete_upload(
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

pub(super) async fn storage_delete(
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

pub(super) async fn storage_copy_from_sandbox(
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

pub(super) async fn storage_copy_to_sandbox(
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
