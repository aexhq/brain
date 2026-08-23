use super::*;

impl Brain {
    /// The closed official `brain.sandbox` capability. Logical inventory, authorization and
    /// quota live in Brain; Environment receives only the exact typed target selected from that durable
    /// inventory. No action can switch on a model-visible Tool name or fabricate a physical id.
    pub(crate) async fn execute_sandbox_capability(
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
                .ok_or_else(|| BrainError::Invalid("sandbox action is required".into()))?;
            let root_id = st.head.root_id.clone();
            match action {
                "create" => {
                    let (sandbox_id, generation, target) =
                        additional_sandbox_identity(&root_id, session_id, operation_id)?;
                    let request_digest =
                        sandbox_request_digest(&root_id, session_id, operation_id, &input)?;
                    let now = crate::wall_ms();
                    let creating: brain_protocol::environment::SandboxStatus =
                        serde_json::from_value(serde_json::json!({
                            "state": "creating",
                            "target": target,
                            "generation": generation,
                            "changed_at_ms": now,
                            "expires_at_ms": null,
                        }))?;
                    let reserved = self
                        .journal
                        .reserve_sandbox(&crate::journal::SandboxReserveRequest {
                            root_id: root_id.clone(),
                            owner_session_id: session_id.to_owned(),
                            sandbox_id: sandbox_id.clone(),
                            operation_id: operation_id.to_owned(),
                            request_digest,
                            generation_intent: generation.clone(),
                            initial_status: creating,
                            now_ms: now,
                        })
                        .await?;
                    if sandbox_status_releases_slot(&reserved.status)
                        || matches!(
                            reserved.status.state,
                            brain_protocol::environment::SandboxState::Running
                                | brain_protocol::environment::SandboxState::Suspended
                        )
                    {
                        return Ok(serde_json::json!({
                            "sandbox_id": sandbox_id,
                            "status": reserved.status,
                        }));
                    }
                    let control = self.sandbox_control.as_ref().ok_or_else(|| {
                        BrainError::Invalid("additional sandbox control is unavailable".into())
                    })?;
                    let request = sandbox_create_request(
                        &st.head,
                        reserved.status.target.clone(),
                        &reserved.generation_intent,
                    )?;
                    let status = match control.create(request).await {
                        Ok(status) => status,
                        Err(error)
                            if error.code == brain_protocol::environment::EnvironmentErrorCode::SandboxGone =>
                        {
                            sandbox_gone_status(&reserved.status, "environment_reported_gone")?
                        }
                        Err(error) => return Err(map_environment_port_error(error)),
                    };
                    let persisted = self
                        .persist_additional_sandbox_status(&reserved, status)
                        .await?;
                    Ok(serde_json::json!({
                        "sandbox_id": sandbox_id,
                        "status": persisted.status,
                    }))
                }
                "list" => {
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = engine_page_limit(&input, "sandbox list", 20)?;
                    let page = self
                        .journal
                        .list_sandbox_page(&crate::journal::SandboxListQuery {
                            root_id: &root_id,
                            limit: limit as usize,
                            cursor: cursor.as_deref(),
                        })
                        .await?;
                    let data = page
                        .sandboxes
                        .into_iter()
                        .map(|item| {
                            serde_json::json!({
                                "sandbox_id": item.sandbox_id,
                                "owner_session_id": item.owner_session_id,
                                "status": item.status,
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(serde_json::json!({
                        "has_more": page.next_cursor.is_some(),
                        "next_cursor": page.next_cursor,
                        "data": data,
                    }))
                }
                "status" => {
                    let sandbox_id = required_identifier(&input, "sandbox_id", "sandbox")?;
                    let item = self.journal.get_sandbox(&root_id, &sandbox_id).await?;
                    let item = self.inspect_additional_sandbox(item).await?;
                    Ok(serde_json::json!({
                        "sandbox_id": item.sandbox_id,
                        "owner_session_id": item.owner_session_id,
                        "status": item.status,
                    }))
                }
                "exec" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let expected_target_ref = item
                        .status
                        .target_ref
                        .as_ref()
                        .map(|value| value.as_str().to_owned())
                        .ok_or_else(|| {
                            BrainError::Environment(
                                "live sandbox status is missing its target reference".into(),
                            )
                        })?;
                    let command = required_bounded_string(&input, "command", 131_072, "sandbox")?;
                    let cwd = optional_bounded_string(&input, "cwd", 4096)?;
                    let interactive = input
                        .get("interactive")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let execution_digest = hex::encode(Sha256::digest(
                        format!("brain.sandbox-execution\0{root_id}\0{operation_id}").as_bytes(),
                    ));
                    let execution_id = format!("exe_{}", &execution_digest[..24]);
                    let mut request: brain_protocol::environment::SandboxExecutionRequest =
                        serde_json::from_value(serde_json::json!({
                            "target": item.status.target,
                            "expected_generation": generation,
                            "execution_id": execution_id,
                            "request_digest": "0".repeat(64),
                            "input": {
                                "command": command,
                                "cwd": cwd,
                                "interactive": interactive,
                            },
                            "resources": {
                                "timeout_ms": 600_000,
                                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                            },
                            "network": sealed_sandbox_network(&st.head)?,
                        }))?;
                    request.request_digest =
                        brain_protocol::contract::sandbox_execution_request_digest(&request);
                    let expected_digest = String::from(request.request_digest.clone());
                    let receipt = self
                        .sandbox_control
                        .as_ref()
                        .ok_or_else(|| {
                            BrainError::Invalid("additional sandbox control is unavailable".into())
                        })?
                        .execute(request)
                        .await
                        .map_err(map_environment_port_error)?;
                    if String::from(receipt.operation.operation_id.clone()) != execution_id
                        || String::from(receipt.operation.request_digest.clone()) != expected_digest
                        || serde_json::to_value(&receipt.operation.target)?
                            != serde_json::to_value(&item.status.target)?
                        || String::from(receipt.operation.generation.clone()) != generation
                        || String::from(receipt.operation.target_ref.clone()) != expected_target_ref
                        || serde_json::to_value(&receipt.observation.operation)?
                            != serde_json::to_value(&receipt.operation)?
                    {
                        return Err(BrainError::Environment(
                            "sandbox execution receipt identity mismatch".into(),
                        ));
                    }
                    if let Some(terminal) = &receipt.observation.terminal
                        && (brain_protocol::contract::terminal_result_digest(terminal)
                            != terminal.terminal_digest
                            || terminal.inline.as_ref().is_some_and(|value| {
                                !brain_protocol::contract::terminal_inline_fits(value)
                            }))
                    {
                        return Err(BrainError::Environment(
                            "sandbox execution terminal receipt is invalid or oversized".into(),
                        ));
                    }
                    Ok(serde_json::json!({
                        "execution_id": execution_id,
                        "state": receipt.observation.state,
                        "output": receipt.observation.output,
                        "terminal": receipt.observation.terminal,
                    }))
                }
                "write_stdin" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let expected_target_ref = item
                        .status
                        .target_ref
                        .as_ref()
                        .map(|value| value.as_str().to_owned())
                        .ok_or_else(|| {
                            BrainError::Environment(
                                "live sandbox status is missing its target reference".into(),
                            )
                        })?;
                    let eof = input
                        .get("eof")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let execution_id = required_identifier(&input, "execution_id", "sandbox")?;
                    let text = optional_bounded_string(&input, "text", 4096)?.unwrap_or_default();
                    if text.len() > 4096 {
                        return Err(BrainError::Invalid(
                            "sandbox write_stdin text exceeds 4096 UTF-8 bytes".into(),
                        ));
                    }
                    let mut request: brain_protocol::environment::WriteStdinRequest =
                        serde_json::from_value(serde_json::json!({
                            "operation_id": operation_id,
                            "request_digest": "0".repeat(64),
                            "target": item.status.target,
                            "expected_generation": generation,
                            "execution_id": execution_id,
                            "text": text,
                            "eof": eof,
                        }))?;
                    request.request_digest =
                        brain_protocol::contract::write_stdin_request_digest(&request);
                    let expected_digest = request.request_digest.clone();
                    let receipt = self
                        .sandbox_control
                        .as_ref()
                        .ok_or_else(|| {
                            BrainError::Invalid("additional sandbox control is unavailable".into())
                        })?
                        .write_stdin(request)
                        .await
                        .map_err(map_environment_port_error)?;
                    if String::from(receipt.operation_id.clone()) != operation_id
                        || receipt.request_digest != expected_digest
                        || String::from(receipt.observation.operation.operation_id.clone())
                            != execution_id
                        || serde_json::to_value(&receipt.observation.operation.target)?
                            != serde_json::to_value(&item.status.target)?
                        || String::from(receipt.observation.operation.generation.clone())
                            != generation
                        || String::from(receipt.observation.operation.target_ref.clone())
                            != expected_target_ref
                    {
                        return Err(BrainError::Environment(
                            "sandbox stdin receipt identity mismatch".into(),
                        ));
                    }
                    if let Some(terminal) = &receipt.observation.terminal
                        && (brain_protocol::contract::terminal_result_digest(terminal)
                            != terminal.terminal_digest
                            || terminal.inline.as_ref().is_some_and(|value| {
                                !brain_protocol::contract::terminal_inline_fits(value)
                            }))
                    {
                        return Err(BrainError::Environment(
                            "sandbox stdin terminal receipt is invalid or oversized".into(),
                        ));
                    }
                    Ok(serde_json::json!({
                        "accepted": receipt.accepted,
                        "replayed": receipt.replayed,
                        "state": receipt.observation.state,
                        "output": receipt.observation.output,
                        "terminal": receipt.observation.terminal,
                    }))
                }
                "list_files" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = engine_page_limit(&input, "sandbox file list", 50)?;
                    let page = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .list(crate::environment::SandboxFileListRequest {
                            target: item.status.target,
                            expected_generation: generation,
                            path,
                            cursor,
                            limit: limit as u32,
                        })
                        .await
                        .map_err(map_environment_port_error)?;
                    Ok(serde_json::to_value(page)?)
                }
                "stat_file" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let entry = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .stat(sandbox_file_request(
                            &item.status.target,
                            &generation,
                            &path,
                        )?)
                        .await
                        .map_err(map_environment_port_error)?;
                    Ok(serde_json::to_value(entry)?)
                }
                "read_file" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let content = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .read(sandbox_file_request(
                            &item.status.target,
                            &generation,
                            &path,
                        )?)
                        .await
                        .map_err(map_environment_port_error)?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(&content.content_base64)
                        .map_err(|_| {
                            BrainError::Environment("sandbox returned invalid base64".into())
                        })?;
                    if bytes.len() > brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES {
                        return Err(BrainError::FileTooLarge {
                            limit: brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                        });
                    }
                    let text = String::from_utf8(bytes).map_err(|_| {
                        BrainError::Invalid(
                            "sandbox read_file is model-inline UTF-8 only; use save for binary data"
                                .into(),
                        )
                    })?;
                    Ok(serde_json::json!({"entry": content.entry, "text": text}))
                }
                "write_file" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let text = required_bounded_string(
                        &input,
                        "text",
                        brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                        "sandbox",
                    )?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let request = sandbox_file_write_request(
                        operation_id,
                        &item.status.target,
                        &generation,
                        &path,
                        text.as_bytes(),
                        overwrite,
                    )?;
                    let expected_digest = request.request_digest.clone();
                    let result = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .write(request)
                        .await
                        .map_err(map_environment_port_error)?;
                    validate_sandbox_file_write_result(&result, operation_id, &expected_digest)?;
                    Ok(serde_json::to_value(result.file)?)
                }
                "find_files" | "grep_files" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let field = if action == "find_files" {
                        "glob"
                    } else {
                        "query"
                    };
                    let expression = required_bounded_string(&input, field, 4096, "sandbox")?;
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = engine_page_limit(&input, "sandbox search", 50)?;
                    let request = sandbox_search_request(
                        &item.status.target,
                        &generation,
                        &path,
                        &expression,
                        cursor.as_deref(),
                        limit as u32,
                    )?;
                    let files = self.sandbox_files.as_ref().ok_or_else(|| {
                        BrainError::Invalid("sandbox files are unavailable".into())
                    })?;
                    let page = if action == "find_files" {
                        files.find(request).await
                    } else {
                        files.grep(request).await
                    }
                    .map_err(map_environment_port_error)?;
                    Ok(serde_json::to_value(page)?)
                }
                "load" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let key = required_bounded_string(&input, "key", 1024, "sandbox")?;
                    crate::storage::validate_storage_key(&key)?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
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
                    let copy = sandbox_copy_request(
                        operation_id,
                        &item.status.target,
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
                        .map_err(map_environment_port_error)?;
                    validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                    if result.object.is_some() {
                        return Err(BrainError::Environment(
                            "sandbox import returned an unexpected object identity".into(),
                        ));
                    }
                    Ok(serde_json::to_value(result.file)?)
                }
                "save" => {
                    let (item, generation) =
                        self.additional_sandbox_for_action(&root_id, &input).await?;
                    let key = required_bounded_string(&input, "key", 1024, "sandbox")?;
                    crate::storage::validate_storage_key(&key)?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let files = self.sandbox_files.as_ref().ok_or_else(|| {
                        BrainError::Invalid("sandbox files are unavailable".into())
                    })?;
                    let entry = files
                        .stat(sandbox_file_request(
                            &item.status.target,
                            &generation,
                            &path,
                        )?)
                        .await
                        .map_err(map_environment_port_error)?;
                    if entry.kind != brain_protocol::environment::FileEntryKind::File {
                        return Err(BrainError::Invalid(
                            "sandbox save source must be a regular file".into(),
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
                        &item.status.target,
                        &generation,
                        &path,
                        None,
                        &ticket,
                        "export",
                        false,
                    )?;
                    let expected_digest = copy.request_digest.clone();
                    let result = files
                        .transfer(copy)
                        .await
                        .map_err(map_environment_port_error)?;
                    validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                    let exported = result.object.as_ref().ok_or_else(|| {
                        BrainError::Environment("sandbox export omitted its object identity".into())
                    })?;
                    if exported.object_id.as_str() != ticket.object_id
                        || exported.bytes != entry.bytes
                    {
                        return Err(BrainError::Environment(
                            "sandbox export returned a different object identity".into(),
                        ));
                    }
                    let object =
                        complete_storage_upload_state(self, session_id, st, ticket.transfer_id)
                            .await?;
                    if object.bytes != exported.bytes || object.sha256 != exported.sha256.as_str() {
                        return Err(BrainError::Journal(
                            "published storage object differs from the sandbox export".into(),
                        ));
                    }
                    Ok(serde_json::to_value(object)?)
                }
                "terminate" => {
                    let sandbox_id = required_identifier(&input, "sandbox_id", "sandbox")?;
                    let current = self.journal.get_sandbox(&root_id, &sandbox_id).await?;
                    if sandbox_status_releases_slot(&current.status) {
                        return Ok(serde_json::json!({
                            "sandbox_id": sandbox_id,
                            "status": current.status,
                        }));
                    }
                    let status = match self
                        .sandbox_control
                        .as_ref()
                        .ok_or_else(|| {
                            BrainError::Invalid("additional sandbox control is unavailable".into())
                        })?
                        .terminate(current.status.target.clone())
                        .await
                    {
                        Ok(status) => status,
                        Err(error)
                            if error.code == brain_protocol::environment::EnvironmentErrorCode::SandboxGone =>
                        {
                            sandbox_gone_status(&current.status, "environment_reported_gone")?
                        }
                        Err(error) => return Err(map_environment_port_error(error)),
                    };
                    if !sandbox_status_releases_slot(&status) {
                        return Err(BrainError::Environment(
                            "sandbox termination did not return a confirmed terminal state".into(),
                        ));
                    }
                    let persisted = self
                        .persist_additional_sandbox_status(&current, status)
                        .await?;
                    Ok(serde_json::json!({
                        "sandbox_id": sandbox_id,
                        "status": persisted.status,
                    }))
                }
                other => Err(BrainError::Invalid(format!(
                    "unknown sandbox action {other:?}"
                ))),
            }
        };
        let result = tokio::select! {
            result = operation => result,
            _ = cancel.cancelled() => Err(BrainError::Cancelled),
        };
        engine_outcome(result, started)
    }

    /// Every sandbox action requires the caller's observed generation: acting on a replaced
    /// physical target is a conflict, never a silent redirect.
    async fn additional_sandbox_for_action(
        &self,
        root_id: &str,
        input: &serde_json::Value,
    ) -> Result<(crate::journal::SandboxInventoryDoc, String)> {
        let sandbox_id = required_identifier(input, "sandbox_id", "sandbox")?;
        let mut item = self.journal.get_sandbox(root_id, &sandbox_id).await?;
        if item.root_id != root_id {
            return Err(BrainError::FileNotFound(format!("sandbox {sandbox_id}")));
        }
        let generation = required_identifier(input, "generation", "sandbox")?;
        {
            let observed = item
                .status
                .generation
                .as_ref()
                .map(|generation| generation.as_str());
            if observed != Some(generation.as_str()) {
                return Err(if sandbox_status_releases_slot(&item.status) {
                    BrainError::SandboxGone
                } else {
                    BrainError::SandboxGenerationConflict
                });
            }
        }
        if sandbox_status_releases_slot(&item.status) {
            return Err(BrainError::SandboxGone);
        }
        if item
            .status
            .expires_at_ms
            .is_some_and(|expiry| expiry.get() <= crate::wall_ms())
        {
            item = self.inspect_additional_sandbox(item).await?;
            if sandbox_status_releases_slot(&item.status) {
                return Err(BrainError::SandboxGone);
            }
        }
        if !matches!(
            item.status.state,
            brain_protocol::environment::SandboxState::Running
                | brain_protocol::environment::SandboxState::Suspended
        ) {
            return Err(BrainError::EnvironmentUnavailable(
                "additional sandbox has not reached a live state".into(),
            ));
        }
        Ok((item, generation))
    }

    async fn inspect_additional_sandbox(
        &self,
        current: crate::journal::SandboxInventoryDoc,
    ) -> Result<crate::journal::SandboxInventoryDoc> {
        if sandbox_status_releases_slot(&current.status) {
            return Ok(current);
        }
        let control = self.sandbox_control.as_ref().ok_or_else(|| {
            BrainError::Invalid("additional sandbox control is unavailable".into())
        })?;
        let status = match control.inspect(current.status.target.clone()).await {
            Ok(status) => status,
            Err(error)
                if error.code == brain_protocol::environment::EnvironmentErrorCode::SandboxGone =>
            {
                sandbox_gone_status(&current.status, "environment_reported_gone")?
            }
            Err(error) => return Err(map_environment_port_error(error)),
        };
        self.persist_additional_sandbox_status(&current, status)
            .await
    }
}
