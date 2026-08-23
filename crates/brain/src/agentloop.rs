//! The agentloop seam: exactly one loop implementation drives each turn's policy, and every
//! durable mechanism stays on the kernel side of [`TurnCtx`].
//!
//! The public wire form of this seam is `contracts/agentloop/v1` (activations + ctx operations).
//! [`SequentialAgentloop`] is the in-process reference sequential policy — the engine composition of the
//! same seam a remote loop host will implement over the contract. Design record:
//! aex-research `docs/harness-extension-design.md` (HX4/HX5, §6¾ B).
//!
//! The kernel never lets a loop widen authority: model rounds run against the sealed
//! provider/model, dispatch validates against the sealed grant, and round/wall budgets are
//! enforced kernel-side regardless of what a loop decides.

use async_trait::async_trait;

use crate::BrainError;
use crate::journal::TurnStopReason;

type Result<T> = std::result::Result<T, BrainError>;

/// Gate before a model round: the kernel compacts and enforces context budgets.
#[derive(Debug, PartialEq, Eq)]
pub enum PrepareOutcome {
    /// The next round fits the sealed context budget.
    Ready,
    /// A required compaction could not complete; the turn ends `interrupted`.
    Interrupted,
}

/// One journaled model round, as the loop sees it. The kernel has already committed the
/// assistant message and every tool intent before the loop learns the outcome.
#[derive(Debug, PartialEq, Eq)]
pub enum RoundOutcome {
    /// The provider requested tool calls; they are journaled as intents, not yet dispatched.
    ToolCalls { count: usize },
    /// The provider finished with text.
    Final { refusal: bool },
    /// The turn was cancelled during the round.
    Cancelled,
    /// The provider recovery budget is exhausted; the turn ends `interrupted`.
    Interrupted,
}

/// What dispatching the journaled batch produced.
#[derive(Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Results are journaled; the loop decides what happens next.
    Continue,
    /// A `return_direct` tool committed the turn terminal atomically with its result.
    TerminalCommitted { stop_reason: TurnStopReason },
}

/// The verdict a loop returns; the kernel maps it onto the durable turn terminal.
#[derive(Debug, PartialEq, Eq)]
pub struct LoopVerdict {
    pub stop_reason: TurnStopReason,
    /// True when the terminal was already committed inside dispatch (`return_direct`).
    pub terminal_committed: bool,
}

impl LoopVerdict {
    fn stop(reason: TurnStopReason) -> Self {
        Self {
            stop_reason: reason,
            terminal_committed: false,
        }
    }
}

/// A terminal the loop declared through the contract's `turn_finish`/`turn_fail` ops. Recorded
/// on the ctx during the activation; the kernel maps it onto the durable turn terminal after
/// the loop returns — a loop declares outcomes, the kernel commits them.
#[derive(Debug, Clone)]
pub enum LoopTerminal {
    Finished {
        result: Option<serde_json::Map<String, serde_json::Value>>,
        /// The loop's terminal claim; `EndTurn` when unstated. Cancelled/interrupted stay
        /// kernel-owned outcomes a loop cannot claim.
        stop_reason: crate::journal::TurnStopReason,
    },
    Failed {
        error: brain_protocol::agentloop::AgentloopError,
    },
}

/// Convenience constructor for a guest-visible op error. The message is clamped to the
/// contract's 1..=4096 character bound rather than failing over a diagnostic string.
pub fn op_error(
    code: brain_protocol::agentloop::AgentloopErrorCode,
    message: impl Into<String>,
    retryable: bool,
) -> brain_protocol::agentloop::AgentloopError {
    let mut message: String = message.into().chars().take(4096).collect();
    if message.is_empty() {
        message = "unspecified agentloop error".into();
    }
    brain_protocol::agentloop::AgentloopError {
        code,
        message: message.parse().expect("clamped to the contract bound"),
        retryable,
        details: serde_json::Map::new(),
    }
}

/// A contract ctx-op outcome: `Ok(result)` for success, `Err(error)` for a guest-visible op
/// failure the loop may handle (invalid input, unsealed tool, provider failure). Kernel faults
/// never appear here — they travel as the outer `BrainError` and always fail the turn.
pub type ContractOpOutcome = std::result::Result<
    brain_protocol::agentloop::CtxOpResult,
    brain_protocol::agentloop::AgentloopError,
