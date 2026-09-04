use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use brain_protocol::{AgentloopIdentity, TurnError, TurnInput, TurnOutput, codes};
use sha2::{Digest as _, Sha256};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{
    Cache, Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap, UpdateDeadline,
};

use crate::{HostCall, LoopLimits};

/// How often the engine's epoch advances. This is the granularity of a guest's compute
/// bound: a turn is trapped somewhere inside the last tick of its budget. Fine enough
/// that a two-second budget is held to within half a percent, coarse enough that the
/// ticker costs one atomic store every ten milliseconds for the whole process.
const EPOCH_TICK: Duration = Duration::from_millis(10);

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../contracts/agentloop/v1",
        world: "agentloop",
    });
}

use bindings::aex::agentloop::types as wit;

/// What answers the guest's host calls while a turn runs. Synchronous on purpose: the
/// guest toolchain can only import synchronous functions, so the host thread waits.
pub trait GuestHost: Send + Sync {
    fn call(&self, call: HostCall) -> Result<String, TurnError>;
}

pub struct AdmissionEngine {
    engine: Engine,
    limits: LoopLimits,
    allowed_imports: Vec<String>,
}

pub struct AdmittedAgentloop {
    pub digest: AgentloopIdentity,
    pub component: Arc<Component>,
}

impl AdmissionEngine {
    pub fn new(limits: LoopLimits, allowed_imports: Vec<String>) -> Result<Self, String> {
        let mut config = Config::new();
        // Epoch interruption, not fuel. Both bound a runaway guest at the same places —
        // loop backedges and function entries — but fuel decrements and checks a counter
        // at each one, where an epoch check is a load and a compare against a value the
        // ticker below advances. Fuel cost 5-13% of an activation and bounded work
        // rather than time.
        config.wasm_component_model(true).epoch_interruption(true);
        let cache = Cache::from_file(None).map_err(|error| {
            format!("failed to configure the Wasmtime compilation cache: {error}")
        })?;
        config.cache(Some(cache));
        let engine = Engine::new(&config).map_err(|error| error.to_string())?;
        spawn_epoch_ticker(&engine)?;
        Ok(Self {
            engine,
            limits,
            allowed_imports,
        })
    }

