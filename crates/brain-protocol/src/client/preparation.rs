use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AdmissionStatus, AgentloopId, ProgramId, ToolId};

/// Reusable artifacts to load on the built-in Environment's execution workers.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrainPreparation {
    pub agentloops: Vec<AgentloopId>,
    pub tools: Vec<ToolId>,
    pub programs: Vec<ProgramId>,
}

impl BrainPreparation {
    pub fn validate(&self) -> Result<(), &'static str> {
        for id in self
            .agentloops
            .iter()
            .map(AgentloopId::as_str)
            .chain(self.tools.iter().map(ToolId::as_str))
            .chain(self.programs.iter().map(ProgramId::as_str))
        {
            if !crate::ids::is_sha256(id) {
                return Err("artifact id must be a lowercase SHA-256 digest");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramAdmission {
    pub id: ProgramId,
    pub status: AdmissionStatus,
}
