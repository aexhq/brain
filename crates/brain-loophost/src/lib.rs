//! The loop host: runs agentloop guest components inside wasmtime and drives Brain turns
//! through the agentloop seam.
//!
//! A guest is JavaScript compiled into a wasm component (StarlingMonkey via ComponentizeJS).
//! Its whole world is one imported `host.call(json) -> json` function; the kernel's `TurnCtx`
//! services those calls. The hardening posture is the H0.5-measured one: epoch interruption
//! (CPU-slice deadline per activation), a per-instance memory limiter, and no ambient
//! authority — the component is built with wasi:http disabled and receives only this ABI.
//!
//! Two compositions share one [`WasmLoopEngine`]:
//! - [`WasmAgentloop`] runs the guest in the brain process (local mode, tests).
//! - [`daemon`] serves guest activations from a separate per-tenant process over the
//!   [`wire`] protocol; [`remote::RemoteAgentloop`] is the brain-side client.
//!
//! A kernel error latched while servicing a ctx op always fails the turn, whatever the guest
//! returns afterwards: a loop cannot mask kernel failures.

pub mod daemon;
pub mod remote;
pub mod wire;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use brain::BrainError;
use brain::agentloop::{
    Agentloop, DispatchOutcome, LoopVerdict, PrepareOutcome, RoundOutcome, TurnCtx,
};
use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    path: "wit",
    world: "guest",
    imports: { default: async },
    exports: { default: async },
});

/// Epoch ticks are 10 ms; one activation gets 30 s of guest CPU slices (awaiting the kernel
/// does not consume the budget because the deadline is re-armed around every serviced op).
const EPOCH_TICK: Duration = Duration::from_millis(10);
const EPOCH_DEADLINE_TICKS: u64 = 3_000;
/// One loop instance may grow to 256 MiB of linear memory before allocation fails.
const MEMORY_LIMIT_BYTES: usize = 256 << 20;
/// Bounded ctx-op payloads in both directions, mirroring `brain_protocol::MAX_CTX_OP_BYTES`.
const MAX_OP_BYTES: usize = 768 * 1024;

/// One guest ctx op awaiting service: the op JSON and the channel its response goes back on.
pub type CtxRequest = (String, oneshot::Sender<String>);

/// A running activation: resolves to the guest's verdict JSON, or an error string naming what
/// the guest did (trapped, failed to instantiate). Owns the store — dropping it kills the guest.
pub type ActivationFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;

/// Store data for one activation: the channel to the ctx service plus WASI plumbing.
struct HostState {
    tx: mpsc::Sender<CtxRequest>,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl loophost::abi::host::Host for HostState {
    async fn call(&mut self, payload: String) -> String {
        if payload.len() > MAX_OP_BYTES {
            return error_json("invalid_request", "ctx op payload exceeds the wire bound");
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        if self.tx.send((payload, reply_tx)).await.is_err() {
            return error_json("aborted", "the turn is no longer being serviced");
        }
        reply_rx
            .await
            .unwrap_or_else(|_| error_json("aborted", "the turn is no longer being serviced"))
    }
}

/// wasmtime 48 uses its own error type without a `std::error::Error` impl; carry it as text.
fn wt<T>(result: Result<T, wasmtime::Error>) -> anyhow::Result<T> {
    result.map_err(|error| anyhow::anyhow!("{error}"))
}

fn error_json(code: &str, message: &str) -> String {
    serde_json::json!({ "error": { "code": code, "message": message, "retryable": false } })
        .to_string()
}

/// The compiled guest component plus its engine, shared by the in-process host and the daemon.
/// Compilation happens once at construction; instantiation per activation restores the
/// wizer-snapshotted memory copy-on-write.
pub struct WasmLoopEngine {
    engine: Engine,
    pre: GuestPre<HostState>,
}

impl WasmLoopEngine {
    pub fn from_component_file(path: &Path) -> anyhow::Result<Arc<Self>> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        let engine = wt(Engine::new(&config))?;
        let ticker = engine.clone();
        std::thread::Builder::new()
            .name("loophost-epoch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(EPOCH_TICK);
                    ticker.increment_epoch();
                }
            })?;
        let component = wt(Component::from_file(&engine, path))?;
        let mut linker: Linker<HostState> = Linker::new(&engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(Guest::add_to_linker::<
            HostState,
            wasmtime::component::HasSelf<HostState>,
        >(&mut linker, |state| state))?;
        let pre = wt(GuestPre::new(wt(linker.instantiate_pre(&component))?))?;
        Ok(Arc::new(Self { engine, pre }))
    }

