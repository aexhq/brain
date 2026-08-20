//! In-process `task` subagents.
//!
//! A child is self-similar: it uses the session's provider, key, hand, MCP
//! runtime, cancellation token, and sealed prefix. Its history starts with the
//! supplied prompt and its durable records use a deterministic child agent id.
//!
//! Child journal writes are serialized by the root turn. A child sends a batch
//! to [`ChildJournal`] and waits for the root to commit it before dispatching
//! any tool intent. This preserves journal-before-dispatch without sharing a
//! mutable lease between concurrent children.

use crate::adapter::{CallOutcome, CallRequest, HandAdapter};
use crate::config::{SealedPrefix, SessionConfig, ToolRoute};
use crate::events::EventHub;
use crate::journal::Record;
use crate::message::{ContentBlock, Message};
use crate::provider::Provider;
use crate::tools::TaskInput;
use crate::{BrainError, Result, Shared};
use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// A child's final report is an ordinary tool result. Keep it well below the
/// journal record bound so the result envelope always fits.
const MAX_TASK_RESULT_BYTES: usize = 48 * 1024;

pub(crate) struct CommitRequest {
    pub records: Vec<Record>,
    pub committed: oneshot::Sender<()>,
}

/// The child side of the root-owned journal coordinator.
#[derive(Clone)]
pub(crate) struct ChildJournal {
    tx: mpsc::UnboundedSender<CommitRequest>,
}

impl ChildJournal {
    pub(crate) fn channel() -> (Self, mpsc::UnboundedReceiver<CommitRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Waits until the root has durably committed the records. A dropped ack
    /// means the root commit failed and the whole turn is already unwinding.
    async fn commit(&self, records: Vec<Record>) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let (committed, wait) = oneshot::channel();
        self.tx
            .send(CommitRequest { records, committed })
            .map_err(|_| BrainError::Journal("subagent journal coordinator stopped".into()))?;
        wait.await
            .map_err(|_| BrainError::Journal("subagent journal commit failed".into()))
    }
}

/// Everything a child needs from the session, clonable for recursion.
#[derive(Clone)]
pub(crate) struct SubagentCtx {
    pub session_id: String,
    pub turn_id: String,
    /// The one uniform child prefix, shared by every identity and depth.
    pub prefix: Shared<SealedPrefix>,
    pub session: SessionConfig,
    pub provider: Arc<dyn Provider>,
    pub provider_name: String,
    pub hub: Arc<EventHub>,
    pub cancel: CancellationToken,
    pub model_permits: Arc<Semaphore>,
    pub history_budget_bytes: usize,
    pub seq: crate::turn::Seq,
    pub hand: Arc<dyn HandAdapter>,
    pub mcp: Option<Arc<crate::mcp::McpRuntime>>,
    pub web: Arc<crate::web::WebRuntime>,
    /// Session-lifetime count of child identities already minted.
    pub identities: Arc<AtomicU64>,
    pub journal: ChildJournal,
}

/// Runs one `task` call. `depth` is the child's depth (root is depth 0).
pub(crate) fn run_task(
    ctx: Arc<SubagentCtx>,
    depth: u32,
    call_id: String,
    input: serde_json::Value,
) -> BoxFuture<'static, CallOutcome> {
    Box::pin(run_task_inner(ctx, depth, call_id, input))
}

