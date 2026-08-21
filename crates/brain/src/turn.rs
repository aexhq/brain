//! The turn loop: model round -> journal the decision -> dispatch tools -> journal results ->
//! feed back -> repeat, until the model stops or a cap fires.
//!
//! Rules the loop holds:
//! - only a COMPLETE assistant message is journaled; a stream that dies mid-message leaves no
//!   trace in history;
//! - tool intents are journaled BEFORE dispatch (an ambiguous outcome is recorded as
//!   possibly-run) and managed results are journaled before the typed Hand terminal ACK lets the
//!   substrate forget them;
//! - the error flag on a failed tool result is always set (a dropped flag turns a failure
//!   into a success in the model's eyes);
//! - a lost substrate is reconciled through the durable operation receipt and rooted target;
//! - cancellation is graceful: adapters get the token, results journal `cancelled`, the turn
//!   completes with `stop_reason = cancelled`.
//!
//! WHERE arbitrary user code runs is not this module's business: managed dispatch goes through
//! the transport-neutral typed Hand receipt port.

use crate::adapter::{CallOutcome, TerminalOutcome, ToolExecutor};
use crate::config::{SealedPrefix, SessionConfig, ToolRoute};
use crate::events::EventHub;
use crate::journal::{self, HeadDoc, Journal, Lease, Record};
use crate::message::{ContentBlock, Message, StopReason};
use crate::provider::{Accumulator, Provider, ProviderEvent};
use crate::{BrainError, Result, Shared};
use base64::Engine as _;
use brain_protocol::session::EventStream;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Keeps the assistant decision and worst-case bounded Tool results within one journal
/// transaction, independently of the provider's own Tool-call limit.
pub const MAX_TOOL_CALLS_PER_MODEL_ROUND: usize = 24;

fn validate_model_tool_call_count(count: usize) -> Result<()> {
    if count > MAX_TOOL_CALLS_PER_MODEL_ROUND {
        return Err(BrainError::Protocol(format!(
            "provider returned {count} Tool calls in one round; maximum is {MAX_TOOL_CALLS_PER_MODEL_ROUND}"
        )));
    }
    Ok(())
}

/// Everything a turn borrows from the session for its lifetime. The actor moves this in and
/// gets it back when the turn future resolves, mutated.
pub struct TurnState {
    pub history: Vec<Message>,
    pub head: HeadDoc,
    /// Last HEAD returned by a successful journal decision. Mutations are staged in `head`; a
    /// conditional or adapter commit failure restores this local snapshot without depending on
    /// another backend read. This prevents a rejected intent from becoming a resident fast path.
    pub persisted_head: HeadDoc,
    pub lease: Lease,
    /// The next seq to allocate. Ephemeral events (deltas, tool output) consume seqs too;
    /// every commit persists the high-water mark.
    pub seq: Seq,
}

/// The session's sequence allocator.
pub type Seq = Arc<AtomicU64>;

impl TurnState {
    pub fn take_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }
}

/// The immutable turn context.
pub struct TurnRun {
    /// Weak back-reference used only by the three closed engine capabilities. It cannot keep a
    /// Brain process alive and keeps their typed state-machine ports out of provider/Hand adapters.
    pub engine: Weak<crate::session::Brain>,
    pub session_id: String,
    pub turn_id: String,
    pub prefix: Shared<SealedPrefix>,
    pub session: SessionConfig,
    pub provider: Arc<dyn Provider>,
    pub provider_name: String,
    pub outbound: crate::outbound::Outbound,
    pub journal: Journal,
    pub hub: Arc<EventHub>,
    pub cancel: CancellationToken,
    /// Bounds concurrent model rounds across the whole Brain.
    pub model_permits: Arc<Semaphore>,
    pub context_soft_tokens: usize,
    pub context_hard_tokens: usize,
    pub context_tail_tokens: usize,
    pub context_summary_tokens: usize,
    pub context_window_tokens: usize,
    pub provider_header_timeout: std::time::Duration,
    pub provider_idle_timeout: std::time::Duration,
    pub provider_total_timeout: std::time::Duration,
    pub compactor: Arc<dyn crate::compact::CompactionPort>,
    pub external_executor: Arc<dyn ToolExecutor>,
    pub hand: Option<Arc<dyn crate::hand::HandPort>>,
    pub managed_bindings: Arc<HashMap<String, brain_protocol::hand::ResolvedBinding>>,
    pub customer: Option<Arc<crate::customer::CustomerCoordinator>>,
    pub tenant_id: String,
    pub customer_client_id: Option<String>,
    pub customer_submit_retries: u32,
    pub customer_timeout: std::time::Duration,
    /// Trusted metadata journaled with this message and forwarded only to host executors.
    pub context: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct TurnReport {
    pub stop_reason: String,
    pub rounds: u64,
    pub tool_calls: u64,
    /// True when the tool-result decision also journaled the turn terminal atomically.
    pub terminal_committed: bool,
}

struct RootRound {
    message: Message,
    stop: StopReason,
    usage: crate::message::Usage,
    logical_operation_id: String,
    attempt_id: String,
    request_digest: String,
}

struct CompletedCompaction {
    result: crate::compact::CompactionResult,
    logical_operation_id: String,
    attempt_id: String,
    request_digest: String,
}

struct DispatchedOutcome {
    outcome: CallOutcome,
    customer_terminal: Option<crate::customer::CustomerTerminalReceipt>,
    managed_terminal: Option<ManagedTerminalReceipt>,
}

#[derive(Clone)]
struct ManagedTerminalReceipt {
    operation: brain_protocol::hand::OperationRef,
    terminal_digest: String,
}

#[derive(Clone)]
enum PreparedCustomerDispatch {
    Intent(crate::customer::CustomerOperationIntent),
    Failure(CallOutcome),
}

impl From<CallOutcome> for DispatchedOutcome {
    fn from(outcome: CallOutcome) -> Self {
        Self {
            outcome,
            customer_terminal: None,
            managed_terminal: None,
        }
    }
}

impl TurnRun {
    /// Publishes the SSE events for freshly committed records. Call AFTER the commit: an event
    /// a client saw must exist in the journal.
    fn publish_records(&self, records: &[(u64, Record)]) {
        let now = crate::wall_ms();
        for (seq, record) in records {
            if let Some(e) = crate::events::derive(&self.session_id, *seq, now, record) {
                self.hub.publish(&self.session_id, e);
            }
        }
    }

    async fn commit(&self, st: &mut TurnState, records: Vec<(u64, Record)>) -> Result<()> {
        st.head.updated_ms = crate::wall_ms();
        let high_water = st.seq.load(Ordering::Relaxed).saturating_sub(1);
        st.head.last_seq = high_water;
        let mut lease = st.lease.clone();
        let persisted = match self
            .journal
            .commit(&self.session_id, &mut lease, &records, &st.head, high_water)
            .await
        {
            Ok(persisted) => persisted,
            Err(error) => {
                st.head = st.persisted_head.clone();
                return Err(error);
            }
        };
        st.lease = lease;
        st.persisted_head = persisted.clone();
        st.head = persisted;
        self.publish_records(&records);
        Ok(())
    }

    /// The whole turn. The admitted user message and `turn_started` are already committed by
    /// the actor; `st.history` already carries the user message.
    pub async fn run(&self, st: &mut TurnState) -> Result<TurnReport> {
        self.resume(st, 0, 0).await
    }

    /// Continue a journaled turn after replay-safe host-tool recovery. The counters are derived
    /// from already committed root assistant/tool-call records so the eventual terminal event
    /// remains cumulative across process ownership changes.
    pub(crate) async fn resume(
        &self,
        st: &mut TurnState,
        rounds: u64,
        tool_calls: u64,
    ) -> Result<TurnReport> {
        let report = self.run_work_from(st, rounds, tool_calls).await?;
        if report.terminal_committed {
            return Ok(report);
        }
        self.complete(st, &report.stop_reason, report.rounds, report.tool_calls)
            .await
    }

    /// Execute through the final assistant answer without necessarily committing the turn
    /// terminal. Host-executed terminal tools commit their tool result and terminal state in one
    /// decision; ordinary messages finish through [`Self::run`].
    pub async fn run_work(&self, st: &mut TurnState) -> Result<TurnReport> {
        self.run_work_from(st, 0, 0).await
    }

