use super::*;

impl Brain {
    /// Engine-owned model facade over the same ordinary child-session resources used by the
    /// public API. Model-visible names never select an implementation; the sealed capability ID
    /// reaches this method only after the ToolCall intent is durable.
    pub(crate) async fn execute_child_capability(
        self: &Arc<Self>,
        parent_id: &str,
        operation_id: &str,
        input: serde_json::Value,
        cancel: CancellationToken,
    ) -> CallOutcome {
        let started = std::time::Instant::now();
        let action = input.get("action").and_then(serde_json::Value::as_str);
        let result: Result<serde_json::Value> = async {
            if cancel.is_cancelled() {
                return Err(BrainError::Cancelled);
            }
            match action {
                Some("spawn_agent") => {
                    let task_name = required_child_string(&input, "task_name")?;
                    let message = required_child_string(&input, "message")?;
                    let fork_turns = input
                        .get("fork_turns")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    // Direct creation, never a command to this session's own actor: the
                    // spawning turn may itself be the actor's inline hydration turn (a
                    // child's first turn runs during attach), and a self-delivered
                    // CreateChild would deadlock the session — the 2026-08-23 dev wedge.
                    let child = create_child_session(
                        self,
                        parent_id,
                        message,
                        Some(task_name),
                        ForkTurns::parse(fork_turns.as_deref())?,
                        Some(operation_id),
                    )
                    .await?;
                    child_doc(&child)
                }
                Some("send_message" | "follow_up") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    let message = required_child_string(&input, "message")?;
                    self.get_child(parent_id, &child_id).await?;
                    let content =
                        MessageRequestContent::String(message.parse().map_err(|error| {
                            BrainError::Invalid(format!("child message: {error}"))
                        })?);
                    let (turn_id, seq) = self
                        .message_with_metadata_idempotent(
                            &child_id,
                            content,
                            HashMap::new(),
                            Some(operation_id),
                        )
                        .await?;
                    Ok(serde_json::json!({
                        "child_id": child_id,
                        "turn_id": turn_id,
                        "seq": seq,
                    }))
                }
                Some("peek") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    child_doc(&self.get_child(parent_id, &child_id).await?)
                }
                Some("wait") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    let timeout = input
                        .get("timeout_ms")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(30_000)
                        .min(300_000);
                    let wait =
                        self.wait_child(parent_id, &child_id, Duration::from_millis(timeout));
                    let child = tokio::select! {
                        result = wait => result?,
                        _ = cancel.cancelled() => return Err(BrainError::Cancelled),
                    };
                    child_doc(&child)
                }
                Some("list_children") => {
                    let cursor = input.get("cursor").and_then(serde_json::Value::as_str);
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(20)
                        .min(100) as usize;
                    let (data, next_cursor) = self.list_children(parent_id, cursor, limit).await?;
                    Ok(serde_json::json!({
                        "has_more": next_cursor.is_some(),
                        "next_cursor": next_cursor,
                        "data": child_docs(&data)?,
                    }))
                }
                Some("interrupt_agent") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    self.get_child(parent_id, &child_id).await?;
                    child_doc(&self.cancel(&child_id).await?)
                }
                Some("end_agent") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    self.get_child(parent_id, &child_id).await?;
                    child_doc(&self.end(&child_id).await?)
                }
                Some(other) => Err(BrainError::Invalid(format!(
                    "unknown subagents action {other:?}"
                ))),
                None => Err(BrainError::Invalid("subagents action is required".into())),
            }
        }
        .await;
        match result {
            Ok(value) => CallOutcome {
                outcome: TerminalOutcome::Completed,
                content: serde_json::to_string(&value)
                    .unwrap_or_else(|_| "child operation completed".into()),
                value: Some(value),
                is_error: false,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                truncated: false,
                terminal: None,
            },
            Err(BrainError::Cancelled) => CallOutcome {
                outcome: TerminalOutcome::Cancelled,
                content: "child operation cancelled".into(),
                value: None,
                is_error: true,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                truncated: false,
                terminal: None,
            },
            Err(error) => {
                let mut outcome = CallOutcome::failed(error.to_string());
                outcome.duration_ms = started.elapsed().as_millis() as u64;
                outcome
            }
        }
    }
}