    pub fn admit(&self, package_bytes: &[u8]) -> Result<AdmittedAgentloop, String> {
        if package_bytes.len() > self.limits.package_bytes {
            return Err("Agentloop package exceeds the configured admission limit".into());
        }
        let (package, component_bytes) = crate::package::decode(package_bytes)?;
        if package.manifest.contract_version != brain_protocol::AGENTLOOP_CONTRACT_VERSION {
            return Err(format!(
                "Agentloop contract version {:?} is not supported; this Brain runs {}",
                package.manifest.contract_version,
                brain_protocol::AGENTLOOP_CONTRACT_VERSION
            ));
        }
        let actual = AgentloopIdentity::new(hex_digest(&component_bytes));
        if actual != package.manifest.component_identity {
            return Err("Agentloop component digest does not match its manifest".into());
        }
        let component = Component::new(&self.engine, &component_bytes)
            .map_err(|error| format!("Agentloop component is invalid: {error}"))?;
        for (name, _) in component.component_type().imports(&self.engine) {
            if !self
                .allowed_imports
                .iter()
                .any(|allowed| name == allowed || name.starts_with(&format!("{allowed}/")))
            {
                return Err(format!("Agentloop import {name:?} is not allowed"));
            }
        }
        if component
            .component_type()
            .get_export(&self.engine, "turn")
            .is_none()
        {
            return Err("Agentloop component does not export turn".into());
        }
        Ok(AdmittedAgentloop {
            digest: actual,
            component: Arc::new(component),
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn limits(&self) -> &LoopLimits {
        &self.limits
    }
}

/// The store's data: the memory limiter, the bridge for the turn in flight, and the
/// clock the compute budget is kept against.
pub struct HostState {
    limits: StoreLimits,
    bridge: Option<Arc<dyn GuestHost>>,
    /// When the running turn started.
    started: Instant,
    /// Time the guest spent waiting on the host during this turn. Not the guest's own
    /// time, so not charged against its compute budget.
    host_elapsed: Duration,
    budget: Duration,
}

impl HostState {
    fn call(&mut self, call: HostCall) -> Result<String, TurnError> {
        let Some(bridge) = &self.bridge else {
            return Err(TurnError::new(
                "no_turn",
                "the guest called the host outside a turn",
            ));
        };
        let at = Instant::now();
        let result = bridge.call(call);
        self.host_elapsed += at.elapsed();
        result
    }
}

impl bindings::aex::agentloop::host::Host for HostState {
    fn model(&mut self, request_json: String) -> Result<String, wit::TurnError> {
        self.call(HostCall::Model { request_json })
            .map_err(wit_error)
    }

    fn dispatch(&mut self, calls_json: String) -> Result<String, wit::TurnError> {
        self.call(HostCall::Dispatch { calls_json })
            .map_err(wit_error)
    }

    fn append(&mut self, kind: String, payload_json: String) -> Result<u64, wit::TurnError> {
        let answer = self
            .call(HostCall::Append { kind, payload_json })
            .map_err(wit_error)?;
        answer.trim().parse().map_err(|_| {
            wit_error(TurnError::new(
                "internal",
                "append answered without a sequence",
            ))
        })
    }

    fn telemetry(&mut self, record_json: String) {
        let _ = self.call(HostCall::Telemetry { record_json });
    }
}

fn wit_error(error: TurnError) -> wit::TurnError {
    wit::TurnError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

/// Warm instances, keyed by session and component: the guest that answered a session's
/// last turn, kept alive so the next one skips instantiation.
///
/// This is a cache, not a contract: an entry can vanish at any moment (eviction, a trap,
/// a worker restart), so a correct agentloop keeps everything it needs in its transcript
/// and slots.
#[derive(Default)]
pub struct WarmInstances {
    entries: std::sync::Mutex<Vec<WarmEntry>>,
}

/// Core instances one guest may hold: its own modules plus the shims wasmtime builds
/// for its imports.
const MAX_CORE_INSTANCES: usize = 8;

/// Sessions kept warm at once. Each entry is a live JS engine whose heap holds one
/// conversation, so this bounds worker memory the way `running` bounds instances.
const WARM_SESSIONS: usize = 8;

struct WarmEntry {
    session: String,
    digest: AgentloopIdentity,
    store: Store<HostState>,
    bindings: bindings::Agentloop,
}

impl WarmInstances {
    fn take(&self, session: &str, digest: &AgentloopIdentity) -> Option<WarmEntry> {
        let mut entries = self.entries.lock().ok()?;
        let at = entries
            .iter()
            .position(|entry| entry.session == session && &entry.digest == digest)?;
        Some(entries.remove(at))
    }

    fn keep(&self, entry: WarmEntry) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        entries.retain(|kept| kept.session != entry.session);
        if entries.len() >= WARM_SESSIONS {
            entries.remove(0);
        }
        entries.push(entry);
    }
}

impl AdmittedAgentloop {
    /// Runs one turn on the calling thread. The guest's host calls go to `bridge` and
    /// block this thread until answered; only the guest's own compute counts against
    /// `limits.wall_time`.
    pub fn turn(
        &self,
        engine: &Engine,
        limits: &LoopLimits,
        warm: &WarmInstances,
        session: &str,
        input: TurnInput,
        bridge: Arc<dyn GuestHost>,
    ) -> Result<TurnOutput, TurnError> {
        let mut entry = match warm.take(session, &self.digest) {
            Some(entry) => entry,
            None => {
                // A component that imports the host is more than one core instance:
                // wasmtime lowers its imports into shim instances beside the guest's
                // own modules. The count is fixed by the component's structure, not by
                // anything the guest does at run time, so this only needs headroom.
                let store_limits = StoreLimitsBuilder::new()
                    .memory_size(limits.linear_memory_bytes)
                    .instances(MAX_CORE_INSTANCES)
                    .build();
                let mut store = Store::new(
                    engine,
                    HostState {
                        limits: store_limits,
                        bridge: None,
                        started: Instant::now(),
                        host_elapsed: Duration::ZERO,
                        budget: limits.wall_time,
                    },
                );
                store.limiter(|state| &mut state.limits);
                // The compute budget is kept against the guest's own time: when the
                // epoch deadline fires, time the guest spent inside host calls is given
                // back and the deadline is pushed out by that much.
                store.epoch_deadline_callback(|context| {
                    let state = context.data();
                    let guest = state.started.elapsed().saturating_sub(state.host_elapsed);
                    if guest >= state.budget {
                        Err(wasmtime::Error::msg(WALL_TIME_EXCEEDED))
                    } else {
                        Ok(UpdateDeadline::Continue(epoch_deadline_ticks(
                            state.budget - guest,
                        )))
                    }
                });
                let mut linker = Linker::<HostState>::new(engine);
                bindings::aex::agentloop::host::add_to_linker::<_, HasSelf<HostState>>(
                    &mut linker,
                    |state| state,
                )
                .map_err(host_failure)?;
                let bindings =
                    bindings::Agentloop::instantiate(&mut store, &self.component, &linker)
                        .map_err(host_failure)?;
                WarmEntry {
                    session: session.to_owned(),
                    digest: self.digest.clone(),
                    store,
                    bindings,
                }
            }
        };
        {
            let state = entry.store.data_mut();
            state.bridge = Some(bridge);
            state.started = Instant::now();
            state.host_elapsed = Duration::ZERO;
            state.budget = limits.wall_time;
        }
        entry
            .store
            .set_epoch_deadline(epoch_deadline_ticks(limits.wall_time));
        let input = to_wit_input(input).map_err(host_failure)?;
        let called = entry.bindings.call_turn(&mut entry.store, &input);
        entry.store.data_mut().bridge = None;
        let output = match called {
            Ok(Ok(output)) => output,
            // The loop's own failure, with the code it chose.
            Ok(Err(error)) => {
                return Err(TurnError {
                    code: error.code,
                    message: error.message,
                    retryable: error.retryable,
                });
            }
            // A trapped guest's heap is not a state anyone can vouch for: the entry is
            // dropped rather than kept.
            Err(error) => return Err(host_failure(turn_error(error))),
        };
        warm.keep(entry);
        let output = from_wit_output(output).map_err(host_failure)?;
        validate_output(&output).map_err(host_failure)?;
        Ok(output)
    }
}

const WALL_TIME_EXCEEDED: &str = "Agentloop turn exceeded its compute budget";

/// A turn that failed on this side of the guest: a trap, a budget, an output the
/// contract refuses. The loop did not choose a code, so it gets the one that says so.
fn host_failure(message: impl std::fmt::Display) -> TurnError {
    TurnError::new(codes::failure::AGENTLOOP_FAILED, message.to_string())
}

/// A guest stopped by its epoch deadline reports the budget it exceeded, not the trap
/// that stopped it.
fn turn_error(error: wasmtime::Error) -> String {
    if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt)
        || error
            .chain()
            .any(|cause| cause.to_string().contains(WALL_TIME_EXCEEDED))
    {
        return WALL_TIME_EXCEEDED.into();
    }
    error.to_string()
}

/// Advances the engine's epoch until the engine is dropped. Held as a weak reference so
/// the ticker does not keep an engine alive, and so a process that builds one and drops
/// it does not leak a thread.
fn spawn_epoch_ticker(engine: &Engine) -> Result<(), String> {
    let weak = engine.weak();
    std::thread::Builder::new()
        .name("agentloop-epoch".into())
        .spawn(move || {
            loop {
                match weak.upgrade() {
                    Some(engine) => {
                        engine.increment_epoch();
                        drop(engine);
                    }
                    None => return,
                }
                std::thread::sleep(EPOCH_TICK);
            }
        })
        .map_err(|error| format!("failed to start the Agentloop epoch ticker: {error}"))?;
    Ok(())
}

/// Ticks that cover `budget`, at least one.
fn epoch_deadline_ticks(budget: Duration) -> u64 {
    let ticks = budget.as_nanos().div_ceil(EPOCH_TICK.as_nanos());
    u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
}

fn to_wit_input(input: TurnInput) -> Result<wit::TurnInput, String> {
    let json = |value: &dyn erased_serialize::Serialize| -> Result<String, String> {
        value.to_json().map_err(|error| error.to_string())
    };
    Ok(wit::TurnInput {
        input_json: json(&input.input)?,
        transcript_json: json(&input.transcript)?,
        slots_json: json(&input.slots)?,
        events_json: json(&input.events)?,
        configuration_json: json(&input.configuration)?,
        system: input.system,
        tools_json: json(&input.tools)?,
        runtime: wit::RuntimeEnvelope {
            logical_time_ms: input.runtime.logical_time_ms,
            deterministic_seed: input.runtime.deterministic_seed,
        },
    })
}

/// A tiny shim so `to_wit_input` can serialise fields of different types through one
/// closure without a generic bound per call.
mod erased_serialize {
    pub trait Serialize {
        fn to_json(&self) -> Result<String, serde_json::Error>;
    }
    impl<T: serde::Serialize> Serialize for T {
        fn to_json(&self) -> Result<String, serde_json::Error> {
            serde_json::to_string(self)
        }
    }
}

fn from_wit_output(output: wit::TurnOutput) -> Result<TurnOutput, String> {
    Ok(TurnOutput {
        transcript: serde_json::from_str(&output.transcript_json)
            .map_err(|error| format!("Agentloop transcript is invalid JSON: {error}"))?,
        slots: serde_json::from_str(&output.slots_json)
            .map_err(|error| format!("Agentloop slots are invalid JSON: {error}"))?,
        result: output
            .result_json
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| format!("Agentloop result is invalid JSON: {error}"))?,
    })
}

fn validate_output(output: &TurnOutput) -> Result<(), String> {
    if output.transcript.len() > brain_protocol::MAX_TRANSCRIPT_ITEMS {
        return Err(format!(
            "Agentloop transcript exceeds {} items",
            brain_protocol::MAX_TRANSCRIPT_ITEMS
        ));
    }
    if output.slots.len() > 128 || output.slots.keys().any(|name| !valid_identifier(name)) {
        return Err("Agentloop slots must be at most 128 identifier-named values".into());
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    use wasm_encoder::{
        BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
        Module, TypeSection, ValType,
    };
    use wasmtime::{Instance, Module as CoreModule, Store, Trap};

    use super::*;

    /// The guest's budget in these tests. Short, because the test waits it out.
    const BUDGET: Duration = Duration::from_millis(200);

    /// How long before the guest is called unstopped. Far above the budget, so a loaded
    /// machine or a coarse sleep clock cannot fail it, and far below "forever", which is
    /// what this test exists to rule out.
    const CEILING: Duration = Duration::from_secs(20);

    #[test]
    fn a_budget_becomes_at_least_one_tick() {
        assert_eq!(epoch_deadline_ticks(Duration::ZERO), 1);
        assert_eq!(epoch_deadline_ticks(Duration::from_nanos(1)), 1);
        assert_eq!(epoch_deadline_ticks(EPOCH_TICK), 1);
        assert_eq!(
            epoch_deadline_ticks(EPOCH_TICK + Duration::from_nanos(1)),
            2
        );
        assert_eq!(epoch_deadline_ticks(Duration::from_secs(2)), 200);
        assert_eq!(epoch_deadline_ticks(Duration::MAX), u64::MAX);
    }

    /// `(func (export "spin") (loop (br 0)))` - a backedge and nothing else, so the only
    /// way out is a trap at the loop's interruption check.
    fn spinning_module() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types
            .ty()
            .function(Vec::<ValType>::new(), Vec::<ValType>::new());
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("spin", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();
        let mut spin = Function::new([]);
        spin.instruction(&Instruction::Loop(BlockType::Empty));
        spin.instruction(&Instruction::Br(0));
        spin.instruction(&Instruction::End);
        spin.instruction(&Instruction::End);
        code.function(&spin);
        module.section(&code);

        module.finish()
    }

    /// The epoch advances and a deadline set against it traps: the smallest module that
    /// never returns is stopped within its budget.
    #[test]
    fn a_guest_that_never_returns_is_trapped_within_its_budget() {
        let admission = AdmissionEngine::new(
            LoopLimits {
                wall_time: BUDGET,
                ..LoopLimits::default()
            },
            Vec::new(),
        )
        .unwrap();
        let engine = admission.engine();

        let module = CoreModule::new(engine, spinning_module()).unwrap();
        let mut store = Store::new(engine, ());
        store.set_epoch_deadline(epoch_deadline_ticks(BUDGET));
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();

        let (finished, waiting) = mpsc::channel();
        let started = Instant::now();
        thread::spawn(move || {
            let _ = finished.send(spin.call(&mut store, ()));
        });
        let outcome = waiting.recv_timeout(CEILING).unwrap_or_else(|_| {
            panic!("the guest was still running after {CEILING:?} on a {BUDGET:?} budget")
        });
        let elapsed = started.elapsed();

        let error = outcome.expect_err("a guest that never returns must not return");
        assert_eq!(
            error.downcast_ref::<Trap>(),
            Some(&Trap::Interrupt),
            "the guest must be stopped by the epoch deadline, not by anything else: {error}"
        );
        assert_eq!(turn_error(error), WALL_TIME_EXCEEDED);
        assert!(
            elapsed < CEILING,
            "the guest ran for {elapsed:?} against a {BUDGET:?} budget"
        );
    }

    /// Time spent in a host call is given back: a guest whose budget would have expired
    /// during a long host call keeps running afterwards.
    #[test]
    fn host_call_time_is_not_charged_to_the_guest() {
        let admission = AdmissionEngine::new(
            LoopLimits {
                wall_time: BUDGET,
                ..LoopLimits::default()
            },
            Vec::new(),
        )
        .unwrap();
        let engine = admission.engine();
        let mut store = Store::new(
            engine,
            HostState {
                limits: StoreLimitsBuilder::new().build(),
                bridge: None,
                started: Instant::now(),
                host_elapsed: Duration::ZERO,
                budget: BUDGET,
            },
        );
        store.epoch_deadline_callback(|context| {
            let state = context.data();
            let guest = state.started.elapsed().saturating_sub(state.host_elapsed);
            if guest >= state.budget {
                Err(wasmtime::Error::msg(WALL_TIME_EXCEEDED))
            } else {
                Ok(UpdateDeadline::Continue(epoch_deadline_ticks(
                    state.budget - guest,
                )))
            }
        });
        store.set_epoch_deadline(epoch_deadline_ticks(BUDGET));
        // Pretend the guest spent three budgets inside the host before running.
        thread::sleep(BUDGET * 3);
        store.data_mut().host_elapsed = BUDGET * 3;
        let module = CoreModule::new(engine, spinning_module()).unwrap();
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();
        let started = Instant::now();
        let error = spin.call(&mut store, ()).unwrap_err();
        let elapsed = started.elapsed();
        assert_eq!(turn_error(error), WALL_TIME_EXCEEDED);
        assert!(
            elapsed >= BUDGET / 2,
            "the guest was stopped after {elapsed:?}, before it had its own {BUDGET:?}"
        );
    }

    #[test]
    fn an_engine_can_be_dropped_while_its_epoch_is_being_driven() {
        for _ in 0..4 {
            drop(AdmissionEngine::new(LoopLimits::default(), Vec::new()).unwrap());
        }
    }
}
