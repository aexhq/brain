use crate::{EnvironmentRegistry, locks::KeyedLocks};
use brain::{Feed, LocalSessionStore, Session, SessionRuntime, SessionStore, Writer};
use brain_protocol::{
    ApiError, EnvironmentCallRequest, EnvironmentCallResult, EnvironmentName, EventPage,
    MessageRequest, SessionConfig, SessionId, SessionList, SessionStatus, SessionSummary, codes,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub struct SessionResources {
    pub sessions_dir: PathBuf,
    pub writer: Arc<Writer>,
    pub feed: Arc<Feed>,
    pub session_runtime: Arc<SessionRuntime>,
    pub session_idle_ttl: Option<Duration>,
    pub environments: Arc<EnvironmentRegistry>,
}

#[derive(Clone)]
pub struct Sessions {
    draining: Arc<AtomicBool>,
    active: Arc<tokio::sync::RwLock<()>>,
    resources: Arc<SessionResources>,
    sessions: Arc<StdMutex<HashMap<SessionId, Entry>>>,
    stores: Arc<StdMutex<HashMap<SessionId, Weak<LocalSessionStore>>>>,
    store_locks: Arc<KeyedLocks<SessionId>>,
    session_locks: Arc<KeyedLocks<SessionId>>,
    background_turns: Arc<StdMutex<HashMap<SessionId, Vec<u64>>>>,
}
struct Entry {
    store: Arc<LocalSessionStore>,
    /// The running task, or `None` while the session is suspended.
    session: Option<Session>,
    last_touch: Instant,
    /// Zero means never suspend.
    idle_ttl: Option<Duration>,
}

/// An exclusively reserved session whose next message has not been dispatched.
pub struct MessageAdmission {
    api: Sessions,
    session_id: SessionId,
    request: MessageRequest,
    active: tokio::sync::OwnedRwLockReadGuard<()>,
    guard: tokio::sync::OwnedMutexGuard<()>,
    session: Session,
    store: Arc<LocalSessionStore>,
}

impl MessageAdmission {
    /// Start work independently of the submitting request, after its intent is durable.
    pub async fn start(
        self,
    ) -> Result<
        (
            u64,
            tokio::sync::oneshot::Receiver<Result<SessionSummary, brain::Error>>,
        ),
        ApiError,
    > {
        let Self {
            api,
            session_id,
            request,
            active,
            guard,
            session,
            store,
        } = self;
        let (reply, accepted) = tokio::sync::oneshot::channel();
        let (finished, result) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            match session.submit(request).await {
                Ok(turn) => {
                    let sequence = turn.sequence;
                    let _ = reply.send(Ok(sequence));
                    let _ = finished.send(turn.wait().await);
                    let tools = session.tools();
                    let following = api
                        .background_turns
                        .lock()
                        .expect("background turn table poisoned")
                        .contains_key(&session_id);
                    if !following
                        && !tools.has_work_after(store.processed_through().map_err(api_error)?)
                    {
                        api.resources
                            .environments
                            .turn_ended(&session, &*store, sequence)
                            .await
                            .map_err(api_error)?;
                        return api.passivate_unretained(&session_id).await;
                    }
                    let passivated = api.passivate_unretained(&session_id).await;
                    drop(session);
                    api.follow_tools(session_id, store, tools, sequence, active);
                    drop(guard);
                    passivated
                }
                Err(error) => {
                    let _ = reply.send(Err(api_error(error)));
                    api.passivate_unretained(&session_id).await
                }
            }
        });
        let sequence = accepted
            .await
            .map_err(|_| internal("turn admission stopped"))??;
        Ok((sequence, result))
    }
}

impl Sessions {
    pub fn new(resources: SessionResources) -> Result<Self, brain::Error> {
        resources
            .environments
            .bind_executions(&resources.session_runtime.tool_executions);
        std::fs::create_dir_all(&resources.sessions_dir)
            .map_err(|error| brain::Error::Journal(error.to_string()))?;
        Ok(Self {
            draining: Arc::default(),
            active: Arc::default(),
            resources: Arc::new(resources),
            sessions: Arc::default(),
            stores: Arc::default(),
            store_locks: Arc::new(KeyedLocks::default()),
            session_locks: Arc::new(KeyedLocks::default()),
            background_turns: Arc::default(),
        })
    }

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