    /// Begin one guest activation. The returned future owns the fresh store and resolves to the
    /// guest's verdict; ctx ops arrive on the receiver until it does. The caller decides how ops
    /// are serviced — locally against a `TurnCtx`, or forwarded over the wire.
    pub fn start_activation(
        self: &Arc<Self>,
        kind: &str,
        payload: &str,
    ) -> (mpsc::Receiver<CtxRequest>, ActivationFuture) {
        // Capacity 1 is exact: the guest ABI is call/response, so at most one op is in flight.
        let (tx, rx) = mpsc::channel(1);
        let this = self.clone();
        let kind = kind.to_string();
        let payload = payload.to_string();
        let future = Box::pin(async move {
            let mut store = Store::new(
                &this.engine,
                HostState {
                    tx,
                    wasi: WasiCtxBuilder::new().build(),
                    table: ResourceTable::new(),
                    limits: StoreLimitsBuilder::new()
                        .memory_size(MEMORY_LIMIT_BYTES)
                        .build(),
                },
            );
            store.limiter(|state| &mut state.limits);
            store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
            let guest = this
                .pre
                .instantiate_async(&mut store)
                .await
                .map_err(|error| format!("guest instantiation failed: {error}"))?;
            guest
                .call_activate(&mut store, &kind, &payload)
                .await
                .map_err(|error| format!("guest trapped: {error}"))
        });
        (rx, future)
    }
}

/// Service one guest ctx op against the kernel's `TurnCtx`. The first kernel error is latched
/// into `first_error` (the guest sees an error response too); the caller must fail the turn
/// with it regardless of the guest's verdict.
///
/// Two vocabularies share the channel: `contracts/agentloop/v1` envelopes (an `op` object with
/// an `op_id`), and the host-internal `engine.*` string ops that drive kernel-managed context.
pub(crate) async fn service_ctx_op(
    ctx: &mut dyn TurnCtx,
    payload: &str,
    first_error: &mut Option<BrainError>,
) -> String {
    if let Ok(request) = serde_json::from_str::<brain_protocol::agentloop::CtxOpRequest>(payload) {
        return service_contract_op(ctx, request, first_error).await;
    }
    let request: serde_json::Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(_) => return error_json("invalid_request", "ctx op payload is not JSON"),
    };
    let op = request["op"].as_str().unwrap_or_default();
    let served: Result<serde_json::Value, BrainError> = match op {
        "engine.prepare_round" => ctx.prepare_round().await.map(|outcome| match outcome {
            PrepareOutcome::Ready => serde_json::json!({ "outcome": "ready" }),
            PrepareOutcome::Interrupted => serde_json::json!({ "outcome": "interrupted" }),
        }),
        "engine.model_round" => ctx.model_round().await.map(|outcome| match outcome {
            RoundOutcome::ToolCalls { count } => {
                serde_json::json!({ "outcome": "tool_calls", "count": count })
            }
            RoundOutcome::Final { refusal } => {
                serde_json::json!({ "outcome": "final", "refusal": refusal })
            }
            RoundOutcome::Cancelled => serde_json::json!({ "outcome": "cancelled" }),
            RoundOutcome::Interrupted => serde_json::json!({ "outcome": "interrupted" }),
        }),
        "engine.dispatch_pending" => ctx.dispatch_pending().await.map(|outcome| match outcome {
            DispatchOutcome::Continue => serde_json::json!({ "outcome": "continue" }),
            DispatchOutcome::TerminalCommitted { stop_reason } => {
                serde_json::json!({ "outcome": "terminal", "stop_reason": stop_reason })
            }
        }),
        "engine.budget" => Ok(serde_json::json!({
            "rounds": ctx.rounds(),
            "max_rounds": ctx.max_rounds(),
            "cancelled": ctx.cancelled(),
        })),
        // The session_start hydration payload, fetched lazily: the host (or a loop that wants
        // it) asks instead of the kernel assembling a tail for every activation.
        "engine.session_start" => ctx.session_start_payload().await,
        other => {
            return error_json(
                "invalid_request",
                &format!("unknown ctx op {other:?} for this composition"),
            );
        }
    };
    match served {
        Ok(value) => value.to_string(),
        Err(error) => {
            let message = error.to_string();
            if first_error.is_none() {
                *first_error = Some(error);
            }
            error_json("internal", &message)
        }
    }
}

