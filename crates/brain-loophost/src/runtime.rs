use std::sync::Arc;

use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopDigest, ContextEnvelope, Decision, Observation,
    ToolInvocation,
};
use sha2::{Digest as _, Sha256};
use wasmtime::component::{Component, Linker};
use wasmtime::{Cache, Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::{AgentloopPackage, LoopLimits};

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
    pub digest: AgentloopDigest,
    pub component: Arc<Component>,
}

impl AdmissionEngine {
    pub fn new(limits: LoopLimits, allowed_imports: Vec<String>) -> Result<Self, String> {
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        let cache = Cache::from_file(None).map_err(|error| {
            format!("failed to configure the Wasmtime compilation cache: {error}")
        })?;
        config.cache(Some(cache));
        let engine = Engine::new(&config).map_err(|error| error.to_string())?;
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
        let actual = AgentloopDigest::new(hex_digest(&component_bytes));
        if actual != package.manifest.component_digest {
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
        store
            .set_fuel(1_000_000_000)
            .map_err(|error| error.to_string())?;
        store.set_epoch_deadline(1);
        let linker = Linker::<StoreLimits>::new(engine);
        let bindings = bindings::Agentloop::instantiate(&mut store, &self.component, &linker)
            .map_err(|error| error.to_string())?;
        let input = to_wit_input(input)?;
        let output = bindings
            .call_step(&mut store, &input)
            .map_err(|error| error.to_string())?
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        from_wit_output(output)
    }
}

fn to_wit_input(
    input: ActivationInput,
) -> Result<bindings::aex::agentloop::types::ActivationInput, String> {
    use bindings::aex::agentloop::types as wit;
    let observation = match input.observation {
        Observation::SessionStarted => wit::Observation::SessionStarted,
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
            digest: input.presentation.digest,
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
