use brain_protocol::{ContextEnvelope, Identity, ModelPresentation, Presentation, canonical_json};

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
    // Sealed once, for the life of the session. Everything that needs to know whether
    // it is looking at this presentation compares the identity rather than the bytes.
    let bytes =
        canonical_json(&sealed).map_err(|error| KernelError::InvalidState(error.to_string()))?;
    let digest = Identity::of_bytes(&bytes);
    Ok(Presentation { bytes, digest })
}
