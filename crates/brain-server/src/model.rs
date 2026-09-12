use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use brain::{
    ModelExecutor,
    model::{Dialect, ModelTransport, ProviderRegistry, RemoteModelClient},
};
use brain_protocol::{
    EnvironmentName, ModelBinding, ModelRequest, ModelResult, ModelSelection, ModelStreamEvent,
    SessionId, ToolDefinition,
};
use zeroize::Zeroizing;

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

/// The custody of a session's credentials: its model key and the credential of each
/// Environment it reaches over HTTP, sealed under the session id and forgotten together.
pub trait CredentialStore: Send + Sync + 'static {
    fn put_model(
        &self,
        session_id: &SessionId,
        selection: &ModelSelection,
    ) -> Result<(), brain::Error>;
    fn model(&self, session_id: &SessionId) -> Result<Option<ModelCredential>, brain::Error>;
    fn put_environment(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
        credential: &str,
    ) -> Result<(), brain::Error>;
    fn environment(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
    ) -> Result<Option<Zeroizing<String>>, brain::Error>;
    fn forget(&self, session_id: &SessionId) -> Result<(), brain::Error>;
}

impl CredentialStore for crate::metadata::ServerMetadata {
    fn put_model(
        &self,
        session_id: &SessionId,
        selection: &ModelSelection,
    ) -> Result<(), brain::Error> {
        self.put_model(session_id, selection)
    }

    fn model(&self, session_id: &SessionId) -> Result<Option<ModelCredential>, brain::Error> {
        self.model(session_id)
    }

    fn put_environment(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
        credential: &str,
    ) -> Result<(), brain::Error> {
        self.put_environment(session_id, environment, credential)
    }

    fn environment(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
    ) -> Result<Option<Zeroizing<String>>, brain::Error> {
        self.environment(session_id, environment)
    }

    fn forget(&self, session_id: &SessionId) -> Result<(), brain::Error> {
        self.forget(session_id)
    }
}

pub struct ServerModelExecutor {
    credentials: Arc<dyn CredentialStore>,
    /// One connection pool per provider for the process. The credential is the only
    /// part of a model call that varies by session, and a credential is a header,
    /// not a client. `reqwest` pools connect lazily, so building one per registered
    /// provider up front costs memory, not sockets.
    transports: HashMap<String, (Dialect, Arc<ModelTransport>)>,
}

