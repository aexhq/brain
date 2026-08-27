use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex as StdMutex, Weak},
};

use async_trait::async_trait;
use brain::{Kernel, KernelError, LoopExecutor};
use brain_http::BrainApi;
use brain_loophost::WorkerPool;
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopDigest, ApiError, CreateSessionRequest,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, EventPage, MessageRequest,
    ModelBinding, ResolvedSessionRequest, Session, SessionId, SessionList,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;

use crate::{EnvironmentRegistry, LocalSessionOwnership, ModelBindingStore, SessionOwnership};

pub struct ServerResources {
    pub kernel: Kernel,
    pub loops: Arc<WorkerPool>,
    pub environments: Arc<EnvironmentRegistry>,
    pub models: Arc<dyn ModelBindingStore>,
}

#[derive(Clone)]
pub struct ServerApi {
    resources: Arc<ServerResources>,
    idempotency_locks: Arc<KeyedLocks<String>>,
    session_locks: Arc<KeyedLocks<SessionId>>,
    ownership: Arc<dyn SessionOwnership>,
}

struct KeyedLocks<K> {
    entries: StdMutex<HashMap<K, Weak<Mutex<()>>>>,
}

impl<K> Default for KeyedLocks<K> {
    fn default() -> Self {
        Self {
            entries: StdMutex::new(HashMap::new()),
        }
    }
}

impl<K: Clone + Eq + Hash> KeyedLocks<K> {
    fn acquire(&self, key: K) -> Result<Arc<Mutex<()>>, ApiError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| internal("keyed lock table is poisoned"))?;
        entries.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = entries.get(&key).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        entries.insert(key, Arc::downgrade(&lock));
        Ok(lock)
    }
}

impl ServerApi {
    pub fn new(resources: ServerResources) -> Self {
        Self {
            resources: Arc::new(resources),
            idempotency_locks: Arc::new(KeyedLocks::default()),
            session_locks: Arc::new(KeyedLocks::default()),
            ownership: Arc::new(LocalSessionOwnership),
        }
    }

    pub fn with_ownership(
        resources: ServerResources,
        ownership: Arc<dyn SessionOwnership>,
    ) -> Self {
        Self {
            resources: Arc::new(resources),
            idempotency_locks: Arc::new(KeyedLocks::default()),
            session_locks: Arc::new(KeyedLocks::default()),
            ownership,
        }
    }

