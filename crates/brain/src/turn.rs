//! The turn loop: model round -> journal the decision -> dispatch tools -> journal results ->
//! feed back -> repeat, until the model stops or a cap fires.
//!
//! Rules the loop holds:
//! - only a COMPLETE assistant message is journaled; a stream that dies mid-message leaves no
//!   trace in history;
//! - tool intents are journaled BEFORE dispatch (an ambiguous outcome is recorded as
//!   possibly-run) and results are journaled before `release` tells the hand to forget them;
//! - the error flag on a failed tool result is always set (a dropped flag turns a failure
//!   into a success in the model's eyes);
//! - a lost hand interrupts its calls -- `interrupted`, never replayed (I10) -- and the turn
//!   goes on: the next hand-routed call re-materialises a fresh incarnation;
//! - cancellation is graceful: in-flight calls get `cancel` with a grace, results are
//!   journaled `cancelled`, the turn completes with `stop_reason = cancelled`.

use crate::config::{SealedPrefix, SessionConfig, ToolRoute};
use crate::events::EventHub;
use crate::hand::HandRuntime;
use crate::journal::{self, HeadDoc, Journal, Lease, Record};
use crate::message::{ContentBlock, Message, StopReason};
use crate::provider::{Accumulator, Provider, ProviderEvent};
use crate::tools::TodoState;
use crate::{BrainError, Result, Shared};
use aex_contracts::abi::{
    CancelRequest, Cursor, LaneMode, LaneRef, OperationStatus, PollRequest, ReleaseRequest,
    Stream as AbiStream,
};
use aex_contracts::session::EventStream;
use base64::Engine;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Long-poll windows against the hand. `start` waits briefly (most calls are short: one round
/// trip); `poll` waits long (the hand answers early on state change).
const START_WAIT_MS: u64 = 10_000;
const POLL_WAIT_MS: u64 = 30_000;
const START_MAX_BYTES: u64 = 64 * 1024;
const POLL_MAX_BYTES: u64 = 256 * 1024;
/// SIGTERM grace before SIGKILL on cancel.
const CANCEL_GRACE_MS: u64 = 2_000;

/// Everything a turn borrows from the session for its lifetime. The actor moves this in and
/// gets it back when the turn future resolves, mutated.
pub struct TurnState {
    pub history: Vec<Message>,
    pub head: HeadDoc,
    pub lease: Lease,
    pub hand: HandRuntime,
    pub todo: Arc<TodoState>,
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

struct CallOutcome {
    outcome: String,
    content: String,
    is_error: bool,
    exit_code: Option<i64>,
    duration_ms: u64,
    truncated: bool,
}

impl TurnRun {
    /// Publishes the SSE events for freshly committed records. Call AFTER the commit: an event
    /// a client saw must exist in the journal.
    fn publish_records(&self, st: &TurnState, records: &[(u64, Record)]) {
        let info = crate::hand::hand_info(&st.head);
        let now = crate::wall_ms();
        for (seq, record) in records {
            if let Some(e) = crate::events::derive(&self.session_id, *seq, now, record, &info) {
                self.hub.publish(&self.session_id, e);
            }
        }
    }

    async fn commit(&self, st: &mut TurnState, records: Vec<(u64, Record)>) -> Result<()> {
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

            // Journal the decision: the complete assistant message, its usage, and every tool
            // intent -- one durable write, BEFORE dispatch.
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
            // Usage was folded into the accumulator's terminal event; carried on the message
            // via the round report below.
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

            // Only after the results are durable may the hand forget them.
            if let Some(client) = st.hand.client() {
                let ids = calls
                    .iter()
                    .filter_map(|(id, _, _)| id.parse().ok())
                    .collect();
                let _ = client.release(ReleaseRequest { operation_ids: ids }).await;
            }
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
        // Usage rides its own record so `model.usage` has its own seq. Committed with the
        // assistant message by the caller? No: usage is known here; journal it here to keep
        // per-round granularity even when the round produced no tool calls.
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

    /// Dispatches one assistant message's calls. Parallel over one ephemeral lane per batch
    /// (they share a (cwd, env) snapshot and discard mutations); a single call runs on the
    /// caller's persistent lane. Results come back in CALL order, every slot filled.
    async fn dispatch_batch(
        &self,
        st: &mut TurnState,
        calls: &[(String, String, serde_json::Value)],
    ) -> Result<Vec<CallOutcome>> {
        // Route check first: an undeclared tool is a typed per-call failure the model sees.
        let needs_hand = calls.iter().any(|(_, name, _)| {
            matches!(
                self.prefix.tool(name).map(|t| t.route),
                Some(ToolRoute::Hand) | None
            )
        });
        if needs_hand {
            match st.hand.ensure_ready(&mut st.head).await {
                Ok(None) => {}
                Ok(Some(lost)) => {
                    // A previous incarnation died between turns: nothing was in flight, but
                    // clients are told, durably.
                    tracing::warn!(session = %self.session_id, reason = %lost.reason, "hand lost between turns");
                    let seq = st.take_seq();
                    self.commit(
                        st,
                        vec![(
                            seq,
                            Record::HandLost {
                                turn: Some(self.turn_id.clone()),
                                interrupted: vec![],
                                synced_ms: st.head.sync.synced_ms,
                            },
                        )],
                    )
                    .await?;
                }
                Err(e) => return Err(e),
            }
            st.hand.hold_up();
        }

        // A lane serializes its operations, so parallel calls each fork their OWN ephemeral
        // lane off the caller's (shared cwd/env snapshot, mutations discarded). A single call
        // runs on the persistent root lane.
        let batch = calls.len() > 1;

        let sem = Arc::new(Semaphore::new(self.prefix.limits.max_parallel_tools));
        let mut join = tokio::task::JoinSet::new();
        for (idx, (op_id, name, input)) in calls.iter().cloned().enumerate() {
            let route = self.prefix.tool(&name).map(|t| t.route);
            match route {
                None => {
                    // Undeclared: never dispatched, still answered.
                    join.spawn(async move {
                        (
                            idx,
                            CallOutcome {
                                outcome: "failed".into(),
                                content: crate::tools::undeclared(&name),
                                is_error: true,
                                exit_code: None,
                                duration_ms: 0,
                                truncated: false,
                            },
                        )
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
                Some(ToolRoute::Connector) => {
                    join.spawn(async move {
                        (
                            idx,
                            CallOutcome {
                                outcome: "failed".into(),
                                content: format!("tool {name} routes to the connector tier (M1)"),
                                is_error: true,
                                exit_code: None,
                                duration_ms: 0,
                                truncated: false,
                            },
                        )
                    });
                }
                Some(ToolRoute::Hand) if st.hand.local().is_some() => {
                    let hand = st.hand.local().expect("guarded").clone();
                    let cancel = self.cancel.clone();
                    let hub = self.hub.clone();
                    let sid = self.session_id.clone();
                    let turn = self.turn_id.clone();
                    let sem = sem.clone();
                    let seq_base = st.next_seq + idx as u64 * 4096;
                    join.spawn(async move {
                        let _p = sem.acquire().await;
                        let out_seq = Arc::new(std::sync::atomic::AtomicU64::new(seq_base));
                        let emit = {
                            let hub = hub.clone();
                            let (sid, turn, op) = (sid.clone(), turn.clone(), op_id.clone());
                            move |stream: &str, offset: u64, text: String| {
                                let stream = if stream == "stderr" {
                                    EventStream::Stderr
                                } else {
                                    EventStream::Stdout
                                };
                                let seq =
                                    out_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                if let Some(e) = crate::events::output_event(
                                    &sid, seq, &turn, &op, stream, offset, text,
                                ) {
                                    hub.publish(&sid, e);
                                }
                            }
                        };
                        let out = hand.call(&name, input, &cancel, emit).await;
                        (
                            idx,
                            CallOutcome {
                                outcome: out.outcome.into(),
                                content: out.content,
                                is_error: out.is_error,
                                exit_code: out.exit_code,
                                duration_ms: out.duration_ms,
                                truncated: false,
                            },
                        )
                    });
                }
                Some(ToolRoute::Hand) => {
                    let lane = if batch {
                        LaneRef {
                            id: crate::mint_id("lane", 12)
                                .parse()
                                .map_err(|_| BrainError::Invalid("lane id".into()))?,
                            mode: LaneMode::Ephemeral,
                            parent: Some("0".parse().expect("root lane id")),
                        }
                    } else {
                        hand_client::root_lane()
                    };
                    let Some(client) = st.hand.client() else {
                        join.spawn(async move {
                            (
                                idx,
                                CallOutcome {
                                    outcome: "failed".into(),
                                    content: "hand unavailable".into(),
                                    is_error: true,
                                    exit_code: None,
                                    duration_ms: 0,
                                    truncated: false,
                                },
                            )
                        });
                        continue;
                    };
                    let sem = sem.clone();
                    let cancel = self.cancel.clone();
                    let hub = self.hub.clone();
                    let sid = self.session_id.clone();
                    let turn = self.turn_id.clone();
                    let seq_base = st.next_seq; // ephemeral output seqs: see note below
                    join.spawn(async move {
                        let _p = sem.acquire().await;
                        let out = hand_call(
                            &client,
                            &sid,
                            &turn,
                            &hub,
                            &cancel,
                            &lane,
                            &op_id,
                            &name,
                            input,
                            seq_base + idx as u64 * 4096,
                        )
                        .await;
                        (idx, out)
                    });
                }
            }
        }

        // Reserve an ephemeral seq window for tool.output events emitted inside the tasks.
        // Coarse but safe: durable seqs continue after the window; replay has gaps, ids never
        // collide.
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
                        *slot = Some(CallOutcome {
                            outcome: "failed".into(),
                            content: format!("tool task did not complete: {e}"),
                            is_error: true,
                            exit_code: None,
                            duration_ms: 0,
                            truncated: false,
                        });
                    }
                }
            }
        }
        st.hand.let_idle();
        Ok(done
            .into_iter()
            .map(|o| {
                o.unwrap_or(CallOutcome {
                    outcome: "failed".into(),
                    content: "tool produced no result".into(),
                    is_error: true,
                    exit_code: None,
                    duration_ms: 0,
                    truncated: false,
                })
            })
            .collect())
    }
}

fn decode_slices(
    slices: &[aex_contracts::abi::OutputSlice],
    stdout: &mut String,
    stderr: &mut String,
) {
    for s in slices {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s.data_base64)
            .unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        match s.stream {
            AbiStream::Stdout => stdout.push_str(&text),
            AbiStream::Stderr => stderr.push_str(&text),
        }
    }
}

/// One hand tool call to terminal state: start (short wait), poll loop (long wait), output
/// events streamed as slices arrive, cancel honoured with grace.
#[allow(clippy::too_many_arguments)]
async fn hand_call(
    client: &hand_client::HandClient,
    session_id: &str,
    turn_id: &str,
    hub: &EventHub,
    cancel: &CancellationToken,
    lane: &LaneRef,
    op_id: &str,
    tool: &str,
    input: serde_json::Value,
    mut out_seq: u64,
) -> CallOutcome {
    let t0 = Instant::now();
    let fail = |content: String, outcome: &str, t0: Instant| CallOutcome {
        outcome: outcome.into(),
        content,
        is_error: outcome != "completed",
        exit_code: None,
        duration_ms: t0.elapsed().as_millis() as u64,
        truncated: false,
    };

    let started = match client
        .start(hand_client::start_request(
            op_id,
            tool,
            input,
            lane.clone(),
            None,
            false,
            START_WAIT_MS,
            START_MAX_BYTES,
        ))
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return fail(
                format!("hand start failed: {e}"),
                interrupted_or_failed(&e),
                t0,
            );
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut view = started.view;
    decode_slices(&started.slices, &mut stdout, &mut stderr);
    publish_output(
        hub,
        session_id,
        turn_id,
        op_id,
        &mut out_seq,
        &started.slices,
    );

    let mut cancelled = false;
    while view.status != OperationStatus::Terminal {
        if cancel.is_cancelled() && !cancelled {
            cancelled = true;
            let _ = client
                .cancel(CancelRequest {
                    operation_id: match op_id.parse() {
                        Ok(id) => id,
                        Err(_) => return fail("operation id".into(), "failed", t0),
                    },
                    grace_ms: Some(CANCEL_GRACE_MS),
                })
                .await;
        }
        let poll = client
            .poll(PollRequest {
                operation_id: match op_id.parse() {
                    Ok(id) => id,
                    Err(_) => return fail("operation id".into(), "failed", t0),
                },
                cursors: vec![
                    Cursor {
                        stream: AbiStream::Stdout,
                        offset: stdout.len() as u64,
                    },
                    Cursor {
                        stream: AbiStream::Stderr,
                        offset: stderr.len() as u64,
                    },
                ],
                wait_ms: POLL_WAIT_MS,
                max_bytes: POLL_MAX_BYTES,
            })
            .await;
        match poll {
            Ok(p) => {
                decode_slices(&p.slices, &mut stdout, &mut stderr);
                publish_output(hub, session_id, turn_id, op_id, &mut out_seq, &p.slices);
                view = p.view;
            }
            Err(e) => {
                // The connection died under the call. I10: the loss is classified by the
                // session layer; here the call is interrupted, never replayed.
                return fail(
                    format!("hand connection lost mid-call: {e}"),
                    "interrupted",
                    t0,
                );
            }
        }
    }

    let terminal = view.terminal.as_ref();
    let outcome = terminal
        .map(|t| match t.outcome {
            aex_contracts::abi::Outcome::Completed => "completed",
            aex_contracts::abi::Outcome::Failed => "failed",
            aex_contracts::abi::Outcome::Cancelled => "cancelled",
            aex_contracts::abi::Outcome::DeadlineExceeded => "deadline_exceeded",
            aex_contracts::abi::Outcome::Interrupted => "interrupted",
        })
        .unwrap_or("failed")
        .to_string();
    let exit_code = terminal.and_then(|t| t.exit_code);

    let mut content = stdout;
    if !stderr.is_empty() {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[stderr]\n");
        content.push_str(&stderr);
    }
    if let Some(t) = terminal {
        if let Some(err) = &t.error {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&format!("[error] {}: {}", err.code, err.message));
        }
        if let Some(out) = &t.output {
            let is_bash = tool == "bash";
            let timed_out = out
                .get("timed_out")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_bash && timed_out {
                content = format!("[command timed out]\n{content}");
            }
            if !is_bash {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(&format!("[meta] {out}"));
            }
        }
    }
    let mut truncated = false;
    if content.len() > journal::MAX_RECORD_CONTENT_BYTES {
        // Tail-retained: the end of the output is where compilers and tests put the verdict.
        let keep_from = content.len() - journal::MAX_RECORD_CONTENT_BYTES;
        let mut start = keep_from;
        while !content.is_char_boundary(start) {
            start += 1;
        }
        content = format!(
            "[output truncated: first {start} bytes elided]\n{}",
            &content[start..]
        );
        truncated = true;
    }

    let is_error = outcome != "completed";
    CallOutcome {
        outcome,
        content,
        is_error,
        exit_code,
        duration_ms: t0.elapsed().as_millis() as u64,
        truncated,
    }
}

fn interrupted_or_failed(e: &hand_client::ClientError) -> &'static str {
    let s = e.to_string();
    if s.contains("connection") || s.contains("closed") || s.contains("timed out") {
        "interrupted"
    } else {
        "failed"
    }
}

fn publish_output(
    hub: &EventHub,
    session_id: &str,
    turn_id: &str,
    op_id: &str,
    out_seq: &mut u64,
    slices: &[aex_contracts::abi::OutputSlice],
) {
    for s in slices {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s.data_base64)
            .unwrap_or_default();
        if bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        let stream = match s.stream {
            AbiStream::Stdout => EventStream::Stdout,
            AbiStream::Stderr => EventStream::Stderr,
        };
        let seq = *out_seq;
        *out_seq += 1;
        if let Some(e) =
            crate::events::output_event(session_id, seq, turn_id, op_id, stream, s.offset, text)
        {
            hub.publish(session_id, e);
        }
    }
}
