use super::*;

#[derive(Debug)]
pub(super) struct PendingVolatile {
    pub(super) seq: u64,
    pub(super) turn: String,
    pub(super) agent: String,
    pub(super) call: String,
    pub(super) name: String,
}

#[derive(Debug, Clone)]
pub(super) struct PendingExternal {
    pub(super) seq: u64,
    pub(super) turn: String,
    pub(super) call: String,
    pub(super) name: String,
    pub(super) input: serde_json::Value,
    pub(super) context: HashMap<String, String>,
    pub(super) policy: crate::config::ServerToolPolicy,
    pub(super) parallel_batch: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PendingCustomer {
    pub(super) seq: u64,
    pub(super) turn: String,
    pub(super) call: String,
    pub(super) name: String,
    pub(super) intent: crate::customer::CustomerOperationIntent,
}

#[derive(Debug, Clone)]
pub(super) struct PendingManaged {
    pub(super) seq: u64,
    pub(super) turn: String,
    pub(super) call: String,
    pub(super) name: String,
    pub(super) envelope: brain_protocol::hand::OperationEnvelope,
    pub(super) operation: Option<brain_protocol::hand::OperationRef>,
    pub(super) submit_unknown: bool,
}

/// Host-tool calls whose intent committed but whose result did not. Only the stable sealed policy
/// determines whether replay is permitted; model arguments cannot opt a call into replay.
pub(super) fn pending_external(entries: &[Entry], prefix: &PrefixDoc) -> Vec<PendingExternal> {
    let answered: HashSet<&str> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ToolResult { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect();
    let terminal_turns: HashSet<&str> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::TurnCompleted { turn, .. } | Record::TurnFailed { turn, .. } => {
                Some(turn.as_str())
            }
            _ => None,
        })
        .collect();
    let contexts: HashMap<&str, &HashMap<String, String>> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::UserMessage { turn, metadata, .. } => Some((turn.as_str(), metadata)),
            _ => None,
        })
        .collect();
    let policies: HashMap<String, crate::config::ServerToolPolicy> = resolve_sealed_tools(prefix)
        .into_iter()
        .filter_map(|tool| match tool.route {
            crate::config::ToolRoute::Server(policy) => Some((tool.name, policy)),
            _ => None,
        })
        .collect();

    let mut pending = Vec::new();
    for entry in entries {
        let Record::ToolCall {
            turn,
            agent,
            call,
            name,
            input,
            ..
        } = &entry.record
        else {
            continue;
        };
        let Some(policy) = policies.get(name.as_str()).cloned() else {
            continue;
        };
        if agent != "root"
            || answered.contains(call.as_str())
            || terminal_turns.contains(turn.as_str())
        {
            continue;
        }
        let assistant_seq = entries
            .iter()
            .rev()
            .find_map(|candidate| match &candidate.record {
                Record::Assistant {
                    turn: assistant_turn,
                    agent,
                    ..
                } if candidate.seq < entry.seq && assistant_turn == turn && agent == "root" => {
                    Some(candidate.seq)
                }
                _ => None,
            })
            .unwrap_or(0);
        let next_assistant_seq = entries
            .iter()
            .filter_map(|candidate| match &candidate.record {
                Record::Assistant {
                    turn: assistant_turn,
                    agent,
                    ..
                } if candidate.seq > assistant_seq && assistant_turn == turn && agent == "root" => {
                    Some(candidate.seq)
                }
                _ => None,
            })
            .min()
            .unwrap_or(u64::MAX);
        let batch_size = entries
            .iter()
            .filter(|candidate| {
                candidate.seq > assistant_seq
                    && candidate.seq < next_assistant_seq
                    && matches!(
                        &candidate.record,
                        Record::ToolCall { turn: other_turn, agent, .. }
                            if other_turn == turn && agent == "root"
                    )
            })
            .count();
        pending.push(PendingExternal {
            seq: entry.seq,
            turn: turn.clone(),
            call: call.clone(),
            name: name.clone(),
            input: input.clone(),
            context: contexts
                .get(turn.as_str())
                .map_or_else(HashMap::new, |v| (*v).clone()),
            policy,
            parallel_batch: batch_size > 1,
        });
    }
    pending.sort_by_key(|call| call.seq);
    pending
}

