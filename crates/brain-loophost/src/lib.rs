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
pub mod registry;
pub mod remote;
pub mod wire;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use brain::BrainError;
use brain::agentloop::{Agentloop, LoopVerdict, TurnCtx};
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
/// Bounded ctx-op payloads in both directions — the protocol constant, one authority.
const MAX_OP_BYTES: usize = brain_protocol::MAX_CTX_OP_BYTES;

/// One guest ctx op awaiting service: the op JSON and the channel its response goes back on.
pub type CtxRequest = (String, oneshot::Sender<String>);

/// Store data for one resident instance: the current activation's ctx channel plus WASI
/// plumbing. The channel is swapped per activation by [`SessionInstance::arm`].
struct HostState {
    tx: mpsc::Sender<CtxRequest>,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    /// Ops serviced so far; the epoch-deadline callback re-arms the CPU slice when this moved,
    /// so awaiting the kernel never consumes guest CPU budget.
    ops_serviced: u64,
    ops_at_last_deadline: u64,
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
        let response = reply_rx
            .await
            .unwrap_or_else(|_| error_json("aborted", "the turn is no longer being serviced"));
        self.ops_serviced += 1;
        response
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
        // The on-disk compilation cache turns every later load of the same component under the
        // same engine config into a fast deserialize — across processes, so daemon restarts
        // and every test after the first skip the multi-minute cranelift compile. wasmtime
        // keys entries by engine config itself (the B11 requirement). An unavailable cache
        // dir degrades to plain compilation: a cache is an optimization, never a dependency.
        match wasmtime::CacheConfig::from_file(None)
            .and_then(wasmtime::Cache::new)
            .map_err(|error| error.to_string())
        {
            Ok(cache) => {
                config.cache(Some(cache));
            }
            Err(error) => {
                tracing::warn!(%error, "wasmtime compilation cache unavailable; compiling without it");
            }
        }
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

    /// Instantiate one resident guest instance for a session. Creation restores the
    /// wizer-snapshotted memory copy-on-write (33 µs on the deployment substrate), so instances
    /// are the cheapest resident thing and are evicted first under pressure.
    pub async fn new_instance(self: &Arc<Self>) -> Result<SessionInstance, String> {
        // The channel is replaced per activation by `arm`; this initial one is never used.
        let (tx, _rx) = mpsc::channel(1);
        let mut store = Store::new(
            &self.engine,
            HostState {
                tx,
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
                limits: StoreLimitsBuilder::new()
                    .memory_size(MEMORY_LIMIT_BYTES)
                    .build(),
                ops_serviced: 0,
                ops_at_last_deadline: 0,
            },
        );
        store.limiter(|state| &mut state.limits);
        // The CPU-slice deadline bounds contiguous guest execution: when ops were serviced
        // since the last check, the guest was awaiting the kernel, and the slice re-arms.
        store.epoch_deadline_callback(|mut context| {
            let state = context.data_mut();
            if state.ops_serviced != state.ops_at_last_deadline {
                state.ops_at_last_deadline = state.ops_serviced;
                Ok(wasmtime::UpdateDeadline::Continue(EPOCH_DEADLINE_TICKS))
            } else {
                Err(wasmtime::Error::msg(format!(
                    "the loop exceeded its {}s guest CPU slice",
                    EPOCH_TICK.as_millis() as u64 * EPOCH_DEADLINE_TICKS / 1_000
                )))
            }
        });
        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        let guest = self
            .pre
            .instantiate_async(&mut store)
            .await
            .map_err(|error| format!("guest instantiation failed: {error}"))?;
        Ok(SessionInstance {
            store,
            guest,
            started: false,
        })
    }
}

/// One resident guest instance: its store (linear memory and limits) and the instantiated
/// exports. Lives as long as the session stays warm in the host (60 s idle, capped residency);
/// loop memory is a cache — durable state rides the journal and rehydrates via `session_start`.
pub struct SessionInstance {
    store: Store<HostState>,
    guest: Guest,
    /// True once this instance received its `session_start` activation.
    pub started: bool,
}

impl SessionInstance {
    /// Prepare one activation: wire a fresh ctx channel into the store and re-arm the CPU
    /// slice. The caller drives [`Self::activate`] while servicing the returned receiver.
    pub fn arm(&mut self) -> mpsc::Receiver<CtxRequest> {
        // Capacity 1 is exact: the guest ABI is call/response, so at most one op is in flight.
        let (tx, rx) = mpsc::channel(1);
        self.store.data_mut().tx = tx;
        self.store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        rx
    }