    async fn run_work_from(
        &self,
        st: &mut TurnState,
        mut rounds: u64,
        mut tool_calls: u64,
    ) -> Result<TurnReport> {
        let max_rounds = self.prefix.limits.max_rounds as u64;

        loop {
            let preflight_request = self.provider.build_request(
                &self.prefix,
                &st.history,
                &self.session.key,
                &self.session.base_url,
            )?;
            let preflight = crate::compact::validate_model_request_budget(
                preflight_request.body_len(),
                self.prefix.sampling.max_tokens as usize,
                self.context_window_tokens,
                "model",
            );
            let force_compaction = preflight.is_err();
            if let Some(plan) = crate::compact::plan(
                &st.history,
                st.head.context.is_some(),
                self.context_soft_tokens,
                self.context_tail_tokens,
                (self.context_hard_tokens / 4).max(1_024),
                force_compaction,
            ) {
                let request = crate::compact::CompactionRequest {
                    previous_summary: plan.previous_summary.clone(),
                    new_span: plan.new_span.clone(),
                    max_output_tokens: u32::try_from(self.context_summary_tokens).map_err(
                        |_| BrainError::Journal("sealed context summary limit exceeds u32".into()),
                    )?,
                };
                let Some(completed) = self.run_compaction(st, request).await? else {
                    return Ok(TurnReport {
                        stop_reason: "interrupted".into(),
                        rounds,
                        tool_calls,
                        terminal_committed: false,
                    });
                };
                let semantic = crate::compact::finish_plan(
                    plan,
                    completed.result.clone(),
                    self.context_summary_tokens,
                )?;
                self.install_context_checkpoint(st, semantic, completed)
                    .await?;
                // A large single turn may need several bounded compactions. Re-evaluate both
                // the semantic threshold and the exact provider wire after every installation.
                continue;
            }
            preflight?;
            let projected = crate::compact::estimate_tokens(&st.history);
            if projected > self.context_hard_tokens {
                return Err(BrainError::Protocol(format!(
                    "model conversation history is estimated at {projected} tokens after compaction and exceeds the sealed hard limit {}",
                    self.context_hard_tokens
                )));
            }

            if rounds >= max_rounds {
                return Ok(TurnReport {
                    stop_reason: "max_rounds".into(),
                    rounds,
                    tool_calls,
                    terminal_committed: false,
                });
            }

            // One model round.
            let round = loop {
                match self.root_round(st).await {
                    Ok(value) => break value,
                    Err(BrainError::Cancelled) => {
                        return Ok(TurnReport {
                            stop_reason: "cancelled".into(),
                            rounds,
                            tool_calls,
                            terminal_committed: false,
                        });
                    }
                    Err(error)
                        if st
                            .head
                            .provider_attempt
                            .as_ref()
                            .is_some_and(|attempt| attempt.state == "unknown") =>
                    {
                        let attempt = st.head.provider_attempt.as_mut().expect("checked");
                        if attempt.replacements_used >= st.head.prefix.provider_recovery_retries {
                            return Ok(TurnReport {
                                stop_reason: "interrupted".into(),
                                rounds,
                                tool_calls,
                                terminal_committed: false,
                            });
                        }
                        tracing::warn!(
                            session = %self.session_id,
                            logical_operation_id = %attempt.logical_operation_id,
                            error = %error,
                            "provider outcome unknown; authorizing digest-identical replacement"
                        );
                        attempt.replacements_used += 1;
                        attempt.state = "replacement_ready".into();
                        st.head.active_phase = Some("ready_to_build_model_request".into());
                        // Persist the consumed recovery budget before the replacement send. A
                        // crash here resumes this ready attempt without consuming it twice.
                        self.commit(st, vec![]).await?;
                    }
                    Err(error) => return Err(error),
                }
            };
            let RootRound {
                mut message,
                stop,
                usage,
                logical_operation_id,
                attempt_id,
                request_digest,
            } = round;
            rounds += 1;
            st.head.active_rounds = rounds;

            // Journal the decision: the complete assistant message and every tool intent --
            // one durable write, BEFORE dispatch.
            let calls = mint_tool_calls(&mut message);
            validate_model_tool_call_count(calls.len())?;
            let mut prepared_customer = HashMap::new();
            for (operation_id, name, input) in &calls {
                let Some(tool) = self.prefix.tool(name) else {
                    continue;
                };
                let ToolRoute::Customer { registration } = &tool.route else {
                    continue;
                };
                let prepared = match (&self.customer, &self.customer_client_id) {
                    (Some(customer), Some(client_id)) => {
                        let deadline_at_ms = crate::wall_ms().saturating_add(
                            self.customer_timeout.as_millis().min(u64::MAX as u128) as u64,
                        );
                        match customer
                            .prepare_operation(
                                &self.tenant_id,
                                client_id,
                                &self.session_id,
                                operation_id,
                                registration,
                                name,
                                &tool.contract_digest,
                                input.clone(),
                                deadline_at_ms,
                            )
                            .await
                        {
                            Ok(intent) => PreparedCustomerDispatch::Intent(intent),
                            Err(error) => PreparedCustomerDispatch::Failure(
                                customer_preparation_failure(error),
                            ),
                        }
                    }
                    _ => PreparedCustomerDispatch::Failure(CallOutcome::failed(
                        "customer application transport is unavailable",
                    )),
                };
                prepared_customer.insert(operation_id.clone(), prepared);
            }

            let mut records = Vec::new();
            records.push((
                st.take_seq(),
                Record::ModelCallCompleted {
                    turn: self.turn_id.clone(),
                    logical_operation_id,
                    attempt_id: attempt_id.clone(),
                    request_digest,
                },
            ));
            records.push((
                st.take_seq(),
                Record::Usage {
                    turn: self.turn_id.clone(),
                    agent: "root".into(),
                    provider: self.provider_name.clone(),
                    model: self.prefix.model.clone(),
                    usage,
                },
            ));
            let assistant_seq = st.take_seq();
            records.push((
                assistant_seq,
                Record::Assistant {
                    turn: self.turn_id.clone(),
                    agent: "root".into(),
                    attempt_id,
                    content: message.content.clone(),
                    stop,
                },
            ));
            for (op_id, name, input) in &calls {
                if let Some(PreparedCustomerDispatch::Intent(intent)) = prepared_customer.get(op_id)
                {
                    records.push((
                        st.take_seq(),
                        Record::CustomerCallIntent {
                            turn: self.turn_id.clone(),
                            call: op_id.clone(),
                            client_id: intent.client_id.clone(),
                            process_id: intent.process_id.clone(),
                            request_digest: intent.request_digest.clone(),
                            deadline_at_ms: intent.deadline_at_ms,
                        },
                    ));
                }
                records.push((
                    st.take_seq(),
                    Record::ToolCall {
                        turn: self.turn_id.clone(),
                        agent: "root".into(),
                        call: op_id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        detach: false,
                    },
                ));
            }
            st.head.provider_attempt = None;
            st.head.active_phase = Some(if calls.is_empty() {
                "ready_to_finish".into()
            } else {
                "ready_to_dispatch_tools".into()
            });
            self.commit(st, records).await?;
            st.history.push(message.clone());

            if calls.is_empty() {
                return Ok(TurnReport {
                    stop_reason: if stop == StopReason::Refusal {
                        "refusal".into()
                    } else {
                        "end_turn".into()
                    },
                    rounds,
                    tool_calls,
                    terminal_committed: false,
                });
            }
            tool_calls += calls.len() as u64;
            st.head.active_tool_calls = tool_calls;

            // Dispatch the batch and journal the results.
            let outcomes = self.dispatch_batch(st, &calls, &prepared_customer).await?;
            let mut result_records = Vec::new();
            let mut blocks = Vec::with_capacity(calls.len());
            for (i, dispatched) in outcomes.iter().enumerate() {
                let o = &dispatched.outcome;
                // Providers reject an EMPTY tool_result that carries is_error (the Anthropic
                // wire refuses it outright), and an empty success reads as ambiguity anyway:
                // say what happened.
                let content = if o.content.is_empty() {
                    format!("[{}: no output]", o.outcome)
                } else {
                    o.content.clone()
                };
                blocks.push(ContentBlock::ToolResult {
                    tool_use_id: calls[i].0.clone(),
                    content: content.clone(),
                    is_error: o.is_error,
                });
                result_records.push((
                    st.take_seq(),
                    Record::ToolResult {
                        turn: self.turn_id.clone(),
                        agent: "root".into(),
                        call: calls[i].0.clone(),
                        name: calls[i].1.clone(),
                        outcome: o.outcome.clone(),
                        content,
                        is_error: o.is_error,
                        exit_code: o.exit_code,
                        duration_ms: o.duration_ms,
                        truncated: o.truncated,
                    },
                ));
                if let Some(receipt) = &dispatched.customer_terminal {
                    let client_id = self.customer_client_id.clone().ok_or_else(|| {
                        BrainError::Protocol(
                            "customer terminal has no sealed client identity".into(),
                        )
                    })?;
                    let pending = journal::CustomerTerminalAckDoc {
                        turn: self.turn_id.clone(),
                        call: calls[i].0.clone(),
                        client_id: client_id.clone(),
                        process_id: receipt.process_id.clone(),
                        request_digest: receipt.request_digest.clone(),
                        terminal_digest: receipt.terminal_digest.clone(),
                    };
                    if !st
                        .head
                        .pending_customer_acks
                        .iter()
                        .any(|current| current.call == pending.call)
                    {
                        st.head.pending_customer_acks.push(pending);
                    }
                    result_records.push((
                        st.take_seq(),
                        Record::CustomerTerminalReceived {
                            turn: self.turn_id.clone(),
                            call: calls[i].0.clone(),
                            client_id,
                            process_id: receipt.process_id.clone(),
                            request_digest: receipt.request_digest.clone(),
                            terminal_digest: receipt.terminal_digest.clone(),
                        },
                    ));
                }
                if let Some(receipt) = &dispatched.managed_terminal {
                    let pending = journal::ManagedTerminalAckDoc {
                        turn: self.turn_id.clone(),
                        call: calls[i].0.clone(),
                        operation: receipt.operation.clone(),
                        terminal_digest: receipt.terminal_digest.clone(),
                    };
                    if !st
                        .head
                        .pending_managed_acks
                        .iter()
                        .any(|current| current.call == pending.call)
                    {
                        st.head.pending_managed_acks.push(pending);
                    }
                    result_records.push((
                        st.take_seq(),
                        Record::ManagedTerminalReceived {
                            turn: self.turn_id.clone(),
                            call: calls[i].0.clone(),
                            operation: receipt.operation.clone(),
                            terminal_digest: receipt.terminal_digest.clone(),
                        },
                    ));
                }
            }
            let terminal = outcomes.iter().enumerate().find_map(|(index, outcome)| {
                outcome.outcome.terminal.clone().map(|value| (index, value))
            });
            let terminal_report = if let Some((index, terminal)) = terminal {
                st.head.state = st.head.lifecycle_after_turn();
                st.head.turn = None;
                st.head.active_phase = None;
                st.head.provider_attempt = None;
                st.head.active_context.clear();
                st.head.active_rounds = 0;
                st.head.active_tool_calls = 0;
                match terminal {
                    TerminalOutcome::Complete { value, metadata } => {
                        let result = brain_protocol::session::TurnResult {
                            call_id: calls[index].0.parse().map_err(|error| {
                                BrainError::Protocol(format!("external call id: {error}"))
                            })?,
                            metadata,
                            name: calls[index].1.clone(),
                            value,
                        };
                        result_records.push((
                            st.take_seq(),
                            Record::TurnCompleted {
                                turn: self.turn_id.clone(),
                                stop_reason: "end_turn".into(),
                                rounds,
                                tool_calls,
                                result: Some(result),
                            },
                        ));
                        Some("end_turn")
                    }
                    TerminalOutcome::Fail { error } => {
                        result_records.push((
                            st.take_seq(),
                            Record::TurnFailed {
                                turn: self.turn_id.clone(),
                                code: error.code.to_string(),
                                message: error.message,
                                details: error.details,
                            },
                        ));
                        Some("error")
                    }
                }
            } else {
                st.head.active_phase = Some("ready_to_continue_model".into());
                None
            };
            if terminal_report.is_some() {
                result_records.push((
                    st.take_seq(),
                    Record::State {
                        state: st.head.state.clone(),
                        turn: None,
                    },
                ));
            }
            self.commit(st, result_records).await?;

            if let Some(hand) = &self.hand {
                let previous_pending = st.head.pending_managed_acks.clone();
                let mut acknowledged = Vec::new();
                for receipt in outcomes
                    .iter()
                    .filter_map(|outcome| outcome.managed_terminal.as_ref())
                {
                    let request = brain_protocol::hand::AcknowledgeTerminalRequest {
                        operation: receipt.operation.clone(),
                        terminal_digest: receipt.terminal_digest.parse().map_err(|error| {
                            BrainError::Protocol(format!("terminal digest: {error}"))
                        })?,
                    };
                    match hand.acknowledge_terminal(request).await {
                        Ok(ack) if ack.acknowledged => acknowledged.push(receipt.clone()),
                        Ok(_) => tracing::warn!(
                            session = %self.session_id,
                            operation = receipt.operation.operation_id.as_str(),
                            "managed terminal acknowledgement was not accepted"
                        ),
                        Err(error) => tracing::warn!(
                            session = %self.session_id,
                            operation = receipt.operation.operation_id.as_str(),
                            error = error.message.as_str(),
                            "managed terminal acknowledgement remains pending after durable commit"
                        ),
                    }
                }
                if !acknowledged.is_empty() {
                    let records = acknowledged
                        .iter()
                        .map(|receipt| {
                            (
                                st.take_seq(),
                                Record::ManagedTerminalAcknowledged {
                                    turn: self.turn_id.clone(),
                                    call: receipt.operation.operation_id.to_string(),
                                    request_digest: receipt.operation.request_digest.to_string(),
                                    terminal_digest: receipt.terminal_digest.clone(),
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    st.head.pending_managed_acks.retain(|pending| {
                        !acknowledged.iter().any(|receipt| {
                            pending.operation.operation_id == receipt.operation.operation_id
                                && pending.operation.request_digest
                                    == receipt.operation.request_digest
                                && pending.terminal_digest == receipt.terminal_digest
                        })
                    });
                    if let Err(error) = self.commit(st, records).await {
                        st.head.pending_managed_acks = previous_pending;
                        return Err(error);
                    }
                }
            }

            if let Some(customer) = &self.customer {
                let previous_pending = st.head.pending_customer_acks.clone();
                let mut acknowledged = Vec::new();
                for receipt in outcomes
                    .iter()
                    .filter_map(|outcome| outcome.customer_terminal.as_ref())
                {
                    match customer.acknowledge_terminal(receipt).await {
                        Ok(()) => acknowledged.push(receipt.clone()),
                        Err(error) => {
                            tracing::warn!(
                                session = %self.session_id,
                                operation = %receipt.operation_id,
                                error = %error,
                                "customer terminal acknowledgement remains pending after durable commit"
                            );
                        }
                    }
                }
                if !acknowledged.is_empty() {
                    let records = acknowledged
                        .iter()
                        .map(|receipt| {
                            (
                                st.take_seq(),
                                Record::CustomerTerminalAcknowledged {
                                    turn: self.turn_id.clone(),
                                    call: receipt.operation_id.clone(),
                                    request_digest: receipt.request_digest.clone(),
                                    terminal_digest: receipt.terminal_digest.clone(),
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    st.head.pending_customer_acks.retain(|pending| {
                        !acknowledged.iter().any(|receipt| {
                            pending.call == receipt.operation_id
                                && pending.request_digest == receipt.request_digest
                                && pending.terminal_digest == receipt.terminal_digest
                        })
                    });
                    if let Err(error) = self.commit(st, records).await {
                        st.head.pending_customer_acks = previous_pending;
                        return Err(error);
                    }
                }
            }
            st.history.push(Message::tool_results(blocks));

            if let Some(stop_reason) = terminal_report {
                return Ok(TurnReport {
                    stop_reason: stop_reason.into(),
                    rounds,
                    tool_calls,
                    terminal_committed: true,
                });
            }

            if self.cancel.is_cancelled() {
                return Ok(TurnReport {
                    stop_reason: "cancelled".into(),
                    rounds,
                    tool_calls,
                    terminal_committed: false,
                });
            }
        }
    }

    async fn complete(
        &self,
        st: &mut TurnState,
        stop_reason: &str,
        rounds: u64,
        tool_calls: u64,
    ) -> Result<TurnReport> {
        let seq = st.take_seq();
        let state_seq = st.take_seq();
        st.head.state = st.head.lifecycle_after_turn();
        st.head.turn = None;
        st.head.active_phase = None;
        st.head.provider_attempt = None;
        st.head.active_context.clear();
        st.head.active_rounds = 0;
        st.head.active_tool_calls = 0;
        self.commit(
            st,
            vec![
                (
                    seq,
                    Record::TurnCompleted {
                        turn: self.turn_id.clone(),
                        stop_reason: stop_reason.into(),
                        rounds,
                        tool_calls,
                        result: None,
                    },
                ),
                (
                    state_seq,
                    Record::State {
                        state: st.head.state.clone(),
                        turn: None,
                    },
                ),
            ],
        )
        .await?;
        Ok(TurnReport {
            stop_reason: stop_reason.into(),
            rounds,
            tool_calls,
            terminal_committed: true,
        })
    }

    async fn install_context_checkpoint(
        &self,
        st: &mut TurnState,
        plan: crate::compact::SemanticPlan,
        completed: CompletedCompaction,
    ) -> Result<()> {
        let encoded = crate::compact::encode_payload(&plan)?;
        let checkpoint_id = format!("ctx_{}", &encoded.digest[..24]);
        let covers_through_sequence = st.seq.load(Ordering::Relaxed).saturating_sub(1);
        let chunk_start_sequence = st.seq.load(Ordering::Relaxed);
        let total = u32::try_from(encoded.chunks_base64.len())
            .map_err(|_| BrainError::Journal("too many context chunks".into()))?;
        let mut records = Vec::with_capacity(encoded.chunks_base64.len() + 3);
        records.push((
            st.take_seq(),
            Record::CompactionCompleted {
                turn: self.turn_id.clone(),
                logical_operation_id: completed.logical_operation_id,
                attempt_id: completed.attempt_id,
                request_digest: completed.request_digest,
            },
        ));
        records.push((
            st.take_seq(),
            Record::Usage {
                turn: self.turn_id.clone(),
                agent: "compactor".into(),
                provider: completed.result.provider,
                model: completed.result.model,
                usage: completed.result.usage,
            },
        ));
        for (index, content_base64) in encoded.chunks_base64.into_iter().enumerate() {
            let content = base64::engine::general_purpose::STANDARD
                .decode(&content_base64)
                .map_err(|_| BrainError::Journal("generated context chunk is invalid".into()))?;
            records.push((
                st.take_seq(),
                Record::ContextChunk {
                    checkpoint_id: checkpoint_id.clone(),
                    index: index as u32,
                    total,
                    sha256: hex::encode(Sha256::digest(content)),
                    content_base64,
                },
            ));
        }
        let chunk_end_sequence = st.seq.load(Ordering::Relaxed).saturating_sub(1);
        let base_checkpoint_id = st
            .head
            .context
            .as_ref()
            .map(|context| context.checkpoint_id.clone());
        let context_generation = st
            .head
            .context
            .as_ref()
            .map_or(1, |context| context.context_generation + 1);
        let retained_from_sequence = st
            .head
            .context
            .as_ref()
            .map_or(1, |context| context.retained_from_sequence);
        let created_at_ms = crate::wall_ms();
        let pointer = crate::journal::ContextPointerDoc {
            checkpoint_id: checkpoint_id.clone(),
            base_checkpoint_id: base_checkpoint_id.clone(),
            covers_through_sequence,
            chunk_start_sequence,
            chunk_end_sequence,
            retained_messages: plan.retained_messages,
            payload_digest: encoded.digest.clone(),
            base_prefix_digest: st.head.prefix.rendered_base_digest.clone(),
            source_context_digest: plan.source_context_digest.clone(),
            token_estimate: plan.token_estimate,
            context_generation,
            summary_kind: plan.summary_kind.clone(),
            compactor_provider: plan.compactor_provider.clone(),
            compactor_model: plan.compactor_model.clone(),
            retained_from_sequence,
            created_at_ms,
        };
        records.push((
            st.take_seq(),
            Record::ContextInstalled {
                checkpoint_id,
                base_checkpoint_id,
                covers_through_sequence,
                retained_messages: plan.retained_messages,
                payload_digest: encoded.digest,
                base_prefix_digest: pointer.base_prefix_digest.clone(),
                source_context_digest: plan.source_context_digest,
                token_estimate: plan.token_estimate,
                context_generation,
                summary_kind: plan.summary_kind,
                compactor_provider: plan.compactor_provider,
                compactor_model: plan.compactor_model,
                retained_from_sequence,
                created_at_ms,
            },
        ));
        st.head.context = Some(pointer);
        st.head.provider_attempt = None;
        st.head.active_phase = Some("ready_to_build_model_request".into());
        self.commit(st, records).await?;
        st.history = Vec::with_capacity(plan.tail.len() + 1);
        st.history.push(Message::user_text(plan.summary));
        st.history.extend(plan.tail);
        Ok(())
    }

    async fn run_compaction(
        &self,
        st: &mut TurnState,
        request: crate::compact::CompactionRequest,
    ) -> Result<Option<CompletedCompaction>> {
        self.compactor.preflight(
            &request,
            &crate::compact::CompactionModel {
                provider: self.provider.clone(),
                session: &self.session,
                provider_name: &self.provider_name,
                outbound: &self.outbound,
                cancel: &self.cancel,
                header_timeout: self.provider_header_timeout,
                idle_timeout: self.provider_idle_timeout,
                total_timeout: self.provider_total_timeout,
                context_window_tokens: self.context_window_tokens,
            },
        )?;
        let request_digest = hex::encode(Sha256::digest(serde_jcs::to_vec(&request)?));
        let (logical_operation_id, mut replacements_used) = match st.head.provider_attempt.as_ref()
        {
            Some(attempt) if attempt.logical_operation_id.starts_with("cmp_") => {
                if !matches!(attempt.state.as_str(), "unknown" | "replacement_ready") {
                    return Err(BrainError::Journal(format!(
                        "compaction attempt cannot resume from state {}",
                        attempt.state
                    )));
                }
                if attempt.request_digest != request_digest {
                    return Err(BrainError::Journal(
                        "replacement compaction request does not match committed digest".into(),
                    ));
                }
                (
                    attempt.logical_operation_id.clone(),
                    attempt.replacements_used,
                )
            }
            Some(attempt) => {
                return Err(BrainError::Journal(format!(
                    "model attempt {} is active while compaction is required",
                    attempt.logical_operation_id
                )));
            }
            None => (crate::mint_id("cmp", 20), 0),
        };

        loop {
            let attempt_id = crate::mint_id("att", 20);
            st.head.active_phase = Some("compaction_intent_committed".into());
            st.head.provider_attempt = Some(crate::journal::ProviderAttemptDoc {
                logical_operation_id: logical_operation_id.clone(),
                attempt_id: attempt_id.clone(),
                request_digest: request_digest.clone(),
                state: "intent".into(),
                replacements_used,
            });
            self.commit(
                st,
                vec![(
                    st.take_seq(),
                    Record::CompactionIntent {
                        turn: self.turn_id.clone(),
                        logical_operation_id: logical_operation_id.clone(),
                        attempt_id: attempt_id.clone(),
                        request_digest: request_digest.clone(),
                        replacement: replacements_used,
                    },
                )],
            )
            .await?;
            st.head.active_phase = Some("compaction_running".into());
            if let Some(attempt) = &mut st.head.provider_attempt {
                attempt.state = "running".into();
            }
            let permit = tokio::select! {
                permit = self.model_permits.clone().acquire_owned() => {
                    permit.map_err(|_| BrainError::Overloaded)?
                }
                () = self.cancel.cancelled() => return Err(BrainError::Cancelled),
            };
            let compact = self.compactor.compact(
                request.clone(),
                crate::compact::CompactionModel {
                    provider: self.provider.clone(),
                    session: &self.session,
                    provider_name: &self.provider_name,
                    outbound: &self.outbound,
                    cancel: &self.cancel,
                    header_timeout: self.provider_header_timeout,
                    idle_timeout: self.provider_idle_timeout,
                    total_timeout: self.provider_total_timeout,
                    context_window_tokens: self.context_window_tokens,
                },
            );
            let result = tokio::select! {
                result = tokio::time::timeout(self.provider_total_timeout, compact) => {
                    result.unwrap_or_else(|_| Err(BrainError::Transport(
                        "compactor call exceeded its total deadline".into(),
                    )))
                }
                () = self.cancel.cancelled() => Err(BrainError::Cancelled),
            };
            drop(permit);
            match result {
                Ok(result) => {
                    return Ok(Some(CompletedCompaction {
                        result,
                        logical_operation_id,
                        attempt_id,
                        request_digest,
                    }));
                }
                Err(error) if provider_failure_is_unknown(&error) => {
                    st.head.active_phase = Some("compaction_unknown".into());
                    if let Some(attempt) = &mut st.head.provider_attempt {
                        attempt.state = "unknown".into();
                    }
                    self.commit(
                        st,
                        vec![(
                            st.take_seq(),
                            Record::CompactionUnknown {
                                turn: self.turn_id.clone(),
                                logical_operation_id: logical_operation_id.clone(),
                                attempt_id,
                                request_digest: request_digest.clone(),
                                possibly_duplicated: true,
                            },
                        )],
                    )
                    .await?;
                    if replacements_used >= st.head.prefix.provider_recovery_retries {
                        return Ok(None);
                    }
                    replacements_used += 1;
                    let attempt = st
                        .head
                        .provider_attempt
                        .as_mut()
                        .expect("compaction attempt");
                    attempt.replacements_used = replacements_used;
                    attempt.state = "replacement_ready".into();
                    st.head.active_phase = Some("ready_to_compact".into());
                    self.commit(st, vec![]).await?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// One streamed model round for this ordinary session. The wrapper journals Usage and
    /// provider-attempt recovery evidence around the streamed transport.
    async fn root_round(&self, st: &mut TurnState) -> Result<RootRound> {
        let request = self.provider.build_request(
            &self.prefix,
            &st.history,
            &self.session.key,
            &self.session.base_url,
        )?;
        crate::compact::validate_model_request_budget(
            request.body_len(),
            self.prefix.sampling.max_tokens as usize,
            self.context_window_tokens,
            "model",
        )?;
        let request_digest = model_request_digest(&request);
        let (logical_operation_id, replacements_used, superseded_attempt_id) = st
            .head
            .provider_attempt
            .as_ref()
            .filter(|attempt| matches!(attempt.state.as_str(), "unknown" | "replacement_ready"))
            .map(|attempt| {
                if attempt.request_digest != request_digest {
                    return Err(BrainError::Journal(
                        "replacement provider request does not match committed digest".into(),
                    ));
                }
                Ok((
                    attempt.logical_operation_id.clone(),
                    attempt.replacements_used,
                    Some(attempt.attempt_id.clone()),
                ))
            })
            .transpose()?
            .unwrap_or_else(|| (crate::mint_id("mdl", 20), 0, None));
        let attempt_id = crate::mint_id("att", 20);
        st.head.active_phase = Some("model_intent_committed".into());
        st.head.provider_attempt = Some(crate::journal::ProviderAttemptDoc {
            logical_operation_id: logical_operation_id.clone(),
            attempt_id: attempt_id.clone(),
            request_digest: request_digest.clone(),
            state: "intent".into(),
            replacements_used,
        });
        let mut intent_records = Vec::with_capacity(2);
        if let Some(superseded_attempt_id) = superseded_attempt_id {
            intent_records.push((
                st.take_seq(),
                Record::ModelAttemptSuperseded {
                    turn: self.turn_id.clone(),
                    logical_operation_id: logical_operation_id.clone(),
                    superseded_attempt_id,
                    replacement_attempt_id: attempt_id.clone(),
                    reason: "unknown".into(),
                },
            ));
        }
        let intent_seq = st.take_seq();
        intent_records.push((
            intent_seq,
            Record::ModelCallIntent {
                turn: self.turn_id.clone(),
                logical_operation_id: logical_operation_id.clone(),
                attempt_id: attempt_id.clone(),
                request_digest: request_digest.clone(),
                replacement: replacements_used,
            },
        ));
        self.commit(st, intent_records).await?;
        st.head.active_phase = Some("model_running".into());
        if let Some(attempt) = &mut st.head.provider_attempt {
            attempt.state = "running".into();
        }
        let result = model_round_request(
            RoundCtx {
                provider: &self.provider,
                outbound: &self.outbound,
                header_timeout: self.provider_header_timeout,
                idle_timeout: self.provider_idle_timeout,
                total_timeout: self.provider_total_timeout,
                permits: &self.model_permits,
                cancel: &self.cancel,
                hub: &self.hub,
                session_id: &self.session_id,
                turn_id: &self.turn_id,
                agent: "root",
                attempt_id: &attempt_id,
                seq: &st.seq,
            },
            request,
        )
        .await;
        let (message, stop, usage) = match result {
            Ok(result) => result,
            Err(error) if provider_failure_is_unknown(&error) => {
                st.head.active_phase = Some("model_unknown".into());
                if let Some(attempt) = &mut st.head.provider_attempt {
                    attempt.state = "unknown".into();
                }
                let seq = st.take_seq();
                self.commit(
                    st,
                    vec![(
                        seq,
                        Record::ModelCallUnknown {
                            turn: self.turn_id.clone(),
                            logical_operation_id,
                            attempt_id,
                            request_digest,
                            possibly_duplicated: true,
                        },
                    )],
                )
                .await?;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        Ok(RootRound {
            message,
            stop,
            usage,
            logical_operation_id,
            attempt_id,
            request_digest,
        })
    }

    async fn execute_managed(
        &self,
        st: &mut TurnState,
        operation_id: &str,
        name: &str,
        input: serde_json::Value,
    ) -> Result<DispatchedOutcome> {
        use brain_protocol::hand::{HandErrorCode, OperationState, OutputChunkStream};

        let hand = self.hand.as_ref().ok_or_else(|| {
            BrainError::HandUnavailable(
                "managed Tools require the canonical Hand receipt port".into(),
            )
        })?;
        let binding = self.managed_bindings.get(name).ok_or_else(|| {
            BrainError::HandUnavailable(format!(
                "managed Tool {name} has no prepared immutable binding"
            ))
        })?;
        let input_bytes = serde_jcs::to_vec(&input)?.len();
        if input_bytes > brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES
            || input_bytes > binding.limits.max_inline_input_bytes.get() as usize
        {
            return Ok(DispatchedOutcome::from(CallOutcome::failed(format!(
                "managed Tool {name} input is {input_bytes} bytes; maximum is {}",
                brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES
                    .min(binding.limits.max_inline_input_bytes.get() as usize)
            ))));
        }

        let resources = crate::session::managed_hand_resources()?;
        let deadline_at_ms = crate::wall_ms().saturating_add(resources.timeout_ms.get());
        let current_target =
            st.head
                .default_sandbox
                .as_ref()
                .and_then(|status| match status.state {
                    brain_protocol::hand::SandboxState::Running
                    | brain_protocol::hand::SandboxState::Suspended => status
                        .generation
                        .as_ref()
                        .zip(status.target_ref.as_ref())
                        .map(|(generation, target_ref)| {
                            (generation.to_string(), target_ref.to_string())
                        }),
                    _ => None,
                });
        let mut envelope: brain_protocol::hand::OperationEnvelope =
            serde_json::from_value(serde_json::json!({
                "operation_id": operation_id,
                "request_digest": "0".repeat(64),
                "session_id": self.session_id,
                "root_id": st.head.root_id,
                "turn_id": self.turn_id,
                "caller_id": "agent_root",
                "fence": st.lease.fence,
                "generation": current_target.as_ref().map(|(generation, _)| generation),
                "binding_ref": binding.binding_ref,
                "capability": name,
                "input": {"kind": "inline", "value": input},
                "target_ref": current_target.as_ref().map(|(_, target_ref)| target_ref),
                "deadline_at_ms": deadline_at_ms,
                "resources": resources,
                "network": crate::session::sealed_sandbox_network(&st.head)?,
                "trace": {},
            }))?;
        envelope.request_digest = brain_protocol::contract::operation_request_digest(&envelope);
        let request_digest = envelope.request_digest.to_string();
        st.head.active_phase = Some("managed_running".into());
        self.commit(
            st,
            vec![(
                st.take_seq(),
                Record::ManagedCallIntent {
                    turn: self.turn_id.clone(),
                    call: operation_id.to_owned(),
                    name: name.to_owned(),
                    envelope: envelope.clone(),
                },
            )],
        )
        .await?;

        let submit_request = brain_protocol::hand::SubmitRequest {
            envelope: envelope.clone(),
            wait_up_to_ms: binding.limits.max_wait_ms.min(30_000),
        };
        let mut reprepared = false;
        let receipt = loop {
            let remaining = deadline_at_ms.saturating_sub(crate::wall_ms()).max(1);
            let submit = tokio::time::timeout(
                std::time::Duration::from_millis(remaining),
                hand.submit(submit_request.clone()),
            );
            let result = tokio::select! {
                result = submit => result.map_err(|_| BrainError::HandUnavailable(
                    "managed Tool submit exceeded its sealed deadline".into()
                ))?,
                () = self.cancel.cancelled() => return Err(BrainError::Cancelled),
            };
            match result {
                Ok(receipt) => break receipt,
                Err(error) if error.code == HandErrorCode::CapabilityUnavailable && !reprepared => {
                    let brain = self.engine.upgrade().ok_or_else(|| {
                        BrainError::HandUnavailable(
                            "managed Tool preparation coordinator is unavailable".into(),
                        )
                    })?;
                    let bindings = brain
                        .prepare_managed_session(&self.session_id, &st.head)
                        .await?;
                    let refreshed = bindings.get(name).ok_or_else(|| {
                        BrainError::HandUnavailable(format!(
                            "managed Tool {name} disappeared during re-preparation"
                        ))
                    })?;
                    if refreshed.binding_ref != binding.binding_ref {
                        return Err(BrainError::HandUnavailable(
                            "managed Tool binding changed during exact re-preparation".into(),
                        ));
                    }
                    reprepared = true;
                }
                Err(error) if error.code == HandErrorCode::OperationUnknown => {
                    return self
                        .finish_managed_submit_unknown(st, operation_id, name, &request_digest)
                        .await;
                }
                Err(error) => return Err(crate::session::map_hand_port_error(error)),
            }
        };
        verify_managed_operation(
            &receipt.operation,
            operation_id,
            &request_digest,
            &self.session_id,
            &st.head,
        )?;
        verify_managed_observation(&receipt.observation, &receipt.operation)?;

        if let Some(target) = &receipt.observation.target {
            let status: brain_protocol::hand::SandboxStatus =
                serde_json::from_value(serde_json::json!({
                    "state": "running",
                    "target": receipt.operation.target,
                    "generation": target.generation,
                    "target_ref": target.target_ref,
                    "changed_at_ms": crate::wall_ms(),
                    "expires_at_ms": target.expires_at_ms,
                }))?;
            st.head.default_sandbox = Some(status);
        }
        self.commit(
            st,
            vec![(
                st.take_seq(),
                Record::ManagedCallAccepted {
                    turn: self.turn_id.clone(),
                    call: operation_id.to_owned(),
                    operation: receipt.operation.clone(),
                },
            )],
        )
        .await?;

        let mut observation = receipt.observation;
        let mut output_bytes = 0usize;
        let mut output_chunks = 0usize;
        let mut cancellation_sent = false;
        loop {
            verify_managed_observation(&observation, &receipt.operation)?;
            for chunk in &observation.output {
                output_chunks = output_chunks.saturating_add(1);
                output_bytes = output_bytes.saturating_add(chunk.text.len());
                if output_chunks > 1_024
                    || output_bytes > brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES
                {
                    return Err(BrainError::Protocol(
                        "managed Tool provisional output exceeded its bounded observation budget"
                            .into(),
                    ));
                }
                let stream = match chunk.stream {
                    OutputChunkStream::Stderr => EventStream::Stderr,
                    OutputChunkStream::Stdout | OutputChunkStream::Progress => EventStream::Stdout,
                };
                if let Some(event) = crate::events::output_event(
                    &self.session_id,
                    st.take_seq(),
                    &self.turn_id,
                    operation_id,
                    stream,
                    chunk.offset,
                    chunk.text.to_string(),
                ) {
                    self.hub.publish(&self.session_id, event);
                }
            }
            if observation.state == OperationState::Terminal {
                let terminal = observation.terminal.ok_or_else(|| {
                    BrainError::Protocol(
                        "managed Hand reported terminal state without a terminal receipt".into(),
                    )
                })?;
                return managed_terminal_outcome(receipt.operation, terminal);
            }
            if observation.terminal.is_some() {
                return Err(BrainError::Protocol(
                    "managed Hand returned a terminal receipt before terminal state".into(),
                ));
            }
            if crate::wall_ms() >= deadline_at_ms {
                return Err(BrainError::HandUnavailable(
                    "managed Tool did not reach a terminal receipt before its sealed deadline"
                        .into(),
                ));
            }
            if self.cancel.is_cancelled() && !cancellation_sent {
                let request: brain_protocol::hand::CancelRequest =
                    serde_json::from_value(serde_json::json!({
                        "operation": receipt.operation,
                        "reason": "turn_cancelled",
                    }))?;
                match hand.cancel(request).await {
                    Ok(_) => {}
                    Err(error) if error.code == HandErrorCode::OperationUnknown => {
                        return self
                            .finish_managed_submit_unknown(st, operation_id, name, &request_digest)
                            .await;
                    }
                    Err(error) => return Err(crate::session::map_hand_port_error(error)),
                }
                cancellation_sent = true;
            }
            let wait_ms = binding
                .limits
                .max_wait_ms
                .min(30_000)
                .min(deadline_at_ms.saturating_sub(crate::wall_ms()));
            let request: brain_protocol::hand::ObserveRequest =
                serde_json::from_value(serde_json::json!({
                    "operation": receipt.operation,
                    "cursor": observation.next_cursor,
                    "wait_ms": wait_ms,
                }))?;
            let observed = tokio::time::timeout(
                std::time::Duration::from_millis(wait_ms.saturating_add(1_000).max(1)),
                hand.observe(request),
            )
            .await
            .map_err(|_| {
                BrainError::HandUnavailable("managed Tool observation timed out".into())
            })?;
            observation = match observed {
                Ok(observation) => observation,
                Err(error) if error.code == HandErrorCode::OperationUnknown => {
                    return self
                        .finish_managed_submit_unknown(st, operation_id, name, &request_digest)
                        .await;
                }
                Err(error) => return Err(crate::session::map_hand_port_error(error)),
            };
        }
    }

    async fn finish_managed_submit_unknown(
        &self,
        st: &mut TurnState,
        operation_id: &str,
        name: &str,
        request_digest: &str,
    ) -> Result<DispatchedOutcome> {
        self.commit(
            st,
            vec![(
                st.take_seq(),
                Record::ManagedCallUnknown {
                    turn: self.turn_id.clone(),
                    call: operation_id.to_owned(),
                    request_digest: request_digest.to_owned(),
                },
            )],
        )
        .await?;
        let brain = self.engine.upgrade().ok_or_else(|| {
            BrainError::HandUnavailable(
                "managed Tool unknown-outcome reconciliation coordinator is unavailable".into(),
            )
        })?;
        crate::session::reconcile_managed_unknown_default_sandbox(&brain, &self.session_id, st)
            .await?;
        Ok(DispatchedOutcome::from(managed_unknown_call_outcome(name)))
    }

    /// Dispatches one assistant message's calls through the adapter, bounded-parallel.
    /// Results come back in CALL order, every slot filled.
    async fn dispatch_batch(
        &self,
        st: &mut TurnState,
        calls: &[(String, String, serde_json::Value)],
        prepared_customer: &HashMap<String, PreparedCustomerDispatch>,
    ) -> Result<Vec<DispatchedOutcome>> {
        let batch = calls.len() > 1;
        let sem = Arc::new(Semaphore::new(self.prefix.limits.max_parallel_tools.max(1)));
        let mut join = tokio::task::JoinSet::new();
        let mut done: Vec<Option<DispatchedOutcome>> =
            std::iter::repeat_with(|| None).take(calls.len()).collect();
        for (idx, (op_id, name, input)) in calls.iter().cloned().enumerate() {
            let route = self.prefix.tool(&name).map(|t| t.route.clone());
            let permit = sem.clone();
            if let Some(error) = self
                .prefix
                .tool(&name)
                .and_then(|tool| crate::tools::input_error(tool, &input))
            {
                join.spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    (idx, DispatchedOutcome::from(CallOutcome::failed(error)))
                });
                continue;
            }
            match route {
                None => {
                    // Undeclared: never dispatched, still answered.
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        (
                            idx,
                            DispatchedOutcome::from(CallOutcome::failed(crate::tools::undeclared(
                                &name,
                            ))),
                        )
                    });
                }
                Some(ToolRoute::Intrinsic(capability)) if capability == "brain.subagents" => {
                    let brain = self.engine.clone();
                    let parent_id = self.session_id.clone();
                    let cancel = self.cancel.clone();
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let outcome = match brain.upgrade() {
                            Some(brain) => {
                                brain
                                    .execute_child_capability(&parent_id, &op_id, input, cancel)
                                    .await
                            }
                            None => CallOutcome::failed(
                                "ordinary child-session coordinator is unavailable",
                            ),
                        };
                        (idx, DispatchedOutcome::from(outcome))
                    });
                }
                Some(ToolRoute::Intrinsic(capability)) if capability == "brain.storage" => {
                    let _permit = permit
                        .acquire_owned()
                        .await
                        .map_err(|_| BrainError::Overloaded)?;
                    let brain = self.engine.upgrade().ok_or_else(|| {
                        BrainError::Journal("engine capability coordinator is unavailable".into())
                    })?;
                    let outcome = brain
                        .execute_storage_capability(
                            &self.session_id,
                            &op_id,
                            input,
                            self.cancel.clone(),
                            st,
                        )
                        .await?;
                    done[idx] = Some(DispatchedOutcome::from(outcome));
                }
                Some(ToolRoute::Intrinsic(capability)) if capability == "brain.sandbox" => {
                    let _permit = permit
                        .acquire_owned()
                        .await
                        .map_err(|_| BrainError::Overloaded)?;
                    let brain = self.engine.upgrade().ok_or_else(|| {
                        BrainError::Journal("engine capability coordinator is unavailable".into())
                    })?;
                    let outcome = brain
                        .execute_sandbox_capability(
                            &self.session_id,
                            &op_id,
                            input,
                            self.cancel.clone(),
                            st,
                        )
                        .await?;
                    done[idx] = Some(DispatchedOutcome::from(outcome));
                }
                Some(ToolRoute::Intrinsic(capability)) => {
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        (
                            idx,
                            DispatchedOutcome::from(CallOutcome::failed(format!(
                                "intrinsic capability {capability} is unavailable"
                            ))),
                        )
                    });
                }
                Some(ToolRoute::Server(policy)) => {
                    let executor = self.external_executor.clone();
                    let cancel = self.cancel.clone();
                    let session_id = self.session_id.clone();
                    let turn_id = self.turn_id.clone();
                    let context = self.context.clone();
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let out = execute_external(
                            executor,
                            policy,
                            batch,
                            session_id,
                            turn_id,
                            "root".into(),
                            op_id,
                            name,
                            input,
                            context,
                            cancel,
                        )
                        .await;
                        (idx, DispatchedOutcome::from(out))
                    });
                }
                Some(ToolRoute::Customer { registration }) => {
                    let customer = self.customer.clone();
                    let cancel = self.cancel.clone();
                    let submit_retries = self.customer_submit_retries;
                    let prepared = prepared_customer.get(&op_id).cloned();
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let outcome = match (customer, prepared) {
                            (
                                Some(customer),
                                Some(PreparedCustomerDispatch::Intent(intent)),
                            ) => {
                                let execution = customer
                                    .execute_prepared(intent, submit_retries, cancel)
                                    .await;
                                DispatchedOutcome {
                                    outcome: execution.outcome,
                                    customer_terminal: execution.terminal_receipt,
                                    managed_terminal: None,
                                }
                            }
                            (_, Some(PreparedCustomerDispatch::Failure(outcome))) => {
                                DispatchedOutcome::from(outcome)
                            }
                            _ => DispatchedOutcome::from(CallOutcome::failed(format!(
                                "customer application transport is unavailable for registration {registration}"
                            ))),
                        };
                        (idx, outcome)
                    });
                }
                Some(ToolRoute::Hand(_)) => {
                    let _permit = permit
                        .acquire_owned()
                        .await
                        .map_err(|_| BrainError::Overloaded)?;
                    done[idx] = Some(self.execute_managed(st, &op_id, &name, input).await?);
                }
            }
        }
        while let Some(joined) = join.join_next().await {
            match joined {
                Ok((idx, out)) => done[idx] = Some(out),
                Err(error) => {
                    if let Some(slot) = done.iter_mut().find(|slot| slot.is_none()) {
                        *slot = Some(DispatchedOutcome::from(CallOutcome::failed(format!(
                            "tool task did not complete: {error}"
                        ))));
                    }
                }
            }
        }

        Ok(done
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                let outcome = outcome.unwrap_or_else(|| {
                    DispatchedOutcome::from(CallOutcome::failed("tool produced no result"))
                });
                let tool = self.prefix.tool(&calls[index].1);
                DispatchedOutcome {
                    outcome: crate::tools::enforce_outcome(tool, &calls[index].1, outcome.outcome),
                    customer_terminal: outcome.customer_terminal,
                    managed_terminal: outcome.managed_terminal,
                }
            })
            .collect())
    }
}

pub(crate) fn verify_managed_operation(
    operation: &brain_protocol::hand::OperationRef,
    operation_id: &str,
    request_digest: &str,
    session_id: &str,
    head: &HeadDoc,
) -> Result<()> {
    if operation.operation_id.as_str() != operation_id
        || operation.request_digest.as_str() != request_digest
        || operation.target.session_id.as_str() != session_id
        || operation.target.root_id.as_str() != head.root_id
        || operation.target.kind != brain_protocol::hand::TargetKind::Default
        || operation.target.sandbox_id.is_some()
    {
        return Err(BrainError::Protocol(
            "managed Hand returned an operation outside the committed session/root request".into(),
        ));
    }
    Ok(())
}

pub(crate) fn verify_managed_observation(
    observation: &brain_protocol::hand::OperationObservation,
    operation: &brain_protocol::hand::OperationRef,
) -> Result<()> {
    if serde_jcs::to_vec(&observation.operation)? != serde_jcs::to_vec(operation)? {
        return Err(BrainError::Protocol(
            "managed Hand observation references a different operation".into(),
        ));
    }
    if let Some(target) = &observation.target
        && (target.generation != operation.generation || target.target_ref != operation.target_ref)
    {
        return Err(BrainError::Protocol(
            "managed Hand observation target conflicts with its rooted operation receipt".into(),
        ));
    }
    Ok(())
}

fn managed_terminal_outcome(
    operation: brain_protocol::hand::OperationRef,
    terminal: brain_protocol::hand::TerminalResult,
) -> Result<DispatchedOutcome> {
    let (outcome, terminal_digest) = managed_terminal_call_outcome(terminal)?;
    Ok(DispatchedOutcome {
        outcome,
        customer_terminal: None,
        managed_terminal: Some(ManagedTerminalReceipt {
            operation,
            terminal_digest,
        }),
    })
}

pub(crate) fn managed_unknown_call_outcome(name: &str) -> CallOutcome {
    let mut outcome = CallOutcome::failed(format!(
        "managed Tool {name} may have run, but its terminal receipt is unavailable; Brain will not submit it again"
    ));
    outcome.outcome = "interrupted".into();
    outcome
}

pub(crate) fn managed_terminal_call_outcome(
    terminal: brain_protocol::hand::TerminalResult,
) -> Result<(CallOutcome, String)> {
    let expected = brain_protocol::contract::terminal_result_digest(&terminal);
    if expected != terminal.terminal_digest {
        return Err(BrainError::Protocol(
            "managed Hand terminal digest does not match its canonical receipt".into(),
        ));
    }
    if terminal
        .inline
        .as_ref()
        .is_some_and(|value| !brain_protocol::contract::terminal_inline_fits(value))
    {
        return Err(BrainError::Protocol(format!(
            "managed Hand terminal exceeds the {}-byte inline result limit",
            brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES
        )));
    }
    let completed = terminal.outcome == brain_protocol::hand::TerminalOutcome::Completed;
    if terminal.is_error == completed {
        return Err(BrainError::Protocol(
            "managed Hand terminal outcome and is_error flag conflict".into(),
        ));
    }
    let value = terminal.inline.clone().or_else(|| {
        terminal
            .object
            .as_ref()
            .and_then(|object| serde_json::to_value(object).ok())
    });
    let content = match &value {
        Some(value) => serde_json::to_string(value)?,
        None => format!("[{}: no inline result]", terminal.outcome),
    };
    let terminal_digest = terminal.terminal_digest.to_string();
    Ok((
        CallOutcome {
            outcome: terminal.outcome.to_string(),
            value,
            content,
            is_error: terminal.is_error,
            exit_code: terminal.exit_code,
            duration_ms: terminal.duration_ms.unwrap_or(0),
            truncated: false,
            terminal: None,
        },
        terminal_digest,
    ))
}

fn provider_failure_is_unknown(error: &BrainError) -> bool {
    match error {
        BrainError::Transport(_) | BrainError::Protocol(_) => true,
        BrainError::ProviderStatus { status, .. } => *status >= 500,
        BrainError::Cancelled | BrainError::Overloaded => false,
        _ => false,
    }
}

fn customer_preparation_failure(error: BrainError) -> CallOutcome {
    let retryable = matches!(
        error,
        BrainError::HandUnavailable(_) | BrainError::Overloaded
    );
    let mut outcome = CallOutcome::failed(error.to_string());
    if retryable {
        outcome.outcome = "interrupted".into();
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_external(
    executor: Arc<dyn ToolExecutor>,
    policy: crate::config::ServerToolPolicy,
    parallel_batch: bool,
    session_id: String,
    turn_id: String,
    agent_id: String,
    call_id: String,
    name: String,
    input: serde_json::Value,
    context: std::collections::HashMap<String, String>,
    cancel: CancellationToken,
) -> CallOutcome {
    use brain_protocol::session::{ExternalToolCompletion, ExternalToolDisposition, ToolOutcome};

    if parallel_batch && policy.completion == ExternalToolCompletion::ReturnDirect {
        return CallOutcome::failed(format!(
            "return-direct external tool {name} must be the only tool call in an assistant message"
        ));
    }
    let input_bytes = match serde_json::to_vec(&input) {
        Ok(bytes) => bytes.len(),
        Err(error) => return CallOutcome::failed(format!("external tool input: {error}")),
    };
    if policy.max_input_bytes == 0
        || policy.max_input_bytes > brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES
    {
        return CallOutcome::failed(format!(
            "external tool {name} has an invalid sealed input ceiling; maximum is {} bytes",
            brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES
        ));
    }
    if input_bytes > policy.max_input_bytes {
        return CallOutcome::failed(format!(
            "external tool {name} input is {input_bytes} bytes; the sealed limit is {} bytes",
            policy.max_input_bytes
        ));
    }
    let request = match serde_json::from_value(serde_json::json!({
        "session_id": session_id,
        "turn_id": turn_id,
        "agent_id": agent_id,
        "call_id": call_id,
        "name": name,
        "input": input,
        "context": context,
    })) {
        Ok(request) => request,
        Err(error) => {
            return CallOutcome::failed(format!("external tool request contract: {error}"));
        }
    };
    let started = Instant::now();
    let response = match executor.call(&policy.capability, request, cancel).await {
        Ok(response) => response,
        Err(BrainError::Cancelled) => {
            return CallOutcome {
                outcome: "cancelled".into(),
                value: None,
                content: "external tool call cancelled".into(),
                is_error: true,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                truncated: false,
                terminal: None,
            };
        }
        Err(error) => {
            let mut outcome = CallOutcome::failed(format!("external tool executor: {error}"));
            outcome.duration_ms = started.elapsed().as_millis() as u64;
            return outcome;
        }
    };
    if !brain_protocol::contract::external_tool_response_inline_fits(&response) {
        return CallOutcome::failed(format!(
            "external tool {name} result exceeds the {}-byte inline terminal limit",
            brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES
        ));
    }
    let content = String::from(response.content);
    let structured = response.result.clone();
    let terminal = match response.disposition {
        ExternalToolDisposition::Continue => None,
        disposition if policy.completion != ExternalToolCompletion::ReturnDirect => {
            return CallOutcome::failed(format!(
                "external tool {name} returned terminal disposition {disposition} but is sealed as continue"
            ));
        }
        ExternalToolDisposition::CompleteTurn => {
            if response.outcome != ToolOutcome::Completed || response.is_error {
                return CallOutcome::failed(format!(
                    "external tool {name} returned complete_turn with a failed outcome"
                ));
            }
            let Some(value) = response.result else {
                return CallOutcome::failed(format!(
                    "external tool {name} returned complete_turn without result"
                ));
            };
            Some(TerminalOutcome::Complete {
                value,
                metadata: response.result_metadata,
            })
        }
        ExternalToolDisposition::FailTurn => {
            if response.outcome == ToolOutcome::Completed || !response.is_error {
                return CallOutcome::failed(format!(
                    "external tool {name} returned fail_turn with a successful outcome"
                ));
            }
            let Some(error) = response.error else {
                return CallOutcome::failed(format!(
                    "external tool {name} returned fail_turn without error"
                ));
            };
            Some(TerminalOutcome::Fail { error })
        }
    };
    CallOutcome {
        outcome: response.outcome.to_string(),
        value: structured,
        content,
        is_error: response.is_error,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated: false,
        terminal,
    }
}

// ---------------------------------------------------------------------------------------------
// Provider streaming
// ---------------------------------------------------------------------------------------------

/// Everything one streamed model round needs, borrowed.
pub(crate) struct RoundCtx<'a> {
    pub provider: &'a Arc<dyn Provider>,
    pub outbound: &'a crate::outbound::Outbound,
    pub header_timeout: std::time::Duration,
    pub idle_timeout: std::time::Duration,
    pub total_timeout: std::time::Duration,
    pub permits: &'a Arc<Semaphore>,
    pub cancel: &'a CancellationToken,
    pub hub: &'a Arc<EventHub>,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub agent: &'a str,
    pub attempt_id: &'a str,
    pub seq: &'a Seq,
}

/// Replaces provider-local tool-use ids with the brain-minted call ids that
/// own journal, SSE, and hand attribution. The normalized assistant message
/// and its following results then stay internally linked after cold replay.
pub(crate) fn mint_tool_calls(message: &mut Message) -> Vec<(String, String, serde_json::Value)> {
    message
        .content
        .iter_mut()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => {
                let call = crate::mint_id("op", 16);
                *id = call.clone();
                Some((call, name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn model_request_digest(request: &crate::provider::ModelRequest) -> String {
    fn field(digest: &mut Sha256, bytes: &[u8]) {
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }

    let mut digest = Sha256::new();
    field(&mut digest, b"aex.model-request.v1");
    field(&mut digest, request.method.as_bytes());
    field(&mut digest, request.url.as_bytes());
    let mut headers: Vec<_> = request
        .headers
        .iter()
        .enumerate()
        .map(|(index, (name, value))| (name.to_ascii_lowercase(), value.as_bytes(), index))
        .collect();
    headers.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.2.cmp(&right.2)));
    field(&mut digest, &(headers.len() as u64).to_be_bytes());
    for (name, value, _) in headers {
        field(&mut digest, name.as_bytes());
        // The whole request projection is one-way hashed before journaling; credentials are
        // therefore included in replacement identity without being persisted or logged.
        field(&mut digest, value);
    }
    field(&mut digest, &request.body);
    hex::encode(digest.finalize())
}

async fn model_round_request(
    ctx: RoundCtx<'_>,
    req: crate::provider::ModelRequest,
) -> Result<(Message, StopReason, crate::message::Usage)> {
    let _permit = tokio::select! {
        permit = ctx.permits.clone().acquire_owned() => {
            permit.map_err(|_| BrainError::Overloaded)?
        }
        () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
    };

    let mut total = std::pin::pin!(tokio::time::sleep(ctx.total_timeout));
    let mut stream = tokio::select! {
        s = tokio::time::timeout(ctx.header_timeout, ctx.provider.stream(req, ctx.outbound)) => {
            s.map_err(|_| BrainError::Transport("provider response header timed out".into()))??
        },
        () = &mut total => return Err(BrainError::Transport("provider call exceeded its total deadline".into())),
        () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
    };
    let mut acc = Accumulator::default();
    loop {
        let idle = tokio::time::sleep(ctx.idle_timeout);
        tokio::pin!(idle);
        let ev = tokio::select! {
            ev = stream.next() => ev,
            () = &mut idle => return Err(BrainError::Transport("provider stream idle timeout".into())),
            () = &mut total => return Err(BrainError::Transport("provider call exceeded its total deadline".into())),
            () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
        };
        match ev {
            Some(Ok(ev)) => {
                if let ProviderEvent::TextDelta { text, .. } = &ev {
                    let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
                    if let Some(e) = crate::events::delta_event(
                        ctx.session_id,
                        seq,
                        ctx.turn_id,
                        ctx.agent,
                        ctx.attempt_id,
                        text.clone(),
                    ) {
                        ctx.hub.publish(ctx.session_id, e);
                    }
                }
                // Drain to EOF -- never break on the terminal stop frame because a provider may
                // deliver final usage in a following frame. The idle/total budgets still apply.
                acc.push(ev)?;
            }
            Some(Err(e)) => return Err(e),
            None => break,
        }
    }
    if !acc.saw_terminal {
        return Err(BrainError::Protocol(
            "provider stream ended without a terminal message event".into(),
        ));
    }
    let (message, stop, usage) = acc.finish()?;
    if stop == StopReason::ToolUse && message.tool_uses().next().is_none() {
        return Err(BrainError::Protocol(
            "provider reported stop_reason=tool_use with no tool_use block".into(),
        ));
    }
    Ok((message, stop, usage))
}

#[cfg(test)]
mod decision_limit_tests {
    use super::*;

    #[test]
    fn provider_tool_call_limit_has_an_explicit_boundary() {
        validate_model_tool_call_count(MAX_TOOL_CALLS_PER_MODEL_ROUND).unwrap();
        let error = validate_model_tool_call_count(MAX_TOOL_CALLS_PER_MODEL_ROUND + 1).unwrap_err();
        assert!(error.to_string().contains("Tool calls in one round"));
    }
}
