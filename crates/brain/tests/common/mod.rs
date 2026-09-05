//! What the host gives every session, built the way the server builds it: one writer
//! and one feed per process, one store per session directory, and the executors a
//! session performs its effects with.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use brain::{
    Error, Feed, LocalSessionStore, LoopExecutor, ModelExecutor, Session, SessionRuntime,
    SessionStore, ToolExecutor, TurnServices, Writer,
};
use brain_protocol::{
    AgentloopIdentity, EnvironmentId, Message, ModelBinding, SessionConfig, SessionId, TurnInput,
    TurnOutput,
};
use futures_util::future::BoxFuture;

pub struct Runtime {
    pub data_dir: PathBuf,
    pub writer: Arc<Writer>,
    pub feed: Arc<Feed>,
    pub config: Arc<SessionRuntime>,
    stores: Mutex<HashMap<SessionId, Arc<LocalSessionStore>>>,
}

impl Runtime {
    /// Opens every session already under `data_dir`, closing any turn a previous
    /// process left running, exactly as the server does at boot.
    pub fn open(
        data_dir: &Path,
        telemetry: brain_telemetry::TelemetryPublisher,
        max_model_calls_per_turn: usize,
        tool_deadline_ms: u64,
        loop_executor: Arc<dyn LoopExecutor>,
        model_executor: Arc<dyn ModelExecutor>,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        let writer = Writer::spawn();
        let feed = Arc::new(Feed::new(telemetry.clone()));
        let stores =
            LocalSessionStore::open_all(&data_dir.join("sessions"), writer.clone(), feed.clone())
                .unwrap()
                .into_iter()
                .map(|store| (store.session_id().clone(), store))
                .collect();
        let config = Arc::new(SessionRuntime {
            max_model_calls_per_turn,
            max_turn_ms: 0,
            tool_deadline_ms,
            loop_executor,
            model_executor,
            tool_executor,
            live: feed.live_sender(),
            telemetry,
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
    /// transcript the caller carries forward, then admission. There is no shortcut past
    /// `Session::begin`, so tests exercise the validation the production path enforces.
    pub fn create(&self, config: &SessionConfig, transcript: &[Message]) -> Result<Session, Error> {
        let session_id = SessionId::new(brain::random_id("ses"));
        let store = LocalSessionStore::create(
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
        Session::begin(store, self.config.clone(), config, transcript)?.complete(config.clone())
    }

    pub fn store(&self, session_id: &SessionId) -> Arc<LocalSessionStore> {
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

    pub fn kinds(&self, session_id: &SessionId) -> Vec<String> {
        self.events(session_id, 0, 10_000)
            .events
            .into_iter()
            .map(|event| event.event_type)
            .collect()
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

pub fn temporary_directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "brain-{name}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

/// The smallest admitted configuration: an Agentloop and its Environment, no tools.
pub fn config() -> SessionConfig {
    SessionConfig {
        agentloop_identity: AgentloopIdentity::new("a".repeat(64)),
        agentloop_environment_id: EnvironmentId::new("workspace"),
        brain_configuration: serde_json::json!({}),
        model: ModelBinding {
            binding_id: "gateway".into(),
            model: "openai/test".into(),
        },
        system: "test".into(),
        response_format: None,
        tools: Vec::new(),
        environments: vec![brain_protocol::EnvironmentAttachment {
            environment_id: EnvironmentId::new("workspace"),
            configuration: serde_json::json!({"driver": "brain_wasm"}),
            managed: true,
            idle_ttl_ms: None,
            binding: Some(brain_protocol::EnvironmentBinding {
                environment_id: EnvironmentId::new("workspace"),
                directory_generation: 1,
            }),
            attachment_id: Some(brain_protocol::AttachmentId::new("attachment")),
            resources: Default::default(),
        }],
        tool_bindings: Vec::new(),
        idle_ttl_ms: None,
    }
}

type Script = Box<
    dyn Fn(TurnInput, Arc<dyn TurnServices>) -> BoxFuture<'static, Result<TurnOutput, Error>>
        + Send
        + Sync,
>;

/// A loop written as one closure over the turn's input and Brain's services.
pub struct ScriptedLoop {
    script: Script,
}

pub fn scripted<F, Fut>(script: F) -> Arc<ScriptedLoop>
where
    F: Fn(TurnInput, Arc<dyn TurnServices>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<TurnOutput, Error>> + Send + 'static,
{
    Arc::new(ScriptedLoop {
        script: Box::new(move |input, services| Box::pin(script(input, services))),
    })
}

#[async_trait]
impl LoopExecutor for ScriptedLoop {
    async fn turn(
        &self,
        _session: &SessionId,
        _agentloop: &AgentloopIdentity,
        _environment: serde_json::Value,
        input: TurnInput,
        services: Arc<dyn TurnServices>,
    ) -> Result<TurnOutput, Error> {
        (self.script)(input, services).await
    }
}

/// A loop that finishes at once with the transcript it was given plus the user message.
pub fn echo_loop() -> Arc<ScriptedLoop> {
    scripted(|input, _services| async move {
        let mut transcript = input.transcript;
        transcript.push(Message::user_text(&input.input.message));
        Ok(TurnOutput {
            transcript,
            slots: Default::default(),
            result: Some(serde_json::json!({"ok": true})),
        })
    })
}

/// A model that answers every call with one short assistant message and streams one
/// delta first.
pub struct ScriptedModel;

#[async_trait]
impl ModelExecutor for ScriptedModel {
    async fn execute(
        &self,
        _binding: &ModelBinding,
        request: brain_protocol::ModelRequest,
        _tools: &[brain_protocol::ToolDefinition],
        on_event: &mut (dyn FnMut(brain_protocol::ModelStreamEvent) + Send),
    ) -> Result<brain_protocol::ModelResult, Error> {
        on_event(brain_protocol::ModelStreamEvent::TextDelta {
            index: 0,
            text: "ok".into(),
        });
        Ok(brain_protocol::ModelResult {
            message: Message::assistant(vec![brain_protocol::ContentBlock::text(format!(
                "ok ({} messages)",
                request.messages.len()
            ))]),
            stop_reason: brain_protocol::StopReason::EndTurn,
            usage: brain_protocol::Usage::default(),
        })
    }
}

/// A model that never answers within a test's patience.
pub struct SlowModel;

#[async_trait]
impl ModelExecutor for SlowModel {
    async fn execute(
        &self,
        _binding: &ModelBinding,
        _request: brain_protocol::ModelRequest,
        _tools: &[brain_protocol::ToolDefinition],
        _on_event: &mut (dyn FnMut(brain_protocol::ModelStreamEvent) + Send),
    ) -> Result<brain_protocol::ModelResult, Error> {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        Err(Error::Executor("the slow model was not cancelled".into()))
    }
}

/// A model no test expects to be called.
pub struct NoModels;

#[async_trait]
impl ModelExecutor for NoModels {
    async fn execute(
        &self,
        _binding: &ModelBinding,
        _request: brain_protocol::ModelRequest,
        _tools: &[brain_protocol::ToolDefinition],
        _on_event: &mut (dyn FnMut(brain_protocol::ModelStreamEvent) + Send),
    ) -> Result<brain_protocol::ModelResult, Error> {
        Err(Error::Executor("no model in this test".into()))
    }
}

/// A tool executor no test expects to be called.
pub struct NoTools;

#[async_trait]
impl ToolExecutor for NoTools {
    async fn execute(
        &self,
        _: brain_protocol::ToolDispatch,
        _: &dyn brain::ToolServices,
    ) -> Result<brain_protocol::Outcome, Error> {
        Err(Error::Executor("no tools in this test".into()))
    }
    async fn cancel(&self, _: brain_protocol::ToolCancellation) -> Result<(), Error> {
        Ok(())
    }
}
