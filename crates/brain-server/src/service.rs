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
    AdmissionStatus, AgentloopAdmission, AgentloopIdentity, ApiError, CreateSessionRequest,
    EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, EventPage, MessageRequest,
    ModelBinding, ModelPresentation, RequestedToolBinding, ResolvedSessionRequest, Session,
    SessionId, SessionList, ToolDefinition,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;

use crate::{EnvironmentRegistry, LocalSessionOwnership, ModelBindingStore, SessionOwnership};

pub struct ServerResources {
    pub kernel: Kernel,
    pub loops: Arc<WorkerPool>,
    pub environments: Arc<EnvironmentRegistry>,
    pub models: Arc<dyn ModelBindingStore>,
    /// The composed provider set this deployment admits sessions against.
    pub providers: Arc<brain::model::ProviderRegistry>,
    /// What the server knows about a session that its records do not: the journal it was
    /// given. Written when the session is created so a restart can restore it.
    pub metadata: Arc<crate::metadata::ServerMetadata>,
}

#[derive(Clone)]
pub struct ServerApi {
    resources: Arc<ServerResources>,
    idempotency_locks: Arc<KeyedLocks<String>>,
    session_locks: Arc<KeyedLocks<SessionId>>,
    ownership: Arc<dyn SessionOwnership>,
}

struct KeyedLocks<K> {
    entries: StdMutex<Table<K>>,
}

struct Table<K> {
    locks: HashMap<K, Weak<Mutex<()>>>,
    /// Sweep once the table has doubled since the last sweep, rather than on every
    /// acquire. A sweep is O(table) under a process-global lock, and every mutating
    /// request acquires at least one lock, so sweeping per call made request cost grow
    /// with the number of live sessions.
    sweep_at: usize,
}

/// Below this a sweep is cheaper than the arithmetic deciding whether to sweep.
const MIN_SWEEP: usize = 16;

impl<K> Default for KeyedLocks<K> {
    fn default() -> Self {
        Self {
            entries: StdMutex::new(Table {
                locks: HashMap::new(),
                sweep_at: MIN_SWEEP,
            }),
        }
    }
}

