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

pub fn presentation(value: &ModelPresentation) -> Result<Presentation, KernelError> {
    let bytes =
        canonical_json(value).map_err(|error| KernelError::InvalidState(error.to_string()))?;
    let digest =
        request_digest(value).map_err(|error| KernelError::InvalidState(error.to_string()))?;
    Ok(Presentation { bytes, digest })
}
