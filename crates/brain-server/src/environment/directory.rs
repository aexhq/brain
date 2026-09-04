use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use brain_protocol::{EnvironmentAttachment, EnvironmentBinding, EnvironmentId};

#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    pub binding: EnvironmentBinding,
    pub endpoint: String,
}

#[async_trait]
pub trait EnvironmentDirectory: Send + Sync + 'static {
    async fn resolve(
        &self,
        requirement: &EnvironmentAttachment,
    ) -> Result<DirectoryEntry, brain::Error>;
    async fn get(&self, binding: &EnvironmentBinding) -> Result<DirectoryEntry, brain::Error>;
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
        requirement: &EnvironmentAttachment,
    ) -> Result<DirectoryEntry, brain::Error> {
        if self.endpoint.trim().is_empty() {
            return Err(brain::Error::InvalidState(
                "no Environment endpoint is configured".into(),
            ));
        }
        let digest = crate::digest::identity_of(&requirement.configuration)?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| brain::Error::InvalidState("Environment directory poisoned".into()))?;
        // An environment id names one environment: the first session to name it
        // provisions it, and later sessions attach to what is there.
        if let Some(existing) = entries.get(&requirement.environment_id) {
            return Ok(existing.clone());
        }
        let entry = DirectoryEntry {
            binding: EnvironmentBinding {
                environment_id: requirement.environment_id.clone(),
                configuration_identity: digest,
                directory_generation: 1,
                lifecycle_policy: requirement.lifecycle_policy.clone(),
            },
            endpoint: self.endpoint.clone(),
        };
        entries.insert(requirement.environment_id.clone(), entry.clone());
        Ok(entry)
    }

    async fn get(&self, binding: &EnvironmentBinding) -> Result<DirectoryEntry, brain::Error> {
        if self.endpoint.trim().is_empty() {
            return Err(brain::Error::InvalidState(
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
            configuration_identity: brain_protocol::Identity::from_hex(&"b".repeat(64)).unwrap(),
            directory_generation: 7,
            lifecycle_policy: LifecyclePolicy::Shared,
        };

        let entry = directory.get(&binding).await.expect("sealed binding");

        assert_eq!(
            entry.binding.environment_id,
            EnvironmentId::new("shared-workspace")
        );
        assert_eq!(entry.endpoint, "https://environment.example");
    }
}
