use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use brain::{
    ModelExecutor,
    model::{Dialect, MaxTokensField, ModelTransport, ProviderRegistry, RemoteModelClient},
};
use brain_protocol::{
    ModelBinding, ModelRequest, ModelResult, ModelSelection, ModelStreamEvent, ToolDefinition,
};

pub use crate::metadata::ModelCredential;

/// The `--providers-file` format: custom provider definitions in the same
/// shape as the registry's `ProviderDef`, merged over the built-in catalog.
/// An error here is fatal at startup — the operator wrote this file, unlike
/// the third-party catalog rows, so it must be exactly right.
pub fn load_providers_file(
    path: &std::path::Path,
) -> Result<Vec<brain::model::ProviderDef>, brain::Error> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ProvidersFile {
        providers: Vec<brain::model::ProviderDef>,
    }
    let raw = std::fs::read_to_string(path).map_err(|error| {
        brain::Error::InvalidState(format!("providers file {}: {error}", path.display()))
    })?;
    let file: ProvidersFile = serde_json::from_str(&raw).map_err(|error| {
        brain::Error::InvalidState(format!("providers file {}: {error}", path.display()))
    })?;
    Ok(file.providers)
}

pub trait ModelBindingStore: Send + Sync + 'static {
    fn put(&self, binding_id: &str, selection: &ModelSelection) -> Result<(), brain::Error>;
    fn get(&self, binding_id: &str) -> Result<Option<ModelCredential>, brain::Error>;
    fn delete(&self, binding_id: &str) -> Result<(), brain::Error>;
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
    fn put(&self, binding_id: &str, selection: &ModelSelection) -> Result<(), brain::Error> {
        self.metadata.put_binding(binding_id, selection)
    }

    fn get(&self, binding_id: &str) -> Result<Option<ModelCredential>, brain::Error> {
        self.metadata.binding(binding_id)
    }

    fn delete(&self, binding_id: &str) -> Result<(), brain::Error> {
        self.metadata.forget_binding(binding_id)
    }
}

pub struct ServerModelExecutor {
    bindings: Arc<dyn ModelBindingStore>,
    /// One connection pool per provider for the process. The credential is the only
    /// part of a model call that varies by session, and a credential is a header,
    /// not a client. `reqwest` pools connect lazily, so building one per registered
    /// provider up front costs memory, not sockets.
    transports: HashMap<String, (Dialect, MaxTokensField, Arc<ModelTransport>)>,
}

impl ServerModelExecutor {
    /// Builds a transport for every provider in the composed registry. A row whose
    /// transport cannot be built is skipped with a warning rather than aborting
    /// startup: custom providers and override flags were already validated fatally
    /// when the registry composed, so what fails here is third-party catalog data,
    /// and third-party data must not brick the server.
    pub fn new(
        bindings: Arc<dyn ModelBindingStore>,
        providers: &ProviderRegistry,
        timeout: Duration,
    ) -> Result<Self, brain::Error> {
        let mut transports = HashMap::new();
        for def in providers.iter() {
            match ModelTransport::new(&def.base_url, timeout) {
                Ok(transport) => {
                    transports.insert(
                        def.name.clone(),
                        (def.dialect, def.max_tokens_field, Arc::new(transport)),
                    );
                }
                Err(error) => {
                    tracing::warn!(provider = %def.name, %error, "skipping provider whose transport cannot be built");
                }
            }
        }
        if transports.is_empty() {
            return Err(brain::Error::InvalidState(
                "no model provider transport could be built".into(),
            ));
        }
        Ok(Self {
            bindings,
            transports,
        })
    }

    #[cfg(test)]
    fn has_transport(&self, provider: &str) -> bool {
        self.transports.contains_key(provider)
    }
}