    pub async fn activate(&mut self, kind: &str, payload: &str) -> Result<String, String> {
        self.guest
            .call_activate(&mut self.store, kind, payload)
            .await
            .map_err(|error| format!("guest trapped: {error}"))
    }
}

/// The per-session resident-instance table shared by a host: get-or-create keyed by session,
/// idle-timer eviction independent of brain actor residency, LRU-capped.
pub struct SessionInstances {
    engine: Arc<WasmLoopEngine>,
    map: std::sync::Mutex<HashMap<String, InstanceEntry>>,
}

struct InstanceEntry {
    instance: Arc<tokio::sync::Mutex<SessionInstance>>,
    last_used: std::time::Instant,
}

/// Idle eviction and residency cap for loop instances (design ledger A5: recreation costs
/// 33 µs, so loops are reclaimed before anything else).
pub const LOOP_INSTANCE_IDLE: Duration = Duration::from_secs(60);
pub const LOOP_INSTANCE_CAP: usize = 256;

impl SessionInstances {
    pub fn new(engine: Arc<WasmLoopEngine>) -> Self {
        Self {
            engine,
            map: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The session's resident instance, created fresh when absent or evicted. Exactly one turn
    /// runs per session, so the get-or-create race cannot occur for one key.
    pub async fn acquire(
        &self,
        session_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<SessionInstance>>, String> {
        self.sweep();
        if let Some(entry) = self.map.lock().expect("instances").get_mut(session_id) {
            entry.last_used = std::time::Instant::now();
            return Ok(entry.instance.clone());
        }
        let instance = Arc::new(tokio::sync::Mutex::new(self.engine.new_instance().await?));
        self.map.lock().expect("instances").insert(
            session_id.to_string(),
            InstanceEntry {
                instance: instance.clone(),
                last_used: std::time::Instant::now(),
            },
        );
        Ok(instance)
    }

    /// Drop a session's instance (after a trap or a failed turn: a fresh instance rehydrates
    /// from durable state instead of trusting possibly-corrupt loop memory).
    pub fn remove(&self, session_id: &str) {
        self.map.lock().expect("instances").remove(session_id);
    }

    /// Evict idle instances and enforce the residency cap (oldest first). A running activation
    /// holds its own `Arc`, so eviction never kills live work — it only drops the table's hold.
    pub fn sweep(&self) {
        let mut map = self.map.lock().expect("instances");
        let now = std::time::Instant::now();
        map.retain(|_, entry| now.duration_since(entry.last_used) < LOOP_INSTANCE_IDLE);
        while map.len() > LOOP_INSTANCE_CAP {
            let Some(oldest) = map
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            map.remove(&oldest);
        }
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
    let Some(op) = request["op"].as_str() else {
        // An `op_id` marks a contract-envelope attempt that failed typed parsing above; give
        // the author that diagnosis instead of a misleading "unknown ctx op".
        return error_json(
            "invalid_request",
            if request.get("op_id").is_some() {
                "the payload does not parse as a contracts/agentloop/v1 ctx op envelope"
            } else {
                "ctx op payload has no string \"op\""
            },
        );
    };
    let served: Result<serde_json::Value, BrainError> = match op {
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
                    stop_reason: brain::journal::TurnStopReason::EndTurn,
                    terminal_committed: false,
                })
            }
            ActivationResultOutcome::Completed => Err(BrainError::Agentloop(
                "the message activation completed without turn_finish or turn_fail".into(),
            )),
            ActivationResultOutcome::Aborted => Ok(LoopVerdict {
                stop_reason: brain::journal::TurnStopReason::Cancelled,
                terminal_committed: false,
            }),
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
        .parse::<brain::journal::TurnStopReason>()
        .map_err(|_| BrainError::Agentloop("guest verdict stop_reason is unknown".into()))?;
    Ok(LoopVerdict {
        stop_reason,
        terminal_committed: verdict["terminal_committed"].as_bool().unwrap_or(false),
    })
}

/// Serve one activation on a resident instance, servicing every guest ctx op locally against
/// the driving turn's `TurnCtx`.
async fn serve_local_activation(
    instance: &mut SessionInstance,
    kind: &str,
    payload: &str,
    ctx: &mut dyn TurnCtx,
    first_error: &mut Option<BrainError>,
) -> Result<String, String> {
    let mut rx = instance.arm();
    let mut activation = Box::pin(instance.activate(kind, payload));
    loop {
        tokio::select! {
            biased;
            Some((payload, reply)) = rx.recv() => {
                let response = service_ctx_op(ctx, &payload, first_error).await;
                let _ = reply.send(response);
            }
            result = &mut activation => break result,
        }
    }
}

/// A `session_start` activation's return: any clean return is acceptance, but a contract loop
/// reporting failed or aborted fails the turn.
pub(crate) fn check_session_start(returned: &str) -> Result<(), BrainError> {
    use brain_protocol::agentloop::{ActivationResult, ActivationResultOutcome};
    if let Ok(result) = serde_json::from_str::<ActivationResult>(returned)
        && !matches!(result.outcome, ActivationResultOutcome::Completed)
    {
        return Err(BrainError::Agentloop(format!(
            "the session_start activation ended {}",
            result.outcome
        )));
    }
    Ok(())
}

/// The session id an activation payload addresses.
pub(crate) fn payload_session_id(payload: &serde_json::Value) -> Result<String, BrainError> {
    payload
        .get("session")
        .and_then(|session| session.get("session_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| BrainError::Agentloop("the activation payload carries no session id".into()))
}

/// An agentloop implementation that runs a guest component in-process with per-session
/// resident instances: a fresh instance receives its `session_start` hydration before the
/// first message activation, and loop memory persists across turns until idle eviction.
/// The remote counterpart is [`remote::RemoteAgentloop`].
pub struct WasmAgentloop {
    instances: SessionInstances,
}

impl WasmAgentloop {
    /// Every imported loop speaks `contracts/agentloop/v1` plus the read-only
    /// `engine.session_start` hydration.
    pub fn from_component_file(path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            instances: SessionInstances::new(WasmLoopEngine::from_component_file(path)?),
        })
    }
}

#[async_trait]
impl Agentloop for WasmAgentloop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict, BrainError> {
        let message_payload = ctx.activation_message()?;
        let session_id = payload_session_id(&message_payload)?;
        let instance = self
            .instances
            .acquire(&session_id)
            .await
            .map_err(BrainError::Agentloop)?;
        let mut guard = instance.lock().await;
        let mut first_error: Option<BrainError> = None;
        if !guard.started {
            let hydration = ctx.session_start_payload().await?.to_string();
            let outcome = serve_local_activation(
                &mut guard,
                "session_start",
                &hydration,
                ctx,
                &mut first_error,
            )
            .await;
            if let Some(error) = first_error.take() {
                drop(guard);
                self.instances.remove(&session_id);
                return Err(error);
            }
            match outcome {
                Ok(returned) => {
                    if let Err(error) = check_session_start(&returned) {
                        drop(guard);
                        self.instances.remove(&session_id);
                        return Err(error);
                    }
                    guard.started = true;
                }
                Err(guest) => {
                    drop(guard);
                    self.instances.remove(&session_id);
                    return Err(BrainError::Agentloop(guest));
                }
            }
        }
        let outcome = serve_local_activation(
            &mut guard,
            "message",
            &message_payload.to_string(),
            ctx,
            &mut first_error,
        )
        .await;
        drop(guard);
        if let Some(error) = first_error {
            self.instances.remove(&session_id);
            return Err(error);
        }
        match outcome {
            Ok(verdict_json) => resolve_verdict(&verdict_json, ctx),
            Err(guest) => {
                // Never reuse an instance that trapped: the replacement rehydrates from
                // durable state instead of trusting possibly-corrupt loop memory.
                self.instances.remove(&session_id);
                Err(BrainError::Agentloop(guest))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyCtx;

    #[async_trait]
    impl TurnCtx for EmptyCtx {}

    #[test]
    fn an_aborted_activation_maps_to_a_cancelled_turn() {
        let returned = serde_json::json!({
            "activation_id": "act-cancelled",
            "outcome": "aborted",
            "error": {
                "code": "aborted",
                "message": "the turn was cancelled",
                "retryable": false
            }
        })
        .to_string();

        let verdict = resolve_verdict(&returned, &EmptyCtx).expect("cancelled verdict");
        assert_eq!(
            verdict.stop_reason,
            brain::journal::TurnStopReason::Cancelled
        );
        assert!(!verdict.terminal_committed);
    }
}
