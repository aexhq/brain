//! What the host gives every session, built the way the server builds it: one writer
//! and one feed per process, one store per session directory, and the executors a
//! session performs its effects with.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use brain::{
    Error, Feed, JournalStore, LoopExecutor, ModelExecutor, Session, SessionRuntime, SessionStore,
    ToolExecutor, Writer,
};
use brain_protocol::{HistoryEvent, SessionConfig, SessionId};

pub struct Runtime {
    pub data_dir: PathBuf,
    pub writer: Arc<Writer>,
    pub feed: Arc<Feed>,
    pub config: Arc<SessionRuntime>,
    stores: Mutex<HashMap<SessionId, Arc<SessionStore>>>,
}

impl Runtime {
    /// Opens every session already under `data_dir`, closing any turn a previous
    /// process left running, exactly as the server does at boot.
    pub fn open(
        data_dir: &Path,
        telemetry: brain_telemetry::TelemetryPublisher,
        max_decisions_per_turn: usize,
        tool_deadline_ms: u64,
        loop_executor: Arc<dyn LoopExecutor>,
        model_executor: Arc<dyn ModelExecutor>,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        let writer = Writer::spawn();
        let feed = Arc::new(Feed::new(telemetry));
        let stores =
            SessionStore::open_all(&data_dir.join("sessions"), writer.clone(), feed.clone())
                .unwrap()
                .into_iter()
                .map(|store| (store.session_id().clone(), store))
                .collect();
        let config = Arc::new(SessionRuntime {
            max_decisions_per_turn,
            tool_deadline_ms,
            loop_executor,
            model_executor,
            tool_executor,
            live: feed.live_sender(),
        });
        Self {
            data_dir: data_dir.to_path_buf(),
            writer,
            feed,
            config,
            stores: Mutex::new(stores),
        }
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }

    /// Creates a session the way the server does: a directory, a genesis record, the
    /// history the caller handed back, then admission. There is no shortcut past
    /// `Session::begin`, so tests exercise the validation the production path enforces.
    pub fn create(
        &self,
        config: &SessionConfig,
        history: &[HistoryEvent],
    ) -> Result<Session, Error> {
        let session_id = SessionId::new(brain::random_id("ses"));
        let store = SessionStore::create(
            &self.sessions_dir().join(session_id.as_str()),
            session_id.clone(),
            &serde_json::to_value(config).unwrap(),
            self.writer.clone(),
            self.feed.clone(),
        )?;
        self.stores
            .lock()
            .unwrap()
            .insert(session_id, store.clone());
        Session::begin(store, self.config.clone(), config, history)?.complete(config.clone())
    }

    pub fn store(&self, session_id: &SessionId) -> Arc<SessionStore> {
        self.stores
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .expect("the session was created or opened through this runtime")
    }

    /// Resumes a session from its store, as the server does on the first request after
    /// a restart or a suspension.
    pub fn open_session(&self, session_id: &SessionId) -> Result<Session, Error> {
        Session::open(self.store(session_id), self.config.clone())
    }

    pub fn events(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> brain_protocol::EventPage {
        brain::event_page(
            self.store(session_id).records_after(after, limit).unwrap(),
            after,
        )
    }

    pub fn session(&self, session_id: &SessionId) -> brain_protocol::SessionSummary {
        self.store(session_id).session_summary().unwrap()
    }

    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.feed.subscribe()
    }

    /// Waits until everything appended so far is on disk.
    pub fn drain(&self) {
        self.writer.sync().unwrap();
    }
}

/// Bytes of every file under `path`.
pub fn dir_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                dir_bytes(&path)
            } else {
                entry.metadata().map(|meta| meta.len()).unwrap_or(0)
            }
        })
        .sum()
}