async fn run_task_inner(
    ctx: Arc<SubagentCtx>,
    depth: u32,
    call_id: String,
    input: serde_json::Value,
) -> CallOutcome {
    let started = Instant::now();
    let limits = ctx.prefix.limits;
    let cap = limits.max_subagent_identities as u64;
    // The parent intent is already durable. Reserve its deterministic child
    // identity before validation so every non-cap-refused task intent spends
    // exactly one slot, matching the count rebuilt from the journal.
    if ctx
        .identities
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            (n < cap).then_some(n + 1)
        })
        .is_err()
    {
        return CallOutcome::failed(format!(
            "subagent identity limit ({cap} per session) reached; do this work yourself"
        ));
    }

    let parsed: TaskInput = match serde_json::from_value(input) {
        Ok(p) => p,
        Err(e) => return CallOutcome::failed(format!("invalid task input: {e}")),
    };
    if parsed.description.is_empty() || parsed.description.chars().count() > 100 {
        return CallOutcome::failed(
            "invalid task input: description must contain 1 to 100 characters",
        );
    }
    if parsed.prompt.is_empty() {
        return CallOutcome::failed("invalid task input: prompt must not be empty");
    }

    if depth > limits.max_subagent_depth {
        return CallOutcome::failed(format!(
            "subagent depth limit ({}) reached; do this work yourself",
            limits.max_subagent_depth
        ));
    }
    // Attribution is derivable from the already-journaled parent task intent.
    let agent_id = format!("agt_{}", call_id.strip_prefix("op_").unwrap_or(&call_id));
    tracing::info!(
        session = %ctx.session_id,
        agent = %agent_id,
        depth,
        description = %parsed.description,
        "subagent start"
    );

    let child_prefix = ctx.prefix.clone();
    let mut history = vec![Message::user_text(parsed.prompt)];
    let mut rounds = 0u64;

    loop {
        if !compact_history(&mut history, ctx.history_budget_bytes) {
            return failed(
                started,
                format!(
                    "subagent stopped: history exceeded the {} byte budget",
                    ctx.history_budget_bytes
                ),
            );
        }
        if rounds >= limits.max_rounds as u64 {
            return failed(
                started,
                format!(
                    "subagent stopped: max_rounds ({}) reached without a final report",
                    limits.max_rounds
                ),
            );
        }

        let round = crate::turn::model_round(
            crate::turn::RoundCtx {
                provider: &ctx.provider,
                prefix: &child_prefix,
                session: &ctx.session,
                permits: &ctx.model_permits,
                cancel: &ctx.cancel,
                hub: &ctx.hub,
                session_id: &ctx.session_id,
                turn_id: &ctx.turn_id,
                agent: &agent_id,
                seq: &ctx.seq,
            },
            &history,
        )
        .await;
        let (mut message, stop, usage) = match round {
            Ok(v) => v,
            Err(BrainError::Cancelled) => return cancelled(started),
            Err(e) => return failed(started, format!("subagent model round failed: {e}")),
        };
        rounds += 1;

        if let Err(e) = ctx
            .journal
            .commit(vec![Record::Usage {
                turn: ctx.turn_id.clone(),
                agent: agent_id.clone(),
                provider: ctx.provider_name.clone(),
                model: child_prefix.model.clone(),
                usage,
            }])
            .await
        {
            return failed(started, format!("subagent journal failed: {e}"));
        }

        let calls = crate::turn::mint_tool_calls(&mut message);

        // One decision: complete assistant message plus every intent. The
        // coordinator ack is the journal-before-dispatch barrier.
        let mut decision = Vec::with_capacity(1 + calls.len());
        decision.push(Record::Assistant {
            turn: ctx.turn_id.clone(),
            agent: agent_id.clone(),
            content: message.content.clone(),
            stop,
        });
        for (op_id, name, input) in &calls {
            decision.push(Record::ToolCall {
                turn: ctx.turn_id.clone(),
                agent: agent_id.clone(),
                call: op_id.clone(),
                name: name.clone(),
                input: input.clone(),
                detach: false,
            });
        }
        if let Err(e) = ctx.journal.commit(decision).await {
            return failed(started, format!("subagent journal failed: {e}"));
        }
        history.push(message.clone());

        if calls.is_empty() {
            let (content, truncated) = final_report(&message);
            tracing::info!(
                session = %ctx.session_id,
                agent = %agent_id,
                rounds,
                "subagent done"
            );
            return CallOutcome {
                outcome: "completed".into(),
                content,
                is_error: false,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                truncated,
                terminal: None,
            };
        }

        let dispatched = match dispatch(&ctx, depth, &child_prefix, &calls).await {
            Ok(v) => v,
            Err(BrainError::Cancelled) => return cancelled(started),
            Err(e) => return failed(started, format!("subagent dispatch failed: {e}")),
        };

        let mut records = Vec::with_capacity(calls.len());
        let mut blocks = Vec::with_capacity(calls.len());
        for (i, outcome) in dispatched.outcomes.iter().enumerate() {
            let content = visible_content(outcome);
            blocks.push(ContentBlock::ToolResult {
                tool_use_id: calls[i].0.clone(),
                content: content.clone(),
                is_error: outcome.is_error,
            });
            records.push(Record::ToolResult {
                turn: ctx.turn_id.clone(),
                agent: agent_id.clone(),
                call: calls[i].0.clone(),
                name: calls[i].1.clone(),
                outcome: outcome.outcome.clone(),
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            });
        }
        if let Err(e) = ctx.journal.commit(records).await {
            return failed(started, format!("subagent journal failed: {e}"));
        }
        // Only durable hand results may be forgotten by the substrate.
        if !dispatched.hand_ack_ids.is_empty() {
            ctx.hand.acknowledge(&dispatched.hand_ack_ids).await;
        }
        history.push(Message::tool_results(blocks));

        if ctx.cancel.is_cancelled() {
            return cancelled(started);
        }
    }
}

