use std::{path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use brain::{Feed, Session, SessionRuntime, Writer};
use brain_env::LoopError;
use brain_env::WorkerPool;
use brain_http::BrainApi;
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopId, ApiError, CreateSessionRequest, Driver,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentName, EventPage, ExecutionCall,
    HostEvent, HostEventAck, HostId, HostRegistration, HostResult, MessageRequest, ModelBinding,
    SessionConfig, SessionId, SessionList, SessionSummary, ToolAdmission, ToolAdmissionStatus,
};
use tokio::sync::Mutex;

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;

use crate::{CredentialStore, EnvironmentRegistry, Executions, IdempotencyStore};
use brain_sessions::locks::KeyedLocks;

pub struct ServerResources {
    /// Where every session's directory lives.
    pub sessions_dir: PathBuf,
    /// The one thread that puts every session's records on disk.
    pub writer: Arc<Writer>,
    /// The live feed of everything every session appends.
    pub feed: Arc<Feed>,
    /// What every session runs with: executors, limits, and where live output goes.
    pub session_runtime: Arc<SessionRuntime>,
    /// How long an idle session keeps its task and memory before it is suspended to
    /// disk. A session may set its own at create; zero means never.
    pub session_idle_ttl: Option<Duration>,
    /// Answers already given to keyed requests, so a retry replays instead of repeats.
    pub idempotency: IdempotencyStore,
    /// Admits Agentloop and Tool Components; the brain env runs them.
    pub loops: Arc<WorkerPool>,
    pub environments: Arc<EnvironmentRegistry>,
    pub hosts: crate::HostEnvironment,
    /// The executions open right now whose loop runs outside this process.
    pub executions: Arc<Executions>,
    /// What the server knows about a session that its records do not: the credentials
    /// it calls its model and its Environments with.
    pub credentials: Arc<dyn CredentialStore>,
    /// The composed provider set this deployment admits sessions against.
    pub providers: Arc<brain::model::ProviderRegistry>,
}

#[derive(Clone)]
pub struct ServerApi {
    resources: Arc<ServerResources>,
    /// Only sessions whose execution is currently retained.
    sessions: brain_sessions::Sessions,
    idempotency_locks: Arc<KeyedLocks<String>>,
}

impl ServerApi {
    pub async fn shutdown(&self) {
        self.resources.loops.shutdown().await;
    }
    pub fn new(resources: ServerResources) -> Result<Self, brain::Error> {
        let sessions = brain_sessions::Sessions::new(brain_sessions::SessionResources {
            sessions_dir: resources.sessions_dir.clone(),
            writer: resources.writer.clone(),
            feed: resources.feed.clone(),
            session_runtime: resources.session_runtime.clone(),
            session_idle_ttl: resources.session_idle_ttl,
            environments: resources.environments.clone(),
        })?;
        Ok(Self {
            resources: Arc::new(resources),
            sessions,
            idempotency_locks: Arc::default(),
        })
    }
    pub fn spawn_idle_sweeper(&self) -> tokio::task::JoinHandle<()> {
        self.sessions.spawn_idle_sweeper()
    }
    fn idempotency_lock(
        &self,
        scope: &str,
        idempotency_key: &str,
    ) -> Result<Arc<Mutex<()>>, ApiError> {
        self.idempotency_locks
            .acquire(format!("{scope}\0{idempotency_key}"))
    }
    fn replay<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, ApiError> {
        serde_json::from_value(value).map_err(|error| internal(error.to_string()))
    }
}

#[async_trait]
impl BrainApi for ServerApi {
    async fn register_host(&self) -> Result<HostRegistration, ApiError> {
        self.resources.hosts.register()
    }

    async fn connect_host(
        &self,
        host_id: HostId,
        token: String,
    ) -> Result<brain_http::HostConnection, ApiError> {
        self.resources.hosts.connect(&host_id, &token)
    }

    async fn resolve_host(
        &self,
        host_id: HostId,
        token: String,
        result: HostResult,
    ) -> Result<(), ApiError> {
        self.resources.hosts.resolve(&host_id, &token, result)
    }

    async fn emit_host_event(
        &self,
        host_id: HostId,
        token: String,
        event: HostEvent,
    ) -> Result<HostEventAck, ApiError> {
        self.resources.hosts.emit(&host_id, &token, event).await
    }

    async fn execution_call(
        &self,
        session_id: SessionId,
        sequence: u64,
        token: String,
        call: ExecutionCall,
    ) -> Result<serde_json::Value, ApiError> {
        self.resources
            .executions
            .call(&session_id, sequence, &token, call)
            .await
            .map_err(api_error)
    }

