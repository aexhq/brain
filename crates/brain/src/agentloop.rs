//! The agentloop seam: exactly one loop implementation drives each turn's policy, and every
//! durable mechanism stays on the kernel side of [`TurnCtx`].
//!
//! The public wire form of this seam is `contracts/agentloop/v1` (activations + ctx operations).
//! [`BuiltinAexLoop`] is the in-process official `aex` policy — the engine composition of the
//! same seam a remote loop host will implement over the contract. Design record:
//! aex-research `docs/harness-extension-design.md` (HX4/HX5, §6¾ B).
//!
//! The kernel never lets a loop widen authority: model rounds run against the sealed
//! provider/model, dispatch validates against the sealed grant, and round/wall budgets are
//! enforced kernel-side regardless of what a loop decides.

use async_trait::async_trait;

use crate::BrainError;

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
    TerminalCommitted { stop_reason: String },
}

/// The verdict a loop returns; the kernel maps it onto the durable turn terminal.
#[derive(Debug, PartialEq, Eq)]
pub struct LoopVerdict {
    pub stop_reason: String,
    /// True when the terminal was already committed inside dispatch (`return_direct`).
    pub terminal_committed: bool,
}

impl LoopVerdict {
    fn stop(reason: &str) -> Self {
        Self {
            stop_reason: reason.into(),
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
/// The `engine.*` methods drive kernel-managed context (the official `aex` policy); the
/// contract surface (`contract_op` and the activation payloads) is `contracts/agentloop/v1`,
/// where the loop composes its own context and the kernel executes and journals every effect.
#[async_trait]
pub trait TurnCtx: Send {
    /// Compact until the next round fits the sealed context budget.
    async fn prepare_round(&mut self) -> Result<PrepareOutcome>;
    /// Run one model round against the kernel-managed context and journal its decisions.
    async fn model_round(&mut self) -> Result<RoundOutcome>;
    /// Dispatch the round's journaled tool intents and journal their results.
    async fn dispatch_pending(&mut self) -> Result<DispatchOutcome>;
    /// The graceful at-cap round: one more model round with `tool_choice: none` so the model
    /// wraps the turn up in text. Tool calls a defective provider still returns are journaled
    /// with immediate not-executed results and never dispatched.
    async fn closing_round(&mut self) -> Result<RoundOutcome> {
        Err(BrainError::Agentloop(
            "this composition has no closing round".into(),
        ))
    }
    /// Completed model rounds in this turn, cumulative across recovery.
    fn rounds(&self) -> u64;
    /// The sealed per-turn round ceiling (kernel authorization, not loop policy).
    fn max_rounds(&self) -> u64;
    /// True once the turn has been cancelled.
    fn cancelled(&self) -> bool;

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

    /// Resolve an official loop name to the pinned identity this composition seals for it.
    fn pin_official(&self, name: &str) -> Result<crate::journal::AgentloopSelectorDoc>;

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

/// The default registry: exactly one official loop, `aex`, backed by whatever implementation
/// the composition installed (in-process builtin, wasm guest, or remote loop host).
pub struct OfficialAexRegistry {
    pub aex: std::sync::Arc<dyn Agentloop>,
}

impl AgentloopRegistry for OfficialAexRegistry {
    fn resolve(
        &self,
        selector: &crate::journal::AgentloopSelectorDoc,
    ) -> Result<std::sync::Arc<dyn Agentloop>> {
        match selector {
            crate::journal::AgentloopSelectorDoc::Official { name, .. } if name == "aex" => {
                Ok(self.aex.clone())
            }
            crate::journal::AgentloopSelectorDoc::Official { name, version } => {
                Err(BrainError::Invalid(format!(
                    "official agentloop {name}@{version} is not available in this composition"
                )))
            }
            crate::journal::AgentloopSelectorDoc::Custom { .. } => Err(BrainError::Invalid(
                "custom agentloops are not enabled in this composition".into(),
            )),
        }
    }

    fn pin_official(&self, name: &str) -> Result<crate::journal::AgentloopSelectorDoc> {
        if name == "aex" {
            Ok(crate::journal::AgentloopSelectorDoc::official_aex())
        } else {
            Err(BrainError::Invalid(format!(
                "official agentloop {name:?} is not available in this composition"
            )))
        }
    }
}

/// The official `aex` loop policy: model round, dispatch every requested tool call, continue
/// until the model finishes, stop gracefully at the sealed round ceiling.
pub struct BuiltinAexLoop;

#[async_trait]
impl Agentloop for BuiltinAexLoop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict> {
        loop {
            if ctx.prepare_round().await? == PrepareOutcome::Interrupted {
                return Ok(LoopVerdict::stop("interrupted"));
            }
            if ctx.rounds() >= ctx.max_rounds() {
                // The cap is reached mid-work: close gracefully with one text-only round so
                // the turn ends with an answer, not a truncation. The stop reason still says
                // the ceiling was the reason the loop stopped.
                return Ok(LoopVerdict::stop(match ctx.closing_round().await? {
                    RoundOutcome::Cancelled => "cancelled",
                    RoundOutcome::Interrupted => "interrupted",
                    RoundOutcome::Final { .. } | RoundOutcome::ToolCalls { .. } => "max_rounds",
                }));
            }
            match ctx.model_round().await? {
                RoundOutcome::Cancelled => return Ok(LoopVerdict::stop("cancelled")),
                RoundOutcome::Interrupted => return Ok(LoopVerdict::stop("interrupted")),
                RoundOutcome::Final { refusal } => {
                    return Ok(LoopVerdict::stop(if refusal {
                        "refusal"
                    } else {
                        "end_turn"
                    }));
                }
                RoundOutcome::ToolCalls { .. } => match ctx.dispatch_pending().await? {
                    DispatchOutcome::TerminalCommitted { stop_reason } => {
                        return Ok(LoopVerdict {
                            stop_reason,
                            terminal_committed: true,
                        });
                    }
                    DispatchOutcome::Continue => {
                        if ctx.cancelled() {
                            return Ok(LoopVerdict::stop("cancelled"));
                        }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted ctx: each model round consumes the next script step; the harness records the
    /// exact call sequence so policy tests assert ordering, not just outcomes.
    struct Scripted {
        rounds: u64,
        max_rounds: u64,
        cancelled_after_dispatches: Option<usize>,
        dispatches: usize,
        script: Vec<RoundOutcome>,
        dispatch_script: Vec<DispatchOutcome>,
        prepare_script: Vec<PrepareOutcome>,
        closing_script: Vec<RoundOutcome>,
        calls: Vec<&'static str>,
    }

    impl Scripted {
        fn new(script: Vec<RoundOutcome>) -> Self {
            Self {
                rounds: 0,
                max_rounds: 128,
                cancelled_after_dispatches: None,
                dispatches: 0,
                script,
                dispatch_script: Vec::new(),
                prepare_script: Vec::new(),
                closing_script: Vec::new(),
                calls: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl TurnCtx for Scripted {
        async fn prepare_round(&mut self) -> Result<PrepareOutcome> {
            self.calls.push("prepare");
            Ok(if self.prepare_script.is_empty() {
                PrepareOutcome::Ready
            } else {
                self.prepare_script.remove(0)
            })
        }
        async fn model_round(&mut self) -> Result<RoundOutcome> {
            self.calls.push("model");
            self.rounds += 1;
            Ok(self.script.remove(0))
        }
        async fn dispatch_pending(&mut self) -> Result<DispatchOutcome> {
            self.calls.push("dispatch");
            self.dispatches += 1;
            Ok(if self.dispatch_script.is_empty() {
                DispatchOutcome::Continue
            } else {
                self.dispatch_script.remove(0)
            })
        }
        async fn closing_round(&mut self) -> Result<RoundOutcome> {
            self.calls.push("closing");
            self.rounds += 1;
            Ok(if self.closing_script.is_empty() {
                RoundOutcome::Final { refusal: false }
            } else {
                self.closing_script.remove(0)
            })
        }
        fn rounds(&self) -> u64 {
            self.rounds
        }
        fn max_rounds(&self) -> u64 {
            self.max_rounds
        }
        fn cancelled(&self) -> bool {
            self.cancelled_after_dispatches
                .is_some_and(|n| self.dispatches >= n)
        }
    }

    /// The scripted ctx never parks, so a noop-waker poll loop is a complete executor here.
    fn drive(ctx: &mut Scripted) -> LoopVerdict {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn raw() -> RawWaker {
            fn clone(_: *const ()) -> RawWaker {
                raw()
            }
            fn noop(_: *const ()) {}
            RawWaker::new(
                std::ptr::null(),
                &RawWakerVTable::new(clone, noop, noop, noop),
            )
        }
        let waker = unsafe { Waker::from_raw(raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = Box::pin(BuiltinAexLoop.drive_turn(ctx));
        loop {
            if let Poll::Ready(verdict) = fut.as_mut().poll(&mut cx) {
                return verdict.expect("verdict");
            }
        }
    }

    #[test]
    fn a_tool_round_dispatches_then_a_final_round_finishes() {
        let mut ctx = Scripted::new(vec![
            RoundOutcome::ToolCalls { count: 2 },
            RoundOutcome::Final { refusal: false },
        ]);
        let verdict = drive(&mut ctx);
        assert_eq!(verdict, LoopVerdict::stop("end_turn"));
        assert_eq!(
            ctx.calls,
            vec!["prepare", "model", "dispatch", "prepare", "model"]
        );
    }

    #[test]
    fn refusal_maps_to_the_refusal_stop_reason() {
        let mut ctx = Scripted::new(vec![RoundOutcome::Final { refusal: true }]);
        assert_eq!(drive(&mut ctx), LoopVerdict::stop("refusal"));
    }

    #[test]
    fn the_round_ceiling_closes_gracefully_instead_of_calling_the_model_again() {
        let mut ctx = Scripted::new(vec![RoundOutcome::ToolCalls { count: 1 }]);
        ctx.max_rounds = 1;
        let verdict = drive(&mut ctx);
        assert_eq!(verdict, LoopVerdict::stop("max_rounds"));
        assert_eq!(
            ctx.calls,
            vec!["prepare", "model", "dispatch", "prepare", "closing"],
            "the cap triggers one closing round, never an ordinary one"
        );
    }

    #[test]
    fn resumed_turns_count_prior_rounds_toward_the_ceiling() {
        let mut ctx = Scripted::new(vec![]);
        ctx.rounds = 128;
        assert_eq!(drive(&mut ctx), LoopVerdict::stop("max_rounds"));
        assert_eq!(ctx.calls, vec!["prepare", "closing"]);
    }

    #[test]
    fn a_cancelled_or_interrupted_closing_round_keeps_its_honest_stop_reason() {
        let mut cancelled = Scripted::new(vec![]);
        cancelled.rounds = 128;
        cancelled.closing_script = vec![RoundOutcome::Cancelled];
        assert_eq!(drive(&mut cancelled), LoopVerdict::stop("cancelled"));

        let mut interrupted = Scripted::new(vec![]);
        interrupted.rounds = 128;
        interrupted.closing_script = vec![RoundOutcome::Interrupted];
        assert_eq!(drive(&mut interrupted), LoopVerdict::stop("interrupted"));
    }

    #[test]
    fn cancellation_between_rounds_ends_the_turn() {
        let mut ctx = Scripted::new(vec![
            RoundOutcome::ToolCalls { count: 1 },
            RoundOutcome::Final { refusal: false },
        ]);
        ctx.cancelled_after_dispatches = Some(1);
        assert_eq!(drive(&mut ctx), LoopVerdict::stop("cancelled"));
    }

    #[test]
    fn a_return_direct_terminal_passes_through_with_its_stop_reason() {
        let mut ctx = Scripted::new(vec![RoundOutcome::ToolCalls { count: 1 }]);
        ctx.dispatch_script = vec![DispatchOutcome::TerminalCommitted {
            stop_reason: "end_turn".into(),
        }];
        let verdict = drive(&mut ctx);
        assert!(verdict.terminal_committed);
        assert_eq!(verdict.stop_reason, "end_turn");
    }

    #[test]
    fn interrupted_compaction_and_recovery_exhaustion_both_interrupt() {
        let mut ctx = Scripted::new(vec![]);
        ctx.prepare_script = vec![PrepareOutcome::Interrupted];
        assert_eq!(drive(&mut ctx), LoopVerdict::stop("interrupted"));

        let mut ctx = Scripted::new(vec![RoundOutcome::Interrupted]);
        assert_eq!(drive(&mut ctx), LoopVerdict::stop("interrupted"));
    }

    #[test]
    fn cancellation_inside_the_round_maps_to_cancelled() {
        let mut ctx = Scripted::new(vec![RoundOutcome::Cancelled]);
        assert_eq!(drive(&mut ctx), LoopVerdict::stop("cancelled"));
    }
}