>;

/// Kernel-side capabilities one turn exposes to its agentloop. Every method journals before its
/// effect; delivery of one logical step is at-least-once with kernel-side deduplication, so a
/// loop implementation must treat repeated invocation after recovery as normal.
///
/// The `engine.*` methods drive kernel-managed context (the reference sequential policy); the
/// contract surface (`contract_op` and the activation payloads) is `contracts/agentloop/v1`,
/// where the loop composes its own context and the kernel executes and journals every effect.
#[async_trait]
pub trait TurnCtx: Send {
    /// Execute one `contracts/agentloop/v1` ctx operation.
    async fn contract_op(
        &mut self,
        op: brain_protocol::agentloop::CtxOp,
    ) -> Result<ContractOpOutcome> {
        let _ = op;
        Ok(Err(op_error(
            brain_protocol::agentloop::AgentloopErrorCode::Internal,
            "contract ctx operations are not available on this composition",
            false,
        )))
    }

    /// The `message` activation request for this turn (`contracts/agentloop/v1`
    /// `ActivationRequest`), serialized. The loop host passes it to the guest verbatim.
    fn activation_message(&self) -> Result<serde_json::Value> {
        Err(BrainError::Agentloop(
            "this composition cannot assemble activation payloads".into(),
        ))
    }

    /// The `session_start` activation request: kv map, latest mark and the bounded entry tail
    /// after it — the kernel's checkpoint-plus-tail hydration shape, pushed as data.
    async fn session_start_payload(&mut self) -> Result<serde_json::Value> {
        Err(BrainError::Agentloop(
            "this composition cannot assemble activation payloads".into(),
        ))
    }

    /// A terminal declared through `turn_finish`/`turn_fail` in this activation, if any.
    fn loop_terminal(&self) -> Option<&LoopTerminal> {
        None
    }
}

/// One agentloop implementation. Exactly one drives a session's turns.
#[async_trait]
pub trait Agentloop: Send + Sync {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict>;
}

/// Maps a session's sealed selector to the loop implementation this composition runs for it.
/// Resolution happens at create (rejecting unavailable loops before anything seals) and again
/// per turn (a session sealed under a richer composition fails honestly on a poorer one).
pub trait AgentloopRegistry: Send + Sync {
    fn resolve(
        &self,
        selector: &crate::journal::AgentloopSelectorDoc,
    ) -> Result<std::sync::Arc<dyn Agentloop>>;

    /// Admit a customer source bundle (already digest-verified by the caller) and return the
    /// identity to seal. Compositions with a loop store override this; the default refuses.
    fn admit_custom(
        &self,
        source_bundle_sha256: &str,
        toolchain: &str,
        bundle: &[u8],
    ) -> Result<crate::journal::AgentloopSelectorDoc> {
        let _ = (source_bundle_sha256, toolchain, bundle);
        Err(BrainError::Invalid(
            "custom agentloops are not enabled in this composition".into(),
        ))
    }
}

pub(crate) struct TestAgentloopRegistry;

impl AgentloopRegistry for TestAgentloopRegistry {
    fn resolve(
        &self,
        _selector: &crate::journal::AgentloopSelectorDoc,
    ) -> Result<std::sync::Arc<dyn Agentloop>> {
        Ok(std::sync::Arc::new(SequentialAgentloop))
    }

    fn admit_custom(
        &self,
        source_bundle_sha256: &str,
        toolchain: &str,
        bundle: &[u8],
    ) -> Result<crate::journal::AgentloopSelectorDoc> {
        Ok(crate::journal::AgentloopSelectorDoc {
            source_bundle_sha256: source_bundle_sha256.into(),
            source_bundle_bytes: bundle.len() as u64,
            toolchain: toolchain.into(),
        })
    }
}

/// The reference sequential loop policy, contract mode: the in-process twin of the wasm guest
/// (`crates/brain-loophost/guest/loop-aex.mjs`), driven entirely through
/// `contracts/agentloop/v1` ctx ops. Stateless per turn: it rebuilds loop memory from the
/// session_start hydration exactly as a fresh guest instance would, so residency is an
/// optimization the guest adds, never a semantic.
pub struct SequentialAgentloop;

