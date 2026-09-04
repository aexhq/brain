use std::{
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, Weak},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use brain::{Feed, JournalStore, LoopExecutor, Session, SessionRuntime, SessionStore, Writer};
use brain_http::BrainApi;
use brain_loophost::LoopError;
use brain_loophost::WorkerPool;
use brain_protocol::codes;
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopIdentity, ApiError, CreateEnvironmentRequest,
    CreateSessionRequest, EnvironmentAttachment, EnvironmentCallRequest, EnvironmentCallResult,
    EnvironmentId, EnvironmentList, EnvironmentSummary, EventPage, MessageRequest, ModelBinding,
    Outcome, SessionConfig, SessionId, SessionList, SessionStatus, SessionSummary, ToolBinding,
    ToolDefinition,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;

use crate::{
    EnvironmentNotice, EnvironmentNoticeKind, EnvironmentRegistry, IdempotencyStore,
    LocalSessionOwnership, ModelBindingStore, SessionOwnership,
};

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
    pub session_idle_ttl: Duration,
    /// Answers already given to keyed requests, so a retry replays instead of repeats.
    pub idempotency: IdempotencyStore,
    pub loops: Arc<WorkerPool>,
    pub environments: Arc<EnvironmentRegistry>,
    pub models: Arc<dyn ModelBindingStore>,
    /// The composed provider set this deployment admits sessions against.
    pub providers: Arc<brain::model::ProviderRegistry>,
    /// What the server knows about a session that its records do not: the credential it
    /// calls a model with.
    pub metadata: Arc<crate::metadata::ServerMetadata>,
    /// Keys every session's share key. Derived from the API token when one is
    /// configured — so share keys survive a restart — and random in open mode.
    pub serve_secret: [u8; 32],
}

#[derive(Clone)]
pub struct ServerApi {
    resources: Arc<ServerResources>,
    /// Every session on disk. Its store is always open; its task runs only while the
    /// session is active, and is dropped when the session has sat idle past its TTL.
    sessions: Arc<StdMutex<HashMap<SessionId, Slot>>>,
    idempotency_locks: Arc<KeyedLocks<String>>,
    session_locks: Arc<KeyedLocks<SessionId>>,
    ownership: Arc<dyn SessionOwnership>,
}

struct Slot {
    store: Arc<SessionStore>,
    /// The running task, or `None` while the session is suspended.
    session: Option<Session>,
    last_touch: Instant,
    /// Zero means never suspend.
    idle_ttl: Duration,
    /// The environments the session attached, so the environment side can ask who is
    /// attached without reading every configuration.
    environments: Vec<EnvironmentId>,
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
    /// Opens every session on disk. Each comes back suspended: its records are indexed
    /// and any turn the last process left running is closed, but no task is started
    /// until something asks for it.
    pub fn new(resources: ServerResources) -> Result<Self, brain::Error> {
        Self::with_ownership(resources, Arc::new(LocalSessionOwnership))
    }

    pub fn with_ownership(
        resources: ServerResources,
        ownership: Arc<dyn SessionOwnership>,
    ) -> Result<Self, brain::Error> {
        let stores = SessionStore::open_all(
            &resources.sessions_dir,
            resources.writer.clone(),
            resources.feed.clone(),
        )?;
        let api = Self {
            resources: Arc::new(resources),
            sessions: Arc::default(),
            idempotency_locks: Arc::new(KeyedLocks::default()),
            session_locks: Arc::new(KeyedLocks::default()),
            ownership,
        };
        for store in stores {
            api.insert_slot(store, None)
                .map_err(|error| brain::Error::Journal(error.message))?;
        }
        Ok(api)
    }

