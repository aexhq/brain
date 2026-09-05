use std::{
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, Weak},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use brain::{Feed, LocalSessionStore, LoopExecutor, Session, SessionRuntime, SessionStore, Writer};
use brain_http::BrainApi;
use brain_loophost::LoopError;
use brain_loophost::WorkerPool;
use brain_protocol::codes;
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopIdentity, ApiError, CreateSessionRequest,
    EnvironmentAttachment, EnvironmentCallRequest, EnvironmentCallResult, EnvironmentId, EventPage,
    HostEvent, HostEventAck, HostId, HostRegistration, HostResult, MessageRequest, ModelBinding,
    SessionConfig, SessionId, SessionList, SessionStatus, SessionSummary, ToolAdmission,
    ToolAdmissionStatus, ToolBinding, ToolDefinition, ToolHosting, ToolIdentity,
};
use tokio::sync::Mutex;

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;

use crate::{EnvironmentRegistry, IdempotencyStore, ModelBindingStore};

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
    pub loops: Arc<WorkerPool>,
    pub environments: Arc<EnvironmentRegistry>,
    pub resident_hosts: crate::ResidentHosts,
    pub models: Arc<dyn ModelBindingStore>,
    /// The composed provider set this deployment admits sessions against.
    pub providers: Arc<brain::model::ProviderRegistry>,
    /// What the server knows about a session that its records do not: the credential it
    /// calls a model with.
    pub metadata: Arc<crate::metadata::ServerMetadata>,
}

#[derive(Clone)]
pub struct ServerApi {
    resources: Arc<ServerResources>,
    /// Only sessions whose execution is currently retained.
    sessions: Arc<StdMutex<HashMap<SessionId, Slot>>>,
    stores: Arc<StdMutex<HashMap<SessionId, Weak<LocalSessionStore>>>>,
    store_locks: Arc<KeyedLocks<SessionId>>,
    idempotency_locks: Arc<KeyedLocks<String>>,
    session_locks: Arc<KeyedLocks<SessionId>>,
}

struct Slot {
    store: Arc<LocalSessionStore>,
    /// The running task, or `None` while the session is suspended.
    session: Option<Session>,
    last_touch: Instant,
    /// Zero means never suspend.
    idle_ttl: Option<Duration>,
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
    /// Initializes lookup without opening session histories or starting actors.
    pub fn new(resources: ServerResources) -> Result<Self, brain::Error> {
        std::fs::create_dir_all(&resources.sessions_dir)
            .map_err(|error| brain::Error::Journal(error.to_string()))?;
        Ok(Self {
            resources: Arc::new(resources),
            sessions: Arc::default(),
            stores: Arc::default(),
            store_locks: Arc::new(KeyedLocks::default()),
            idempotency_locks: Arc::new(KeyedLocks::default()),
            session_locks: Arc::new(KeyedLocks::default()),
        })
    }