impl<K: Clone + Eq + Hash> KeyedLocks<K> {
    fn acquire(&self, key: K) -> Result<Arc<Mutex<()>>, ApiError> {
        let mut table = self
            .entries
            .lock()
            .map_err(|_| internal("keyed lock table is poisoned"))?;
        if table.locks.len() >= table.sweep_at {
            table.locks.retain(|_, lock| lock.strong_count() > 0);
            // Amortised: the next sweep is at least as far away as the work this one
            // did, so the total sweeping stays linear in the number of acquires.
            table.sweep_at = table.locks.len().saturating_mul(2).max(MIN_SWEEP);
        }
        if let Some(lock) = table.locks.get(&key).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        table.locks.insert(key, Arc::downgrade(&lock));
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
        let identity = self
            .resources
            .loops
            .admit(package.clone())
            .await
            .map_err(loop_error)?;
        let admission = AgentloopAdmission {
            identity,
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

    async fn get_agentloop(
        &self,
        identity: AgentloopIdentity,
    ) -> Result<AgentloopAdmission, ApiError> {
        if !valid_identity(identity.as_str()) {
            return Err(ApiError::invalid_request(
                "an Agentloop is named by 64 lowercase hexadecimal characters",
            ));
        }
        if !self
            .resources
            .loops
            .status(&identity)
            .await
            .map_err(loop_error)?
        {
            return Err(not_found("Agentloop is not admitted"));
        }
        Ok(AgentloopAdmission {
            identity,
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
            .status(&request.agentloop.identity)
            .await
            .map_err(loop_error)?
        {
            return Err(ApiError::invalid_request(
                "session Agentloop has not been admitted",
            ));
        }
        validate_model(&request, &self.resources.providers)?;
        let binding_id = model_binding_id(&idempotency_key);
        self.resources
            .models
            .put(&binding_id, &request.model)
            .map_err(api_error)?;
        let (presentation, tool_bindings) = split_tools(&request);
        let resolved = ResolvedSessionRequest {
            agentloop_identity: request.agentloop.identity.clone(),
            brain_configuration: request.agentloop.configuration.clone(),
            model: ModelBinding {
                binding_id: binding_id.clone(),
                model: request.model.name.clone(),
            },
            presentation,
            environments: request.environments.clone(),
            tool_bindings,
            history: request.history.clone(),
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
        // The one thing about a session that its own records never carry. Written now so a
        // restart can give the session back the journal it has been writing to; best
        // effort, like everything else here.
        self.resources
            .metadata
            .put_journal(session_id.as_str(), creation.journal_id().as_str())
            .map_err(api_error)?;
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

    fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.resources.kernel.subscribe()
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
        agentloop: &AgentloopIdentity,
        input: brain_protocol::ActivationInput,
    ) -> Result<brain_protocol::ActivationOutput, KernelError> {
        self.0
            .activate(agentloop.clone(), input)
            .await
            .map_err(KernelError::Executor)
    }
}

fn valid_identity(value: &str) -> bool {
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

/// Splits each wire tool back into its two internal halves: what the model sees
/// (`ModelPresentation`) and where the call goes (`RequestedToolBinding`). The kernel
/// still validates the split result against the session contract.
fn split_tools(request: &CreateSessionRequest) -> (ModelPresentation, Vec<RequestedToolBinding>) {
    let mut definitions = Vec::with_capacity(request.tools.len());
    let mut bindings = Vec::with_capacity(request.tools.len());
    for tool in &request.tools {
        definitions.push(ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
        });
        bindings.push(RequestedToolBinding {
            name: tool.name.clone(),
            environment_id: tool.environment_id.clone(),
            remote_tool_id: tool.remote_tool_id.clone(),
            tool_configuration: tool.configuration.clone(),
            grant: tool.grant.clone(),
        });
    }
    (
        ModelPresentation {
            system: request.system.clone(),
            tools: definitions,
            response_format: request.response_format.clone(),
        },
        bindings,
    )
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
    // Rejected here, at create, instead of as a kernel error on the first turn.
    if request.response_format.is_some()
        && !providers.supports_response_format(&request.model.provider, &request.model.name)
    {
        return Err(ApiError::invalid_request(
            "the selected model provider does not support response_format",
        ));
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
        KernelError::ProviderStatus { status, .. } => ApiError {
            code: "model_provider_failed".into(),
            message,
            // The executor already retried what was worth retrying in place; a
            // whole-turn retry can still help for transient statuses.
            retryable: matches!(status, 408 | 429) || status >= 500,
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
    fn model_selection_is_admitted_against_the_composed_registry() {
        let registry = brain::model::ProviderRegistry::default_set();
        let request = |provider: &str, name: &str, response_format: bool| {
            serde_json::from_value::<brain_protocol::CreateSessionRequest>(serde_json::json!({
                "agentloop": {
                    "identity": "a".repeat(64),
                    "configuration": {},
                },
                "model": { "provider": provider, "name": name, "api_key": "k" },
                "system": "",
                "tools": [],
                "response_format": if response_format { Some(serde_json::json!({"type": "json_object"})) } else { None },
                "environments": [],
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

    #[test]
    fn keyed_locks_share_live_keys_and_reclaim_stale_keys() {
        let locks = KeyedLocks::default();
        let first = locks.acquire("same").unwrap();
        let replay = locks.acquire("same").unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &replay));
        drop(first);
        drop(replay);

        // Enough distinct keys to cross the first sweep threshold, so the dead entry
        // above is reclaimed rather than accumulating.
        let mut held = Vec::new();
        for key in KEYS {
            held.push(locks.acquire(key).unwrap());
        }
        assert!(
            locks.entries.lock().unwrap().locks.len() <= super::MIN_SWEEP,
            "a sweep must reclaim the dead key once the table reaches the threshold"
        );
        assert!(
            held.iter()
                .all(|lock| std::sync::Arc::strong_count(lock) == 1)
        );
    }

    /// Distinct static keys, so the test does not depend on a `String` key type.
    const KEYS: [&str; super::MIN_SWEEP] = [
        "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13",
        "k14", "k15",
    ];

    /// The table must not grow without bound when every key is used once and dropped,
    /// which is what an idempotency key looks like.
    #[test]
    fn keyed_locks_stay_bounded_under_single_use_keys() {
        let locks: KeyedLocks<u64> = KeyedLocks::default();
        for key in 0..10_000_u64 {
            drop(locks.acquire(key).unwrap());
        }
        let held = locks.entries.lock().unwrap().locks.len();
        assert!(
            held <= 2 * super::MIN_SWEEP,
            "single-use keys must not accumulate: {held} entries remain"
        );
    }
}