pub(super) fn pending_customer(
    entries: &[Entry],
    prefix: &PrefixDoc,
    tenant_id: &str,
    session_id: &str,
) -> Vec<PendingCustomer> {
    let answered = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ToolResult { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let intents = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::CustomerCallIntent {
                call,
                client_id,
                process_id,
                request_digest,
                deadline_at_ms,
                ..
            } => Some((
                call.as_str(),
                (
                    client_id.clone(),
                    process_id.clone(),
                    request_digest.clone(),
                    *deadline_at_ms,
                ),
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let tools = resolve_sealed_tools(prefix)
        .into_iter()
        .map(|tool| (tool.name.clone(), tool))
        .collect::<HashMap<_, _>>();
    let mut pending = Vec::new();
    for entry in entries {
        let Record::ToolCall {
            turn,
            agent,
            call,
            name,
            input,
            ..
        } = &entry.record
        else {
            continue;
        };
        if agent != "root" || answered.contains(call.as_str()) {
            continue;
        }
        let Some((client_id, process_id, request_digest, deadline_at_ms)) =
            intents.get(call.as_str())
        else {
            continue;
        };
        let Some(tool) = tools.get(name) else {
            continue;
        };
        let crate::config::ToolRoute::Customer { registration } = &tool.route else {
            continue;
        };
        pending.push(PendingCustomer {
            seq: entry.seq,
            turn: turn.clone(),
            call: call.clone(),
            name: name.clone(),
            intent: crate::customer::CustomerOperationIntent {
                tenant_id: tenant_id.to_owned(),
                client_id: client_id.clone(),
                process_id: process_id.clone(),
                session_id: session_id.to_owned(),
                operation_id: call.clone(),
                registration: registration.clone(),
                name: name.clone(),
                contract_digest: tool.contract_digest.clone(),
                input: input.clone(),
                deadline_at_ms: *deadline_at_ms,
                request_digest: request_digest.clone(),
            },
        });
    }
    pending.sort_by_key(|call| call.seq);
    pending
}

pub(super) fn pending_managed(entries: &[Entry]) -> Result<Vec<PendingManaged>> {
    let mut pending = HashMap::<String, PendingManaged>::new();
    for entry in entries {
        match &entry.record {
            Record::ManagedCallIntent {
                turn,
                call,
                name,
                envelope,
            } => {
                let next = PendingManaged {
                    seq: entry.seq,
                    turn: turn.clone(),
                    call: call.clone(),
                    name: name.clone(),
                    envelope: envelope.clone(),
                    operation: None,
                    submit_unknown: false,
                };
                if let Some(previous) = pending.insert(call.clone(), next)
                    && (previous.turn != *turn
                        || previous.name != *name
                        || serde_jcs::to_vec(&previous.envelope)? != serde_jcs::to_vec(envelope)?)
                {
                    return Err(BrainError::Journal(
                        "managed operation id maps to conflicting durable intents".into(),
                    ));
                }
            }
            Record::ManagedCallAccepted {
                turn,
                call,
                operation,
            } => {
                let item = pending.get_mut(call).ok_or_else(|| {
                    BrainError::Journal(
                        "managed accepted receipt has no preceding durable intent".into(),
                    )
                })?;
                if item.turn != *turn {
                    return Err(BrainError::Journal(
                        "managed accepted receipt references a different turn".into(),
                    ));
                }
                if let Some(previous) = &item.operation
                    && serde_jcs::to_vec(previous)? != serde_jcs::to_vec(operation)?
                {
                    return Err(BrainError::Journal(
                        "managed operation id maps to conflicting accepted receipts".into(),
                    ));
                }
                item.operation = Some(operation.clone());
            }
            Record::ManagedCallUnknown {
                turn,
                call,
                request_digest,
            } => {
                let item = pending.get_mut(call).ok_or_else(|| {
                    BrainError::Journal(
                        "managed unknown marker has no preceding durable intent".into(),
                    )
                })?;
                if item.turn != *turn
                    || item.envelope.request_digest.as_str() != request_digest.as_str()
                {
                    return Err(BrainError::Journal(
                        "managed unknown marker conflicts with its durable intent".into(),
                    ));
                }
                item.submit_unknown = true;
            }
            Record::ToolResult { call, .. } => {
                pending.remove(call);
            }
            _ => {}
        }
    }
    let mut pending = pending.into_values().collect::<Vec<_>>();
    pending.sort_by_key(|call| call.seq);
    Ok(pending)
}

/// Finds unanswered calls whose executor cannot be recovered unambiguously. A newly claimed
/// session never reassigns these calls: Hand, customer-app and in-process intrinsic effects may
/// already have happened. Replay-safe server capabilities are handled separately.
pub(super) fn pending_volatile(entries: &[Entry], prefix: &PrefixDoc) -> Vec<PendingVolatile> {
    let customer_intents = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::CustomerCallIntent { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let managed_intents = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ManagedCallIntent { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let volatile_names: HashSet<String> = resolve_sealed_tools(prefix)
        .into_iter()
        .filter_map(|tool| match tool.route {
            crate::config::ToolRoute::Server(_) => None,
            _ => Some(tool.name),
        })
        .collect();
    let mut pending = HashMap::<String, PendingVolatile>::new();
    for entry in entries {
        match &entry.record {
            Record::ToolCall {
                turn,
                agent,
                call,
                name,
                detach,
                ..
            } if volatile_names.contains(name)
                && !customer_intents.contains(call.as_str())
                && !managed_intents.contains(call.as_str())
                && !detach =>
            {
                pending.insert(
                    call.clone(),
                    PendingVolatile {
                        seq: entry.seq,
                        turn: turn.clone(),
                        agent: agent.clone(),
                        call: call.clone(),
                        name: name.clone(),
                    },
                );
            }
            Record::ToolResult { call, .. } => {
                pending.remove(call);
            }
            _ => {}
        }
    }
    let mut pending: Vec<_> = pending.into_values().collect();
    pending.sort_by_key(|call| call.seq);
    pending
}

pub(super) async fn recover_customer_calls(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    entries: &[Entry],
) -> Result<bool> {
    let Some(customer) = &brain.customer else {
        if pending_customer(
            entries,
            &resident.st.head.prefix,
            &resident.st.head.tenant_id,
            session_id,
        )
        .is_empty()
        {
            return Ok(false);
        }
        return Err(BrainError::HandUnavailable(
            "customer application coordinator is unavailable during recovery".into(),
        ));
    };
    let pending = pending_customer(
        entries,
        &resident.st.head.prefix,
        &resident.st.head.tenant_id,
        session_id,
    );
    let mut changed = false;
    for call in pending {
        let execution = customer
            .execute_prepared(
                call.intent.clone(),
                resident.st.head.prefix.customer_submit_retries,
                CancellationToken::new(),
            )
            .await;
        if execution.retryable_without_effect && crate::wall_ms() < call.intent.deadline_at_ms {
            return Err(BrainError::HandUnavailable(
                "sealed customer application process has not reconnected".into(),
            ));
        }
        let tool = crate::tools::resolve(&resident.st.head.prefix.tools)?
            .into_iter()
            .find(|tool| tool.name == call.name);
        let outcome = crate::tools::enforce_outcome(tool.as_ref(), &call.name, execution.outcome);
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content.clone()
        };
        let mut records = vec![(
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn.clone(),
                agent: "root".into(),
                call: call.call.clone(),
                name: call.name.clone(),
                outcome: crate::events::tool_outcome(outcome.outcome),
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        )];
        if let Some(receipt) = &execution.terminal_receipt {
            let ack = crate::journal::CustomerTerminalAckDoc {
                turn: call.turn.clone(),
                call: call.call.clone(),
                client_id: call.intent.client_id.clone(),
                process_id: receipt.process_id.clone(),
                request_digest: receipt.request_digest.clone(),
                terminal_digest: receipt.terminal_digest.clone(),
            };
            if !resident
                .st
                .head
                .pending_customer_acks
                .iter()
                .any(|current| current.call == ack.call)
            {
                resident.st.head.pending_customer_acks.push(ack);
            }
            records.push((
                resident.st.take_seq(),
                Record::CustomerTerminalReceived {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    client_id: call.intent.client_id.clone(),
                    process_id: receipt.process_id.clone(),
                    request_digest: receipt.request_digest.clone(),
                    terminal_digest: receipt.terminal_digest.clone(),
                },
            ));
        }
        commit(brain, session_id, &mut resident.st, records).await?;
        changed = true;

        if let Some(receipt) = execution.terminal_receipt
            && customer.acknowledge_terminal(&receipt).await.is_ok()
        {
            let previous = resident.st.head.pending_customer_acks.clone();
            resident.st.head.pending_customer_acks.retain(|pending| {
                pending.call != receipt.operation_id
                    || pending.request_digest != receipt.request_digest
                    || pending.terminal_digest != receipt.terminal_digest
            });
            let acked = vec![(
                resident.st.take_seq(),
                Record::CustomerTerminalAcknowledged {
                    turn: call.turn,
                    call: call.call,
                    request_digest: receipt.request_digest,
                    terminal_digest: receipt.terminal_digest,
                },
            )];
            if let Err(error) = commit(brain, session_id, &mut resident.st, acked).await {
                resident.st.head.pending_customer_acks = previous;
                return Err(error);
            }
        }
    }
    Ok(changed)
}

pub(super) async fn recover_managed_calls(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    entries: &[Entry],
) -> Result<bool> {
    use brain_protocol::hand::{HandErrorCode, OperationState};

    let pending = pending_managed(entries)?;
    if pending.is_empty() {
        return Ok(false);
    }
    let tools = crate::tools::resolve(&resident.st.head.prefix.tools)?;
    let active_turn = resident.st.head.turn.clone();
    let rounds = resident.st.head.active_rounds;
    let tool_calls = resident.st.head.active_tool_calls;
    let mut recovered = Vec::new();
    let mut sandbox_gone = None;

    let (stale, pending): (Vec<_>, Vec<_>) = pending
        .into_iter()
        .partition(|call| active_turn.as_deref() != Some(call.turn.as_str()));
    for call in stale {
        if !call.submit_unknown {
            let unknown = vec![(
                resident.st.take_seq(),
                Record::ManagedCallUnknown {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    request_digest: call.envelope.request_digest.to_string(),
                },
            )];
            commit(brain, session_id, &mut resident.st, unknown).await?;
        }
        reconcile_managed_unknown_default_sandbox(brain, session_id, &mut resident.st).await?;
        let outcome = crate::tools::enforce_outcome(
            tools.iter().find(|tool| tool.name == call.name),
            &call.name,
            crate::turn::managed_unknown_call_outcome(&call.name),
        );
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content
        };
        let result = vec![(
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn,
                agent: "root".into(),
                call: call.call,
                name: call.name,
                outcome: crate::events::tool_outcome(outcome.outcome),
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        )];
        commit(brain, session_id, &mut resident.st, result).await?;
    }
    if pending.is_empty() {
        return Ok(true);
    }

    let hand = brain.hand.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "managed Hand is unavailable for durable operation recovery".into(),
        )
    })?;

    'managed_calls: for call in pending {
        if call.submit_unknown {
            reconcile_managed_unknown_default_sandbox(brain, session_id, &mut resident.st).await?;
            recovered.push((
                call.clone(),
                call.operation.clone(),
                crate::turn::managed_unknown_call_outcome(&call.name),
                None,
            ));
            continue;
        }
        let binding = resident.managed_bindings.get(&call.name).ok_or_else(|| {
            BrainError::HandUnavailable(format!(
                "managed Tool {} has no recovered immutable binding",
                call.name
            ))
        })?;
        if binding.binding_ref != call.envelope.binding_ref {
            return Err(BrainError::Protocol(
                "managed Tool binding changed across durable recovery".into(),
            ));
        }

        let mut accepted_now = false;
        let (operation, mut observation) = if let Some(operation) = call.operation.clone() {
            crate::turn::verify_managed_operation(
                &operation,
                &call.call,
                call.envelope.request_digest.as_str(),
                session_id,
                &resident.st.head,
            )?;
            let request: brain_protocol::hand::ObserveRequest =
                serde_json::from_value(serde_json::json!({
                    "operation": operation,
                    "cursor": "",
                    "wait_ms": binding.limits.max_wait_ms.min(30_000),
                }))?;
            let observed = tokio::time::timeout(
                Duration::from_millis(request.wait_ms.saturating_add(1_000).max(1)),
                hand.observe(request),
            )
            .await
            .map_err(|_| {
                BrainError::HandUnavailable("managed Tool recovery observation timed out".into())
            })?;
            match observed {
                Ok(observation) => (operation, observation),
                Err(error) if error.code == HandErrorCode::SandboxGone => {
                    sandbox_gone = Some(managed_operation_gone_status(&operation)?);
                    recovered.push((
                        call,
                        Some(operation),
                        CallOutcome {
                            outcome: TerminalOutcome::Interrupted,
                            value: None,
                            content: "managed Tool target disappeared before its durable terminal could be recovered".into(),
                            is_error: true,
                            exit_code: None,
                            duration_ms: 0,
                            truncated: false,
                            terminal: None,
                        },
                        None,
                    ));
                    continue;
                }
                Err(error) => return Err(map_hand_port_error(error)),
            }
        } else {
            let request = brain_protocol::hand::SubmitRequest {
                envelope: call.envelope.clone(),
                wait_up_to_ms: binding.limits.max_wait_ms.min(30_000),
            };
            let mut reprepared = false;
            let receipt = loop {
                let submitted = tokio::time::timeout(
                    Duration::from_millis(request.wait_up_to_ms.saturating_add(1_000).max(1)),
                    hand.submit(request.clone()),
                )
                .await
                .map_err(|_| {
                    BrainError::HandUnavailable("managed Tool recovery submit timed out".into())
                })?;
                match submitted {
                    Ok(receipt) => break receipt,
                    Err(error)
                        if error.code == HandErrorCode::CapabilityUnavailable && !reprepared =>
                    {
                        let refreshed = brain
                            .prepare_managed_session(session_id, &resident.st.head)
                            .await?;
                        let refreshed = refreshed.get(&call.name).ok_or_else(|| {
                            BrainError::HandUnavailable(
                                "managed Tool disappeared during exact re-preparation".into(),
                            )
                        })?;
                        if refreshed.binding_ref != binding.binding_ref {
                            return Err(BrainError::Protocol(
                                "managed Tool binding changed during exact re-preparation".into(),
                            ));
                        }
                        reprepared = true;
                    }
                    Err(error) if error.code == HandErrorCode::OperationUnknown => {
                        let unknown = vec![(
                            resident.st.take_seq(),
                            Record::ManagedCallUnknown {
                                turn: call.turn.clone(),
                                call: call.call.clone(),
                                request_digest: call.envelope.request_digest.to_string(),
                            },
                        )];
                        commit(brain, session_id, &mut resident.st, unknown).await?;
                        reconcile_managed_unknown_default_sandbox(
                            brain,
                            session_id,
                            &mut resident.st,
                        )
                        .await?;
                        recovered.push((
                            call.clone(),
                            None,
                            crate::turn::managed_unknown_call_outcome(&call.name),
                            None,
                        ));
                        continue 'managed_calls;
                    }
                    Err(error) => return Err(map_hand_port_error(error)),
                }
            };
            crate::turn::verify_managed_operation(
                &receipt.operation,
                &call.call,
                call.envelope.request_digest.as_str(),
                session_id,
                &resident.st.head,
            )?;
            crate::turn::verify_managed_observation(&receipt.observation, &receipt.operation)?;
            accepted_now = true;
            (receipt.operation, receipt.observation)
        };

        if accepted_now {
            if let Some(target) = &observation.target {
                resident.st.head.default_sandbox = Some(managed_operation_running_status(
                    &operation,
                    target.expires_at_ms,
                )?);
            }
            let accepted = vec![(
                resident.st.take_seq(),
                Record::ManagedCallAccepted {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    operation: operation.clone(),
                },
            )];
            commit(brain, session_id, &mut resident.st, accepted).await?;
        }

        crate::turn::verify_managed_observation(&observation, &operation)?;
        if observation.state != OperationState::Terminal {
            if crate::wall_ms() >= call.envelope.deadline_at_ms.get() {
                let cancel: brain_protocol::hand::CancelRequest =
                    serde_json::from_value(serde_json::json!({
                        "operation": operation,
                        "reason": "recovery_deadline_elapsed",
                    }))?;
                match hand.cancel(cancel).await {
                    Ok(_) => {}
                    Err(error) if error.code == HandErrorCode::SandboxGone => {
                        sandbox_gone = Some(managed_operation_gone_status(&operation)?);
                    }
                    Err(error) => return Err(map_hand_port_error(error)),
                }
            }
            let request: brain_protocol::hand::ObserveRequest =
                serde_json::from_value(serde_json::json!({
                    "operation": operation,
                    "cursor": observation.next_cursor,
                    "wait_ms": binding.limits.max_wait_ms.min(30_000),
                }))?;
            match tokio::time::timeout(
                Duration::from_millis(request.wait_ms.saturating_add(1_000).max(1)),
                hand.observe(request),
            )
            .await
            .map_err(|_| {
                BrainError::HandUnavailable("managed Tool recovery observation timed out".into())
            })? {
                Ok(next) => observation = next,
                Err(error) if error.code == HandErrorCode::SandboxGone => {
                    sandbox_gone = Some(managed_operation_gone_status(&operation)?);
                    recovered.push((
                        call,
                        Some(operation),
                        CallOutcome {
                            outcome: TerminalOutcome::Interrupted,
                            value: None,
                            content: "managed Tool target disappeared before its durable terminal could be recovered".into(),
                            is_error: true,
                            exit_code: None,
                            duration_ms: 0,
                            truncated: false,
                            terminal: None,
                        },
                        None,
                    ));
                    continue;
                }
                Err(error) => return Err(map_hand_port_error(error)),
            }
            crate::turn::verify_managed_observation(&observation, &operation)?;
        }
        if observation.state != OperationState::Terminal {
            return Err(BrainError::HandUnavailable(
                "managed Tool remains in progress during durable recovery".into(),
            ));
        }
        let terminal = observation.terminal.ok_or_else(|| {
            BrainError::Protocol(
                "managed Hand reported terminal state without a terminal receipt".into(),
            )
        })?;
        let (outcome, terminal_digest) = crate::turn::managed_terminal_call_outcome(terminal)?;
        recovered.push((call, Some(operation), outcome, Some(terminal_digest)));
    }

    let mut records = Vec::new();
    let mut terminal = None;
    for (call, operation, outcome, terminal_digest) in recovered {
        let tool = tools.iter().find(|tool| tool.name == call.name);
        let outcome = crate::tools::enforce_outcome(tool, &call.name, outcome);
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content.clone()
        };
        records.push((
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn.clone(),
                agent: "root".into(),
                call: call.call.clone(),
                name: call.name.clone(),
                outcome: crate::events::tool_outcome(outcome.outcome),
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        ));
        if let Some(terminal_digest) = terminal_digest {
            let operation = operation.ok_or_else(|| {
                BrainError::Journal(
                    "managed terminal receipt has no accepted operation reference".into(),
                )
            })?;
            let pending = crate::journal::ManagedTerminalAckDoc {
                turn: call.turn.clone(),
                call: call.call.clone(),
                operation: operation.clone(),
                terminal_digest: terminal_digest.clone(),
            };
            if !resident
                .st
                .head
                .pending_managed_acks
                .iter()
                .any(|current| current.call == pending.call)
            {
                resident.st.head.pending_managed_acks.push(pending);
            }
            records.push((
                resident.st.take_seq(),
                Record::ManagedTerminalReceived {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    operation,
                    terminal_digest,
                },
            ));
        }
        if terminal.is_none() {
            terminal = outcome.terminal.map(|value| (call.call, call.name, value));
        }
    }
    if let Some(status) = sandbox_gone {
        resident.st.head.default_sandbox = Some(status.clone());
        records.push((
            resident.st.take_seq(),
            Record::DefaultSandboxChanged { status },
        ));
    }
    if let Some((call, name, terminal)) = terminal {
        let turn = active_turn.expect("pending managed call required an active turn");
        resident.st.head.state = resident.st.head.lifecycle_after_turn();
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.provider_attempt = None;
        resident.st.head.active_context.clear();
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        match terminal {
            TurnTerminal::Complete { value, metadata } => records.push((
                resident.st.take_seq(),
                Record::TurnCompleted {
                    turn: turn.clone(),
                    stop_reason: TurnStopReason::EndTurn,
                    rounds,
                    tool_calls,
                    result: Some(brain_protocol::session::TurnResult {
                        call_id: call.parse().map_err(|error| {
                            BrainError::Protocol(format!("managed call id: {error}"))
                        })?,
                        metadata,
                        name,
                        value,
                    }),
                },
            )),
            TurnTerminal::Fail { error } => records.push((
                resident.st.take_seq(),
                Record::TurnFailed {
                    turn: turn.clone(),
                    code: error.code.to_string(),
                    message: error.message,
                    details: error.details,
                },
            )),
        }
        records.push((
            resident.st.take_seq(),
            Record::State {
                state: resident.st.head.state,
                turn: None,
            },
        ));
    } else {
        resident.st.head.active_phase = Some(TurnPhase::ReadyToContinueModel);
    }
    commit(brain, session_id, &mut resident.st, records).await?;
    let _ = recover_managed_terminal_acks(brain, session_id, resident).await;
    Ok(true)
}