    async fn admit_agentloop(
        &self,
        idempotency_key: String,
        package: Vec<u8>,
    ) -> Result<AgentloopAdmission, ApiError> {
        let lock = self.idempotency_lock("admit_agentloop", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .replay("admit_agentloop", &idempotency_key, &package)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let id = self
            .resources
            .loops
            .admit(package.clone())
            .await
            .map_err(loop_error)?;
        let admission = AgentloopAdmission {
            id,
            status: AdmissionStatus::Admitted,
            error: None,
        };
        self.resources
            .idempotency
            .put(
                "admit_agentloop",
                &idempotency_key,
                &package,
                &serde_json::to_value(&admission).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(admission)
    }

    async fn admit_tool(
        &self,
        idempotency_key: String,
        component: Vec<u8>,
    ) -> Result<ToolAdmission, ApiError> {
        let lock = self.idempotency_lock("admit_tool", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .replay("admit_tool", &idempotency_key, &component)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let id = self
            .resources
            .loops
            .admit_tool(component.clone())
            .await
            .map_err(loop_error)?;
        let admission = ToolAdmission {
            id,
            status: ToolAdmissionStatus::Admitted,
            error: None,
        };
        self.resources
            .idempotency
            .put(
                "admit_tool",
                &idempotency_key,
                &component,
                &serde_json::to_value(&admission).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(admission)
    }

    async fn get_agentloop(&self, id: AgentloopId) -> Result<AgentloopAdmission, ApiError> {
        if !valid_sha256(id.as_str()) {
            return Err(ApiError::invalid_request(
                "an Agentloop is named by 64 lowercase hexadecimal characters",
            ));
        }
        if !self.resources.loops.status(&id).await.map_err(loop_error)? {
            return Err(not_found("Agentloop is not admitted"));
        }
        Ok(AgentloopAdmission {
            id,
            status: AdmissionStatus::Admitted,
            error: None,
        })
    }

    async fn create_session(
        &self,
        idempotency_key: String,
        request: CreateSessionRequest,
    ) -> Result<SessionSummary, ApiError> {
        validate_model(&request, &self.resources.providers)?;
        for environment in &request.environments {
            if let Driver::Http { url, .. } = &environment.driver {
                crate::environment::validate_url(url).map_err(api_error)?;
            }
        }
        let lock = self.idempotency_lock("create_session", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .replay_or_claim("create_session", &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let session_id = SessionId::new(brain::random_id("ses"));
        // Credentials are sealed under the session before anything is journalled, and
        // the configuration that is journalled never holds them.
        let credentials = &self.resources.credentials;
        credentials
            .put_model(&session_id, &request.model)
            .map_err(api_error)?;
        let mut environments = request.environments.clone();
        for environment in &mut environments {
            if let Driver::Http { credential, .. } = &mut environment.driver
                && let Some(credential) = credential.take()
            {
                credentials
                    .put_environment(&session_id, &environment.name, &credential)
                    .map_err(api_error)?;
            }
        }
        let config = SessionConfig {
            agentloop: request.agentloop.clone(),
            model: ModelBinding {
                provider: request.model.provider.clone(),
                name: request.model.name.clone(),
            },
            system: request.system.clone(),
            response_format: request.response_format.clone(),
            tools: request.tools.clone(),
            environments,
            idle_ttl_ms: request.idle_ttl_ms,
        };
        let session = match self
            .sessions
            .create_session(session_id.clone(), config, request.transcript.clone())
            .await
        {
            Ok(session) => session,
            Err(error) => {
                let _ = credentials.forget(&session_id);
                return Err(error);
            }
        };
        self.resources
            .idempotency
            .put(
                "create_session",
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn get_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError> {
        self.sessions.get_session(session_id).await
    }

    async fn list_sessions(&self) -> Result<SessionList, ApiError> {
        self.sessions.list_sessions().await
    }

    async fn transcript(
        &self,
        session_id: SessionId,
    ) -> Result<brain_protocol::SessionTranscript, ApiError> {
        self.sessions.transcript(session_id).await
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<SessionSummary, ApiError> {
        Session::validate_message(&request).map_err(api_error)?;
        let scope = format!("session:{session_id}:message");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .replay_or_claim(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let session = self
            .sessions
            .send_message(session_id.clone(), request.clone())
            .await?;
        self.resources
            .idempotency
            .put(
                &scope,
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn call_environment(
        &self,
        session_id: SessionId,
        environment: EnvironmentName,
        name: String,
        idempotency_key: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError> {
        if !valid_identifier(&name) {
            return Err(ApiError::invalid_request(
                "Environment method name is invalid",
            ));
        }
        let scope = format!("session:{session_id}:environment:{environment}:call:{name}");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        let call = (environment.clone(), name.clone(), request.clone());
        if let Some(saved) = self
            .resources
            .idempotency
            .replay_or_claim(&scope, &idempotency_key, &call)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let result = self
            .sessions
            .call_environment(
                session_id.clone(),
                environment.clone(),
                name.clone(),
                request.clone(),
            )
            .await?;
        self.resources
            .idempotency
            .put(
                &scope,
                &idempotency_key,
                &call,
                &serde_json::to_value(&result).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(result)
    }

    async fn events(
        &self,
        session_id: SessionId,
        after: Option<u64>,
    ) -> Result<EventPage, ApiError> {
        self.sessions.events(session_id, after).await
    }

    fn subscribe(
        &self,
        session_id: &SessionId,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.sessions.subscribe(session_id)
    }

    async fn cancel_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError> {
        let request = (session_id.clone(), "cancel");
        let scope = format!("session:{session_id}:cancel");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if self
            .resources
            .idempotency
            .replay_or_claim::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.sessions.cancel_session(session_id.clone()).await?;
        self.resources
            .idempotency
            .put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)
    }

    async fn end_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<SessionSummary, ApiError> {
        let request = (session_id.clone(), "end");
        let scope = format!("session:{session_id}:end");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .replay_or_claim(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let session = self.sessions.end_session(session_id.clone()).await?;
        self.resources
            .idempotency
            .put(
                &scope,
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn delete_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError> {
        let request = (session_id.clone(), "delete");
        let scope = format!("session:{session_id}:delete");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if self
            .resources
            .idempotency
            .replay_or_claim::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.sessions.delete_session(session_id.clone()).await?;
        self.resources
            .credentials
            .forget(&session_id)
            .map_err(api_error)?;
        self.resources
            .idempotency
            .put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)?;
        Ok(())
    }

    async fn live(&self) -> bool {
        true
    }

    async fn ready(&self) -> bool {
        self.resources.loops.ready().await.is_ok()
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

fn validate_model(
    request: &CreateSessionRequest,
    providers: &brain::model::ProviderRegistry,
) -> Result<(), ApiError> {
    let valid = providers.get(&request.model.provider).is_some_and(|def| {
        brain::model::valid_model_name(def, &request.model.name)
            && !request.model.api_key.is_empty()
            && request.model.api_key.len() <= 16 * 1024
    });
    if !valid {
        return Err(ApiError::invalid_request("model selection is invalid"));
    }
    // Rejected here, at create, instead of failing the first turn.
    if request.response_format.is_some()
        && !providers.supports_response_format(&request.model.provider, &request.model.name)
    {
        return Err(ApiError::invalid_request(
            "the selected model provider does not support response_format",
        ));
    }
    Ok(())
}

fn loop_error(error: LoopError) -> ApiError {
    match error {
        LoopError::Overloaded => ApiError::overloaded(error.to_string()),
        LoopError::Turn(_) => ApiError::invalid_request(error.to_string()),
        LoopError::Failed(message) => ApiError::invalid_request(message),
    }
}

/// A runtime error names its own API code; nothing here reads the message.
fn api_error(error: brain::Error) -> ApiError {
    ApiError::new(error.code(), error.to_string(), error.retryable())
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError::not_found(message)
}

fn internal(message: impl Into<String>) -> ApiError {
    ApiError::internal(message)
}

#[cfg(test)]
mod tests {

    #[test]
    fn model_selection_is_admitted_against_the_composed_registry() {
        let registry = brain::model::ProviderRegistry::default_set();
        let request = |provider: &str, name: &str, response_format: bool| {
            serde_json::from_value::<brain_protocol::CreateSessionRequest>(serde_json::json!({
                "agentloop": {
                    "implementation": {"type": "brain_component", "entrypoint": "turn", "id": "a".repeat(64)},
                    "configuration": {},
                    "environment": "brain",
                },
                "model": { "provider": provider, "name": name, "api_key": "k" },
                "tools": [],
                "response_format": if response_format { Some(serde_json::json!({"type": "json_object"})) } else { None },
                "environments": [{"name": "brain", "driver": "brain"}],
            }))
            .unwrap()
        };
        let validate = |provider: &str, name: &str, rf: bool| {
            super::validate_model(&request(provider, name, rf), &registry)
        };
        assert!(validate("vercel-ai-gateway", "openai/gpt-5-mini", false).is_ok());
        assert!(
            validate("vercel-ai-gateway", "gpt-5-mini", false).is_err(),
            "the gateway requires a provider namespace in the model name"
        );
        assert!(
            validate("deepseek", "brand-new-model", false).is_ok(),
            "open admission: an unknown model on a catalog provider passes"
        );
        assert!(validate("bedrock", "some-model", false).is_err());
        assert!(validate("openai", "gpt-5-mini", true).is_ok());
        assert!(
            validate("anthropic", "claude-sonnet-4-5", true).is_err(),
            "response_format on a provider that cannot carry it is rejected at create"
        );
    }
}
