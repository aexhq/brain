use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use brain::KernelError;
use brain_protocol::{EnvironmentBinding, EnvironmentId, EnvironmentRequirement, request_digest};

#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    pub binding: EnvironmentBinding,
    pub endpoint: String,
}

#[async_trait]
pub trait EnvironmentDirectory: Send + Sync + 'static {
    async fn resolve(
        &self,
        requirement: &EnvironmentRequirement,
    ) -> Result<DirectoryEntry, KernelError>;
    async fn get(&self, environment_id: &EnvironmentId) -> Result<DirectoryEntry, KernelError>;
}

pub struct InMemoryEnvironmentDirectory {
    endpoint: String,
    entries: Mutex<HashMap<EnvironmentId, DirectoryEntry>>,
}

impl InMemoryEnvironmentDirectory {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            entries: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl EnvironmentDirectory for InMemoryEnvironmentDirectory {
    async fn resolve(
        &self,
        requirement: &EnvironmentRequirement,
    ) -> Result<DirectoryEntry, KernelError> {
        if self.endpoint.trim().is_empty() {
            return Err(KernelError::InvalidState(
                "no Environment endpoint is configured".into(),
            ));
        }
        let digest = request_digest(&requirement.configuration)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| KernelError::InvalidState("Environment directory poisoned".into()))?;
        if let Some(existing) = entries.get(&requirement.environment_id) {
            if existing.binding.configuration_digest != digest {
                return Err(KernelError::InvalidState(
                    "Environment identity already has a different configuration digest".into(),
                ));
            }
            return Ok(existing.clone());
        }
        let adapter_binding = serde_jcs::to_string(&requirement.configuration)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        let entry = DirectoryEntry {
            binding: EnvironmentBinding {
                environment_id: requirement.environment_id.clone(),
                configuration_digest: digest,
                adapter_binding,
                directory_generation: 1,
                lifecycle_policy: requirement.lifecycle_policy.clone(),
            },
            endpoint: self.endpoint.clone(),
        };
        entries.insert(requirement.environment_id.clone(), entry.clone());
        Ok(entry)
    }

    async fn get(&self, environment_id: &EnvironmentId) -> Result<DirectoryEntry, KernelError> {
        self.entries
            .lock()
            .map_err(|_| KernelError::InvalidState("Environment directory poisoned".into()))?
            .get(environment_id)
            .cloned()
            .ok_or_else(|| KernelError::InvalidState("Environment is not registered".into()))
    }
}