type OpOutcome = std::result::Result<
    brain_protocol::agentloop::CtxOpResult,
    brain_protocol::agentloop::AgentloopError,
>;

impl SequentialAgentloop {
    fn message_from_view(view: &serde_json::Value) -> Option<serde_json::Value> {
        match view["type"].as_str() {
            Some("user_message") => Some(serde_json::json!({
                "role": "user", "content": view["content"],
            })),
            Some("assistant_message") => Some(serde_json::json!({
                "role": "assistant", "content": view["message"]["content"],
            })),
            Some("tool_result") => Some(serde_json::json!({
                "role": "tool_result",
                "tool_call_id": view["result"]["tool_call_id"],
                "name": view["result"]["name"],
                "is_error": view["result"]["is_error"],
                "content": view["result"]["content"],
            })),
            _ => None,
        }
    }

    fn text_of(content: &serde_json::Value) -> String {
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect()
    }

    fn summary_message(summary: &str) -> serde_json::Value {
        serde_json::json!({
            "role": "user",
            "content": [{
                "type": "text",
                "text": format!("<conversation_summary>\n{summary}\n</conversation_summary>"),
            }],
        })
    }

    async fn op(ctx: &mut dyn TurnCtx, body: serde_json::Value) -> Result<OpOutcome> {
        let op: brain_protocol::agentloop::CtxOp =
            serde_json::from_value(body).map_err(|error| {
                BrainError::Agentloop(format!("sequential loop op encoding: {error}"))
            })?;
        ctx.contract_op(op).await
    }

    async fn finish(ctx: &mut dyn TurnCtx, stop_reason: &str) -> Result<()> {
        let _ = Self::op(
            ctx,
            serde_json::json!({ "op": "turn_finish", "stop_reason": stop_reason }),
        )
        .await?;
        Ok(())
    }

    /// Loop-side compaction: summarize everything but a recent tail through the sealed
    /// model, then continue on `[summary] ++ tail`. The summarization call deliberately
    /// overrides the system text (dropping base reuse for that one round) and is bounded:
    /// one halving retry, then the honest budget error surfaces.
    async fn self_compact(
        ctx: &mut dyn TurnCtx,
        memory: &mut Vec<serde_json::Value>,
    ) -> Result<std::result::Result<(), brain_protocol::agentloop::AgentloopError>> {
        use brain_protocol::agentloop::{AgentloopErrorCode, CtxOpResult};
        // Keep a recent tail (bounded), summarize the rest; a conversation too short to
        // split cannot be compacted and surfaces the honest budget error.
        let tail_len = memory.len().saturating_sub(2).min(4);
        if memory.len() < tail_len + 2 || tail_len == 0 {
            return Ok(Err(op_error(
                AgentloopErrorCode::BudgetExceeded,
                "the conversation cannot be compacted further and exceeds the context budget",
                false,
            )));
        }
        let tail: Vec<serde_json::Value> = memory[memory.len() - tail_len..].to_vec();
        let mut head: Vec<serde_json::Value> = memory[..memory.len() - tail_len].to_vec();
        for _ in 0..2 {
            let mut messages = head.clone();
            messages.push(serde_json::json!({
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "Summarize the conversation above for a successor agent: goals, constraints, decisions, tool outcomes, unresolved failures, identifiers and next actions. Plain text.",
                }],
            }));
            let outcome = Self::op(
                ctx,
                serde_json::json!({
                    "op": "model_stream",
                    "request": {
                        "system": "You compact agent conversations. Reply with the summary text only.",
                        "messages": messages,
                    },
                }),
            )
            .await?;
            match outcome {
                Ok(CtxOpResult::ModelStream { message }) => {
                    let message = serde_json::to_value(&message)
                        .map_err(|error| BrainError::Agentloop(error.to_string()))?;
                    let summary = Self::text_of(&message["content"]);
                    let mut next = vec![Self::summary_message(&summary)];
                    next.extend(tail);
                    *memory = next;
                    return Ok(Ok(()));
                }
                Ok(_) => {
                    return Err(BrainError::Agentloop(
                        "model_stream answered with a foreign result".into(),
                    ));
                }
                Err(error)
                    if error.code == AgentloopErrorCode::BudgetExceeded && head.len() > 2 =>
                {
                    let drop = head.len() / 2;
                    head.drain(..drop);
                }
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(Err(op_error(
            brain_protocol::agentloop::AgentloopErrorCode::BudgetExceeded,
            "compaction could not fit the conversation into the context budget",
            false,
        )))
    }
}