/// Reconcile the rooted default target after Hand reports that Submit may have reached the guest
/// but no operation receipt can be recovered. The durable `ManagedCallUnknown` marker is always
/// committed by the caller first, so this routine may retry status/dematerialization freely but
/// must never authorize another Submit.
pub(crate) async fn reconcile_managed_unknown_default_sandbox(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    use brain_protocol::hand::{HandErrorCode, SandboxState};

    let target = st
        .head
        .default_sandbox
        .as_ref()
        .map(|status| status.target.clone())
        .unwrap_or(default_sandbox_target(&st.head.root_id)?);
    let files = brain.sandbox_files.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "managed Tool unknown-outcome status reconciliation is unavailable".into(),
        )
    })?;
    let mut status = match files.status(target.clone()).await {
        Ok(status) => status,
        Err(error)
            if matches!(
                error.code,
                HandErrorCode::SandboxGone | HandErrorCode::SandboxNotMaterialized
            ) =>
        {
            let current = st
                .head
                .default_sandbox
                .clone()
                .unwrap_or(initial_default_sandbox(&st.head.root_id)?);
            sandbox_gone_status(&current, "managed_operation_target_gone")?
        }
        Err(error) if error.retryable => {
            return Err(BrainError::HandUnavailable(error.message.to_string()));
        }
        Err(error) => return Err(map_hand_port_error(error)),
    };
    if !sandbox_status_matches_target(&status, &target)? {
        return Err(BrainError::Protocol(
            "managed Tool unknown-outcome status references a different default target".into(),
        ));
    }
    if matches!(status.state, SandboxState::NeverMaterialized) {
        status = sandbox_gone_status(&status, "managed_operation_target_not_materialized")?;
    }
    if matches!(
        status.state,
        SandboxState::Running | SandboxState::Suspended
    ) && (status.generation.is_none()
        || status.target_ref.is_none()
        || status.expires_at_ms.is_none())
    {
        return Err(BrainError::Protocol(
            "managed Tool unknown-outcome status lacks generation, target_ref, or expiry".into(),
        ));
    }
    if !sandbox_status_releases_slot(&status) {
        let mut value = serde_json::to_value(&status)?;
        value["reason"] = serde_json::Value::String(MANAGED_UNKNOWN_SANDBOX_REASON.into());
        value["changed_at_ms"] = serde_json::Value::from(crate::wall_ms());
        status = serde_json::from_value(value)?;
    }
    st.head.default_sandbox = Some(status.clone());
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::DefaultSandboxChanged {
                status: status.clone(),
            },
        )],
    )
    .await?;
    if sandbox_status_releases_slot(&status) {
        return Ok(());
    }

    let preparation = brain.session_preparation.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "managed Tool unknown-outcome dematerialization is unavailable".into(),
        )
    })?;
    let terminal = match preparation.dematerialize_default(target.clone()).await {
        Ok(status) => status,
        Err(error)
            if matches!(
                error.code,
                HandErrorCode::SandboxGone | HandErrorCode::SandboxNotMaterialized
            ) =>
        {
            sandbox_gone_status(&status, "managed_operation_target_gone")?
        }
        Err(error) if error.retryable => {
            return Err(BrainError::HandUnavailable(error.message.to_string()));
        }
        Err(error) => return Err(map_hand_port_error(error)),
    };
    if !sandbox_status_matches_target(&terminal, &target)? {
        return Err(BrainError::Protocol(
            "managed Tool unknown-outcome dematerialization references a different target".into(),
        ));
    }
    if !sandbox_status_releases_slot(&terminal) {
        return Err(BrainError::HandUnavailable(
            "managed Tool unknown-outcome target has not reached a terminal sandbox state".into(),
        ));
    }
    st.head.default_sandbox = Some(terminal.clone());
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(seq, Record::DefaultSandboxChanged { status: terminal })],
    )
    .await
}