struct ChildDispatch {
    outcomes: Vec<CallOutcome>,
    hand_ack_ids: Vec<String>,
}

/// Dispatches one child assistant message. Recursive tasks use the same
/// coordinator, so every depth still crosses the root's durable-write seam.
async fn dispatch(
    ctx: &Arc<SubagentCtx>,
    depth: u32,
    child_prefix: &Shared<SealedPrefix>,
    calls: &[(String, String, serde_json::Value)],
) -> Result<ChildDispatch> {
    let route_of = |name: &str| child_prefix.tool(name).map(|t| t.route);
    let has_hand = calls
        .iter()
        .any(|(_, name, _)| matches!(route_of(name), Some(ToolRoute::Hand)));
    let mut hand_down = None;
    if has_hand {
        match ctx.hand.ensure_ready().await {
            Ok(None) => {}
            Ok(Some(lost)) => {
                tracing::warn!(
                    session = %ctx.session_id,
                    reason = %lost.reason,
                    "hand lost under a subagent"
                );
                let synced_ms = ctx
                    .hand
                    .hand_info()
                    .last_sync_at
                    .as_ref()
                    .map(|t| t.0.timestamp_millis() as u64);
                ctx.journal
                    .commit(vec![Record::HandLost {
                        turn: Some(ctx.turn_id.clone()),
                        interrupted: vec![],
                        synced_ms,
                    }])
                    .await?;
            }
            Err(e) => hand_down = Some(e.to_string()),
        }
    }

    let parallel = calls.len() > 1;
    let permits = Arc::new(Semaphore::new(ctx.prefix.limits.max_parallel_tools.max(1)));
    let mut joins = tokio::task::JoinSet::new();
    for (idx, (op_id, name, input)) in calls.iter().cloned().enumerate() {
        let permit = permits.clone();
        match route_of(&name) {
            None => joins.spawn(async move {
                let _permit = permit.acquire_owned().await;
                (
                    idx,
                    CallOutcome::failed(crate::tools::undeclared(&name)),
                    None,
                )
            }),
            Some(ToolRoute::Brain) if name == "task" => {
                let child_ctx = ctx.clone();
                joins.spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    let outcome = run_task(child_ctx, depth + 1, op_id, input).await;
                    (idx, outcome, None)
                })
            }
            Some(ToolRoute::Brain) => joins.spawn(async move {
                let _permit = permit.acquire_owned().await;
                (
                    idx,
                    CallOutcome::failed(crate::tools::undeclared(&name)),
                    None,
                )
            }),
            Some(ToolRoute::Mcp) => {
                let runtime = ctx.mcp.clone();
                let cancel = ctx.cancel.clone();
                joins.spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    let outcome = match &runtime {
                        Some(runtime) => runtime.call(&name, &input, &cancel).await,
                        None => {
                            CallOutcome::failed("MCP dispatch state is missing for this session")
                        }
                    };
                    (idx, outcome, None)
                })
            }
            Some(ToolRoute::Web) => {
                let web = ctx.web.clone();
                let cancel = ctx.cancel.clone();
                joins.spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    let outcome = web.call(&name, &input, &cancel).await;
                    (idx, outcome, None)
                })
            }
            Some(ToolRoute::External(_)) => joins.spawn(async move {
                let _permit = permit.acquire_owned().await;
                (
                    idx,
                    CallOutcome::failed(format!(
                        "external tool {name} is not available to this subagent"
                    )),
                    None,
                )
            }),
            Some(ToolRoute::Hand) => {
                if let Some(error) = &hand_down {
                    let error = error.clone();
                    joins.spawn(async move {
                        let _permit = permit.acquire_owned().await;
                        (
                            idx,
                            CallOutcome::failed(format!("hand unavailable: {error}")),
                            None,
                        )
                    });
                    continue;
                }
                let hand = ctx.hand.clone();
                let cancel = ctx.cancel.clone();
                let seq_base = ctx.seq.fetch_add(4096, Ordering::Relaxed);
                let sink = crate::turn::output_sink(
                    &ctx.hub,
                    &ctx.session_id,
                    &ctx.turn_id,
                    &op_id,
                    seq_base,
                );
                joins.spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    let outcome = hand
                        .call(
                            CallRequest {
                                call_id: op_id.clone(),
                                tool: name,
                                input,
                                parallel,
                            },
                            cancel,
                            sink,
                        )
                        .await;
                    (idx, outcome, Some(op_id))
                })
            }
        };
    }

    let mut outcomes: Vec<Option<CallOutcome>> =
        std::iter::repeat_with(|| None).take(calls.len()).collect();
    let mut hand_ack_ids = Vec::new();
    while let Some(joined) = joins.join_next().await {
        match joined {
            Ok((idx, outcome, ack_id)) => {
                outcomes[idx] = Some(outcome);
                if let Some(id) = ack_id {
                    hand_ack_ids.push(id);
                }
            }
            Err(error) => {
                if let Some(slot) = outcomes.iter_mut().find(|outcome| outcome.is_none()) {
                    *slot = Some(CallOutcome::failed(format!(
                        "tool task did not complete: {error}"
                    )));
                }
            }
        }
    }

    Ok(ChildDispatch {
        outcomes: outcomes
            .into_iter()
            .map(|outcome| {
                outcome.unwrap_or_else(|| CallOutcome::failed("tool produced no result"))
            })
            .collect(),
        hand_ack_ids,
    })
}

