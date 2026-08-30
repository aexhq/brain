use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use brain::{KernelError, ModelExecutor, model::ModelTransport, model::RemoteModelClient};
use brain_protocol::{
    Identity, ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelSelection,
    ModelStreamEvent, OperationId,
};

pub use crate::metadata::ModelCredential;

pub trait ModelBindingStore: Send + Sync + 'static {
    fn put(&self, binding_id: &str, selection: &ModelSelection) -> Result<(), KernelError>;
    fn get(&self, binding_id: &str) -> Result<Option<ModelCredential>, KernelError>;
    fn delete(&self, binding_id: &str) -> Result<(), KernelError>;
}

/// The credential half of the server's session metadata.
///
/// Reads come from memory; writes also append to the metadata log so a session can still
/// call its model after a restart. Nothing fsyncs — see `crate::metadata`.
pub struct LocalModelBindingStore {
    metadata: Arc<crate::metadata::ServerMetadata>,
}

impl LocalModelBindingStore {
    pub fn new(metadata: Arc<crate::metadata::ServerMetadata>) -> Self {
        Self { metadata }
    }
}

impl ModelBindingStore for LocalModelBindingStore {
    fn put(&self, binding_id: &str, selection: &ModelSelection) -> Result<(), KernelError> {
        self.metadata.put_binding(binding_id, selection)
    }

    fn get(&self, binding_id: &str) -> Result<Option<ModelCredential>, KernelError> {
        self.metadata.binding(binding_id)
    }

    fn delete(&self, binding_id: &str) -> Result<(), KernelError> {
        self.metadata.forget_binding(binding_id)
    }
}

pub struct ServerModelExecutor {
    bindings: Arc<dyn ModelBindingStore>,
    /// One connection pool for the process. The credential is the only part of a model
    /// call that varies by session, and a credential is a header, not a client.
    transport: Arc<ModelTransport>,
}

impl ServerModelExecutor {
    pub fn new(
        bindings: Arc<dyn ModelBindingStore>,
        base_url: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, KernelError> {
        Ok(Self {
            bindings,
            transport: Arc::new(ModelTransport::new(&base_url.into(), timeout)?),
        })
    }
}

#[async_trait]
impl ModelExecutor for ServerModelExecutor {
    async fn execute(
        &self,
        operation_id: &OperationId,
        request_identity: &Identity,
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        let credential = self
            .bindings
            .get(&binding.binding_id)?
            .ok_or_else(|| KernelError::Executor("model binding is unavailable".into()))?;
        if credential.provider != "vercel-ai-gateway" {
            return Err(KernelError::Executor(
                "model provider is unsupported".into(),
            ));
        }
        let client =
            RemoteModelClient::bound(self.transport.clone(), credential.api_key.to_string())?;
        client
            .execute(
                operation_id,
                request_identity,
                binding,
                presentation,
                request,
                on_event,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::ServerMetadata;

    fn temporary() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "brain-bindings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn store(directory: &std::path::Path) -> LocalModelBindingStore {
        LocalModelBindingStore::new(Arc::new(ServerMetadata::open(directory).unwrap()))
    }

    fn selection(api_key: &str) -> ModelSelection {
        ModelSelection {
            provider: "vercel-ai-gateway".into(),
            name: "openai/gpt-5-mini".into(),
            api_key: api_key.into(),
        }
    }

    /// A binding is sealed at creation. The same credential again is the idempotent retry
    /// of a create the client did not hear the answer to; a different one under the same
    /// identity is a different request wearing that identity's name.
    #[test]
    fn binding_identity_rejects_different_credentials() {
        let directory = temporary();
        let store = store(&directory);
        store.put("model_a", &selection("first")).unwrap();
        store.put("model_a", &selection("first")).unwrap();
        let error = store
            .put("model_a", &selection("second"))
            .expect_err("a sealed identity must not accept another credential");
        assert!(
            error.to_string().contains("already sealed"),
            "the refusal must say why: {error}"
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_credential_is_readable_until_it_is_deleted() {
        let directory = temporary();
        let store = store(&directory);
        store.put("model_a", &selection("secret")).unwrap();
        let credential = store.get("model_a").unwrap().expect("the binding is there");
        assert_eq!(credential.api_key.as_str(), "secret");

        store.delete("model_a").unwrap();
        assert!(store.get("model_a").unwrap().is_none());
        let _ = std::fs::remove_dir_all(directory);
    }

    /// A session must still be able to call its model after a restart, so the credential
    /// outlives the process — written asynchronously and never fsynced, so a crash may
    /// lose the most recent ones.
    #[test]
    fn a_credential_survives_a_restart_without_plaintext_on_disk() {
        let directory = temporary();
        {
            let store = store(&directory);
            store.put("model_a", &selection("provider-secret")).unwrap();
        }

        let log = std::fs::read(directory.join("metadata.log")).unwrap();
        assert!(
            !String::from_utf8_lossy(&log).contains("provider-secret"),
            "the credential must not be readable in the file it is written to"
        );

        let reopened = store(&directory);
        let credential = reopened
            .get("model_a")
            .unwrap()
            .expect("a credential written before a restart must be there after it");
        assert_eq!(credential.api_key.as_str(), "provider-secret");

        reopened.delete("model_a").unwrap();
        drop(reopened);
        let again = store(&directory);
        assert!(
            again.get("model_a").unwrap().is_none(),
            "a binding deleted before a restart must not come back after it"
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