#[async_trait]
impl ModelExecutor for ServerModelExecutor {
    async fn execute(
        &self,
        binding: &ModelBinding,
        request: ModelRequest,
        tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, brain::Error> {
        let credential = self
            .bindings
            .get(&binding.binding_id)?
            .ok_or_else(|| brain::Error::Executor("model binding is unavailable".into()))?;
        let Some((dialect, max_tokens_field, transport)) =
            self.transports.get(credential.provider.as_str())
        else {
            return Err(brain::Error::Executor(
                "model provider is unsupported".into(),
            ));
        };
        let client = RemoteModelClient::bound(
            transport.clone(),
            credential.api_key.to_string(),
            *dialect,
            *max_tokens_field,
        )?;
        client.execute(binding, request, tools, on_event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::ServerMetadata;

    fn temporary() -> std::path::PathBuf {
        // A counter, not the clock: tests start close enough together that two can share a
        // timestamp, and two tests in one directory share a metadata log.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "brain-bindings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

    #[test]
    fn every_registered_provider_gets_a_transport_including_the_catalog() {
        let directory = temporary();
        let store = Arc::new(store(&directory));
        let executor = ServerModelExecutor::new(
            store,
            &ProviderRegistry::default_set(),
            Duration::from_secs(1),
        )
        .unwrap();
        for provider in ["vercel-ai-gateway", "openai", "anthropic", "deepseek"] {
            assert!(
                executor.has_transport(provider),
                "{provider} should have a transport"
            );
        }
        assert!(!executor.has_transport("bedrock"));
        let _ = std::fs::remove_dir_all(directory);
    }

    /// The whole custom-provider path, end to end: a providers file defines a
    /// provider the catalog has never heard of, the registry admits it, and a
    /// session bound to it streams a model call against its endpoint speaking
    /// the dialect and compat the file declared.
    #[tokio::test]
    async fn a_providers_file_provider_serves_a_model_call_end_to_end() {
        use axum::{Router, body::Bytes, routing::post};
        use brain_protocol::{Message, ModelRequest};
        use tokio::sync::oneshot;

        let (observed_tx, observed_rx) = oneshot::channel();
        let observed_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(observed_tx)));
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |body: Bytes| {
                let observed_tx = observed_tx.clone();
                async move {
                    observed_tx.lock().unwrap().take().unwrap().send(body).unwrap();
                    (
                        [("content-type", "text/event-stream")],
                        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let directory = temporary();
        let file = directory.join("providers.json");
        std::fs::write(
            &file,
            format!(
                r#"{{"providers": [{{
                    "name": "local-llm",
                    "dialect": "openai_chat",
                    "base_url": "http://{address}/v1",
                    "max_tokens_field": "max_tokens",
                    "models": [{{"id": "test-model", "context_window_tokens": 8192}}]
                }}]}}"#
            ),
        )
        .unwrap();
        let custom = load_providers_file(&file).unwrap();
        let registry = ProviderRegistry::compose(custom, &[]).unwrap();
        assert!(registry.model("local-llm", "test-model").is_some());

        let store = Arc::new(store(&directory));
        store
            .put(
                "model_local",
                &ModelSelection {
                    provider: "local-llm".into(),
                    name: "test-model".into(),
                    api_key: "local-key".into(),
                },
            )
            .unwrap();
        let executor = ServerModelExecutor::new(store, &registry, Duration::from_secs(2)).unwrap();
        let result = executor
            .execute(
                &ModelBinding {
                    binding_id: "model_local".into(),
                    model: "test-model".into(),
                },
                ModelRequest {
                    system: None,
                    tools: None,
                    messages: vec![Message::user_text("hi")],
                    response_format: None,
                    max_output_tokens: Some(16),
                },
                &[],
                &mut |_| {},
            )
            .await
            .unwrap();
        assert!(matches!(
            &result.message.content[0],
            brain_protocol::ContentBlock::Text { text } if text == "ok"
        ));
        let body: serde_json::Value = serde_json::from_slice(&observed_rx.await.unwrap()).unwrap();
        assert_eq!(
            body["max_tokens"], 16,
            "the file's max_tokens_field compat must reach the wire"
        );
        assert!(body.get("max_completion_tokens").is_none());
        let _ = std::fs::remove_dir_all(directory);
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