pub(super) async fn recover_external_calls(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    entries: &[Entry],
) -> Result<Option<RecoveredTurn>> {
    let Some(active_turn) = resident.st.head.turn.clone() else {
        return Ok(None);
    };
    let pending: Vec<_> = pending_external(entries, &resident.st.head.prefix)
        .into_iter()
        .filter(|call| call.turn == active_turn)
        .collect();
    let rounds = resident.st.head.active_rounds;
    let tool_calls = resident.st.head.active_tool_calls;
    let context = resident.st.head.active_context.clone();
    if pending.is_empty() {
        return Ok(Some(RecoveredTurn {
            turn: active_turn,
            context,
            rounds,
            tool_calls,
        }));
    }
    let sealed_tools = crate::tools::resolve(&resident.st.head.prefix.tools)?;
    let mut records = Vec::with_capacity(pending.len() + 2);
    let mut blocks = Vec::with_capacity(pending.len());
    let mut terminal = None;
    let mut unreplayable = false;

    for call in pending {
        let outcome =
            if call.policy.effect == brain_protocol::session::ExternalToolEffect::ReplaySafe {
                crate::turn::execute_external(
                    brain.external_executor.clone(),
                    call.policy,
                    call.parallel_batch,
                    session_id.to_string(),
                    call.turn.clone(),
                    "root".into(),
                    call.call.clone(),
                    call.name.clone(),
                    call.input,
                    call.context,
                    CancellationToken::new(),
                )
                .await
            } else {
                unreplayable = true;
                CallOutcome {
                    outcome: TerminalOutcome::Interrupted,
                    value: None,
                    content: format!(
                        "external tool {} was interrupted and its opaque effect was not replayed",
                        call.name
                    ),
                    is_error: true,
                    exit_code: None,
                    duration_ms: 0,
                    truncated: false,
                    terminal: None,
                }
            };
        let tool = sealed_tools.iter().find(|tool| tool.name == call.name);
        let outcome = match tool {
            Some(tool) => crate::tools::enforce_outcome(Some(tool), &call.name, outcome),
            None => crate::tools::enforce_outcome(
                None,
                &call.name,
                CallOutcome::failed(format!(
                    "tool {} is absent from the recovered execution seal",
                    call.name
                )),
            ),
        };
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content.clone()
        };
        blocks.push(ContentBlock::ToolResult {
            tool_use_id: call.call.clone(),
            content: content.clone(),
            is_error: outcome.is_error,
        });
        records.push((
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn,
                agent: "root".into(),
                call: call.call.clone(),
                name: call.name.clone(),
                outcome: crate::events::tool_outcome(outcome.outcome),
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        ));
        if let Some(value) = outcome.terminal {
            terminal = Some((call.call, call.name, value));
        }
    }

    if let Some((call, name, terminal)) = terminal {
        resident.st.head.state = resident.st.head.lifecycle_after_turn();
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.provider_attempt = None;
        resident.st.head.active_context.clear();
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        match terminal {
            TurnTerminal::Complete { value, metadata } => {
                records.push((
                    resident.st.take_seq(),
                    Record::TurnCompleted {
                        turn: active_turn.clone(),
                        stop_reason: TurnStopReason::EndTurn,
                        rounds,
                        tool_calls,
                        result: Some(brain_protocol::session::TurnResult {
                            call_id: call.parse().map_err(|error| {
                                BrainError::Protocol(format!("external call id: {error}"))
                            })?,
                            metadata,
                            name,
                            value,
                        }),
                    },
                ));
            }
            TurnTerminal::Fail { error } => records.push((
                resident.st.take_seq(),
                Record::TurnFailed {
                    turn: active_turn.clone(),
                    code: error.code.to_string(),
                    message: error.message,
                    details: error.details,
                },
            )),
        }
        records.push((
            resident.st.take_seq(),
            Record::State {
                state: resident.st.head.state,
                turn: None,
            },
        ));
    } else if unreplayable {
        resident.st.head.state = resident.st.head.lifecycle_after_turn();
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.provider_attempt = None;
        resident.st.head.active_context.clear();
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        records.push((
            resident.st.take_seq(),
            Record::TurnFailed {
                turn: active_turn.clone(),
                code: "cancelled".into(),
                message:
                    "turn interrupted with an opaque external-tool effect; it was not replayed"
                        .into(),
                details: None,
            },
        ));
        records.push((
            resident.st.take_seq(),
            Record::State {
                state: resident.st.head.state,
                turn: None,
            },
        ));
    }

    commit(brain, session_id, &mut resident.st, records).await?;
    resident.st.history.push(Message::tool_results(blocks));
    if resident.st.head.turn.is_none() {
        Ok(None)
    } else {
        Ok(Some(RecoveredTurn {
            turn: active_turn,
            context,
            rounds,
            tool_calls,
        }))
    }
}

