use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use brain::KernelError;
use brain_protocol::{EnvironmentBinding, EnvironmentId, EnvironmentRequirement, Identity};

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
    async fn get(&self, binding: &EnvironmentBinding) -> Result<DirectoryEntry, KernelError>;
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
        let digest = Identity::of(&requirement.configuration)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| KernelError::InvalidState("Environment directory poisoned".into()))?;
        if let Some(existing) = entries.get(&requirement.environment_id) {
            if existing.binding.configuration_identity != digest {
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
                configuration_identity: digest,
                adapter_binding,
                directory_generation: 1,
                lifecycle_policy: requirement.lifecycle_policy.clone(),
            },
            endpoint: self.endpoint.clone(),
        };
        entries.insert(requirement.environment_id.clone(), entry.clone());
        Ok(entry)
    }

    async fn get(&self, binding: &EnvironmentBinding) -> Result<DirectoryEntry, KernelError> {
        if self.endpoint.trim().is_empty() {
            return Err(KernelError::InvalidState(
                "no Environment endpoint is configured".into(),
            ));
        }
        Ok(DirectoryEntry {
            binding: binding.clone(),
            endpoint: self.endpoint.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use brain_protocol::{EnvironmentId, LifecyclePolicy};

    use super::*;

    #[tokio::test]
    async fn reconstructs_a_sealed_binding_without_process_local_registration() {
        let directory = InMemoryEnvironmentDirectory::new("https://environment.example");
        let binding = EnvironmentBinding {
            environment_id: EnvironmentId::new("shared-workspace"),
            configuration_identity: Identity::of(&"configuration").unwrap(),
            adapter_binding: "sealed".into(),
            directory_generation: 7,
            lifecycle_policy: LifecyclePolicy::Shared,
        };

        let entry = directory.get(&binding).await.expect("sealed binding");

        assert_eq!(entry.binding.adapter_binding, "sealed");
        assert_eq!(entry.endpoint, "https://environment.example");
    }
}