/// Serve one `contracts/agentloop/v1` envelope: guest-visible op errors return in the
/// response; a kernel fault is latched (it will fail the turn) and reported as `internal`.
async fn service_contract_op(
    ctx: &mut dyn TurnCtx,
    request: brain_protocol::agentloop::CtxOpRequest,
    first_error: &mut Option<BrainError>,
) -> String {
    use brain_protocol::agentloop::{AgentloopErrorCode, CtxOpResponse};
    let op_id = request.op_id;
    let response = match ctx.contract_op(request.op).await {
        Ok(Ok(result)) => CtxOpResponse::Variant0 { op_id, result },
        Ok(Err(error)) => CtxOpResponse::Variant1 { error, op_id },
        Err(kernel) => {
            let message = kernel.to_string();
            if first_error.is_none() {
                *first_error = Some(kernel);
            }
            CtxOpResponse::Variant1 {
                error: brain::agentloop::op_error(AgentloopErrorCode::Internal, message, false),
                op_id,
            }
        }
    };
    serde_json::to_string(&response)
        .unwrap_or_else(|_| error_json("internal", "a ctx op response failed to serialize"))
}

/// Resolve what the guest's activation returned into the seam's [`LoopVerdict`].
///
/// A contract loop ends its activation with an `ActivationResult` — the turn outcome itself
/// travels through `turn_finish`/`turn_fail`, which the kernel consulted directly; a completed
/// activation without a terminal op is a loop defect and interrupts the turn. The legacy
/// `{stop_reason}` verdict remains for engine-vocabulary loops.
pub(crate) fn resolve_verdict(
    returned: &str,
    ctx: &dyn TurnCtx,
) -> Result<LoopVerdict, BrainError> {
    use brain_protocol::agentloop::{ActivationResult, ActivationResultOutcome};
    if let Ok(result) = serde_json::from_str::<ActivationResult>(returned) {
        return match result.outcome {
            ActivationResultOutcome::Completed if ctx.loop_terminal().is_some() => {
                // Placeholder only: the kernel maps the declared terminal onto the turn.
                Ok(LoopVerdict {
                    stop_reason: "end_turn".into(),
                    terminal_committed: false,
                })
            }
            ActivationResultOutcome::Completed => Err(BrainError::Agentloop(
                "the message activation completed without turn_finish or turn_fail".into(),
            )),
            outcome => Err(BrainError::Agentloop(format!(
                "the activation ended {outcome}: {}",
                result
                    .error
                    .map(|error| String::from(error.message))
                    .unwrap_or_else(|| "no error detail".into())
            ))),
        };
    }
    parse_verdict(returned)
}

/// Parse the guest's legacy verdict JSON into the seam's [`LoopVerdict`].
pub(crate) fn parse_verdict(verdict_json: &str) -> Result<LoopVerdict, BrainError> {
    let verdict: serde_json::Value = serde_json::from_str(verdict_json).map_err(|error| {
        BrainError::Agentloop(format!("guest returned a non-JSON verdict: {error}"))
    })?;
    let stop_reason = verdict["stop_reason"]
        .as_str()
        .ok_or_else(|| BrainError::Agentloop("guest verdict is missing stop_reason".into()))?
        .to_string();
    Ok(LoopVerdict {
        stop_reason,
        terminal_committed: verdict["terminal_committed"].as_bool().unwrap_or(false),
    })
}

/// An agentloop implementation that runs a guest component in-process, one instance per turn
/// activation. The remote counterpart is [`remote::RemoteAgentloop`].
pub struct WasmAgentloop {
    engine: Arc<WasmLoopEngine>,
}

impl WasmAgentloop {
    pub fn from_component_file(path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            engine: WasmLoopEngine::from_component_file(path)?,
        })
    }
}

#[async_trait]
impl Agentloop for WasmAgentloop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict, BrainError> {
        let payload = ctx.activation_message()?.to_string();
        let (mut rx, mut activation) = self.engine.start_activation("message", &payload);
        let mut first_error: Option<BrainError> = None;
        let outcome = loop {
            tokio::select! {
                biased;
                Some((payload, reply)) = rx.recv() => {
                    let response = service_ctx_op(ctx, &payload, &mut first_error).await;
                    let _ = reply.send(response);
                }
                result = &mut activation => break result,
            }
        };
        drop(activation);
        if let Some(error) = first_error {
            return Err(error);
        }
        let verdict_json = outcome.map_err(BrainError::Agentloop)?;
        resolve_verdict(&verdict_json, ctx)
    }
}

/// Convenience for compositions: install the in-process wasm loop into
/// [`brain::session::BrainServices`].
pub fn services_with_wasm_loop(
    component_path: &Path,
) -> anyhow::Result<brain::session::BrainServices> {
    let agentloop: Arc<dyn Agentloop> =
        Arc::new(WasmAgentloop::from_component_file(component_path)?);
    Ok(brain::session::BrainServices {
        agentloop: Some(agentloop),
        ..brain::session::BrainServices::default()
    })
}
