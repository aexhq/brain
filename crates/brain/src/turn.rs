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

use crate::adapter::{CallOutcome, CallRequest, HandAdapter};
use crate::config::{SealedPrefix, SessionConfig, ToolRoute};
use crate::events::EventHub;
use crate::journal::{self, HeadDoc, Journal, Lease, Record};
use crate::message::{ContentBlock, Message, StopReason};
use crate::provider::{Accumulator, Provider, ProviderEvent};
use crate::tools::TodoState;
use crate::{BrainError, Result, Shared};
use aex_contracts::session::EventStream;
use futures_util::StreamExt;
use std::sync::Arc;
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
    pub todo: Arc<TodoState>,
    /// Sealed MCP dispatch state, rebuilt at hydrate from the prefix doc. `None` when the
    /// session declares no MCP servers.
    pub mcp: Option<Arc<crate::mcp::McpRuntime>>,
    /// The next seq to allocate. Ephemeral events (deltas, tool output) consume seqs too;
    /// every commit persists the high-water mark.
    pub next_seq: u64,
}

impl TurnState {
    pub fn take_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
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
    /// Bounds concurrent model rounds across the whole brain (admission, D9/D11).
    pub model_permits: Arc<Semaphore>,
    pub history_budget_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct TurnReport {
    pub stop_reason: String,
    pub rounds: u64,
    pub tool_calls: u64,
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
        let high_water = st.next_seq - 1;
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
        let mut rounds: u64 = 0;
        let mut tool_calls: u64 = 0;
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
                return self.complete(st, "max_rounds", rounds, tool_calls).await;
            }

            // One model round.
            let (message, stop) = match self.model_round(st).await {
                Ok(v) => v,
                Err(BrainError::Cancelled) => {
                    return self.complete(st, "cancelled", rounds, tool_calls).await;
                }
                Err(e) => return Err(e),
            };
            rounds += 1;

            // Journal the decision: the complete assistant message and every tool intent --
            // one durable write, BEFORE dispatch.
            let calls: Vec<(String, String, serde_json::Value)> = message
                .tool_uses()
                .map(|(_, name, input)| (crate::mint_id("op", 16), name.to_string(), input.clone()))
                .collect();
            let provider_ids: Vec<String> = message
                .tool_uses()
                .map(|(id, _, _)| id.to_string())
                .collect();

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
                return self.complete(st, "end_turn", rounds, tool_calls).await;
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
                    tool_use_id: provider_ids[i].clone(),
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
            self.commit(st, result_records).await?;

            // Only after the results are durable may the substrate forget them.
            let ids: Vec<String> = calls.iter().map(|(id, _, _)| id.clone()).collect();
            st.hand.acknowledge(&ids).await;
            st.history.push(Message::tool_results(blocks));

