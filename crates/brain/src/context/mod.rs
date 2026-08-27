use brain_protocol::{
    ContextEnvelope, ModelPresentation, Presentation, canonical_json, request_digest,
};

use crate::KernelError;

pub fn empty_context() -> ContextEnvelope {
    ContextEnvelope {
        protocol_version: "agentloop/v1".into(),
        items: Vec::new(),
        state: None,
    }
}

pub fn presentation(
    value: &ModelPresentation,
    brain_configuration: &serde_json::Value,
) -> Result<Presentation, KernelError> {
    let sealed =
        serde_json::json!({ "brain_configuration": brain_configuration, "presentation": value });
    let bytes =
        canonical_json(&sealed).map_err(|error| KernelError::InvalidState(error.to_string()))?;
    let digest =
        request_digest(&sealed).map_err(|error| KernelError::InvalidState(error.to_string()))?;
    Ok(Presentation { bytes, digest })
}