    /// Refuse new turns and Environment calls while existing work finishes with its services intact.
    pub async fn drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.resources.environments.on_observation(
            None,
            self.resources.session_runtime.limits.max_emitted_bytes,
        );
        let _finished = self.active.write().await;
    }

    async fn admit_work(&self) -> Result<tokio::sync::OwnedRwLockReadGuard<()>, ApiError> {
        if self.draining.load(Ordering::Acquire) {
            return Err(ApiError::overloaded("Brain is draining active work"));
        }
        let guard = self.active.clone().read_owned().await;
        if self.draining.load(Ordering::Acquire) {
            return Err(ApiError::overloaded("Brain is draining active work"));
        }
        Ok(guard)
    }

    pub async fn suspend_idle(&self) {
        let due: Vec<SessionId> = match self.sessions.lock() {
            Ok(sessions) => sessions
                .iter()
                .filter(|(_, entry)| {
                    entry.session.is_some()
                        && entry
                            .idle_ttl
                            .is_none_or(|ttl| !ttl.is_zero() && entry.last_touch.elapsed() >= ttl)
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

    async fn suspend(&self, session_id: &SessionId) -> Result<(), ApiError> {
        let lock = self.session_lock(session_id)?;
        let _guard = lock.lock().await;
        let (session, store) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| internal("session table is poisoned"))?;
            let Some(entry) = sessions.get(session_id) else {
                return Ok(());
            };
            let Some(session) = entry.session.clone() else {
                return Ok(());
            };
            if entry
                .idle_ttl
                .is_some_and(|ttl| ttl.is_zero() || entry.last_touch.elapsed() < ttl)
            {
                return Ok(());
            }
            (session, entry.store.clone())
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

    fn insert_entry(
        &self,
        store: Arc<LocalSessionStore>,
        session: Option<Session>,
    ) -> Result<(), ApiError> {
        let idle_ttl = brain::session_config(&*store)
            .ok()
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
                Entry {
                    store,
                    session,
                    last_touch: Instant::now(),
                    idle_ttl,
                },
            );
        Ok(())
    }

    fn cache_store(&self, store: &Arc<LocalSessionStore>) -> Result<(), ApiError> {
        self.resources.environments.track(store.clone());
        let draining = Arc::downgrade(&self.draining);
        let active = Arc::downgrade(&self.active);
        let resources = Arc::downgrade(&self.resources);
        let sessions = Arc::downgrade(&self.sessions);
        let stores = Arc::downgrade(&self.stores);
        let store_locks = Arc::downgrade(&self.store_locks);
        let session_locks = Arc::downgrade(&self.session_locks);
        let background_turns = Arc::downgrade(&self.background_turns);
        if !self.draining.load(Ordering::Acquire) {
            self.resources.environments.on_observation(Some(Arc::new(move |session, sequence| {
            let (Some(draining), Some(active), Some(resources), Some(sessions), Some(stores), Some(store_locks), Some(session_locks), Some(background_turns)) =
                (draining.upgrade(), active.upgrade(), resources.upgrade(), sessions.upgrade(), stores.upgrade(), store_locks.upgrade(), session_locks.upgrade(), background_turns.upgrade()) else { return; };
            let api = Sessions { draining, active, resources, sessions, stores, store_locks, session_locks, background_turns };
            tokio::spawn(async move {
                if let Err(error) = api.environment_observed(session, sequence).await {
                    tracing::warn!(error = %error.message, "Environment observation could not activate its Agentloop");
                }
            });
        })), self.resources.session_runtime.limits.max_emitted_bytes);
        }
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

    async fn session(&self, session_id: &SessionId) -> Result<Session, ApiError> {
        if let Some(session) = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get_mut(session_id)
            .and_then(|entry| {
                entry.last_touch = Instant::now();
                entry.session.clone()
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
        self.insert_entry(store, Some(session.clone()))?;
        session
            .record(codes::event::SESSION_RESUMED, serde_json::json!({}))
            .await
            .map_err(api_error)?;
        Ok(session)
    }

    fn remember(&self, store: Arc<LocalSessionStore>, session: Session) -> Result<(), ApiError> {
        self.insert_entry(store, Some(session))
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

    async fn cleanup_environments(&self, store: &dyn SessionStore) -> Result<(), ApiError> {
        let config = brain::session_config(store).map_err(api_error)?;
        let environments = brain::environment::environments(store, &config).map_err(api_error)?;
        for view in environments
            .values()
            .rev()
            .filter(|view| view.state == brain_protocol::EnvironmentState::Ready)
        {
            let environment = brain::environment::descriptor(&config, view).map_err(api_error)?;
            if let Err(error) = self
                .resources
                .environments
                .close(&environment, store, false)
                .await
            {
                tracing::warn!(environment = %environment.name, %error, "failed to clean up Environment after session admission");
            }
        }
        Ok(())
    }

    pub async fn create_session(
        &self,
        session_id: SessionId,
        config: SessionConfig,
        transcript: Vec<brain_protocol::Message>,
    ) -> Result<SessionSummary, ApiError> {
        let _active = self.admit_work().await?;
        if config
            .environments
            .iter()
            .any(|environment| environment.lifecycle.is_none())
        {
            return Err(ApiError::invalid_request(
                "each Environment requires an explicit lifecycle",
            ));
        }
        if config
            .environment(&config.agentloop.environment)
            .is_some_and(|environment| {
                environment.lifecycle == Some(brain_protocol::EnvironmentLifecycle::Manual)
            })
        {
            return Err(ApiError::invalid_request(
                "the Agentloop requires an automatically managed bootstrap Environment",
            ));
        }
        let session_lock = self.session_lock(&session_id)?;
        let _session_guard = session_lock.lock().await;
        let store_lock = self.store_locks.acquire(session_id.clone())?;
        let store_guard = store_lock.lock().await;
        let store = LocalSessionStore::create(
            &self.resources.sessions_dir.join(session_id.as_str()),
            session_id.clone(),
            &serde_json::to_value(&config).map_err(|error| internal(error.to_string()))?,
            self.resources.writer.clone(),
            self.resources.feed.clone(),
        )
        .map_err(api_error)?;
        let mut creation = match Session::begin(
            store.clone(),
            self.resources.session_runtime.clone(),
            &config,
            &transcript,
        ) {
            Ok(creation) => creation,
            Err(error) => {
                let _ = std::fs::remove_dir_all(store.directory());
                return Err(api_error(error));
            }
        };
        self.cache_store(&store)?;
        drop(store_guard);
        for environment in &config.environments {
            if environment.lifecycle == Some(brain_protocol::EnvironmentLifecycle::Manual) {
                continue;
            }
            let views = brain::environment::environments(&*store, &config).map_err(api_error)?;
            if matches!(
                views[&environment.name].state,
                brain_protocol::EnvironmentState::Ready | brain_protocol::EnvironmentState::Deleted
            ) {
                continue;
            }
            if let Err(error) = self
                .resources
                .environments
                .setup(&mut creation, environment)
                .await
            {
                creation
                    .fail(
                        codes::failure::ENVIRONMENT_PREPARATION_FAILED,
                        &error.to_string(),
                    )
                    .map_err(api_error)?;
                self.insert_entry(store.clone(), None)?;
                self.cleanup_environments(&*store).await?;
                return Err(api_error(error));
            }
        }
        let session = match creation.complete(config) {
            Ok(session) => session,
            Err(error) => {
                self.insert_entry(store.clone(), None)?;
                self.cleanup_environments(&*store).await?;
                return Err(api_error(error));
            }
        };
        self.remember(store, session)?;
        let session = self.summary(&session_id).await?;
        self.passivate_unretained(&session_id).await?;
        Ok(session)
    }

    /// Own the running turn independently of the request waiting for its start sequence.
    pub async fn submit_message(
        &self,
        session_id: SessionId,
        request: MessageRequest,
    ) -> Result<u64, ApiError> {
        Ok(self
            .prepare_message(session_id, request, false)
            .await?
            .start()
            .await?
            .0)
    }

    /// Reserve a session before the caller commits its request claim. Dropping this runs no turn.
    pub async fn prepare_message(
        &self,
        session_id: SessionId,
        request: MessageRequest,
        wait_for_session: bool,
    ) -> Result<MessageAdmission, ApiError> {
        Session::validate_message(&request).map_err(api_error)?;
        let active = self.admit_work().await?;
        let lock = self.session_lock(&session_id)?;
        let guard = if wait_for_session {
            lock.lock_owned().await
        } else {
            lock.try_lock_owned()
                .map_err(|_| ApiError::overloaded("session already has active work"))?
        };
        let session = self.session(&session_id).await?;
        let store = self.store(&session_id).await?;
        if !matches!(
            store.session_summary().map_err(api_error)?.status,
            SessionStatus::Idle
        ) {
            return Err(ApiError::conflict("session is not idle"));
        }
        Ok(MessageAdmission {
            api: self.clone(),
            session_id,
            request,
            active,
            guard,
            session,
            store,
        })
    }

    fn follow_tools(
        &self,
        session_id: SessionId,
        store: Arc<LocalSessionStore>,
        tools: Arc<brain::ToolGroup>,
        sequence: u64,
        active: tokio::sync::OwnedRwLockReadGuard<()>,
    ) {
        let mut background = self
            .background_turns
            .lock()
            .expect("background turn table poisoned");
        if let Some(turns) = background.get_mut(&session_id) {
            turns.push(sequence);
            return;
        }
        background.insert(session_id.clone(), vec![sequence]);
        drop(background);
        let api = self.clone();
        tokio::spawn(async move {
            let _active = active;
            if let Err(error) = api.run_tool_events(&session_id, &store, &tools).await {
                tracing::error!(%session_id, error = %error.message, "background Tool observations could not be processed");
                api.background_turns
                    .lock()
                    .expect("background turn table poisoned")
                    .remove(&session_id);
            }
        });
    }

    async fn run_tool_events(
        &self,
        session_id: &SessionId,
        store: &LocalSessionStore,
        tools: &brain::ToolGroup,
    ) -> Result<(), ApiError> {
        loop {
            while let Some(wakeup) = tools.next_wakeup().await {
                let lock = self.session_lock(session_id)?;
                let _guard = lock.lock().await;
                if matches!(
                    store.session_summary().map_err(api_error)?.status,
                    SessionStatus::Ended | SessionStatus::Failed
                ) {
                    self.background_turns
                        .lock()
                        .expect("background turn table poisoned")
                        .remove(session_id);
                    return Ok(());
                }
                if !tools.accepts(&wakeup)
                    || store.processed_through().map_err(api_error)? >= wakeup.through
                {
                    continue;
                }
                let session = self.session(session_id).await?;
                let turn = session.submit_events().await.map_err(api_error)?;
                self.background_turns
                    .lock()
                    .expect("background turn table poisoned")
                    .get_mut(session_id)
                    .expect("background turn is registered")
                    .push(turn.sequence);
                if let Err(error) = turn.wait().await {
                    tracing::warn!(%session_id, %error, "Agentloop failed while processing Tool observations");
                }
                self.passivate_unretained(session_id).await?;
            }
            let lock = self.session_lock(session_id)?;
            let _guard = lock.lock().await;
            if tools.has_work_after(store.processed_through().map_err(api_error)?) {
                continue;
            }
            let turns = self
                .background_turns
                .lock()
                .expect("background turn table poisoned")
                .remove(session_id)
                .unwrap_or_default();
            if matches!(
                store.session_summary().map_err(api_error)?.status,
                SessionStatus::Ended | SessionStatus::Failed
            ) {
                return Ok(());
            }
            let session = self.session(session_id).await?;
            for sequence in turns.into_iter().filter(|sequence| *sequence != 0) {
                self.resources
                    .environments
                    .turn_ended(&session, store, sequence)
                    .await
                    .map_err(api_error)?;
            }
            self.passivate_unretained(session_id).await?;
            return Ok(());
        }
    }

    async fn passivate_unretained(&self, session_id: &SessionId) -> Result<(), ApiError> {
        let release = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get(session_id)
            .is_some_and(|entry| entry.idle_ttl.is_none());
        if release {
            self.passivate(session_id).await?;
        }
        Ok(())
    }

    pub async fn get_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError> {
        self.summary(&session_id).await
    }

    async fn environment_observed(
        &self,
        session_id: SessionId,
        sequence: u64,
    ) -> Result<(), ApiError> {
        let active = self.admit_work().await?;
        let store = self.store(&session_id).await?;
        if !matches!(
            store.session_summary().map_err(api_error)?.status,
            SessionStatus::Idle | SessionStatus::Running
        ) {
            return Ok(());
        }
        let mut after = 0;
        let mut interrupted = false;
        loop {
            let records = store.records_after(after, 1000).map_err(api_error)?;
            if records.is_empty() {
                break;
            }
            for record in records {
                after = record.sequence;
                if record.kind == codes::event::TURN_STARTED
                    && record.payload.get("input").is_some()
                {
                    interrupted = false;
                } else if record.kind == codes::event::SESSION_INTERRUPTED
                    || (record.kind == codes::event::TURN_FAILED
                        && matches!(
                            record.payload["code"].as_str(),
                            Some("cancelled" | "interrupted")
                        ))
                {
                    interrupted = true;
                }
            }
        }
        if interrupted {
            return Ok(());
        }
        let tools = self
            .resources
            .session_runtime
            .tool_executions
            .group(&session_id);
        tools.observe(sequence);
        self.follow_tools(session_id, store, tools, 0, active);
        Ok(())
    }

    pub async fn environment_event(
        &self,
        session: SessionId,
        environment: brain_protocol::EnvironmentRef,
        event: brain_protocol::EnvironmentEvent,
    ) -> Result<u64, ApiError> {
        let _active = self.admit_work().await?;
        let store = self.store(&session).await?;
        self.resources
            .environments
            .report(store, &environment, event)
            .await
            .map_err(api_error)
    }

    pub async fn control_environment(
        &self,
        session: SessionId,
        request: brain_protocol::EnvironmentControlRequest,
    ) -> Result<serde_json::Value, ApiError> {
        let _active = self.admit_work().await?;
        let store = self.store(&session).await?;
        let config = brain::session_config(&*store).map_err(api_error)?;
        use brain_protocol::EnvironmentPermission as Permission;
        let grants: Vec<_> = config
            .environments
            .iter()
            .map(|environment| brain_protocol::EnvironmentGrant {
                environment: environment.name.clone(),
                permissions: vec![
                    Permission::Read,
                    Permission::Create,
                    Permission::Setup,
                    Permission::Update,
                    Permission::Delete,
                    Permission::Call,
                ],
                methods: environment.methods.keys().cloned().collect(),
            })
            .collect();
        self.resources
            .environments
            .control(store, None, &grants, request)
            .await
            .map_err(api_error)
    }

    pub async fn list_sessions(&self) -> Result<SessionList, ApiError> {
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

    pub async fn transcript(
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

    pub async fn events(
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

    pub fn subscribe(
        &self,
        session_id: &SessionId,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.resources.feed.subscribe(session_id)
    }

    pub async fn send_message(
        &self,
        session_id: SessionId,
        request: MessageRequest,
    ) -> Result<SessionSummary, ApiError> {
        let (_, result) = self
            .prepare_message(session_id, request, true)
            .await?
            .start()
            .await?;
        result
            .await
            .map_err(|_| internal("turn stopped"))?
            .map_err(api_error)
    }

    pub async fn call_environment(
        &self,
        session_id: SessionId,
        environment: EnvironmentName,
        name: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError> {
        let _active = self.admit_work().await?;
        if !valid_identifier(&name) {
            return Err(ApiError::invalid_request(
                "Environment method name is invalid",
            ));
        }
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let store = self.store(&session_id).await?;
        let session = self.session(&session_id).await?;
        let result = self
            .resources
            .environments
            .call(&session, &*store, &environment, name, request.input)
            .await
            .map_err(api_error)?;
        Ok(result)
    }

    pub async fn cancel_session(&self, session_id: SessionId) -> Result<(), ApiError> {
        self.store(&session_id)
            .await?
            .append_sync(
                &[brain::AppendRecord::new(
                    codes::event::SESSION_INTERRUPTED,
                    serde_json::json!({}),
                )],
                brain::SessionUpdate::default(),
            )
            .map_err(api_error)?;
        let session = self
            .sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .get(&session_id)
            .and_then(|entry| entry.session.clone());
        if let Some(session) = session {
            session.cancel().await.map_err(api_error)?;
        } else {
            self.summary(&session_id).await?;
            self.resources
                .session_runtime
                .tool_executions
                .group(&session_id)
                .interrupt()
                .await;
        }
        Ok(())
    }

    pub async fn end_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError> {
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let store = self.store(&session_id).await?;
        let tools = self
            .resources
            .session_runtime
            .tool_executions
            .group(&session_id);
        tools.interrupt().await;
        tools.wait().await;
        let summary = store.session_summary().map_err(api_error)?;
        if matches!(summary.status, brain_protocol::SessionStatus::Ended) {
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
        Ok(session)
    }

    pub async fn delete_session(&self, session_id: SessionId) -> Result<(), ApiError> {
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let store = self.store(&session_id).await?;
        store.ensure_deletable().map_err(api_error)?;
        let tools = self
            .resources
            .session_runtime
            .tool_executions
            .group(&session_id);
        tools.interrupt().await;
        tools.wait().await;
        let config = brain::session_config(&*store).map_err(api_error)?;
        let views = brain::environment::environments(&*store, &config).map_err(api_error)?;
        for view in views.values().rev().filter(|view| {
            !matches!(
                view.state,
                brain_protocol::EnvironmentState::Declared
                    | brain_protocol::EnvironmentState::Deleted
            )
        }) {
            let environment = brain::environment::descriptor(&config, view).map_err(api_error)?;
            self.resources
                .environments
                .close(&environment, &*store, true)
                .await
                .map_err(api_error)?;
        }
        store.delete().map_err(api_error)?;
        self.sessions
            .lock()
            .map_err(|_| internal("session table is poisoned"))?
            .remove(&session_id);
        Ok(())
    }
}

pub(crate) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

fn api_error(error: brain::Error) -> ApiError {
    ApiError {
        details: error.details(),
        ..ApiError::new(error.code(), error.to_string(), error.retryable())
    }
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError::not_found(message)
}

fn internal(message: impl Into<String>) -> ApiError {
    ApiError::internal(message)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