    /// Suspends sessions that have sat idle past their TTL, on a timer. Suspension drops
    /// the actor and cached store; the next execution request reopens them from disk.
    pub fn spawn_idle_sweeper(&self) -> tokio::task::JoinHandle<()> {
        let api = self.clone();
        let every = (api
            .resources
            .session_idle_ttl
            .unwrap_or(Duration::from_secs(4))
            / 4)
        .clamp(Duration::from_secs(1), Duration::from_secs(60));
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(every);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                api.suspend_idle().await;
            }
        })
    }

    /// Suspends every active session idle past its TTL. Public so a host without the
    /// sweeper can drive it.
    pub async fn suspend_idle(&self) {
        let due: Vec<SessionId> = match self.sessions.lock() {
            Ok(sessions) => sessions
                .iter()
                .filter(|(_, slot)| {
                    slot.session.is_some()
                        && slot
                            .idle_ttl
                            .is_none_or(|ttl| !ttl.is_zero() && slot.last_touch.elapsed() >= ttl)
                })
                .map(|(id, _)| id.clone())
                .collect(),
            Err(_) => return,
        };
        for session_id in due {
            if let Err(error) = self.suspend(&session_id).await {
                tracing::warn!(%session_id, error = %error.message, "session could not be suspended");
            }
        }
    }

    /// Suspends one session if it is still idle and untouched: journals the suspension,
    /// waits for its records to reach disk, and drops its task.
    async fn suspend(&self, session_id: &SessionId) -> Result<(), ApiError> {
        let lock = self.session_lock(session_id)?;
        let _guard = lock.lock().await;
        let (session, store) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| internal("session table is poisoned"))?;
            let Some(slot) = sessions.get(session_id) else {
                return Ok(());
            };
            let Some(session) = slot.session.clone() else {
                return Ok(());
            };
            if slot
                .idle_ttl
                .is_some_and(|ttl| ttl.is_zero() || slot.last_touch.elapsed() < ttl)
            {
                return Ok(());
            }
            (session, slot.store.clone())
        };
        let summary = store.session_summary().map_err(api_error)?;
        if !matches!(summary.status, brain_protocol::SessionStatus::Idle) {
            return Ok(());
        }
        session
            .record(codes::event::SESSION_SUSPENDED, serde_json::json!({}))
            .await
            .map_err(api_error)?;
        tokio::task::spawn_blocking(move || store.checkpoint())
            .await
            .map_err(|error| internal(error.to_string()))?
            .map_err(api_error)?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?;
        sessions.remove(session_id);
        Ok(())
    }

    fn insert_slot(
        &self,
        store: Arc<LocalSessionStore>,
        session: Option<Session>,
    ) -> Result<(), ApiError> {
        let config = brain::session_config(&*store).ok();
        if let Some(config) = &config
            && !matches!(
                store.session_summary().map_err(api_error)?.status,
                SessionStatus::Failed
            )
        {
            let hosts = config
                .tool_bindings
                .iter()
                .filter_map(|tool| tool.host_id.clone())
                .collect::<Vec<_>>();
            self.resources
                .resident_hosts
                .bind_session(store.session_id(), &hosts)?;
        }
        let idle_ttl = config
            .as_ref()
            .and_then(|config| config.idle_ttl_ms)
            .map(Duration::from_millis)
            .or(self.resources.session_idle_ttl);
        self.cache_store(&store)?;
        if session.is_none() {
            return Ok(());
        }
        self.sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .insert(
                store.session_id().clone(),
                Slot {
                    store,
                    session,
                    last_touch: Instant::now(),
                    idle_ttl,
                },
            );
        Ok(())
    }

    fn cache_store(&self, store: &Arc<LocalSessionStore>) -> Result<(), ApiError> {
        let mut stores = self
            .stores
            .lock()
            .map_err(|_| internal("store table is poisoned"))?;
        stores.retain(|_, store| store.strong_count() > 0);
        stores.insert(store.session_id().clone(), Arc::downgrade(store));
        Ok(())
    }

    async fn store(&self, session_id: &SessionId) -> Result<Arc<LocalSessionStore>, ApiError> {
        if !valid_identifier(session_id.as_str()) {
            return Err(ApiError::invalid_request("invalid session id"));
        }
        let lock = self.store_locks.acquire(session_id.clone())?;
        let _guard = lock.lock().await;
        if let Some(store) = self
            .stores
            .lock()
            .map_err(|_| internal("store table is poisoned"))?
            .get(session_id)
            .and_then(Weak::upgrade)
        {
            return Ok(store);
        }
        let path = self.resources.sessions_dir.join(session_id.as_str());
        let writer = self.resources.writer.clone();
        let feed = self.resources.feed.clone();
        let store = tokio::task::spawn_blocking(move || {
            if !path.is_dir() {
                return Err(not_found("session not found"));
            }
            let store = LocalSessionStore::open(&path, writer, feed).map_err(api_error)?;
            store.interrupt_unfinished_turn().map_err(api_error)?;
            Ok(store)
        })
        .await
        .map_err(|error| internal(error.to_string()))??;
        self.cache_store(&store)?;
        Ok(store)
    }

    /// Called under the session mutation lock, except cancellation which never opens an actor.
    async fn session(&self, session_id: &SessionId) -> Result<Session, ApiError> {
        if let Some(session) = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get_mut(session_id)
            .and_then(|slot| {
                slot.last_touch = Instant::now();
                slot.session.clone()
            })
        {
            return Ok(session);
        }
        let store = self.store(session_id).await?;
        if matches!(
            store.session_summary().map_err(api_error)?.status,
            SessionStatus::Ended | SessionStatus::Failed
        ) {
            return Err(ApiError::invalid_request("session has ended"));
        }
        let runtime = self.resources.session_runtime.clone();
        let opening = store.clone();
        let session = tokio::task::spawn_blocking(move || Session::open(opening, runtime))
            .await
            .map_err(|error| internal(error.to_string()))?
            .map_err(api_error)?;
        self.insert_slot(store, Some(session.clone()))?;
        session
            .record(codes::event::SESSION_RESUMED, serde_json::json!({}))
            .await
            .map_err(api_error)?;
        Ok(session)
    }

    fn remember(&self, store: Arc<LocalSessionStore>, session: Session) -> Result<(), ApiError> {
        self.insert_slot(store, Some(session))
    }

    async fn passivate(&self, session_id: &SessionId) -> Result<(), ApiError> {
        let store = self.store(session_id).await?;
        tokio::task::spawn_blocking(move || store.checkpoint())
            .await
            .map_err(|error| internal(error.to_string()))?
            .map_err(api_error)?;
        self.forget(session_id)
    }

    fn forget(&self, session_id: &SessionId) -> Result<(), ApiError> {
        self.sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .remove(session_id);
        Ok(())
    }

    async fn summary(&self, session_id: &SessionId) -> Result<SessionSummary, ApiError> {
        self.store(session_id)
            .await?
            .session_summary()
            .map_err(api_error)
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

    async fn cleanup_environments(
        &self,
        environment_ids: &[EnvironmentId],
        store: &dyn SessionStore,
    ) {
        for environment_id in environment_ids.iter().rev() {
            if self
                .resources
                .environments
                .resources()
                .get(environment_id)
                .ok()
                .flatten()
                .is_some_and(|record| &record.session_id != store.session_id())
            {
                continue;
            }
            if let Err(error) = self
                .resources
                .environments
                .close(environment_id, store, false)
                .await
            {
                tracing::warn!(%environment_id, %error, "failed to clean up Environment after session admission");
            }
        }
    }
}