fn visible_content(outcome: &CallOutcome) -> String {
    if outcome.content.is_empty() {
        format!("[{}: no output]", outcome.outcome)
    } else {
        outcome.content.clone()
    }
}

/// Child compaction is deterministic but not journaled: children are never
/// resumed mid-flight, and their original records remain intact for audit.
fn compact_history(history: &mut Vec<Message>, budget_bytes: usize) -> bool {
    if let Some((summary, kept)) = crate::compact::plan(history, budget_bytes) {
        let kept = (kept as usize).min(history.len());
        let tail = history.split_off(history.len() - kept);
        history.clear();
        history.push(Message::user_text(summary));
        history.extend(tail);
    }
    history.iter().map(Message::heap_bytes).sum::<usize>() <= budget_bytes
}

fn cancelled(started: Instant) -> CallOutcome {
    CallOutcome {
        outcome: "cancelled".into(),
        content: "subagent cancelled".into(),
        is_error: true,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated: false,
        terminal: None,
    }
}

fn failed(started: Instant, content: String) -> CallOutcome {
    let (content, truncated) = bounded_result(content);
    CallOutcome {
        outcome: "failed".into(),
        content,
        is_error: true,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
        terminal: None,
    }
}

/// Extracts the final assistant text and tail-retains oversized reports.
fn final_report(message: &Message) -> (String, bool) {
    let text = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return ("[subagent returned no text]".into(), false);
    }
    bounded_result(text)
}

fn bounded_result(text: String) -> (String, bool) {
    if text.len() <= MAX_TASK_RESULT_BYTES {
        return (text, false);
    }
    let mut start = text.len() - MAX_TASK_RESULT_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (format!("[...truncated...]\n{}", &text[start..]), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_report_tail_retains_utf8_safely() {
        let text = format!("prefix{}", "é".repeat(MAX_TASK_RESULT_BYTES));
        let (kept, truncated) = final_report(&Message::assistant(vec![ContentBlock::text(text)]));
        assert!(truncated);
        assert!(kept.starts_with("[...truncated...]\n"));
        assert!(kept.is_char_boundary(kept.len()));
        assert!(kept.len() <= MAX_TASK_RESULT_BYTES + 32);
    }

    #[test]
    fn an_uncompactable_child_history_stops_at_the_budget() {
        let mut history = vec![Message::user_text("x".repeat(1024))];
        assert!(!compact_history(&mut history, 128));
    }
}