    fn session_lock(&self, session_id: &SessionId) -> Result<Arc<Mutex<()>>, ApiError> {
        self.session_locks.acquire(session_id.clone())
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
    async fn admit_agentloop(
        &self,
        idempotency_key: String,
        package: Vec<u8>,
    ) -> Result<AgentloopAdmission, ApiError> {
        let lock = self.idempotency_lock("admit_agentloop", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get("admit_agentloop", &idempotency_key, &package)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let digest = self
            .resources
            .loops
            .admit(package.clone())
            .await
            .map_err(loop_error)?;
        let admission = AgentloopAdmission {
            digest,
            status: AdmissionStatus::Admitted,
            error: None,
        };
        self.resources
            .kernel
            .idempotency_put(
                "admit_agentloop",
                &idempotency_key,
                &package,
                &serde_json::to_value(&admission).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(admission)
    }

    async fn get_agentloop(&self, digest: AgentloopDigest) -> Result<AgentloopAdmission, ApiError> {
        if !valid_digest(digest.as_str()) {
            return Err(ApiError::invalid_request(
                "Agentloop digest must be 64 lowercase hexadecimal characters",
            ));
        }
        if !self
            .resources
            .loops
            .status(&digest)
            .await
            .map_err(loop_error)?
        {
            return Err(not_found("Agentloop is not admitted"));
        }
        Ok(AgentloopAdmission {
            digest,
            status: AdmissionStatus::Admitted,
            error: None,
        })
    }

    async fn create_session(
        &self,
        idempotency_key: String,
        request: CreateSessionRequest,
    ) -> Result<Session, ApiError> {
        let lock = self.idempotency_lock("create_session", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get("create_session", &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        if !self
            .resources
            .loops
            .status(&request.agentloop_digest)
            .await
            .map_err(loop_error)?
        {
            return Err(ApiError::invalid_request(
                "session Agentloop has not been admitted",
            ));
        }
        validate_model(&request)?;
        let binding_id = model_binding_id(&idempotency_key);
        self.resources
            .models
            .put(&binding_id, &request.model)
            .map_err(api_error)?;
        let resolved = ResolvedSessionRequest {
            agentloop_digest: request.agentloop_digest.clone(),
            brain_configuration: request.brain_configuration.clone(),
            model: ModelBinding {
                binding_id: binding_id.clone(),
                model: request.model.name.clone(),
            },
            presentation: request.presentation.clone(),
            environments: request.environments.clone(),
            tool_bindings: request.tool_bindings.clone(),
        };
        let creation = self
            .resources
            .kernel
            .begin_session(&resolved)
            .map_err(|error| {
                let _ = self.resources.models.delete(&binding_id);
                api_error(error)
            })?;
        let session_id = creation.session_id().clone();
        if let Err(error) = self.ownership.claim_new(creation.session_id()).await {
            creation
                .fail("session_ownership_failed", &error.to_string())
                .map_err(api_error)?;
            self.resources
                .models
                .delete(&binding_id)
                .map_err(api_error)?;
            return Err(api_error(error));
        }
        let prepared = self
            .resources
            .environments
            .prepare_session(creation, resolved)
            .await;
        let (handle, _) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.ownership
                    .release(&session_id)
                    .await
                    .map_err(api_error)?;
                self.resources
                    .models
                    .delete(&binding_id)
                    .map_err(api_error)?;
                return Err(api_error(error));
            }
        };
        let session = self
            .resources
            .kernel
            .session(handle.id())
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
                "create_session",
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn get_session(&self, session_id: SessionId) -> Result<Session, ApiError> {
        self.resources
            .kernel
            .session(&session_id)
            .map_err(api_error)
    }

    async fn list_sessions(&self) -> Result<SessionList, ApiError> {
        Ok(SessionList {
            sessions: self.resources.kernel.sessions().map_err(api_error)?,
        })
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<Session, ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let scope = format!("session:{session_id}:message");
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let session = self
            .resources
            .kernel
            .handle(&session_id)
            .map_err(api_error)?
            .message(request.clone())
            .await
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
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
        environment_id: EnvironmentId,
        name: String,
        idempotency_key: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        if !valid_identifier(&name) {
            return Err(ApiError::invalid_request(
                "Environment method name is invalid",
            ));
        }
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let scope = format!("session:{session_id}:environment:{environment_id}:call:{name}");
        let call = (environment_id.clone(), name.clone(), request.clone());
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get(&scope, &idempotency_key, &call)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let result = self
            .resources
            .environments
            .call(
                &self.resources.kernel,
                &session_id,
                &environment_id,
                name,
                request.input,
            )
            .await
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
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
        self.resources
            .kernel
            .events(&session_id, after.unwrap_or(0), 1_000)
            .map_err(api_error)
    }

    async fn cancel_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let request = (session_id.clone(), "cancel");
        let scope = format!("session:{session_id}:cancel");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if self
            .resources
            .kernel
            .idempotency_get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.resources
            .kernel
            .handle(&session_id)
            .map_err(api_error)?
            .cancel()
            .await
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)
    }

    async fn end_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<Session, ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let request = (session_id.clone(), "end");
        let scope = format!("session:{session_id}:end");
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let sealed = self
            .resources
            .kernel
            .sealed_config(&session_id)
            .map_err(api_error)?;
        self.resources
            .environments
            .release_session(&self.resources.kernel, &session_id, &sealed)
            .await
            .map_err(api_error)?;
        let session = self
            .resources
            .kernel
            .end_after_lifecycle(&session_id)
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
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
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let request = (session_id.clone(), "delete");
        let scope = format!("session:{session_id}:delete");
        if self
            .resources
            .kernel
            .idempotency_get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        let binding_id = self
            .resources
            .kernel
            .sealed_config(&session_id)
            .map_err(api_error)?
            .model
            .binding_id;
        self.resources
            .models
            .delete(&binding_id)
            .map_err(api_error)?;
        self.resources
            .kernel
            .delete_ended(&session_id)
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)?;
        self.ownership
            .release(&session_id)
            .await
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

pub struct WorkerLoopExecutor(pub Arc<WorkerPool>);

#[async_trait]
impl LoopExecutor for WorkerLoopExecutor {
    async fn activate(
        &self,
        agentloop: &AgentloopDigest,
        input: brain_protocol::ActivationInput,
    ) -> Result<brain_protocol::ActivationOutput, KernelError> {
        self.0
            .activate(agentloop.clone(), input)
            .await
            .map_err(KernelError::Executor)
    }
}

fn valid_digest(value: &str) -> bool {
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

fn validate_model(request: &CreateSessionRequest) -> Result<(), ApiError> {
    if request.model.provider != "vercel-ai-gateway"
        || request.model.name.len() > 256
        || !request
            .model
            .name
            .split_once('/')
            .is_some_and(|(provider, model)| !provider.is_empty() && !model.is_empty())
        || request.model.api_key.is_empty()
        || request.model.api_key.len() > 16 * 1024
    {
        return Err(ApiError::invalid_request("model selection is invalid"));
    }
    Ok(())
}

fn model_binding_id(idempotency_key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"brain.model-binding.v1\0");
    digest.update(idempotency_key.as_bytes());
    format!("model_{}", hex::encode(digest.finalize()))
}

fn loop_error(message: String) -> ApiError {
    if message.contains("queue is full") {
        ApiError {
            code: "overloaded".into(),
            message,
            retryable: true,
            details: None,
        }
    } else {
        ApiError::invalid_request(message)
    }
}

fn api_error(error: KernelError) -> ApiError {
    let message = error.to_string();
    match error {
        KernelError::InvalidState(_) if message.contains("not found") => not_found(message),
        KernelError::InvalidState(_) if message.contains("idempotency key") => ApiError {
            code: "conflict".into(),
            message,
            retryable: false,
            details: None,
        },
        KernelError::InvalidState(_) => ApiError::invalid_request(message),
        KernelError::Ambiguous(_) => ApiError {
            code: "ambiguous".into(),
            message,
            retryable: false,
            details: None,
        },
        KernelError::Executor(_) => ApiError {
            code: "executor_failed".into(),
            message,
            retryable: true,
            details: None,
        },
        KernelError::Journal(_) => internal(message),
    }
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "not_found".into(),
        message: message.into(),
        retryable: false,
        details: None,
    }
}

fn internal(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "internal".into(),
        message: message.into(),
        retryable: false,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::KeyedLocks;

    #[test]
    fn keyed_locks_share_live_keys_and_reclaim_stale_keys() {
        let locks = KeyedLocks::default();
        let first = locks.acquire("same").unwrap();
        let replay = locks.acquire("same").unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &replay));
        drop(first);
        drop(replay);

        let current = locks.acquire("current").unwrap();
        assert_eq!(locks.entries.lock().unwrap().len(), 1);
        assert_eq!(std::sync::Arc::strong_count(&current), 1);
    }
}
