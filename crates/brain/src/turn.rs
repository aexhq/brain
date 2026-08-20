//! The turn loop: model round -> journal the decision -> dispatch tools -> journal results ->
//! feed back -> repeat, until the model stops or a cap fires.
//!
//! Rules the loop holds:
//! - only a COMPLETE assistant message is journaled; a stream that dies mid-message leaves no
//!   trace in history;
//! - tool intents are journaled BEFORE dispatch (an ambiguous outcome is recorded as
//!   possibly-run) and results are journaled before [`crate::adapter::HandAdapter::acknowledge`]
//!   lets the substrate forget them;
//! - the error flag on a failed tool result is always set (a dropped flag turns a failure
//!   into a success in the model's eyes);
//! - a lost substrate interrupts its calls -- `interrupted`, never replayed (I10) -- and the
//!   turn goes on: the next call re-materialises through `ensure_ready`;
//! - cancellation is graceful: adapters get the token, results journal `cancelled`, the turn
//!   completes with `stop_reason = cancelled`.
//!
//! WHERE tools run is not this module's business: dispatch goes through the
//! [`crate::adapter::HandAdapter`] seam, one call at a time, output streamed back through a
//! sink whose event seqs the core owns.

use crate::adapter::{CallOutcome, CallRequest, HandAdapter, TerminalOutcome, ToolExecutor};
use crate::config::{SealedPrefix, SessionConfig, ToolRoute};
use crate::events::EventHub;
use crate::journal::{self, HeadDoc, Journal, Lease, Record};
use crate::message::{ContentBlock, Message, StopReason};
use crate::provider::{Accumulator, Provider, ProviderEvent};
use crate::{BrainError, Result, Shared};
use brain_protocol::session::EventStream;
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Everything a turn borrows from the session for its lifetime. The actor moves this in and
/// gets it back when the turn future resolves, mutated.
pub struct TurnState {
    pub history: Vec<Message>,
    pub head: HeadDoc,
    pub lease: Lease,
    pub hand: Arc<dyn HandAdapter>,
    /// Sealed MCP dispatch state, rebuilt at hydrate from the prefix doc. `None` when the
    /// session declares no MCP servers.
    pub mcp: Option<Arc<crate::mcp::McpRuntime>>,
    /// The next seq to allocate, shared with concurrently running subagents. Ephemeral
    /// events (deltas, tool output) consume seqs too; every commit persists the
    /// high-water mark.
    pub seq: Seq,
    /// Session-lifetime count of minted `task` subagent identities, seeded
    /// from the journal at hydrate so it survives re-materialise.
    pub identities: Arc<AtomicU64>,
}

/// The session's seq allocator: atomic because subagents mint concurrently with the root.
pub type Seq = Arc<AtomicU64>;

impl TurnState {
    pub fn take_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Refreshes the head's adapter-owned fields. Runs before every commit so the journal
    /// always carries the substrate's latest durable state and contract snapshot.
    pub fn snapshot_hand(&mut self) {
        self.head.hand_state = self.hand.state();
        self.head.hand_info = self.hand.hand_info();
        self.head.workspace_bytes = self.hand.workspace_bytes();
    }
}

/// The immutable turn context.
pub struct TurnRun {
    pub session_id: String,
    pub turn_id: String,
    pub prefix: Shared<SealedPrefix>,
    pub session: SessionConfig,
    pub provider: Arc<dyn Provider>,
    pub provider_name: String,
    pub journal: Journal,
    pub hub: Arc<EventHub>,
    pub cancel: CancellationToken,
    /// Bounds concurrent model rounds across the whole Brain.
    pub model_permits: Arc<Semaphore>,
    pub history_budget_bytes: usize,
    pub external_executor: Arc<dyn ToolExecutor>,
    pub attached: Arc<crate::attached::AttachedHub>,
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

impl TurnRun {
    /// Publishes the SSE events for freshly committed records. Call AFTER the commit: an event
    /// a client saw must exist in the journal.
    fn publish_records(&self, st: &TurnState, records: &[(u64, Record)]) {
        let now = crate::wall_ms();
        for (seq, record) in records {
            if let Some(e) =
                crate::events::derive(&self.session_id, *seq, now, record, &st.head.hand_info)
            {
                self.hub.publish(&self.session_id, e);
            }
        }
    }

