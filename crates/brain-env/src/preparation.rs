use std::path::{Path, PathBuf};

use brain_protocol::BrainPreparation;

/// Operator-owned files, resolved relative to the preparation configuration file.
#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreparationConfig {
    pub agentloops: Vec<PathBuf>,
    pub tools: Vec<PathBuf>,
    pub programs: Vec<PathBuf>,
}

impl crate::WorkerPool {
    pub async fn prepare_files(
        &self,
        config: &PreparationConfig,
        directory: &Path,
    ) -> Result<(), crate::LoopError> {
        let mut preparation = BrainPreparation::default();
        for path in &config.agentloops {
            let bytes = tokio::fs::read(directory.join(path))
                .await
                .map_err(|error| error.to_string())?;
            preparation.agentloops.push(self.admit(bytes).await?);
        }
        for path in &config.tools {
            let bytes = tokio::fs::read(directory.join(path))
                .await
                .map_err(|error| error.to_string())?;
            preparation.tools.push(self.admit_tool(bytes).await?);
        }
        for path in &config.programs {
            let source = tokio::fs::read_to_string(directory.join(path))
                .await
                .map_err(|error| error.to_string())?;
            preparation.programs.push(self.admit_program(source).await?);
        }
        self.prepare(&preparation).await
    }
}
