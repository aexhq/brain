use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SandboxFilePathRequest {
    path: String,
    generation: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SandboxFileListRequest {
    path: String,
    generation: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_storage_limit")]
    limit: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SandboxFileReadRequest {
    path: String,
    generation: String,
    #[serde(default = "default_inline_storage_limit")]
    max_bytes: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SandboxFileWriteRequest {
    path: String,
    generation: String,
    content_base64: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SandboxFileUploadRequest {
    path: String,
    generation: String,
    bytes: u64,
    sha256: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SandboxFileSearchRequest {
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
pub(super) struct SandboxFileListResponse {
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

pub(super) async fn sandbox_file_list(
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

pub(super) async fn sandbox_file_stat(
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

pub(super) async fn sandbox_file_read_inline(
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

pub(super) async fn sandbox_file_write_inline(
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

pub(super) async fn sandbox_file_prepare_download(
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

pub(super) async fn sandbox_file_prepare_upload(
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

pub(super) async fn sandbox_file_complete_upload(
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

pub(super) async fn sandbox_file_find(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileSearchRequest>,
) -> Result<Json<SandboxFileListResponse>, Failure> {
    sandbox_file_search(state, headers, id, request, false).await
}

pub(super) async fn sandbox_file_grep(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SandboxFileSearchRequest>,
) -> Result<Json<SandboxFileListResponse>, Failure> {
    sandbox_file_search(state, headers, id, request, true).await
}
