use base64::{Engine as _, engine::general_purpose::STANDARD};
use brain_protocol::AgentloopPackage;

use crate::MAX_PACKAGE_BYTES;

/// Parses an agentloop package and checks the component against its manifest.
pub fn decode(bytes: &[u8]) -> Result<(AgentloopPackage, Vec<u8>), String> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(format!(
            "Agentloop package exceeds {MAX_PACKAGE_BYTES} bytes"
        ));
    }
    let package: AgentloopPackage = serde_json::from_slice(bytes)
        .map_err(|error| format!("Agentloop package is not valid JSON: {error}"))?;
    let component = STANDARD
        .decode(&package.component_base64)
        .map_err(|error| format!("Agentloop component is not valid base64: {error}"))?;
    if component.len() != package.manifest.component_bytes {
        return Err("Agentloop component byte count does not match its manifest".into());
    }
    Ok((package, component))
}