#[async_trait]
impl BrainApi for ServerApi {
    async fn register_host(&self) -> Result<HostRegistration, ApiError> {
        self.resources.resident_hosts.register()
    }

    async fn connect_host(
        &self,
        host_id: HostId,
        token: String,
    ) -> Result<brain_http::HostConnection, ApiError> {
        self.resources.resident_hosts.connect(&host_id, &token)
    }

    async fn resolve_host(
        &self,
        host_id: HostId,
        token: String,
        result: HostResult,
    ) -> Result<(), ApiError> {
        self.resources
            .resident_hosts
            .resolve(&host_id, &token, result)
    }

    async fn emit_host_event(
        &self,
        host_id: HostId,
        token: String,
        event: HostEvent,
    ) -> Result<HostEventAck, ApiError> {
        self.resources
            .resident_hosts
            .emit(&host_id, &token, event)
            .await
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
        let identity = self
            .resources
            .loops
            .admit_tool(component.clone())
            .await
            .map_err(loop_error)?;
        let admission = ToolAdmission {
            identity,
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
    ) -> Result<SessionSummary, ApiError> {
        let Some(agentloop_environment) = request
            .environments
            .iter()
            .find(|environment| environment.environment_id == request.agentloop.environment_id)
        else {
            return Err(ApiError::invalid_request(
                "the Agentloop must name an Environment in this session",
            ));
        };
        if agentloop_environment
            .configuration
            .get("driver")
            .and_then(serde_json::Value::as_str)
            != Some("brain_wasm")
        {
            return Err(ApiError::invalid_request(
                "the MVP executes Agentloops only in the brain_wasm Environment",
            ));
        }
        for environment in &request.environments {
            if environment
                .configuration
                .get("driver")
                .and_then(serde_json::Value::as_str)
                == Some("brain_wasm")
            {
                self.resources
                    .loops
                    .validate_native_environment(&environment.configuration)
                    .map_err(loop_error)?;
            }
        }
        validate_agentloop_identity(&request.agentloop.identity)?;
        validate_model(&request, &self.resources.providers)?;
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
        for tool in &request.tools {
            let Some(identity) = native_tool_identity(&request, tool)? else {
                continue;
            };
            if !self
                .resources
                .loops
                .tool_status(&identity)
                .await
                .map_err(loop_error)?
            {
                return Err(ApiError::invalid_request(
                    "session Tool has not been admitted",
                ));
            }
        }
        for host_id in request
            .tools
            .iter()
            .filter_map(|tool| tool.host_id.as_ref())
        {
            if !self.resources.resident_hosts.is_connected(host_id)? {
                return Err(ApiError::invalid_request(
                    "a resident Tool host is not connected",
                ));
            }
        }
        let session_id = SessionId::new(brain::random_id("ses"));
        let session_lock = self.session_lock(&session_id)?;
        let _session_guard = session_lock.lock().await;
        let store_lock = self.store_locks.acquire(session_id.clone())?;
        let store_guard = store_lock.lock().await;
        let binding_id = format!("model_{session_id}");
        self.resources
            .models
            .put(&binding_id, &request.model)
            .map_err(api_error)?;
        let (tools, tool_bindings) = split_tools(&request);
        let (environments, binding_values) = attachments_for(&request);
        let config = SessionConfig {
            agentloop_identity: request.agentloop.identity.clone(),
            agentloop_environment_id: request.agentloop.environment_id.clone(),
            brain_configuration: request.agentloop.configuration.clone(),
            model: ModelBinding {
                binding_id: binding_id.clone(),
                model: request.model.name.clone(),
            },
            system: request.system.clone(),
            response_format: request.response_format.clone(),
            tools,
            environments,
            tool_bindings,
            idle_ttl_ms: request.idle_ttl_ms,
        };
        let store = LocalSessionStore::create(
            &self.resources.sessions_dir.join(session_id.as_str()),
            session_id.clone(),
            &serde_json::to_value(&config).map_err(|error| internal(error.to_string()))?,
            self.resources.writer.clone(),
            self.resources.feed.clone(),
        )
        .map_err(|error| {
            let _ = self.resources.models.delete(&binding_id);
            api_error(error)
        })?;
        let mut creation = match Session::begin(
            store.clone(),
            self.resources.session_runtime.clone(),
            &config,
            &request.transcript,
        ) {
            Ok(creation) => creation,
            Err(error) => {
                // Nothing was admitted: the directory holds at most a genesis record
                // for a session that never existed.
                let _ = std::fs::remove_dir_all(store.directory());
                let _ = self.resources.models.delete(&binding_id);
                return Err(api_error(error));
            }
        };
        self.cache_store(&store)?;
        drop(store_guard);
        let host_ids = request
            .tools
            .iter()
            .filter_map(|tool| tool.host_id.clone())
            .collect::<Vec<_>>();
        if let Err(error) = self
            .resources
            .resident_hosts
            .bind_session(&session_id, &host_ids)
        {
            creation
                .fail(
                    codes::failure::ENVIRONMENT_PREPARATION_FAILED,
                    &error.message,
                )
                .map_err(api_error)?;
            self.insert_slot(store, None)?;
            self.resources
                .models
                .delete(&binding_id)
                .map_err(api_error)?;
            return Err(error);
        }
        let mut created_environments = Vec::with_capacity(request.environments.len());
        for specification in &request.environments {
            match self
                .resources
                .environments
                .create_for_session(&mut creation, specification)
                .await
            {
                Ok(record) => created_environments.push(record.environment_id),
                Err(error) => {
                    created_environments.push(specification.environment_id.clone());
                    creation
                        .fail(
                            codes::failure::ENVIRONMENT_PREPARATION_FAILED,
                            &error.to_string(),
                        )
                        .map_err(api_error)?;
                    self.insert_slot(store.clone(), None)?;
                    let _ = self.resources.models.delete(&binding_id);
                    self.cleanup_environments(&created_environments, &*store)
                        .await;
                    return Err(api_error(error));
                }
            }
        }
        let prepared = self
            .resources
            .environments
            .prepare_session(creation, config, binding_values)
            .await;
        let (session, _) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.insert_slot(store.clone(), None)?;
                self.resources
                    .models
                    .delete(&binding_id)
                    .map_err(api_error)?;
                self.cleanup_environments(&created_environments, &*store)
                    .await;
                return Err(api_error(error));
            }
        };
        self.remember(store, session)?;
        let session = self.summary(&session_id).await?;
        let release = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get(&session_id)
            .is_some_and(|slot| slot.idle_ttl.is_none());
        if release {
            self.passivate(&session_id).await?;
        }
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
        self.summary(&session_id).await
    }

    async fn list_sessions(&self) -> Result<SessionList, ApiError> {
        let directory = self.resources.sessions_dir.clone();
        let ids = tokio::task::spawn_blocking(move || {
            let mut ids = Vec::new();
            for entry in
                std::fs::read_dir(directory).map_err(|error| internal(error.to_string()))?
            {
                let entry = entry.map_err(|error| internal(error.to_string()))?;
                if entry
                    .file_type()
                    .map_err(|error| internal(error.to_string()))?
                    .is_dir()
                    && let Some(name) = entry
                        .file_name()
                        .to_str()
                        .filter(|name| valid_identifier(name))
                {
                    ids.push(SessionId::new(name));
                }
            }
            Ok::<_, ApiError>(ids)
        })
        .await
        .map_err(|error| internal(error.to_string()))??;
        let mut sessions = Vec::with_capacity(ids.len());
        for id in ids {
            sessions.push(self.summary(&id).await?);
        }
        sessions.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Ok(SessionList { sessions })
    }

    async fn transcript(
        &self,
        session_id: SessionId,
    ) -> Result<brain_protocol::SessionTranscript, ApiError> {
        let store = self.store(&session_id).await?;
        let folded = tokio::task::spawn_blocking(move || store.fold())
            .await
            .map_err(|error| internal(error.to_string()))?
            .map_err(api_error)?;
        Ok(brain_protocol::SessionTranscript {
            messages: folded.transcript,
            through_sequence: folded.through_sequence,
        })
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<SessionSummary, ApiError> {
        Session::validate_message(&request).map_err(api_error)?;
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let scope = format!("session:{session_id}:message");
        if let Some(saved) = self
            .resources
            .idempotency
            .replay_or_claim(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let result = self
            .session(&session_id)
            .await?
            .message(request.clone())
            .await;
        let release = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get(&session_id)
            .is_some_and(|slot| slot.idle_ttl.is_none());
        if release {
            self.passivate(&session_id).await?;
        }
        let session = result.map_err(api_error)?;
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
        environment_id: EnvironmentId,
        name: String,
        idempotency_key: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError> {
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
            .idempotency
            .replay_or_claim(&scope, &idempotency_key, &call)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let store = self.store(&session_id).await?;
        let session = self.session(&session_id).await?;
        let result = self
            .resources
            .environments
            .call(&session, &*store, &environment_id, name, request.input)
            .await
            .map_err(api_error)?;
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
        let after = after.unwrap_or(0);
        let store = self.store(&session_id).await?;
        let records = tokio::task::spawn_blocking(move || store.records_after(after, 1_000))
            .await
            .map_err(|error| internal(error.to_string()))?
            .map_err(api_error)?;
        Ok(brain::event_page(records, after))
    }

    fn subscribe(
        &self,
        session_id: &SessionId,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.resources.feed.subscribe(session_id)
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
        let session = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get(&session_id)
            .and_then(|slot| slot.session.clone());
        if let Some(session) = session {
            session.cancel().await.map_err(api_error)?;
        } else {
            self.summary(&session_id).await?;
        }
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
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let request = (session_id.clone(), "end");
        let scope = format!("session:{session_id}:end");
        if let Some(saved) = self
            .resources
            .idempotency
            .replay_or_claim(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let store = self.store(&session_id).await?;
        let summary = store.session_summary().map_err(api_error)?;
        if matches!(summary.status, brain_protocol::SessionStatus::Ended) {
            self.resources
                .idempotency
                .put(
                    &scope,
                    &idempotency_key,
                    &request,
                    &serde_json::to_value(&summary).map_err(|error| internal(error.to_string()))?,
                )
                .map_err(api_error)?;
            return Ok(summary);
        }
        let config = brain::session_config(&*store).map_err(api_error)?;
        let running = self.session(&session_id).await?;
        running
            .record(codes::event::SESSION_END_STARTED, serde_json::json!({}))
            .await
            .map_err(api_error)?;
        self.resources
            .environments
            .release_session(&running, &config, &*store)
            .await
            .map_err(api_error)?;
        let session = running.end().await.map_err(api_error)?;
        self.forget(&session_id)?;
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
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let request = (session_id.clone(), "delete");
        let scope = format!("session:{session_id}:delete");
        if self
            .resources
            .idempotency
            .replay_or_claim::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        let store = self.store(&session_id).await?;
        store.ensure_deletable().map_err(api_error)?;
        let config = brain::session_config(&*store).map_err(api_error)?;
        let binding_id = config.model.binding_id;
        for environment in config.environments.iter().rev() {
            if self
                .resources
                .environments
                .resources()
                .get(&environment.environment_id)
                .map_err(api_error)?
                .is_some_and(|record| record.session_id != session_id)
            {
                continue;
            }
            self.resources
                .environments
                .close(&environment.environment_id, &*store, true)
                .await
                .map_err(api_error)?;
        }
        self.resources
            .loops
            .remove_workspace(session_id.as_str())
            .await
            .map_err(|error| internal(error.to_string()))?;
        store.delete().map_err(api_error)?;
        self.resources
            .models
            .delete(&binding_id)
            .map_err(api_error)?;
        self.sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .remove(&session_id);
        self.resources.resident_hosts.release_session(&session_id)?;
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

pub struct WorkerLoopExecutor(pub Arc<WorkerPool>);

#[async_trait]
impl LoopExecutor for WorkerLoopExecutor {
    async fn turn(
        &self,
        session: &brain_protocol::SessionId,
        agentloop: &AgentloopIdentity,
        environment: serde_json::Value,
        input: brain_protocol::TurnInput,
        services: Arc<dyn brain::TurnServices>,
    ) -> Result<brain_protocol::TurnOutput, brain::Error> {
        let bridge = ServicesBridge(services);
        self.0
            .turn(
                session.as_str().to_owned(),
                agentloop.clone(),
                environment,
                input,
                &bridge,
            )
            .await
            .map_err(|error| match error {
                LoopError::Overloaded => brain::Error::Overloaded(error.to_string()),
                LoopError::Turn(error) if error.code == codes::failure::CANCELLED => {
                    brain::Error::Cancelled(error.message)
                }
                LoopError::Turn(error) => brain::Error::Loop(error),
                LoopError::Failed(message) => brain::Error::Executor(message),
            })
    }
}

/// Brain's services as the worker's guest reaches them: JSON in, JSON out, one call at a
/// time.
struct ServicesBridge(Arc<dyn brain::TurnServices>);

#[async_trait]
impl brain_loophost::TurnBridge for ServicesBridge {
    async fn call(
        &self,
        call: brain_loophost::HostCall,
    ) -> Result<String, brain_protocol::TurnError> {
        use brain_loophost::HostCall;
        let answer = match call {
            HostCall::Events { after } => {
                let page = self.0.events(after).await.map_err(turn_error)?;
                serde_json::to_string(&page).map_err(|error| bridge_error("internal", error))?
            }
            HostCall::Model { request_json } => {
                let request = serde_json::from_str(&request_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                let result = self.0.model(request).await.map_err(turn_error)?;
                serde_json::to_string(&result).map_err(|error| bridge_error("internal", error))?
            }
            HostCall::Dispatch { calls_json } => {
                let calls = serde_json::from_str(&calls_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                let results = self.0.dispatch(calls).await.map_err(turn_error)?;
                serde_json::to_string(&results).map_err(|error| bridge_error("internal", error))?
            }
            HostCall::Emit { kind, payload_json } => {
                let payload = serde_json::from_str(&payload_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                self.0
                    .emit(kind, payload)
                    .await
                    .map_err(turn_error)?
                    .to_string()
            }
            HostCall::Telemetry { record_json } => {
                if let Ok(record) = serde_json::from_str(&record_json) {
                    self.0.telemetry(record);
                }
                String::new()
            }
        };
        Ok(answer)
    }

    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
}

fn turn_error(error: brain::Error) -> brain_protocol::TurnError {
    brain_protocol::TurnError {
        code: error.code().to_owned(),
        message: error.to_string(),
        retryable: error.retryable(),
    }
}

fn bridge_error(code: &str, error: impl std::fmt::Display) -> brain_protocol::TurnError {
    brain_protocol::TurnError::new(code, error.to_string())
}

fn valid_identity(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_agentloop_identity(identity: &AgentloopIdentity) -> Result<(), ApiError> {
    if !valid_identity(identity.as_str()) {
        return Err(ApiError::invalid_request(
            "session Agentloop identity must be 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn native_tool_identity(
    request: &CreateSessionRequest,
    tool: &brain_protocol::BoundTool,
) -> Result<Option<ToolIdentity>, ApiError> {
    if tool.hosting != ToolHosting::Provisioned {
        return Ok(None);
    }
    let Some(environment_id) = tool.environment_id.as_ref() else {
        return Ok(None);
    };
    let native = request.environments.iter().any(|environment| {
        &environment.environment_id == environment_id
            && environment
                .configuration
                .get("driver")
                .and_then(serde_json::Value::as_str)
                == Some("brain_wasm")
    });
    if !native {
        return Ok(None);
    }
    let identity = tool
        .implementation
        .as_ref()
        .filter(|implementation| {
            implementation
                .get("type")
                .and_then(serde_json::Value::as_str)
                == Some("brain_component")
        })
        .and_then(|implementation| implementation.get("identity"))
        .and_then(serde_json::Value::as_str)
        .filter(|identity| valid_identity(identity))
        .ok_or_else(|| {
            ApiError::invalid_request(
                "a Tool in the brain_wasm Environment requires a brain_component implementation with a 64-character lowercase hexadecimal identity",
            )
        })?;
    Ok(Some(ToolIdentity::new(identity)))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

/// Splits each wire tool back into its two internal halves: what the model may be told
/// (`ToolDefinition`) and where the call goes (`ToolBinding`). The session still
/// validates the split result against the session contract.
fn split_tools(request: &CreateSessionRequest) -> (Vec<ToolDefinition>, Vec<ToolBinding>) {
    let mut definitions = Vec::with_capacity(request.tools.len());
    let mut bindings = Vec::with_capacity(request.tools.len());
    for tool in &request.tools {
        definitions.push(ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
        });
        bindings.push(ToolBinding {
            name: tool.name.clone(),
            environment_id: tool.environment_id.clone(),
            environment: None,
            attachment_id: None,
            host_id: tool.host_id.clone(),
            needs: tool.needs.clone(),
            binding_names: tool.binding_names.clone(),
            hosting: tool.hosting,
            implementation: tool.implementation.clone(),
        });
    }
    (definitions, bindings)
}

/// The environments a create names, before any is attached, and their binding values.
/// The values travel beside the create for exactly as long as attach needs them and never
/// enter the configuration the session journals.
fn attachments_for(
    request: &CreateSessionRequest,
) -> (Vec<EnvironmentAttachment>, crate::SessionBindingValues) {
    let mut environments = Vec::with_capacity(request.environments.len());
    let mut values = crate::SessionBindingValues::new();
    for requirement in &request.environments {
        environments.push(EnvironmentAttachment {
            environment_id: requirement.environment_id.clone(),
            configuration: requirement.configuration.clone(),

            binding: None,
            attachment_id: None,
            resources: Default::default(),
        });
        if !requirement.bindings.is_empty() {
            values.insert(
                requirement.environment_id.clone(),
                requirement.bindings.clone(),
            );
        }
    }
    (environments, values)
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
    use super::{KeyedLocks, native_tool_identity, validate_agentloop_identity};

    fn raw_create(
        agentloop_identity: &str,
        driver: &str,
        implementation: serde_json::Value,
    ) -> brain_protocol::CreateSessionRequest {
        serde_json::from_value(serde_json::json!({
            "agentloop": {
                "identity": agentloop_identity,
                "configuration": {},
                "environment_id": "env_test",
            },
            "model": { "provider": "openai", "name": "gpt-5-mini", "api_key": "k" },
            "tools": [{
                "name": "lookup",
                "description": "Look something up.",
                "input_schema": {"type": "object"},
                "needs": [],
                "binding_names": [],
                "hosting": "provisioned",
                "implementation": implementation,
                "environment_id": "env_test",
            }],
            "environments": [{
                "environment_id": "env_test",
                "configuration": {"driver": driver},
            }],
        }))
        .unwrap()
    }

    #[test]
    fn raw_create_rejects_traversal_shaped_component_identities() {
        let traversal = format!("{}a", "../".repeat(21));
        assert_eq!(traversal.len(), 64);
        let request = raw_create(
            &traversal,
            "brain_wasm",
            serde_json::json!({"type": "brain_component", "identity": traversal}),
        );
        assert!(validate_agentloop_identity(&request.agentloop.identity).is_err());
        assert!(native_tool_identity(&request, &request.tools[0]).is_err());
    }

    #[test]
    fn native_tool_requires_a_component_identity_at_create() {
        let identity = "b".repeat(64);
        let request = raw_create(
            &"a".repeat(64),
            "brain_wasm",
            serde_json::json!({"type": "brain_component", "identity": identity}),
        );
        assert_eq!(
            native_tool_identity(&request, &request.tools[0])
                .unwrap()
                .unwrap()
                .as_str(),
            identity
        );

        let wrong_type = raw_create(
            &"a".repeat(64),
            "brain_wasm",
            serde_json::json!({"type": "remote", "identity": "b".repeat(64)}),
        );
        assert!(native_tool_identity(&wrong_type, &wrong_type.tools[0]).is_err());

        let remote = raw_create(
            &"a".repeat(64),
            "custom_driver",
            serde_json::json!({"type": "remote"}),
        );
        assert!(
            native_tool_identity(&remote, &remote.tools[0])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn model_selection_is_admitted_against_the_composed_registry() {
        let registry = brain::model::ProviderRegistry::default_set();
        let request = |provider: &str, name: &str, response_format: bool| {
            serde_json::from_value::<brain_protocol::CreateSessionRequest>(serde_json::json!({
                "agentloop": {
                    "identity": "a".repeat(64),
                    "configuration": {},
                    "environment_id": "env_test",
                },
                "model": { "provider": provider, "name": name, "api_key": "k" },
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
