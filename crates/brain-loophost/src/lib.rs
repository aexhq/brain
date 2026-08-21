//! The loop host: runs agentloop guest components inside wasmtime and drives Brain turns
//! through the agentloop seam.
//!
//! A guest is JavaScript compiled into a wasm component (StarlingMonkey via ComponentizeJS).
//! Its whole world is one imported `host.call(json) -> json` function; the kernel's `TurnCtx`
//! services those calls. The hardening posture is the H0.5-measured one: epoch interruption
//! (CPU-slice deadline per activation), a per-instance memory limiter, and no ambient
//! authority — the component is built with wasi:http disabled and receives only this ABI.
//!
//! A kernel error latched while servicing a ctx op always fails the turn, whatever the guest
//! returns afterwards: a loop cannot mask kernel failures.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use brain::BrainError;
use brain::agentloop::{Agentloop, DispatchOutcome, LoopVerdict, PrepareOutcome, RoundOutcome, TurnCtx};
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

type CtxRequest = (String, oneshot::Sender<String>);

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

/// An agentloop implementation that runs a guest component per turn activation.
pub struct WasmAgentloop {
    engine: Engine,
    pre: GuestPre<HostState>,
}

impl WasmAgentloop {
    /// Compile a component and prepare it for per-turn instantiation. Compilation happens once
    /// here; instantiation per activation restores the wizer-snapshotted memory copy-on-write.
    pub fn from_component_file(path: &Path) -> anyhow::Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
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
        Ok(Self { engine, pre })
    }

    async fn service(
        ctx: &mut dyn TurnCtx,
        payload: &str,
        first_error: &mut Option<BrainError>,
    ) -> String {
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
            "engine.dispatch_pending" => {
                ctx.dispatch_pending().await.map(|outcome| match outcome {
                    DispatchOutcome::Continue => serde_json::json!({ "outcome": "continue" }),
                    DispatchOutcome::TerminalCommitted { stop_reason } => {
                        serde_json::json!({ "outcome": "terminal", "stop_reason": stop_reason })
                    }
                })
            }
            "engine.budget" => Ok(serde_json::json!({
                "rounds": ctx.rounds(),
                "max_rounds": ctx.max_rounds(),
                "cancelled": ctx.cancelled(),
            })),
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
}

#[async_trait]
impl Agentloop for WasmAgentloop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict, BrainError> {
        let (tx, mut rx) = mpsc::channel::<CtxRequest>(1);
        let mut store = Store::new(
            &self.engine,
            HostState {
                tx,
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
                limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT_BYTES).build(),
            },
        );
        store.limiter(|state| &mut state.limits);
        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        let guest = self
            .pre
            .instantiate_async(&mut store)
            .await
            .map_err(|error| BrainError::Protocol(format!("loop guest instantiation: {error}")))?;

        let mut first_error: Option<BrainError> = None;
        let mut activation = Box::pin(guest.call_activate(&mut store, "message", "{}"));
        let outcome = loop {
            tokio::select! {
                biased;
                Some((payload, reply)) = rx.recv() => {
                    let response = Self::service(ctx, &payload, &mut first_error).await;
                    let _ = reply.send(response);
                }
                result = &mut activation => break result,
            }
        };
        drop(activation);
        drop(store);
        if let Some(error) = first_error {
            return Err(error);
        }
        let verdict_json =
            outcome.map_err(|error| BrainError::Protocol(format!("loop guest trapped: {error}")))?;
        let verdict: serde_json::Value = serde_json::from_str(&verdict_json).map_err(|error| {
            BrainError::Protocol(format!("loop guest returned a non-JSON verdict: {error}"))
        })?;
        let stop_reason = verdict["stop_reason"]
            .as_str()
            .ok_or_else(|| {
                BrainError::Protocol("loop guest verdict is missing stop_reason".into())
            })?
            .to_string();
        Ok(LoopVerdict {
            stop_reason,
            terminal_committed: verdict["terminal_committed"].as_bool().unwrap_or(false),
        })
    }
}

/// Convenience for compositions: install the wasm loop into [`brain::session::BrainServices`].
pub fn services_with_wasm_loop(
    component_path: &Path,
) -> anyhow::Result<brain::session::BrainServices> {
    let agentloop: Arc<dyn Agentloop> = Arc::new(WasmAgentloop::from_component_file(component_path)?);
    Ok(brain::session::BrainServices {
        agentloop: Some(agentloop),
        ..brain::session::BrainServices::default()
    })
}
