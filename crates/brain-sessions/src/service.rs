use crate::{EnvironmentRegistry, locks::KeyedLocks};
use brain::{Feed, LocalSessionStore, Session, SessionRuntime, SessionStore, Writer};
use brain_protocol::{
    ApiError, Environment, EnvironmentCallRequest, EnvironmentCallResult, EnvironmentName,
    EventPage, MessageRequest, SessionConfig, SessionId, SessionList, SessionStatus,
    SessionSummary, codes,
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
}
struct Entry {
    store: Arc<LocalSessionStore>,
    /// The running task, or `None` while the session is suspended.
    session: Option<Session>,
    last_touch: Instant,
    /// Zero means never suspend.
    idle_ttl: Option<Duration>,
}

impl Sessions {
    pub fn new(resources: SessionResources) -> Result<Self, brain::Error> {
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
        let _finished = self.active.write().await;
    }

    async fn admit_work(&self) -> Result<tokio::sync::RwLockReadGuard<'_, ()>, ApiError> {
        if self.draining.load(Ordering::Acquire) {
            return Err(ApiError::overloaded("Brain is draining active work"));
        }
        let guard = self.active.read().await;
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

    async fn cleanup_environments(&self, environments: &[Environment], store: &dyn SessionStore) {
        for environment in environments.iter().rev() {
            if let Err(error) = self
                .resources
                .environments
                .close(environment, store, false)
                .await
            {
                tracing::warn!(environment = %environment.name, %error, "failed to clean up Environment after session admission");
            }
        }
    }

    pub async fn create_session(
        &self,
        session_id: SessionId,
        config: SessionConfig,
        transcript: Vec<brain_protocol::Message>,
    ) -> Result<SessionSummary, ApiError> {
        let _active = self.admit_work().await?;
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
        // Each Environment is set up with the needs of everything placed in it, in
        // declaration order; the first refusal fails the create and tears down the rest.
        let mut ready = Vec::with_capacity(config.environments.len());
        for environment in &config.environments {
            let needs = needs_for(&config, &environment.name);
            if let Err(error) = self
                .resources
                .environments
                .setup(&mut creation, environment, needs)
                .await
            {
                creation
                    .fail(
                        codes::failure::ENVIRONMENT_PREPARATION_FAILED,
                        &error.to_string(),
                    )
                    .map_err(api_error)?;
                self.insert_entry(store.clone(), None)?;
                self.cleanup_environments(&ready, &*store).await;
                return Err(api_error(error));
            }
            ready.push(environment.clone());
        }
        let session = match creation.complete(config) {
            Ok(session) => session,
            Err(error) => {
                self.insert_entry(store.clone(), None)?;
                self.cleanup_environments(&ready, &*store).await;
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
            .is_some_and(|entry| entry.idle_ttl.is_none());
        if release {
            self.passivate(&session_id).await?;
        }
        Ok(session)
    }

    pub async fn get_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError> {
        self.summary(&session_id).await
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
        let _active = self.admit_work().await?;
        Session::validate_message(&request).map_err(api_error)?;
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
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
            .is_some_and(|entry| entry.idle_ttl.is_none());
        if release {
            self.passivate(&session_id).await?;
        }
        let session = result.map_err(api_error)?;
        Ok(session)
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
        }
        Ok(())
    }

    pub async fn end_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError> {
        let lock = self.session_lock(&session_id)?;
        let _guard = lock.lock().await;
        let store = self.store(&session_id).await?;
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
        let config = brain::session_config(&*store).map_err(api_error)?;
        for environment in config.environments.iter().rev() {
            self.resources
                .environments
                .close(environment, &*store, true)
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

fn needs_for(config: &SessionConfig, environment: &EnvironmentName) -> Vec<String> {
    let mut needs = Vec::new();
    let placed = config
        .tools
        .iter()
        .filter_map(|tool| tool.placements.get(environment))
        .flat_map(|placement| placement.needs.iter())
        .chain(
            (&config.agentloop.environment == environment)
                .then_some(config.agentloop.needs.iter())
                .into_iter()
                .flatten(),
        );
    for need in placed {
        if !needs.contains(need) {
            needs.push(need.clone());
        }
    }
    needs
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

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
#[path = "session_tests.rs"]
mod session_tests;

#[cfg(test)]
mod tests {
    use super::needs_for;
    /// An Environment sees the needs of exactly what is placed in it, once each, so
    /// it provisions what this session uses and nothing else.
    #[test]
    fn an_environment_is_set_up_with_the_needs_of_what_is_placed_in_it() {
        let config: brain_protocol::SessionConfig = serde_json::from_value(serde_json::json!({
            "agentloop": {"implementation": {"type": "brain_component", "entrypoint": "turn", "id": "a".repeat(64)}, "configuration": {}, "environment": "brain", "needs": ["https://api.example.com"]},
            "model": {"provider": "openai", "name": "gpt-5-mini"},
            "tools": [
                {"name": "read", "description": "d", "input_schema": {}, "placements": {"brain": {"needs": ["file:///workspace", "https://api.example.com"], "implementation": {}}}},
                {"name": "bash", "description": "d", "input_schema": {}, "placements": {"sandbox": {"needs": ["pkg:apt/bash"], "implementation": {}}}},
                {"name": "note", "description": "d", "input_schema": {}, "placements": {"sandbox": {"needs": [], "implementation": {}}}}
            ],
            "environments": [
                {"name": "brain", "driver": "brain"},
                {"name": "sandbox", "driver": "http", "url": "https://sandbox.example"},
                {"name": "app", "driver": "host", "host_id": "host_12345678901234567890"}
            ]
        }))
        .unwrap();
        let needs = |name: &str| needs_for(&config, &brain_protocol::EnvironmentName::new(name));
        assert_eq!(
            needs("brain"),
            vec!["file:///workspace", "https://api.example.com"]
        );
        assert_eq!(needs("sandbox"), vec!["pkg:apt/bash"]);
        assert!(needs("app").is_empty());
    }
}