    async fn commit(&self, st: &mut TurnState, records: Vec<(u64, Record)>) -> Result<()> {
        st.snapshot_hand();
        st.head.updated_ms = crate::wall_ms();
        let high_water = st.seq.load(Ordering::Relaxed).saturating_sub(1);
        let mut lease = st.lease.clone();
        self.journal
            .commit(&self.session_id, &mut lease, &records, &st.head, high_water)
            .await?;
        st.lease = lease;
        self.publish_records(st, &records);
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
            // Compaction check: linear, prefix-stable, journaled like any other decision.
            if let Some((summary, kept)) =
                crate::compact::plan(&st.history, self.history_budget_bytes)
            {
                let seq = st.take_seq();
                let rec = Record::Compacted { summary, kept };
                let mut f = journal::Fold::from_history(std::mem::take(&mut st.history));
                f.apply(&rec);
                f.finish();
                st.history = f.history;
                self.commit(st, vec![(seq, rec)]).await?;
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
            let (mut message, stop) = match self.root_round(st).await {
                Ok(v) => v,
                Err(BrainError::Cancelled) => {
                    return Ok(TurnReport {
                        stop_reason: "cancelled".into(),
                        rounds,
                        tool_calls,
                        terminal_committed: false,
                    });
                }
                Err(e) => return Err(e),
            };
            rounds += 1;

            // Journal the decision: the complete assistant message and every tool intent --
            // one durable write, BEFORE dispatch.
            let calls = mint_tool_calls(&mut message);

            let mut records = Vec::new();
            let assistant_seq = st.take_seq();
            records.push((
                assistant_seq,
                Record::Assistant {
                    turn: self.turn_id.clone(),
                    agent: "root".into(),
                    content: message.content.clone(),
                    stop,
                },
            ));
            for (op_id, name, input) in &calls {
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

            // Dispatch the batch and journal the results.
            let outcomes = self.dispatch_batch(st, &calls).await?;
            let mut result_records = Vec::new();
            let mut blocks = Vec::with_capacity(calls.len());
            for (i, o) in outcomes.iter().enumerate() {
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
            }
            let terminal = outcomes
                .iter()
                .enumerate()
                .find_map(|(index, outcome)| outcome.terminal.clone().map(|value| (index, value)));
            let terminal_report = if let Some((index, terminal)) = terminal {
                st.head.state = "idle".into();
                st.head.turn = None;
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
                None
            };
            if terminal_report.is_some() {
                result_records.push((
                    st.take_seq(),
                    Record::State {
                        state: "idle".into(),
                        turn: Some(self.turn_id.clone()),
                    },
                ));
            }
            self.commit(st, result_records).await?;

            // Only after the results are durable may the substrate forget them.
            let ids: Vec<String> = calls.iter().map(|(id, _, _)| id.clone()).collect();
            st.hand.acknowledge(&ids).await;
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
        st.head.state = "idle".into();
        st.head.turn = None;
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
                        state: "idle".into(),
                        turn: Some(self.turn_id.clone()),
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

    /// One streamed model round for the ROOT agent. The streaming itself is shared with
    /// subagents ([`model_round`]); the root wrapper journals the Usage record directly,
    /// while children route theirs through the root-owned coordinator.
    async fn root_round(&self, st: &mut TurnState) -> Result<(Message, StopReason)> {
        let (message, stop, usage) = model_round(
            RoundCtx {
                provider: &self.provider,
                prefix: &self.prefix,
                session: &self.session,
                permits: &self.model_permits,
                cancel: &self.cancel,
                hub: &self.hub,
                session_id: &self.session_id,
                turn_id: &self.turn_id,
                agent: "root",
                seq: &st.seq,
            },
            &st.history,
        )
        .await?;
        // Usage rides its own record so `model.usage` has its own seq, keeping per-round
        // granularity even when the round produced no tool calls.
        let seq = st.take_seq();
        self.commit(
            st,
            vec![(
                seq,
                Record::Usage {
                    turn: self.turn_id.clone(),
                    agent: "root".into(),
                    provider: self.provider_name.clone(),
                    model: self.prefix.model.clone(),
                    usage,
                },
            )],
        )
        .await?;
        Ok((message, stop))
    }

    /// Dispatches one assistant message's calls through the adapter, bounded-parallel.
    /// Results come back in CALL order, every slot filled. `task` children send decisions to
    /// the root-owned commit coordinator and wait for durability before dispatch.
    async fn dispatch_batch(
        &self,
        st: &mut TurnState,
        calls: &[(String, String, serde_json::Value)],
    ) -> Result<Vec<CallOutcome>> {
        let needs_hand = calls.iter().any(|(_, name, _)| {
            matches!(
                self.prefix.tool(name).map(|t| &t.route),
                Some(ToolRoute::Hand(_))
            )
        });
        if needs_hand {
            match st.hand.ensure_ready().await {
                Ok(None) => {}
                Ok(Some(lost)) => {
                    // A previous incarnation died between turns: nothing was in flight, but
                    // clients are told, durably.
                    tracing::warn!(session = %self.session_id, reason = %lost.reason, "hand lost between turns");
                    let synced_ms = st
                        .head
                        .hand_info
                        .last_sync_at
                        .as_ref()
                        .map(|t| t.0.timestamp_millis() as u64);
                    let seq = st.take_seq();
                    self.commit(
                        st,
                        vec![(
                            seq,
                            Record::HandLost {
                                turn: Some(self.turn_id.clone()),
                                interrupted: vec![],
                                synced_ms,
                            },
                        )],
                    )
                    .await?;
                }
                Err(e) => return Err(e),
            }
        }

        let batch = calls.len() > 1;
        let sem = Arc::new(Semaphore::new(self.prefix.limits.max_parallel_tools.max(1)));
        // Children cannot own the session lease. They send record batches here
        // and block until this root-owned loop has committed them.
        let (child_journal, mut child_commits) = crate::subagent::ChildJournal::channel();
        let child_prefix: Option<Shared<SealedPrefix>> = calls
            .iter()
            .any(|(_, name, _)| {
                matches!(
                    self.prefix.tool(name).map(|tool| &tool.route),
                    Some(ToolRoute::Intrinsic(capability))
                        if matches!(capability.as_str(), "brain.subagents" | "brain.subagents.v1")
                )
            })
            .then(|| Arc::new(self.prefix.task_child()));
        let mut join = tokio::task::JoinSet::new();
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
                    (idx, CallOutcome::failed(error))
                });
                continue;
            }
            match route {
                None => {
                    // Undeclared: never dispatched, still answered.
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        (idx, CallOutcome::failed(crate::tools::undeclared(&name)))
                    });
                }
                Some(ToolRoute::Intrinsic(capability))
                    if matches!(
                        capability.as_str(),
                        "brain.subagents" | "brain.subagents.v1"
                    ) =>
                {
                    // A self-similar child agent, in-process, inside this turn (slice 8).
                    let ctx = Arc::new(crate::subagent::SubagentCtx {
                        session_id: self.session_id.clone(),
                        turn_id: self.turn_id.clone(),
                        prefix: child_prefix
                            .as_ref()
                            .expect("task call created the child prefix")
                            .clone(),
                        session: self.session.clone(),
                        provider: self.provider.clone(),
                        provider_name: self.provider_name.clone(),
                        hub: self.hub.clone(),
                        cancel: self.cancel.clone(),
                        model_permits: self.model_permits.clone(),
                        history_budget_bytes: self.history_budget_bytes,
                        seq: st.seq.clone(),
                        hand: st.hand.clone(),
                        mcp: st.mcp.clone(),
                        server_executor: self.external_executor.clone(),
                        attached: self.attached.clone(),
                        context: self.context.clone(),
                        identities: st.identities.clone(),
                        journal: child_journal.clone(),
                    });
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let outcome = crate::subagent::run_task(ctx, 1, op_id, input).await;
                        (idx, outcome)
                    });
                }
                Some(ToolRoute::Intrinsic(capability)) => {
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        (
                            idx,
                            CallOutcome::failed(format!(
                                "intrinsic capability {capability} is unavailable"
                            )),
                        )
                    });
                }
                Some(ToolRoute::Mcp { .. }) => {
                    // Sealed MCP tools dispatch through the session's McpRuntime. The runtime
                    // was built at hydrate; a call for a session that somehow has none (or a
                    // tool the runtime no longer knows) is answered as an error, never a panic.
                    let runtime = st.mcp.clone();
                    let cancel = self.cancel.clone();
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let out = match &runtime {
                            Some(rt) => rt.call(&name, &input, &cancel).await,
                            None => CallOutcome::failed(
                                "MCP dispatch state is missing for this session".to_string(),
                            ),
                        };
                        (idx, out)
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
                        (idx, out)
                    });
                }
                Some(ToolRoute::Attached { callback_id }) => {
                    let attached = self.attached.clone();
                    let cancel = self.cancel.clone();
                    let session_id = self.session_id.clone();
                    let output_schema = self
                        .prefix
                        .tool(&name)
                        .expect("sealed route came from this Tool")
                        .output_schema
                        .clone();
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let outcome = attached
                            .call(
                                &session_id,
                                &callback_id,
                                &op_id,
                                &name,
                                input,
                                &output_schema,
                                cancel,
                            )
                            .await;
                        (idx, outcome)
                    });
                }
                Some(ToolRoute::Hand(_)) => {
                    let hand = st.hand.clone();
                    let cancel = self.cancel.clone();
                    // Each streaming call reserves its own ephemeral seq window for
                    // `tool.output` events. Coarse but safe: durable seqs continue after the
                    // window; replay has gaps, ids never collide.
                    let seq_base = st.seq.fetch_add(4096, Ordering::Relaxed);
                    let sink =
                        output_sink(&self.hub, &self.session_id, &self.turn_id, &op_id, seq_base);
                    join.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        let out = hand
                            .call(
                                CallRequest {
                                    call_id: op_id,
                                    tool: name,
                                    input,
                                    parallel: batch,
                                },
                                cancel,
                                sink,
                            )
                            .await;
                        (idx, out)
                    });
                }
            }
        }
        drop(child_journal);

        let mut done: Vec<Option<CallOutcome>> =
            std::iter::repeat_with(|| None).take(calls.len()).collect();
        let mut commits_open = true;
        while !join.is_empty() || commits_open {
            tokio::select! {
                request = child_commits.recv(), if commits_open => {
                    match request {
                        Some(request) => {
                            let records = request
                                .records
                                .into_iter()
                                .map(|record| (st.take_seq(), record))
                                .collect();
                            if let Err(error) = self.commit(st, records).await {
                                join.abort_all();
                                // Dropping the ack unblocks the child while the root
                                // propagates the durable-write failure.
                                drop(request.committed);
                                return Err(error);
                            }
                            let _ = request.committed.send(());
                        }
                        None => commits_open = false,
                    }
                }
                joined = join.join_next(), if !join.is_empty() => {
                    match joined {
                        Some(Ok((idx, out))) => done[idx] = Some(out),
                        Some(Err(e)) => {
                            if let Some(slot) = done.iter_mut().find(|s| s.is_none()) {
                                *slot = Some(CallOutcome::failed(format!(
                                    "tool task did not complete: {e}"
                                )));
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(done
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                let outcome =
                    outcome.unwrap_or_else(|| CallOutcome::failed("tool produced no result"));
                match self.prefix.tool(&calls[index].1) {
                    Some(tool) => crate::tools::enforce_output(tool, outcome),
                    None => outcome,
                }
            })
            .collect())
    }
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
    let content = String::from(response.content);
    let structured = response.result.clone();
    if content.len() > journal::MAX_RECORD_CONTENT_BYTES {
        return CallOutcome::failed(format!(
            "external tool {name} result exceeds the {}-byte journal limit",
            journal::MAX_RECORD_CONTENT_BYTES
        ));
    }
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
// Shared with subagents
// ---------------------------------------------------------------------------------------------

/// Everything one streamed model round needs, borrowed. The root and every subagent go
/// through this one function so the streaming rules (drain to EOF, complete-message-only)
/// cannot drift between them.
pub(crate) struct RoundCtx<'a> {
    pub provider: &'a Arc<dyn Provider>,
    pub prefix: &'a SealedPrefix,
    pub session: &'a SessionConfig,
    pub permits: &'a Arc<Semaphore>,
    pub cancel: &'a CancellationToken,
    pub hub: &'a Arc<EventHub>,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub agent: &'a str,
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

/// One streamed model round: request built from (sealed prefix, history), deltas fanned
/// out live under the caller's agent id, only the complete message returned. The caller
/// journals the usage through its own path (the root directly; children through the
/// root-owned commit coordinator).
pub(crate) async fn model_round(
    ctx: RoundCtx<'_>,
    history: &[Message],
) -> Result<(Message, StopReason, crate::message::Usage)> {
    let req =
        ctx.provider
            .build_request(ctx.prefix, history, &ctx.session.key, &ctx.session.base_url)?;
    let _permit = tokio::select! {
        permit = ctx.permits.clone().acquire_owned() => {
            permit.map_err(|_| BrainError::Overloaded)?
        }
        () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
    };

    let mut stream = tokio::select! {
        s = ctx.provider.stream(req) => s?,
        () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
    };
    let mut acc = Accumulator::default();
    loop {
        let ev = tokio::select! {
            ev = stream.next() => ev,
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
                        text.clone(),
                    ) {
                        ctx.hub.publish(ctx.session_id, e);
                    }
                }
                // Drain to EOF -- never break on the first terminal. The Anthropic wire
                // folds `message_start`'s usage as an early MessageDone{Unknown}; the real
                // stop_reason arrives in message_delta near the end. Breaking early turns
                // a whole message into an empty one.
                acc.push(ev);
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

/// The `tool.output` event sink for one streaming hand call, publishing under a reserved
/// ephemeral seq window. Shared by the root dispatcher and subagents.
pub(crate) fn output_sink(
    hub: &Arc<EventHub>,
    session_id: &str,
    turn_id: &str,
    op_id: &str,
    seq_base: u64,
) -> crate::adapter::OutputSink {
    let hub = hub.clone();
    let (sid, turn, op) = (
        session_id.to_string(),
        turn_id.to_string(),
        op_id.to_string(),
    );
    let out_seq = Arc::new(AtomicU64::new(seq_base));
    Arc::new(move |stream: &str, offset: u64, text: String| {
        let stream = if stream == "stderr" {
            EventStream::Stderr
        } else {
            EventStream::Stdout
        };
        let seq = out_seq.fetch_add(1, Ordering::Relaxed);
        if let Some(e) = crate::events::output_event(&sid, seq, &turn, &op, stream, offset, text) {
            hub.publish(&sid, e);
        }
    })
}
