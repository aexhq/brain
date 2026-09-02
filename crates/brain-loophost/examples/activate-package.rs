use std::{fs, path::PathBuf};

use brain_loophost::{AdmissionEngine, LoopLimits, RUNTIME_SHIM_IMPORTS};
use brain_protocol::{ActivationInput, ContextEnvelope, Observation, RuntimeEnvelope};

fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| "usage: activate-package <package.json>".to_owned())?;
    let package = fs::read(path).map_err(|error| error.to_string())?;
    let engine = AdmissionEngine::new(
        LoopLimits::default(),
        RUNTIME_SHIM_IMPORTS
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    )?;
    let admitted = engine.admit(&package)?;
    let warm = brain_loophost::WarmInstances::default();
    let resident = brain_loophost::ResidentContexts::default();
    let (output, _context_attached) = admitted.activate(
        engine.engine(),
        engine.limits(),
        &warm,
        &resident,
        "ses_example",
        true,
        ActivationInput {
            context: ContextEnvelope {
                protocol_version: "agentloop/v1".into(),
                items: Vec::new(),
                state: None,
            },
            observation: Observation::SessionStarted {
                history: Vec::new(),
            },
            configuration: serde_json::json!({}),
            system: String::new(),
            tools: Vec::new(),
            runtime: RuntimeEnvelope {
                logical_time_ms: 1,
                deterministic_seed: vec![0; 32],
            },
        },
    )?;
    println!(
        "{}",
        serde_json::to_string(&output).map_err(|error| error.to_string())?
    );
    Ok(())
}