#[async_trait]
impl Agentloop for SequentialAgentloop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict> {
        use brain_protocol::agentloop::{AgentloopErrorCode, CtxOpResult};
        let start = ctx.session_start_payload().await?;
        let activation = ctx.activation_message()?;
        let mut memory: Vec<serde_json::Value> = Vec::new();
        // The sealed fork prefix, already in the loop's own message shape, precedes everything.
        for message in start["inherited"].as_array().into_iter().flatten() {
            memory.push(message.clone());
        }
        if let Some(summary) = start["latest_mark"]["data"]["summary"].as_str() {
            memory.push(Self::summary_message(summary));
        }
        for view in start["tail"].as_array().into_iter().flatten() {
            if let Some(message) = Self::message_from_view(view) {
                memory.push(message);
            }
        }
        let admitted = &activation["message"];
        memory.push(serde_json::json!({"role": "user", "content": admitted["content"]}));
        let max_rounds = activation["session"]["limits"]["max_rounds_per_turn"]
            .as_u64()
            .unwrap_or(512);
        let mut rounds = 0u64;
        loop {
            let closing = rounds >= max_rounds;
            let mut request = serde_json::json!({ "messages": memory });
            if closing {
                request["tool_choice"] = serde_json::json!("none");
            }
            let outcome = Self::op(
                ctx,
                serde_json::json!({ "op": "model_stream", "request": request }),
            )
            .await?;
            let message = match outcome {
                Ok(CtxOpResult::ModelStream { message }) => serde_json::to_value(&message)
                    .map_err(|error| BrainError::Agentloop(error.to_string()))?,
                Ok(_) => {
                    return Err(BrainError::Agentloop(
                        "model_stream answered with a foreign result".into(),
                    ));
                }
                Err(error) if error.code == AgentloopErrorCode::Aborted => {
                    return Ok(LoopVerdict::stop(TurnStopReason::Cancelled));
                }
                Err(error) if error.code == AgentloopErrorCode::TurnAlreadyTerminal => {
                    return Ok(LoopVerdict::stop(TurnStopReason::EndTurn));
                }
                Err(error) if error.code == AgentloopErrorCode::BudgetExceeded && !closing => {
                    match Self::self_compact(ctx, &mut memory).await? {
                        Ok(()) => continue,
                        Err(error) => {
                            let _ = Self::op(
                                ctx,
                                serde_json::json!({ "op": "turn_fail", "error": error }),
                            )
                            .await?;
                            return Ok(LoopVerdict::stop(TurnStopReason::EndTurn));
                        }
                    }
                }
                Err(error) => {
                    let _ = Self::op(
                        ctx,
                        serde_json::json!({ "op": "turn_fail", "error": error }),
                    )
                    .await?;
                    return Ok(LoopVerdict::stop(TurnStopReason::EndTurn));
                }
            };
            rounds += 1;
            memory.push(serde_json::json!({
                "role": "assistant", "content": message["content"],
            }));
            if closing {
                Self::finish(ctx, "max_rounds").await?;
                return Ok(LoopVerdict::stop(TurnStopReason::MaxRounds));
            }
            if message["stop_reason"] == "refusal" {
                Self::finish(ctx, "refusal").await?;
                return Ok(LoopVerdict::stop(TurnStopReason::Refusal));
            }
            let calls: Vec<serde_json::Value> = message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|block| block["type"] == "tool_call")
                .map(|block| {
                    serde_json::json!({
                        "tool_call_id": block["tool_call_id"],
                        "name": block["name"],
                        "input": block["input"],
                    })
                })
                .collect();
            if calls.is_empty() {
                Self::finish(ctx, "end_turn").await?;
                return Ok(LoopVerdict::stop(TurnStopReason::EndTurn));
            }
            let dispatched = Self::op(
                ctx,
                serde_json::json!({ "op": "tools_dispatch", "calls": calls }),
            )
            .await?;
            match dispatched {
                Ok(CtxOpResult::ToolsDispatch { results }) => {
                    for result in results {
                        let result = serde_json::to_value(&result)
                            .map_err(|error| BrainError::Agentloop(error.to_string()))?;
                        memory.push(serde_json::json!({
                            "role": "tool_result",
                            "tool_call_id": result["tool_call_id"],
                            "name": result["name"],
                            "is_error": result["is_error"],
                            "content": result["content"],
                        }));
                    }
                }
                Ok(_) => {
                    return Err(BrainError::Agentloop(
                        "tools_dispatch answered with a foreign result".into(),
                    ));
                }
                Err(error) if error.code == AgentloopErrorCode::TurnAlreadyTerminal => {
                    return Ok(LoopVerdict::stop(TurnStopReason::EndTurn));
                }
                Err(error) if error.code == AgentloopErrorCode::Aborted => {
                    return Ok(LoopVerdict::stop(TurnStopReason::Cancelled));
                }
                Err(error) => {
                    return Err(BrainError::Agentloop(format!(
                        "{}: {}",
                        error.code,
                        error.message.as_str()
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_protocol::agentloop::{AgentloopErrorCode, CtxOp, CtxOpResult};

    /// One scripted step for a `model_stream` op: an assistant message view or an op error.
    enum Step {
        Message(serde_json::Value),
        Error(AgentloopErrorCode),
    }

    fn text_message(text: &str) -> Step {
        Step::Message(serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "stop_reason": "end_turn",
            "model": "scripted",
        }))
    }

    fn tool_message(name: &str) -> Step {
        Step::Message(serde_json::json!({
            "content": [{"type": "tool_call", "tool_call_id": "c1", "name": name, "input": {}}],
            "stop_reason": "tool_use",
            "model": "scripted",
        }))
    }

    fn refusal_message() -> Step {
        Step::Message(serde_json::json!({
            "content": [{"type": "text", "text": "no"}],
            "stop_reason": "refusal",
            "model": "scripted",
        }))
    }

    /// A scripted contract ctx: records the op sequence plus every model request, so the
    /// policy tests assert both ordering and the composed presentation (tool_choice, system).
    struct Scripted {
        script: Vec<Step>,
        max_rounds: u64,
        tail: Vec<serde_json::Value>,
        calls: Vec<&'static str>,
        model_requests: Vec<serde_json::Value>,
        marks: Vec<serde_json::Value>,
        finish: Option<(Option<serde_json::Value>, String)>,
        fail: Option<serde_json::Value>,
        terminal: Option<LoopTerminal>,
    }

    impl Scripted {
        fn new(script: Vec<Step>) -> Self {
            Self {
                script,
                max_rounds: 128,
                tail: Vec::new(),
                calls: Vec::new(),
                model_requests: Vec::new(),
                marks: Vec::new(),
                finish: None,
                fail: None,
                terminal: None,
            }
        }
    }

    #[async_trait]
    impl TurnCtx for Scripted {
        async fn contract_op(&mut self, op: CtxOp) -> Result<ContractOpOutcome> {
            let raw = serde_json::to_value(&op).expect("op encodes");
            match op {
                CtxOp::ModelStream { .. } => {
                    self.calls.push("model");
                    self.model_requests.push(raw["request"].clone());
                    match self.script.remove(0) {
                        Step::Message(message) => Ok(Ok(CtxOpResult::ModelStream {
                            message: serde_json::from_value(message).expect("message view"),
                        })),
                        Step::Error(code) => Ok(Err(op_error(code, "scripted", false))),
                    }
                }
                CtxOp::ToolsDispatch { calls } => {
                    self.calls.push("dispatch");
                    let results = calls
                        .iter()
                        .map(|call| {
                            serde_json::json!({
                                "tool_call_id": call.tool_call_id.as_str(),
                                "name": call.name.as_str(),
                                "is_error": false,
                                "content": [{"type": "text", "text": "ok"}],
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(Ok(CtxOpResult::ToolsDispatch {
                        results: serde_json::from_value(serde_json::Value::Array(results))
                            .expect("result views"),
                    }))
                }
                CtxOp::JournalAppend { .. } => {
                    self.calls.push("mark");
                    self.marks.push(raw["entries"].clone());
                    Ok(Ok(serde_json::from_value(serde_json::json!({
                        "op": "journal_append", "first_seq": 1, "last_seq": 1,
                    }))
                    .expect("append result")))
                }
                CtxOp::TurnFinish {
                    result,
                    stop_reason,
                } => {
                    self.calls.push("finish");
                    let reason = stop_reason
                        .map(|value| {
                            serde_json::to_value(value).expect("reason")[""]
                                .as_str()
                                .map(str::to_owned)
                        })
                        .map(|_| String::new());
                    let _ = reason;
                    let reason = raw["stop_reason"]
                        .as_str()
                        .unwrap_or("end_turn")
                        .to_string();
                    self.terminal = Some(LoopTerminal::Finished {
                        result: result.clone().map(|object| object.0),
                        stop_reason: match reason.as_str() {
                            "max_rounds" => TurnStopReason::MaxRounds,
                            "refusal" => TurnStopReason::Refusal,
                            _ => TurnStopReason::EndTurn,
                        },
                    });
                    self.finish = Some((
                        result.map(|object| serde_json::Value::Object(object.0)),
                        reason,
                    ));
                    Ok(Ok(serde_json::from_value(
                        serde_json::json!({"op": "turn_finish"}),
                    )
                    .expect("finish result")))
                }
                CtxOp::TurnFail { error } => {
                    self.calls.push("fail");
                    self.fail = Some(serde_json::to_value(&error).expect("error"));
                    self.terminal = Some(LoopTerminal::Failed { error });
                    Ok(Ok(serde_json::from_value(
                        serde_json::json!({"op": "turn_fail"}),
                    )
                    .expect("fail result")))
                }
                _ => Ok(Err(op_error(
                    AgentloopErrorCode::Internal,
                    "unscripted op",
                    false,
                ))),
            }
        }

        fn activation_message(&self) -> Result<serde_json::Value> {
            Ok(serde_json::json!({
                "message": {
                    "seq": 7,
                    "content": [{"type": "text", "text": "go"}],
                },
                "session": {
                    "limits": { "max_rounds_per_turn": self.max_rounds },
                    "metadata": { "tools": [{"name": "echo"}] },
                },
            }))
        }

        async fn session_start_payload(&mut self) -> Result<serde_json::Value> {
            Ok(serde_json::json!({
                "kv": {},
                "latest_mark": null,
                "tail": self.tail,
                "resumed": false,
            }))
        }

        fn loop_terminal(&self) -> Option<&LoopTerminal> {
            self.terminal.as_ref()
        }
    }

    fn drive(ctx: &mut Scripted) -> LoopVerdict {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop_raw() -> RawWaker {
            fn clone(_: *const ()) -> RawWaker {
                noop_raw()
            }
            fn noop(_: *const ()) {}
            RawWaker::new(
                std::ptr::null(),
                &RawWakerVTable::new(clone, noop, noop, noop),
            )
        }
        let waker = unsafe { Waker::from_raw(noop_raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = Box::pin(SequentialAgentloop.drive_turn(ctx));
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(verdict) => return verdict.expect("verdict"),
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn a_tool_round_dispatches_then_a_final_round_finishes() {
        let mut ctx = Scripted::new(vec![tool_message("echo"), text_message("done")]);
        let verdict = drive(&mut ctx);
        assert_eq!(verdict.stop_reason, TurnStopReason::EndTurn);
        assert_eq!(ctx.calls, vec!["model", "dispatch", "model", "finish"]);
        assert_eq!(ctx.finish.as_ref().unwrap().1, "end_turn");
        assert!(ctx.marks.is_empty(), "ordinary turns write no marks");
    }

    #[test]
    fn refusal_maps_to_the_refusal_stop_reason() {
        let mut ctx = Scripted::new(vec![refusal_message()]);
        let verdict = drive(&mut ctx);
        assert_eq!(verdict.stop_reason, TurnStopReason::Refusal);
        assert_eq!(ctx.finish.as_ref().unwrap().1, "refusal");
    }

    #[test]
    fn the_round_ceiling_closes_gracefully_with_tool_choice_none() {
        let mut ctx = Scripted::new(vec![tool_message("echo"), text_message("closing answer")]);
        ctx.max_rounds = 1;
        let verdict = drive(&mut ctx);
        assert_eq!(verdict.stop_reason, TurnStopReason::MaxRounds);
        assert_eq!(ctx.calls, vec!["model", "dispatch", "model", "finish"]);
        assert_eq!(
            ctx.model_requests[1]["tool_choice"], "none",
            "the closing round constrains tool choice"
        );
        assert!(
            ctx.model_requests[0].get("tool_choice").is_none(),
            "ordinary rounds never constrain tool choice"
        );
        assert_eq!(ctx.finish.as_ref().unwrap().1, "max_rounds");
    }

    #[test]
    fn ordinary_rounds_present_the_sealed_presentation_verbatim() {
        let mut ctx = Scripted::new(vec![text_message("done")]);
        drive(&mut ctx);
        let request = &ctx.model_requests[0];
        assert!(request.get("system").is_none(), "no system override");
        assert!(
            request.get("tools").is_none()
                || request["tools"].as_array().is_some_and(Vec::is_empty),
            "no tool re-presentation: the sealed grant (and its frozen base) applies"
        );
    }

    #[test]
    fn a_committed_terminal_stops_the_loop_cleanly() {
        let mut ctx = Scripted::new(vec![
            tool_message("echo"),
            Step::Error(AgentloopErrorCode::TurnAlreadyTerminal),
        ]);
        let verdict = drive(&mut ctx);
        assert_eq!(verdict.stop_reason, TurnStopReason::EndTurn);
        assert_eq!(ctx.calls, vec!["model", "dispatch", "model"]);
        assert!(ctx.finish.is_none(), "no second terminal is declared");
    }

    #[test]
    fn cancellation_maps_to_cancelled() {
        let mut ctx = Scripted::new(vec![Step::Error(AgentloopErrorCode::Aborted)]);
        let verdict = drive(&mut ctx);
        assert_eq!(verdict.stop_reason, TurnStopReason::Cancelled);
    }

    #[test]
    fn provider_errors_fail_the_turn_through_turn_fail() {
        let mut ctx = Scripted::new(vec![Step::Error(AgentloopErrorCode::ProviderError)]);
        drive(&mut ctx);
        assert_eq!(ctx.calls, vec!["model", "fail"]);
        assert_eq!(ctx.fail.as_ref().unwrap()["code"], "provider_error");
    }

    #[test]
    fn budget_exhaustion_triggers_loop_side_compaction_then_continues() {
        let mut ctx = Scripted::new(vec![
            Step::Error(AgentloopErrorCode::BudgetExceeded),
            text_message("the summary"),
            text_message("done after compaction"),
        ]);
        // Enough hydrated history that a head/tail split exists.
        ctx.tail = (0..6)
            .map(|i| {
                serde_json::json!({
                    "type": "user_message",
                    "content": [{"type": "text", "text": format!("m{i}")}],
                })
            })
            .collect();
        let verdict = drive(&mut ctx);
        assert_eq!(verdict.stop_reason, TurnStopReason::EndTurn);
        assert_eq!(ctx.calls, vec!["model", "model", "model", "finish"]);
        assert!(
            ctx.model_requests[1]["system"].as_str().is_some(),
            "the compaction round overrides the system text"
        );
        assert!(
            ctx.model_requests[2].get("system").is_none(),
            "the continuation returns to the sealed presentation"
        );
    }

    #[test]
    fn an_uncompactable_conversation_fails_honestly() {
        let mut ctx = Scripted::new(vec![Step::Error(AgentloopErrorCode::BudgetExceeded)]);
        drive(&mut ctx);
        assert_eq!(ctx.calls, vec!["model", "fail"]);
        assert_eq!(ctx.fail.as_ref().unwrap()["code"], "budget_exceeded");
    }
}
