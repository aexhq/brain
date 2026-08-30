use std::{sync::Arc, time::Duration};

use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, ContextEnvelope, Decision, Observation,
    ToolInvocation,
};
use sha2::{Digest as _, Sha256};
use wasmtime::component::{Component, Linker};
use wasmtime::{Cache, Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

use crate::{AgentloopPackage, LoopLimits};

/// How often the engine's epoch advances. This is the granularity of a guest's
/// wall-clock bound: an activation is trapped somewhere inside the last tick of its
/// budget. Fine enough that a two-second budget is held to within half a percent,
/// coarse enough that the ticker costs one atomic store every ten milliseconds for
/// the whole process.
const EPOCH_TICK: Duration = Duration::from_millis(10);

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../contracts/agentloop/v1",
        world: "agentloop",
    });
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
        // rather than time, so its limit had no relationship to the wall-clock budget
        // the supervisor enforces.
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
        let (package, component_bytes) = AgentloopPackage::decode(package_bytes)?;
        if package.manifest.contract_version != "agentloop/v1" {
            return Err("Agentloop contract version is not supported".into());
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
            .get_export(&self.engine, "step")
            .is_none()
        {
            return Err("Agentloop component does not export step".into());
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

impl AdmittedAgentloop {
    pub fn activate(
        &self,
        engine: &Engine,
        limits: &LoopLimits,
        input: ActivationInput,
    ) -> Result<ActivationOutput, String> {
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(limits.linear_memory_bytes)
            .instances(1)
            .build();
        let mut store = Store::new(engine, store_limits);
        store.limiter(|limits| limits);
        // The guest's own wall-clock bound. Until this was driven, `epoch_deadline(1)`
        // could never be reached — nothing advanced the epoch — so the only limit on an
        // activation was the supervisor's IPC timeout, which abandons the worker and
        // every warm instance in it. A trap here costs one instance.
        store.set_epoch_deadline(epoch_deadline_ticks(limits.wall_time));
        let linker = Linker::<StoreLimits>::new(engine);
        let bindings = bindings::Agentloop::instantiate(&mut store, &self.component, &linker)
            .map_err(|error| error.to_string())?;
        let input = to_wit_input(input)?;
        let output = bindings
            .call_step(&mut store, &input)
            .map_err(activation_error)?
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        from_wit_output(output)
    }
}

/// A guest stopped by its epoch deadline reports the wall-time limit it exceeded, not
/// the trap that stopped it. The message is the one the supervisor's timeout has always
/// returned, so what a client sees is unchanged by where the bound is now enforced.
fn activation_error(error: wasmtime::Error) -> String {
    if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        return "Agentloop activation exceeded its wall-time limit".into();
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
                    // Dropped before sleeping: holding it across the sleep would keep the
                    // engine alive for a tick past its last owner, and forever if the
                    // upgrade were held in a binding that outlived the loop body.
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

/// Ticks that cover `budget`, at least one. A guest is trapped after this many epoch
/// increments, so it runs for at most `budget` plus the tick it was in.
fn epoch_deadline_ticks(budget: Duration) -> u64 {
    let ticks = budget.as_nanos().div_ceil(EPOCH_TICK.as_nanos());
    u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
}

fn to_wit_input(
    input: ActivationInput,
) -> Result<bindings::aex::agentloop::types::ActivationInput, String> {
    use bindings::aex::agentloop::types as wit;
    let observation = match input.observation {
        Observation::SessionStarted { history } => wit::Observation::SessionStarted(
            serde_json::to_string(&history).map_err(|error| error.to_string())?,
        ),
        Observation::UserMessage { content } => wit::Observation::UserMessage(
            serde_json::to_string(&content).map_err(|error| error.to_string())?,
        ),
        Observation::ModelCompleted { response } => wit::Observation::ModelCompleted(
            serde_json::to_string(&response).map_err(|error| error.to_string())?,
        ),
        Observation::ToolsCompleted { results } => wit::Observation::ToolsCompleted(
            serde_json::to_string(&results).map_err(|error| error.to_string())?,
        ),
        Observation::Emitted { event } => wit::Observation::Emitted(
            serde_json::to_string(&event).map_err(|error| error.to_string())?,
        ),
        Observation::Cancelled => wit::Observation::Cancelled,
    };
    Ok(wit::ActivationInput {
        context: wit::ContextEnvelope {
            protocol_version: input.context.protocol_version,
            items_json: serde_json::to_string(&input.context.items)
                .map_err(|error| error.to_string())?,
            state_json: input
                .context
                .state
                .map(|state| serde_json::to_string(&state))
                .transpose()
                .map_err(|error| error.to_string())?,
        },
        observation,
        configuration_json: serde_json::to_string(&input.configuration)
            .map_err(|error| error.to_string())?,
        presentation: wit::Presentation {
            bytes: input.presentation.bytes,
            identity: input.presentation.identity.to_string(),
        },
        runtime: wit::RuntimeEnvelope {
            logical_time_ms: input.runtime.logical_time_ms,
            deterministic_seed: input.runtime.deterministic_seed,
        },
    })
}

fn from_wit_output(
    output: bindings::aex::agentloop::types::ActivationOutput,
) -> Result<ActivationOutput, String> {
    use bindings::aex::agentloop::types as wit;
    let context = ContextEnvelope {
        protocol_version: output.context.protocol_version,
        items: serde_json::from_str(&output.context.items_json)
            .map_err(|error| format!("Agentloop context items are invalid JSON: {error}"))?,
        state: output
            .context
            .state_json
            .map(|state| serde_json::from_str(&state))
            .transpose()
            .map_err(|error| format!("Agentloop state is invalid JSON: {error}"))?,
    };
    let decision = match output.decision {
        wit::Decision::Model(request) => Decision::Model {
            request: serde_json::from_str(&request)
                .map_err(|error| format!("Agentloop model request is invalid JSON: {error}"))?,
        },
        wit::Decision::Tools(calls) => Decision::Tools {
            calls: calls
                .into_iter()
                .map(|call| {
                    Ok(ToolInvocation {
                        call_id: call.call_id,
                        name: call.name,
                        input: serde_json::from_str(&call.input_json).map_err(|error| {
                            format!("Agentloop Tool input is invalid JSON: {error}")
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        },
        wit::Decision::Emit(event) => Decision::Emit {
            event: serde_json::from_str(&event)
                .map_err(|error| format!("Agentloop event is invalid JSON: {error}"))?,
        },
        wit::Decision::Finish(result) => Decision::Finish {
            result: result
                .map(|value| serde_json::from_str(&value))
                .transpose()
                .map_err(|error| format!("Agentloop finish result is invalid JSON: {error}"))?,
        },
        wit::Decision::Fail((code, message, retryable)) => Decision::Fail {
            code,
            message,
            retryable,
        },
    };
    let output = ActivationOutput { context, decision };
    validate_output(&output)?;
    Ok(output)
}

fn validate_output(output: &ActivationOutput) -> Result<(), String> {
    if output.context.protocol_version != "agentloop/v1" {
        return Err("Agentloop context protocol version is not supported".into());
    }
    if output.context.items.len() > 4_096 {
        return Err("Agentloop context exceeds 4096 items".into());
    }
    match &output.decision {
        Decision::Model { request } => {
            if request.messages.is_empty() || request.messages.len() > 4_096 {
                return Err("Agentloop model request must contain 1..=4096 messages".into());
            }
            if request.max_output_tokens == Some(0) {
                return Err("Agentloop model output token limit must be positive".into());
            }
        }
        Decision::Tools { calls } => {
            if calls.is_empty() || calls.len() > 128 {
                return Err("Agentloop Tool decision must contain 1..=128 calls".into());
            }
            let mut call_ids = std::collections::HashSet::with_capacity(calls.len());
            for call in calls {
                if !valid_identifier(&call.call_id) || !valid_identifier(&call.name) {
                    return Err("Agentloop Tool call identity is invalid".into());
                }
                if !call_ids.insert(&call.call_id) {
                    return Err("Agentloop Tool call IDs must be unique in one decision".into());
                }
            }
        }
        Decision::Fail { code, message, .. } => {
            if !valid_identifier(code) || message.is_empty() || message.len() > 4_096 {
                return Err("Agentloop failure is invalid".into());
            }
        }
        Decision::Emit { .. } | Decision::Finish { .. } => {}
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
        // Rounded up, so the guest is never given less time than its budget.
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

    /// The engine was configured for epoch interruption, but nothing advanced the epoch,
    /// so the deadline could not fire and fuel - a bound on work, not on time - was the
    /// only limit inside the instance. This drives the mechanism directly rather than
    /// through an Agentloop component: the guest is the smallest module that never
    /// returns, so what it proves is that the epoch advances and that a deadline set
    /// against it traps.
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

        // Run the guest on its own thread and wait with a clock. If the deadline never
        // fires this fails at `CEILING` instead of hanging until CI gives up.
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
        assert_eq!(
            activation_error(error),
            "Agentloop activation exceeded its wall-time limit"
        );
        assert!(
            elapsed < CEILING,
            "the guest ran for {elapsed:?} against a {BUDGET:?} budget"
        );
    }

    /// The ticker holds only a weak reference back, so building an engine and dropping it
    /// leaves no thread running.
    #[test]
    fn an_engine_can_be_dropped_while_its_epoch_is_being_driven() {
        for _ in 0..4 {
            drop(AdmissionEngine::new(LoopLimits::default(), Vec::new()).unwrap());
        }
    }
}