/// Resolve the only ambiguous provider phase before a recovered turn is driven. Providers in the
/// MVP do not expose a durable retrieval handle, so an unfinished intent becomes UNKNOWN. A new
/// attempt is permitted only by the sealed crash-recovery budget; strict zero interrupts.
pub(super) async fn recover_provider_attempt(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    let Some(turn) = resident.st.head.turn.clone() else {
        return Ok(());
    };
    let Some(mut attempt) = resident.st.head.provider_attempt.clone() else {
        resident.st.head.active_phase = Some(TurnPhase::ReadyToBuildModelRequest);
        commit(brain, session_id, &mut resident.st, vec![]).await?;
        return Ok(());
    };
    let is_compaction = attempt.logical_operation_id.starts_with("cmp_");

    let mut records = Vec::new();
    if matches!(attempt.state.as_str(), "intent" | "running") {
        records.push((
            resident.st.take_seq(),
            if is_compaction {
                Record::CompactionUnknown {
                    turn: turn.clone(),
                    logical_operation_id: attempt.logical_operation_id.clone(),
                    attempt_id: attempt.attempt_id.clone(),
                    request_digest: attempt.request_digest.clone(),
                    possibly_duplicated: true,
                }
            } else {
                Record::ModelCallUnknown {
                    turn: turn.clone(),
                    logical_operation_id: attempt.logical_operation_id.clone(),
                    attempt_id: attempt.attempt_id.clone(),
                    request_digest: attempt.request_digest.clone(),
                    possibly_duplicated: true,
                }
            },
        ));
        attempt.state = ProviderAttemptState::Unknown;
    }
    if attempt.state == ProviderAttemptState::ReplacementReady {
        resident.st.head.provider_attempt = Some(attempt);
        resident.st.head.active_phase = Some(if is_compaction {
            TurnPhase::ReadyToCompact
        } else {
            TurnPhase::ReadyToBuildModelRequest
        });
        commit(brain, session_id, &mut resident.st, records).await?;
        return Ok(());
    }
    if attempt.state != ProviderAttemptState::Unknown {
        return Err(BrainError::Journal(format!(
            "active provider attempt has invalid state {}",
            attempt.state.as_str()
        )));
    }

    if attempt.replacements_used < resident.st.head.prefix.provider_recovery_retries {
        attempt.replacements_used += 1;
        attempt.state = ProviderAttemptState::ReplacementReady;
        resident.st.head.provider_attempt = Some(attempt);
        resident.st.head.active_phase = Some(if is_compaction {
            TurnPhase::ReadyToCompact
        } else {
            TurnPhase::ReadyToBuildModelRequest
        });
        commit(brain, session_id, &mut resident.st, records).await?;
        return Ok(());
    }

    resident.st.head.state = resident.st.head.lifecycle_after_turn();
    resident.st.head.turn = None;
    resident.st.head.active_phase = None;
    resident.st.head.provider_attempt = None;
    resident.st.head.active_context.clear();
    let rounds = resident.st.head.active_rounds;
    let tool_calls = resident.st.head.active_tool_calls;
    resident.st.head.active_rounds = 0;
    resident.st.head.active_tool_calls = 0;
    records.push((
        resident.st.take_seq(),
        Record::TurnCompleted {
            turn: turn.clone(),
            stop_reason: TurnStopReason::Interrupted,
            rounds,
            tool_calls,
            result: None,
        },
    ));
    records.push((
        resident.st.take_seq(),
        Record::State {
            state: resident.st.head.state,
            turn: None,
        },
    ));
    commit(brain, session_id, &mut resident.st, records).await
}