    /// Suspends sessions that have sat idle past their TTL, on a timer. Suspension drops
    /// the task and its memory; the store stays open and the session is rebuilt from it
    /// on its next request.
    pub fn spawn_idle_sweeper(&self) -> tokio::task::JoinHandle<()> {
        let api = self.clone();
        let every = (api.resources.session_idle_ttl / 4)
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
                        && !slot.idle_ttl.is_zero()
                        && slot.last_touch.elapsed() >= slot.idle_ttl
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
            if slot.last_touch.elapsed() < slot.idle_ttl {
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
        store.sync().map_err(api_error)?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?;
        if let Some(slot) = sessions.get_mut(session_id) {
            slot.session = None;
        }
        Ok(())
    }

    fn insert_slot(
        &self,
        store: Arc<SessionStore>,
        session: Option<Session>,
    ) -> Result<(), ApiError> {
        let config = brain::session_config(&*store).ok();
        let idle_ttl = config
            .as_ref()
            .and_then(|config| config.idle_ttl_ms)
            .map(Duration::from_millis)
            .unwrap_or(self.resources.session_idle_ttl);
        let environments = config
            .map(|config| {
                config
                    .environments
                    .into_iter()
                    .map(|attachment| attachment.environment_id)
                    .collect()
            })
            .unwrap_or_default();
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
                    environments,
                },
            );
        Ok(())
    }

    /// The sessions attached to an environment: those that named it and have not ended.
    fn attached_sessions(
        &self,
        environment_id: &EnvironmentId,
    ) -> Result<Vec<SessionId>, ApiError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?;
        let mut attached = Vec::new();
        for (session_id, slot) in sessions.iter() {
            if !slot.environments.contains(environment_id) {
                continue;
            }
            let status = slot.store.session_summary().map_err(api_error)?.status;
            if !matches!(status, SessionStatus::Ended | SessionStatus::Failed) {
                attached.push(session_id.clone());
            }
        }
        attached.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(attached)
    }

    /// Closes managed environments that have had no session attached for their TTL.
    pub fn spawn_environment_sweeper(&self) -> tokio::task::JoinHandle<()> {
        let api = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                api.close_idle_environments().await;
            }
        })
    }

    /// Closes every managed environment idle past its TTL. Public so a host without the
    /// sweeper can drive it.
    pub async fn close_idle_environments(&self) {
        let Ok(ids) = self.resources.environments.ids() else {
            return;
        };
        for environment_id in ids {
            let Ok(Some(ttl)) = self.resources.environments.idle_ttl(&environment_id) else {
                continue;
            };
            let Ok(Some(since)) = self.resources.environments.idle_since(&environment_id) else {
                continue;
            };
            if since.elapsed() < ttl {
                continue;
            }
            if self
                .attached_sessions(&environment_id)
                .map(|attached| !attached.is_empty())
                .unwrap_or(true)
            {
                continue;
            }
            if let Err(error) = self.resources.environments.close(&environment_id).await {
                tracing::warn!(%environment_id, %error, "idle Environment could not be closed");
            }
        }
    }

    /// Writes environment notices onto every attached session's events, so a loop sees
    /// on its next activation that an environment closed or stopped answering.
    pub fn spawn_environment_notices(&self) -> tokio::task::JoinHandle<()> {
        let api = self.clone();
        let mut notices = self.resources.environments.subscribe();
        tokio::spawn(async move {
            loop {
                match notices.recv().await {
                    Ok(notice) => api.note_environment(notice).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    async fn note_environment(&self, notice: EnvironmentNotice) {
        let kind = match notice.kind {
            EnvironmentNoticeKind::Closed => codes::event::ENVIRONMENT_CLOSED,
            EnvironmentNoticeKind::Unreachable => codes::event::ENVIRONMENT_UNREACHABLE,
        };
        let payload = serde_json::json!({ "environment_id": notice.environment_id });
        let Ok(attached) = self.attached_sessions(&notice.environment_id) else {
            return;
        };
        for session_id in attached {
            let Ok(lock) = self.session_lock(&session_id) else {
                continue;
            };
            let _guard = lock.lock().await;
            let (session, store) = {
                let Ok(sessions) = self.sessions.lock() else {
                    return;
                };
                let Some(slot) = sessions.get(&session_id) else {
                    continue;
                };
                (slot.session.clone(), slot.store.clone())
            };
            let written = match session {
                Some(session) => session.record(kind, payload.clone()).await.map(|_| ()),
                None => store
                    .session_row()
                    .and_then(|row| {
                        store.append(
                            row.through_sequence,
                            &[brain::AppendRecord::new(kind, payload.clone())],
                            brain::SessionUpdate::default(),
                        )
                    })
                    .map(|_| ()),
            };
            if let Err(error) = written {
                tracing::warn!(%session_id, %error, "Environment notice could not be recorded");
            }
        }
    }

    fn store(&self, session_id: &SessionId) -> Result<Arc<SessionStore>, ApiError> {
        self.sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get(session_id)
            .map(|slot| slot.store.clone())
            .ok_or_else(|| not_found("session not found"))
    }

    /// The running session, rebuilt from its directory if it is suspended. An ended or
    /// failed session is not rebuilt: nothing can be asked of it.
    async fn session(&self, session_id: &SessionId) -> Result<Session, ApiError> {
        let resumed = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| internal("session table is poisoned"))?;
            let slot = sessions
                .get_mut(session_id)
                .ok_or_else(|| not_found("session not found"))?;
            slot.last_touch = Instant::now();
            if let Some(session) = &slot.session {
                return Ok(session.clone());
            }
            let status = slot.store.session_summary().map_err(api_error)?.status;
            if matches!(
                status,
                brain_protocol::SessionStatus::Ended | brain_protocol::SessionStatus::Failed
            ) {
                return Err(ApiError::invalid_request("session has ended"));
            }
            let session = Session::open(slot.store.clone(), self.resources.session_runtime.clone())
                .map_err(api_error)?;
            slot.session = Some(session.clone());
            session
        };
        resumed
            .record(codes::event::SESSION_RESUMED, serde_json::json!({}))
            .await
            .map_err(api_error)?;
        Ok(resumed)
    }

    fn remember(&self, store: Arc<SessionStore>, session: Session) -> Result<(), ApiError> {
        self.insert_slot(store, Some(session))
    }

    /// Drops a session's task, keeping its directory and its slot.
    fn forget(&self, session_id: &SessionId) -> Result<(), ApiError> {
        if let Some(slot) = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get_mut(session_id)
        {
            slot.session = None;
        }
        Ok(())
    }

    fn summary(&self, session_id: &SessionId) -> Result<SessionSummary, ApiError> {
        self.store(session_id)?
            .session_summary()
            .map_err(api_error)
            .map(|session| self.branded(session))
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

    /// Stamps the share key onto a session leaving the API. A session is minted without
    /// one — the key is this serving layer's credential, not session state.
    fn branded(&self, mut session: SessionSummary) -> SessionSummary {
        session.share_key = BrainApi::share_key(self, &session.session_id);
        session
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
            .idempotency
            .get("admit_agentloop", &idempotency_key, &package)
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
        let lock = self.idempotency_lock("create_session", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .get("create_session", &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved).map(|session| self.branded(session));
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
        let (tools, tool_bindings) = split_tools(&request);
        let (environments, binding_values) = attachments_for(&request);
        let config = SessionConfig {
            agentloop_identity: request.agentloop.identity.clone(),
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
        let session_id = SessionId::new(brain::random_id("ses"));
        let store = SessionStore::create(
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
        let creation = match Session::begin(
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
        if let Err(error) = self.ownership.claim_new(creation.session_id()).await {
            creation
                .fail(codes::failure::SESSION_OWNERSHIP_FAILED, &error.to_string())
                .map_err(api_error)?;
            self.insert_slot(store, None)?;
            self.resources
                .models
                .delete(&binding_id)
                .map_err(api_error)?;
            return Err(api_error(error));
        }
        let prepared = self
            .resources
            .environments
            .prepare_session(creation, config, binding_values)
            .await;
        let (session, _) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.insert_slot(store, None)?;
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
        self.remember(store, session)?;
        let session = self.summary(&session_id)?;
        self.resources
            .idempotency
            .put(
                "create_session",
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(self.branded(session))
    }

    async fn get_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError> {
        self.summary(&session_id)
    }

    async fn list_sessions(&self) -> Result<SessionList, ApiError> {
        let stores: Vec<Arc<SessionStore>> = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .values()
            .map(|slot| slot.store.clone())
            .collect();
        let mut sessions = Vec::with_capacity(stores.len());
        for store in stores {
            sessions.push(self.branded(store.session_summary().map_err(api_error)?));
        }
        sessions.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Ok(SessionList { sessions })
    }

    async fn create_environment(
        &self,
        idempotency_key: String,
        request: CreateEnvironmentRequest,
    ) -> Result<EnvironmentSummary, ApiError> {
        if request
            .environment_id
            .as_ref()
            .is_some_and(|id| !valid_identifier(id.as_str()))
        {
            return Err(ApiError::invalid_request("Environment id is invalid"));
        }
        let lock = self.idempotency_lock("create_environment", &idempotency_key)?;
        let _guard = lock.lock().await;
        if let Some(saved) = self
            .resources
            .idempotency
            .get("create_environment", &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let record = self
            .resources
            .environments
            .create(request.clone())
            .await
            .map_err(api_error)?;
        let summary = self
            .resources
            .environments
            .summary(&record.environment_id, Vec::new())
            .map_err(api_error)?
            .ok_or_else(|| internal("Environment vanished after creation"))?;
        self.resources
            .idempotency
            .put(
                "create_environment",
                &idempotency_key,
                &request,
                &serde_json::to_value(&summary).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(summary)
    }

    async fn get_environment(
        &self,
        environment_id: EnvironmentId,
    ) -> Result<EnvironmentSummary, ApiError> {
        let attached = self.attached_sessions(&environment_id)?;
        self.resources
            .environments
            .summary(&environment_id, attached)
            .map_err(api_error)?
            .ok_or_else(|| not_found("Environment does not exist"))
    }

    async fn list_environments(&self) -> Result<EnvironmentList, ApiError> {
        let mut environments = Vec::new();
        for environment_id in self.resources.environments.ids().map_err(api_error)? {
            let attached = self.attached_sessions(&environment_id)?;
            if let Some(summary) = self
                .resources
                .environments
                .summary(&environment_id, attached)
                .map_err(api_error)?
            {
                environments.push(summary);
            }
        }
        Ok(EnvironmentList { environments })
    }

    async fn delete_environment(
        &self,
        environment_id: EnvironmentId,
        idempotency_key: String,
    ) -> Result<(), ApiError> {
        let request = (environment_id.clone(), "delete");
        let scope = format!("environment:{environment_id}:delete");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if self
            .resources
            .idempotency
            .get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        let attached = self.attached_sessions(&environment_id)?;
        if !attached.is_empty() {
            return Err(ApiError::conflict(format!(
                "Environment is attached to {} session(s); end them first",
                attached.len()
            )));
        }
        self.resources
            .environments
            .close(&environment_id)
            .await
            .map_err(api_error)?;
        self.resources
            .idempotency
            .put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<SessionSummary, ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let scope = format!("session:{session_id}:message");
        if let Some(saved) = self
            .resources
            .idempotency
            .get(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved).map(|session| self.branded(session));
        }
        let session = self
            .session(&session_id)
            .await?
            .message(request.clone())
            .await
            .map_err(api_error)?;
        self.resources
            .idempotency
            .put(
                &scope,
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(self.branded(session))
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
            .idempotency
            .get(&scope, &idempotency_key, &call)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let store = self.store(&session_id)?;
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
        let records = self
            .store(&session_id)?
            .records_after(after, 1_000)
            .map_err(api_error)?;
        Ok(brain::event_page(records, after))
    }

    fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.resources.feed.subscribe()
    }

    fn share_key(&self, session_id: &SessionId) -> String {
        let mut digest = Sha256::new();
        digest.update(b"brain.share-key.v1\0");
        digest.update(self.resources.serve_secret);
        digest.update(b"\0");
        digest.update(session_id.as_str().as_bytes());
        format!("sk.{session_id}.{}", hex::encode(digest.finalize()))
    }

    async fn client_tool_names(&self, session_id: SessionId) -> Result<Vec<String>, ApiError> {
        Ok(brain::session_config(&*self.store(&session_id)?)
            .map_err(api_error)?
            .tool_bindings
            .into_iter()
            .filter(|binding| matches!(binding.hosting, brain_protocol::ToolHosting::Client))
            .map(|binding| binding.name)
            .collect())
    }

    async fn resolve_tool_call(
        &self,
        session_id: SessionId,
        sequence: u64,
        idempotency_key: String,
        outcome: Outcome,
    ) -> Result<(), ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        // No session lock here on purpose: `send_message` holds it for the whole turn,
        // and this is the request that lets that turn finish.
        let scope = format!("session:{session_id}:tool_result:{sequence}");
        let lock = self.idempotency_lock(&scope, &idempotency_key)?;
        let _guard = lock.lock().await;
        if self
            .resources
            .idempotency
            .get::<_>(&scope, &idempotency_key, &outcome)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.session(&session_id)
            .await?
            .resolve_tool_call(sequence, outcome.clone())
            .map_err(api_error)?;
        self.resources
            .idempotency
            .put(&scope, &idempotency_key, &outcome, &serde_json::json!({}))
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
            .idempotency
            .get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.session(&session_id)
            .await?
            .cancel()
            .await
            .map_err(api_error)?;
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
            .idempotency
            .get(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved).map(|session| self.branded(session));
        }
        let store = self.store(&session_id)?;
        let summary = store.session_summary().map_err(api_error)?;
        if matches!(summary.status, brain_protocol::SessionStatus::Ended) {
            // Already ended: nothing to release, and the answer is the same.
            return Ok(self.branded(summary));
        }
        let config = brain::session_config(&*store).map_err(api_error)?;
        let running = self.session(&session_id).await?;
        self.resources
            .environments
            .release_session(&running, &config)
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
        Ok(self.branded(session))
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
            .idempotency
            .get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        let store = self.store(&session_id)?;
        let binding_id = brain::session_config(&*store)
            .map_err(api_error)?
            .model
            .binding_id;
        store.delete().map_err(api_error)?;
        self.resources
            .models
            .delete(&binding_id)
            .map_err(api_error)?;
        self.sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .remove(&session_id);
        self.resources
            .idempotency
            .put(&scope, &idempotency_key, &request, &serde_json::json!({}))
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
    async fn turn(
        &self,
        session: &brain_protocol::SessionId,
        agentloop: &AgentloopIdentity,
        input: brain_protocol::TurnInput,
        services: Arc<dyn brain::TurnServices>,
    ) -> Result<brain_protocol::TurnOutput, brain::Error> {
        let bridge = ServicesBridge(services);
        self.0
            .turn(
                session.as_str().to_owned(),
                agentloop.clone(),
                input,
                &bridge,
            )
            .await
            .map_err(|error| match error {
                LoopError::Overloaded => brain::Error::Overloaded(error.to_string()),
                LoopError::Failed(message) if message.starts_with("cancelled:") => {
                    brain::Error::Cancelled(message)
                }
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
            HostCall::Append { kind, payload_json } => {
                let payload = serde_json::from_str(&payload_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                self.0
                    .append(kind, payload)
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
            needs: tool.needs.clone(),
            binding_names: tool.binding_names.clone(),
            hosting: tool.hosting,
            program: tool.program.clone(),
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
            binding: None,
            attachment_id: None,
            runtimes: Vec::new(),
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

fn model_binding_id(idempotency_key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"brain.model-binding.v1\0");
    digest.update(idempotency_key.as_bytes());
    format!("model_{}", hex::encode(digest.finalize()))
}

fn loop_error(error: LoopError) -> ApiError {
    match error {
        LoopError::Overloaded => ApiError::overloaded(error.to_string()),
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
