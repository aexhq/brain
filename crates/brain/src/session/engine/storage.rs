use super::*;

impl Brain {
    /// Engine-owned durable-storage facade. The caller passes the turn's already-claimed mutable
    /// state so storage reservations and their ToolCall live under one fence; this must never
    /// recurse through the session actor while that same turn is running.
    pub(crate) async fn execute_storage_capability(
        self: &Arc<Self>,
        session_id: &str,
        operation_id: &str,
        input: serde_json::Value,
        cancel: CancellationToken,
        st: &mut TurnState,
    ) -> Result<CallOutcome> {
        let started = std::time::Instant::now();
        let operation = async {
            let action = input
                .get("action")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| BrainError::Invalid("storage action is required".into()))?;
            match action {
                "list" => {
                    let prefix = optional_bounded_string(&input, "prefix", 1024)?;
                    if let Some(prefix) = &prefix {
                        validate_storage_prefix(prefix)?;
                    }
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(20);
                    if !(1..=100).contains(&limit) {
                        return Err(BrainError::Invalid(
                            "storage list limit must be between 1 and 100".into(),
                        ));
                    }
                    ensure_storage_readable(&st.head, session_id)?;
                    let page = self
                        .storage_port()?
                        .list(
                            session_id,
                            prefix.as_deref(),
                            cursor.as_deref(),
                            limit as u32,
                        )
                        .await?;
                    Ok(serde_json::to_value(page)?)
                }
                "save" => {
                    let key = required_bounded_string(&input, "key", 1024, "storage")?;
                    crate::storage::validate_storage_key(&key)?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let source = input.get("source").ok_or_else(|| {
                        BrainError::Invalid("storage save source is required".into())
                    })?;
                    match source.get("kind").and_then(serde_json::Value::as_str) {
                        Some("inline_text") => {
                            let text = required_bounded_string(
                                source,
                                "text",
                                brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                                "storage save source",
                            )?;
                            if text.len() > brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES {
                                return Err(BrainError::FileTooLarge {
                                    limit: brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                                });
                            }
                            let object = write_storage_inline_state(
                                self,
                                session_id,
                                st,
                                key,
                                base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
                                Some("text/plain; charset=utf-8".into()),
                                overwrite,
                            )
                            .await?;
                            Ok(serde_json::to_value(object)?)
                        }
                        Some("sandbox_path") => {
                            let path = required_bounded_string(
                                source,
                                "path",
                                4096,
                                "storage save source",
                            )?;
                            let generation =
                                required_identifier(source, "generation", "storage save source")?;
                            let target = default_sandbox_target(&st.head.root_id)?;
                            let file_request = sandbox_file_request(&target, &generation, &path)?;
                            let files = self.sandbox_files.as_ref().ok_or_else(|| {
                                BrainError::Invalid("sandbox files are unavailable".into())
                            })?;
                            let entry = files
                                .stat(file_request)
                                .await
                                .map_err(map_hand_port_error)?;
                            if entry.kind != brain_protocol::hand::FileEntryKind::File {
                                return Err(BrainError::Invalid(
                                    "storage save source must be a regular file".into(),
                                ));
                            }
                            let ticket = prepare_storage_upload_state(
                                self,
                                session_id,
                                st,
                                crate::storage::StorageUploadIntent {
                                    key,
                                    bytes: entry.bytes,
                                    sha256: None,
                                    content_type: None,
                                    overwrite,
                                },
                            )
                            .await?;
                            let copy = sandbox_copy_request(
                                operation_id,
                                &target,
                                &generation,
                                &path,
                                None,
                                &ticket,
                                "export",
                                false,
                            )?;
                            let expected_digest = copy.request_digest.clone();
                            let result = files.transfer(copy).await.map_err(map_hand_port_error)?;
                            validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                            let exported = result.object.as_ref().ok_or_else(|| {
                                BrainError::Hand(
                                    "sandbox export omitted its uploaded object identity".into(),
                                )
                            })?;
                            if exported.object_id.as_str() != ticket.object_id
                                || exported.bytes != entry.bytes
                            {
                                return Err(BrainError::Hand(
                                    "sandbox export returned a different object identity".into(),
                                ));
                            }
                            let object = complete_storage_upload_state(
                                self,
                                session_id,
                                st,
                                ticket.transfer_id,
                            )
                            .await?;
                            if object.bytes != exported.bytes
                                || object.sha256 != exported.sha256.as_str()
                            {
                                return Err(BrainError::Journal(
                                    "published storage object differs from the sandbox export"
                                        .into(),
                                ));
                            }
                            Ok(serde_json::to_value(object)?)
                        }
                        Some(other) => Err(BrainError::Invalid(format!(
                            "unknown storage save source kind {other:?}"
                        ))),
                        None => Err(BrainError::Invalid(
                            "storage save source kind is required".into(),
                        )),
                    }
                }
                "load" => {
                    let key = required_bounded_string(&input, "key", 1024, "storage")?;
                    crate::storage::validate_storage_key(&key)?;
                    let path = required_bounded_string(&input, "path", 4096, "storage")?;
                    let generation = required_identifier(&input, "generation", "storage")?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    ensure_storage_readable(&st.head, session_id)?;
                    let storage = self.storage_port()?;
                    let object = storage.stat(session_id, &key).await?;
                    let ticket = storage.prepare_download(session_id, &key).await?;
                    if ticket.max_bytes != object.bytes {
                        return Err(BrainError::Journal(
                            "download authority does not match the stored object size".into(),
                        ));
                    }
                    let reference = storage_object_reference(
                        &ticket.object_id,
                        object.bytes,
                        &object.sha256,
                        object.content_type.as_deref(),
                    )?;
                    let target = default_sandbox_target(&st.head.root_id)?;
                    let copy = sandbox_copy_request(
                        operation_id,
                        &target,
                        &generation,
                        &path,
                        Some(reference),
                        &ticket,
                        "import",
                        overwrite,
                    )?;
                    let expected_digest = copy.request_digest.clone();
                    let result = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .transfer(copy)
                        .await
                        .map_err(map_hand_port_error)?;
                    validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                    if result.object.is_some() {
                        return Err(BrainError::Hand(
                            "sandbox import returned an unexpected object identity".into(),
                        ));
                    }
                    Ok(serde_json::to_value(result.file)?)
                }
                other => Err(BrainError::Invalid(format!(
                    "unknown storage action {other:?}"
                ))),
            }
        };
        let result = tokio::select! {
            result = operation => result,
            _ = cancel.cancelled() => Err(BrainError::Cancelled),
        };
        engine_outcome(result, started)
    }
}