pub(super) async fn recover_customer_terminal_acks(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    let Some(customer) = &brain.customer else {
        return if resident.st.head.pending_customer_acks.is_empty() {
            Ok(())
        } else {
            Err(BrainError::HandUnavailable(
                "customer coordinator is unavailable for a durable terminal acknowledgement".into(),
            ))
        };
    };
    let pending = resident.st.head.pending_customer_acks.clone();
    let mut acknowledged = Vec::new();
    for item in pending {
        let receipt = crate::customer::CustomerTerminalReceipt {
            operation_id: item.call.clone(),
            request_digest: item.request_digest.clone(),
            terminal_digest: item.terminal_digest.clone(),
            process_id: item.process_id.clone(),
        };
        match customer
            .acknowledge_durable_terminal(&resident.st.head.tenant_id, &item.client_id, &receipt)
            .await
        {
            Ok(()) => acknowledged.push(item),
            Err(error) => tracing::warn!(
                session = session_id,
                operation = %receipt.operation_id,
                error = %error,
                "durable customer terminal acknowledgement remains pending"
            ),
        }
    }
    if !acknowledged.is_empty() {
        let records = acknowledged
            .iter()
            .map(|item| {
                (
                    resident.st.take_seq(),
                    Record::CustomerTerminalAcknowledged {
                        turn: item.turn.clone(),
                        call: item.call.clone(),
                        request_digest: item.request_digest.clone(),
                        terminal_digest: item.terminal_digest.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        resident.st.head.pending_customer_acks.retain(|current| {
            !acknowledged.iter().any(|item| {
                current.call == item.call
                    && current.request_digest == item.request_digest
                    && current.terminal_digest == item.terminal_digest
            })
        });
        commit(brain, session_id, &mut resident.st, records).await?;
    }
    if resident.st.head.pending_customer_acks.is_empty() {
        Ok(())
    } else {
        Err(BrainError::HandUnavailable(
            "customer terminal acknowledgement remains pending".into(),
        ))
    }
}

pub(super) async fn recover_managed_terminal_acks(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    let Some(hand) = &brain.hand else {
        return if resident.st.head.pending_managed_acks.is_empty() {
            Ok(())
        } else {
            Err(BrainError::HandUnavailable(
                "managed Hand is unavailable for a durable terminal acknowledgement".into(),
            ))
        };
    };
    let pending = resident.st.head.pending_managed_acks.clone();
    let mut acknowledged = Vec::new();
    for item in pending {
        let terminal_digest = item.terminal_digest.parse().map_err(|error| {
            BrainError::Protocol(format!("persisted managed terminal digest: {error}"))
        })?;
        let request = brain_protocol::hand::AcknowledgeTerminalRequest {
            operation: item.operation.clone(),
            terminal_digest,
        };
        match hand.acknowledge_terminal(request).await {
            Ok(ack) if ack.acknowledged => acknowledged.push(item),
            Ok(_) => tracing::warn!(
                session = session_id,
                operation = item.operation.operation_id.as_str(),
                "durable managed terminal acknowledgement was not accepted"
            ),
            Err(error) => tracing::warn!(
                session = session_id,
                operation = item.operation.operation_id.as_str(),
                code = %error.code,
                "durable managed terminal acknowledgement remains pending"
            ),
        }
    }
    if !acknowledged.is_empty() {
        let records = acknowledged
            .iter()
            .map(|item| {
                (
                    resident.st.take_seq(),
                    Record::ManagedTerminalAcknowledged {
                        turn: item.turn.clone(),
                        call: item.call.clone(),
                        request_digest: item.operation.request_digest.to_string(),
                        terminal_digest: item.terminal_digest.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        resident.st.head.pending_managed_acks.retain(|current| {
            !acknowledged.iter().any(|item| {
                current.operation.operation_id == item.operation.operation_id
                    && current.operation.request_digest == item.operation.request_digest
                    && current.terminal_digest == item.terminal_digest
            })
        });
        commit(brain, session_id, &mut resident.st, records).await?;
    }
    if resident.st.head.pending_managed_acks.is_empty() {
        Ok(())
    } else {
        Err(BrainError::HandUnavailable(
            "managed terminal acknowledgement remains pending".into(),
        ))
    }
}
