use base64::{Engine as _, engine::general_purpose::STANDARD};
use brain_protocol::AgentloopIdentity;
use serde::{Deserialize, Serialize};

use crate::MAX_PACKAGE_BYTES;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopManifest {
    pub contract_version: String,
    pub component_identity: AgentloopIdentity,
    pub component_bytes: usize,
    pub toolchain: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopPackage {
    pub manifest: AgentloopManifest,
    pub component_base64: String,
}

impl AgentloopPackage {
    pub fn decode(bytes: &[u8]) -> Result<(Self, Vec<u8>), String> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(format!(
                "Agentloop package exceeds {MAX_PACKAGE_BYTES} bytes"
            ));
        }
        let package: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Agentloop package is not valid JSON: {error}"))?;
        let component = STANDARD
            .decode(&package.component_base64)
            .map_err(|error| format!("Agentloop component is not valid base64: {error}"))?;
        if component.len() != package.manifest.component_bytes {
            return Err("Agentloop component byte count does not match its manifest".into());
        }
        Ok((package, component))
    }
}