            if self.cancel.is_cancelled() {
                return self.complete(st, "cancelled", rounds, tool_calls).await;
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
        })
    }

    /// One streamed model round: request built from (sealed prefix, history), deltas fanned
    /// out live, only the complete message returned.
    async fn model_round(&self, st: &mut TurnState) -> Result<(Message, StopReason)> {
        let req = self.provider.build_request(
            &self.prefix,
            &st.history,
            &self.session.key,
            &self.session.base_url,
        )?;
        let _permit = self
            .model_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| BrainError::Overloaded)?;

        let mut stream = tokio::select! {
            s = self.provider.stream(req) => s?,
            () = self.cancel.cancelled() => return Err(BrainError::Cancelled),
        };
        let mut acc = Accumulator::default();
        loop {
            let ev = tokio::select! {
                ev = stream.next() => ev,
                () = self.cancel.cancelled() => return Err(BrainError::Cancelled),
            };
            match ev {
                Some(Ok(ev)) => {
                    if let ProviderEvent::TextDelta { text, .. } = &ev {
                        let seq = st.take_seq();
                        if let Some(e) = crate::events::delta_event(
                            &self.session_id,
                            seq,
                            &self.turn_id,
                            "root",
                            text.clone(),
                        ) {
                            self.hub.publish(&self.session_id, e);
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
        if stop == StopReason::ToolUse && message.tool_uses().next().is_none() {
            return Err(BrainError::Protocol(
                "provider reported stop_reason=tool_use with no tool_use block".into(),
            ));
        }
        Ok((message, stop))
    }

    /// Dispatches one assistant message's calls through the adapter, bounded-parallel.
    /// Results come back in CALL order, every slot filled.
    async fn dispatch_batch(
        &self,
        st: &mut TurnState,
        calls: &[(String, String, serde_json::Value)],
    ) -> Result<Vec<CallOutcome>> {
        let needs_hand = calls.iter().any(|(_, name, _)| {
            matches!(
                self.prefix.tool(name).map(|t| t.route),
                Some(ToolRoute::Hand) | None
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
        let sem = Arc::new(Semaphore::new(self.prefix.limits.max_parallel_tools));
        let mut join = tokio::task::JoinSet::new();
        for (idx, (op_id, name, input)) in calls.iter().cloned().enumerate() {
            let route = self.prefix.tool(&name).map(|t| t.route);
            match route {
                None => {
                    // Undeclared: never dispatched, still answered.
                    join.spawn(async move {
                        (idx, CallOutcome::failed(crate::tools::undeclared(&name)))
                    });
                }
                Some(ToolRoute::Brain) => {
                    let todo = st.todo.clone();
                    join.spawn(async move {
                        let t0 = Instant::now();
                        let (content, is_error) = if name == "todo" {
                            todo.execute(&input)
                        } else {
                            (crate::tools::undeclared(&name), true)
                        };
                        (
                            idx,
                            CallOutcome {
                                outcome: if is_error {
                                    "failed".into()
                                } else {
                                    "completed".into()
                                },
                                content,
                                is_error,
                                exit_code: None,
                                duration_ms: t0.elapsed().as_millis() as u64,
                                truncated: false,
                            },
                        )
                    });
                }
                Some(ToolRoute::Mcp) => {
                    // Sealed MCP tools dispatch through the session's McpRuntime. The runtime
                    // was built at hydrate; a call for a session that somehow has none (or a
                    // tool the runtime no longer knows) is answered as an error, never a panic.
                    let runtime = st.mcp.clone();
                    let cancel = self.cancel.clone();
                    join.spawn(async move {
                        let out = match &runtime {
                            Some(rt) => rt.call(&name, &input, &cancel).await,
                            None => CallOutcome::failed(
                                "MCP dispatch state is missing for this session".to_string(),
                            ),
                        };
                        (idx, out)
                    });
                }
                Some(ToolRoute::Hand) => {
                    let hand = st.hand.clone();
                    let sem = sem.clone();
                    let cancel = self.cancel.clone();
                    let hub = self.hub.clone();
                    let sid = self.session_id.clone();
                    let turn = self.turn_id.clone();
                    let seq_base = st.next_seq + idx as u64 * 4096;
                    join.spawn(async move {
                        let _p = sem.acquire().await;
                        let out_seq = Arc::new(std::sync::atomic::AtomicU64::new(seq_base));
                        let sink: crate::adapter::OutputSink = {
                            let (sid2, turn2, op2) = (sid.clone(), turn.clone(), op_id.clone());
                            Arc::new(move |stream: &str, offset: u64, text: String| {
                                let stream = if stream == "stderr" {
                                    EventStream::Stderr
                                } else {
                                    EventStream::Stdout
                                };
                                let seq =
                                    out_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                if let Some(e) = crate::events::output_event(
                                    &sid2, seq, &turn2, &op2, stream, offset, text,
                                ) {
                                    hub.publish(&sid2, e);
                                }
                            })
                        };
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

        // Reserve an ephemeral seq window for the sinks' tool.output events. Coarse but safe:
        // durable seqs continue after the window; replay has gaps, ids never collide.
        if calls.iter().any(|(_, name, _)| {
            matches!(
                self.prefix.tool(name).map(|t| t.route),
                Some(ToolRoute::Hand)
            )
        }) {
            st.next_seq += calls.len() as u64 * 4096;
        }

        let mut done: Vec<Option<CallOutcome>> =
            std::iter::repeat_with(|| None).take(calls.len()).collect();
        while let Some(joined) = join.join_next().await {
            match joined {
                Ok((idx, out)) => done[idx] = Some(out),
                Err(e) => {
                    if let Some(slot) = done.iter_mut().find(|s| s.is_none()) {
                        *slot = Some(CallOutcome::failed(format!(
                            "tool task did not complete: {e}"
                        )));
                    }
                }
            }
        }
        Ok(done
            .into_iter()
            .map(|o| o.unwrap_or_else(|| CallOutcome::failed("tool produced no result")))
            .collect())
    }
}