impl ServerModelExecutor {
    /// Builds a transport for every provider in the composed registry. A row whose
    /// transport cannot be built is skipped with a warning rather than aborting
    /// startup: custom providers and override flags were already validated fatally
    /// when the registry composed, so what fails here is third-party catalog data,
    /// and third-party data must not brick the server.
    pub fn new(
        credentials: Arc<dyn CredentialStore>,
        providers: &ProviderRegistry,
        limits: &brain::Limits,
    ) -> Result<Self, brain::Error> {
        let mut transports = HashMap::new();
        for def in providers.iter() {
            match ModelTransport::new(&def.base_url, limits) {
                Ok(transport) => {
                    transports.insert(def.name.clone(), (def.dialect, Arc::new(transport)));
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
            credentials,
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
        session: &SessionId,
        binding: &ModelBinding,
        request: ModelRequest,
        tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, brain::Error> {
        let credential = self
            .credentials
            .model(session)?
            .ok_or_else(|| brain::Error::Executor("model credential is unavailable".into()))?;
        let Some((dialect, transport)) = self.transports.get(credential.provider.as_str()) else {
            return Err(brain::Error::Executor(
                "model provider is unsupported".into(),
            ));
        };
        let client =
            RemoteModelClient::bound(transport.clone(), credential.api_key.to_string(), *dialect)?;
        client
            .execute(session, binding, request, tools, on_event)
            .await
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
            "brain-credentials-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn store(directory: &std::path::Path) -> ServerMetadata {
        ServerMetadata::open(directory).unwrap()
    }

    fn session(id: &str) -> SessionId {
        SessionId::new(id)
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
            &brain::Limits::default(),
        )
        .unwrap();
        for provider in ["vercel-ai-gateway", "openai", "anthropic", "minimax"] {
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
            "/v1/responses",
            post(move |body: Bytes| {
                let observed_tx = observed_tx.clone();
                async move {
                    observed_tx.lock().unwrap().take().unwrap().send(body).unwrap();
                    (
                        [("content-type", "text/event-stream")],
                        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"ok\"}\n\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
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
                    "dialect": "openai_responses",
                    "base_url": "http://{address}/v1",
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
            .put_model(
                &session("ses_local"),
                &ModelSelection {
                    provider: "local-llm".into(),
                    name: "test-model".into(),
                    api_key: "local-key".into(),
                },
            )
            .unwrap();
        let executor = ServerModelExecutor::new(
            store,
            &registry,
            &brain::Limits {
                max_model_secs: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let result = executor
            .execute(
                &session("ses_local"),
                &ModelBinding {
                    provider: "local-llm".into(),
                    name: "test-model".into(),
                },
                ModelRequest {
                    options: Default::default(),
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
            body["max_output_tokens"], 16,
            "the selected dialect must carry the output token limit"
        );
        assert!(body.get("max_completion_tokens").is_none());
        let _ = std::fs::remove_dir_all(directory);
    }

    /// A credential is sealed at creation. The same credential again is the idempotent
    /// retry of a create the client did not hear the answer to; a different one under the
    /// same session is a different request wearing that session's name.
    #[test]
    fn a_session_rejects_different_credentials_once_sealed() {
        let directory = temporary();
        let store = store(&directory);
        store
            .put_model(&session("ses_a"), &selection("first"))
            .unwrap();
        store
            .put_model(&session("ses_a"), &selection("first"))
            .unwrap();
        let error = store
            .put_model(&session("ses_a"), &selection("second"))
            .expect_err("a sealed session must not accept another credential");
        assert!(
            error.to_string().contains("already sealed"),
            "the refusal must say why: {error}"
        );
        let sandbox = EnvironmentName::new("sandbox");
        store
            .put_environment(&session("ses_a"), &sandbox, "token")
            .unwrap();
        store
            .put_environment(&session("ses_a"), &sandbox, "token")
            .unwrap();
        assert!(
            store
                .put_environment(&session("ses_a"), &sandbox, "other")
                .is_err()
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_sessions_credentials_are_readable_until_the_session_is_forgotten() {
        let directory = temporary();
        let store = store(&directory);
        let sandbox = EnvironmentName::new("sandbox");
        store
            .put_model(&session("ses_a"), &selection("secret"))
            .unwrap();
        store
            .put_environment(&session("ses_a"), &sandbox, "sandbox-token")
            .unwrap();
        store
            .put_model(&session("ses_b"), &selection("secret"))
            .unwrap();
        let credential = store
            .model(&session("ses_a"))
            .unwrap()
            .expect("the credential is there");
        assert_eq!(credential.api_key.as_str(), "secret");
        assert_eq!(
            store
                .environment(&session("ses_a"), &sandbox)
                .unwrap()
                .unwrap()
                .as_str(),
            "sandbox-token"
        );

        store.forget(&session("ses_a")).unwrap();
        assert!(store.model(&session("ses_a")).unwrap().is_none());
        assert!(
            store
                .environment(&session("ses_a"), &sandbox)
                .unwrap()
                .is_none()
        );
        assert!(store.model(&session("ses_b")).unwrap().is_some());
        let _ = std::fs::remove_dir_all(directory);
    }

    /// A session's credentials survive restart without appearing in plaintext on disk.
    #[test]
    fn credentials_survive_a_restart_without_plaintext_on_disk() {
        let directory = temporary();
        let sandbox = EnvironmentName::new("sandbox");
        {
            let store = store(&directory);
            store
                .put_model(&session("ses_a"), &selection("provider-secret"))
                .unwrap();
            store
                .put_environment(&session("ses_a"), &sandbox, "sandbox-secret")
                .unwrap();
        }

        let log = std::fs::read(directory.join("metadata.log")).unwrap();
        let text = String::from_utf8_lossy(&log);
        assert!(
            !text.contains("provider-secret") && !text.contains("sandbox-secret"),
            "a credential must not be readable in the file it is written to"
        );

        let reopened = store(&directory);
        let credential = reopened
            .model(&session("ses_a"))
            .unwrap()
            .expect("a credential written before a restart must be there after it");
        assert_eq!(credential.api_key.as_str(), "provider-secret");
        assert_eq!(
            reopened
                .environment(&session("ses_a"), &sandbox)
                .unwrap()
                .unwrap()
                .as_str(),
            "sandbox-secret"
        );

        reopened.forget(&session("ses_a")).unwrap();
        drop(reopened);
        let again = store(&directory);
        assert!(
            again.model(&session("ses_a")).unwrap().is_none(),
            "a credential forgotten before a restart must not come back after it"
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
