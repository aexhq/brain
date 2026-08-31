mod actor;
mod config;

use std::{
    collections::HashMap,
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use brain_protocol::{
    AgentloopIdentity, Capability, EnvironmentAttachment, EnvironmentId, EventPage, HistoryEvent,
    Identity, JournalId, MessageRequest, ModelBinding, ModelPresentation, OperationId,
    RequestedToolBinding, ResolvedEnvironment, ResolvedSessionRequest, SealedSessionConfig,
    Session, SessionId, SessionStatus, ToolBinding, ToolHosting, ToolPayload, operation_id,
};
use brain_telemetry::TelemetryPublisher;
use rand::RngCore;
use tokio::sync::{mpsc, oneshot};

use crate::{
    KernelError, context,
    journal::{
        AppendRecord, JournalStore, ObservedJournal, SegmentJournal, SessionRow, SessionUpdate,
        event_page,
    },
};
use actor::{SessionActor, SessionCommand};

pub use config::{DEFAULT_TOOL_DEADLINE_MS, KernelConfig};

#[derive(Clone)]
pub struct Kernel {
    inner: Arc<KernelInner>,
}

struct KernelInner {
    /// Concrete rather than `dyn JournalStore`, because the live subscription is not part
    /// of being a store: a journal an embedder brings has no reason to know about HTTP
    /// subscribers, and the wrapper that observes every append is the only thing that
    /// can serve them.
    store: Arc<ObservedJournal>,
    config: KernelConfig,
    sessions: Mutex<HashMap<SessionId, SessionRuntime>>,
}

#[derive(Clone)]
struct SessionRuntime {
    sender: mpsc::Sender<SessionCommand>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct SessionHandle {
    session_id: SessionId,
    sender: mpsc::Sender<SessionCommand>,
    cancelled: Arc<AtomicBool>,
}

/// Record kinds a restart reads a session's state out of, and which a caller therefore
/// cannot supply as history.
const RESERVED_KINDS: [&str; 7] = [
    "session_creation_started",
    "session_created",
    "session_creation_failed",
    "session_ended",
    "turn_started",
    "turn_finished",
    "turn_failed",
];

pub struct CreatingSession {
    kernel: Kernel,
    row: SessionRow,
    /// The events this session opened with, carried in memory to the actor so the
    /// agentloop can be told about them. They are already in the journal; this is not a
    /// second copy of the record, it is the one delivery of it.
    history: Vec<serde_json::Value>,
}

impl Kernel {
    pub fn open(config: KernelConfig, telemetry: TelemetryPublisher) -> Result<Self, KernelError> {
        fs::create_dir_all(&config.data_dir)
            .map_err(|error| KernelError::Journal(error.to_string()))?;
        let store = Arc::new(SegmentJournal::open(
            &config.data_dir.join("journal"),
            crate::journal::DEFAULT_IDEMPOTENCY_RETENTION,
        )?);
        Self::with_store(config, store, telemetry)
    }

    pub fn with_store(
        config: KernelConfig,
        store: Arc<dyn JournalStore>,
        telemetry: TelemetryPublisher,
    ) -> Result<Self, KernelError> {
        let kernel = Self {
            inner: Arc::new(KernelInner {
                store: Arc::new(ObservedJournal::new(store, telemetry)),
                config,
                sessions: Mutex::new(HashMap::new()),
            }),
        };
        kernel.interrupt_unfinished_turns()?;
        Ok(kernel)
    }

    /// Closes turns that the previous process did not finish.
    ///
    /// A session still `Running` after the journal has been read was mid-turn when that
    /// process stopped. Whether the model call or the tool call actually happened is not
    /// knowable from here, so Brain says exactly that and returns the session to Idle
    /// rather than deciding on the client's behalf. Agentloops, tools and SDK clients see
    /// `turn_interrupted` on the event stream and resume or abandon as suits them.
    fn interrupt_unfinished_turns(&self) -> Result<(), KernelError> {
        for session in self.inner.store.session_summaries()? {
            if !matches!(session.status, SessionStatus::Running) {
                continue;
            }
            self.inner.store.append(
                &session.session_id,
                session.last_sequence,
                &[AppendRecord::new(
                    "turn_interrupted",
                    serde_json::json!({
                        "message": "Brain restarted while this turn was in flight; whether its effects reached the model or a tool is not recorded"
                    }),
                )],
                SessionUpdate {
                    status: Some(SessionStatus::Idle),
                    context: None,
                    configuration: None,
                },
            )?;
        }
        Ok(())
    }

    /// Hands restored sessions the journal ids the server kept for them.
    ///
    /// Called once at startup, before anything is served. A session the server has no
    /// record of stays readable and refuses turns rather than taking them against a
    /// journal it cannot name.
    pub fn adopt_journal_ids(
        &self,
        journals: &std::collections::HashMap<String, String>,
    ) -> Result<(), KernelError> {
        self.inner.store.adopt_journal_ids(journals)
    }

    pub fn begin_session(
        &self,
        request: &ResolvedSessionRequest,
    ) -> Result<CreatingSession, KernelError> {
        validate_session_contract(request)?;
        let session_id = SessionId::new(random_id("ses"));
        let journal_id = JournalId::new(random_id("jrn"));
        let presentation =
            context::presentation(&request.presentation, &request.brain_configuration)?;
        let context = context::empty_context();
        let row = SessionRow {
            session_id,
            journal_id,
            status: SessionStatus::Creating,
            through_sequence: 1,
            configuration: serde_json::to_value(request).map_err(json_error)?,
            context: serde_json::to_value(context).map_err(json_error)?,
            presentation_identity: presentation.identity,
        };
        // The creation record is the session's own genesis and comes first; the events the
        // caller handed back are what happened before it, and follow.
        self.inner.store.create_session(
            &row,
            AppendRecord::new(
                "session_creation_started",
                serde_json::to_value(request).map_err(json_error)?,
            ),
        )?;
        let mut row = row;
        let history = self.replay_history(&mut row, &request.history)?;
        Ok(CreatingSession {
            kernel: self.clone(),
            row,
            history,
        })
    }

    pub fn session(&self, session_id: &SessionId) -> Result<Session, KernelError> {
        self.inner
            .store
            .session_summary(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))
    }

    pub fn sessions(&self) -> Result<Vec<Session>, KernelError> {
        self.inner.store.session_summaries()
    }

    pub fn handle(&self, session_id: &SessionId) -> Result<SessionHandle, KernelError> {
        if let Some(runtime) = self
            .inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?
            .get(session_id)
            .cloned()
        {
            return Ok(SessionHandle {
                session_id: session_id.clone(),
                sender: runtime.sender,
                cancelled: runtime.cancelled,
            });
        }
        let row = self
            .inner
            .store
            .session_row(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        // A session this process created was told about its history when it was created,
        // and telling it again would replay a conversation it is already holding. One
        // rebuilt from the journal has an agentloop that has never seen any of it.
        if row.journal_id.as_str().is_empty() {
            return Err(KernelError::InvalidState(
                "this session was restored from the journal but the server no longer has the \
                 metadata it needs to continue it; its history can be read, and a new session \
                 can be created with that history"
                    .into(),
            ));
        }
        let history = if self.inner.store.take_restored(session_id)? {
            self.restored_history(session_id)?
        } else {
            Vec::new()
        };
        self.spawn(row, history)
    }

    /// A restored session's own records, to hand back to its agentloop.
    ///
    /// Bounded at the same count the create API accepts as history. A conversation longer
    /// than that restores and can be read, but is not replayed into a loop: handing over
    /// part of a conversation as though it were the whole of it would be a worse answer
    /// than declining to.
    fn restored_history(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<serde_json::Value>, KernelError> {
        const MOST: usize = 10_000;
        let mut history = Vec::new();
        let mut after = 0;
        loop {
            let page = self.inner.store.records_after(session_id, after, 1_000)?;
            let Some(last) = page.last() else { break };
            after = last.sequence;
            history.extend(page.into_iter().map(|record| record.payload));
            if history.len() > MOST {
                return Ok(Vec::new());
            }
        }
        Ok(history)
    }

    /// Every record appended from now on, across every session.
    ///
    /// Subscribe before reading a page, then drop what the page already carried: a record
    /// appended between the two arrives here rather than being lost in the gap. A
    /// subscriber that falls further behind than the buffer loses records and is told so
    /// by the channel — the journal is the record and `after` reads it back, while this is
    /// a notification that it moved. A slow reader must never be able to hold up a turn.
    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.inner.store.subscribe()
    }

    pub fn events(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<EventPage, KernelError> {
        if limit == 0 || limit > 1_000 {
            return Err(KernelError::InvalidState(
                "event limit must be 1..=1000".into(),
            ));
        }
        Ok(event_page(
            self.inner.store.records_after(session_id, after, limit)?,
            after,
        ))
    }

    pub fn delete_ended(&self, session_id: &SessionId) -> Result<(), KernelError> {
        self.inner.store.delete_ended(session_id)?;
        self.inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?
            .remove(session_id);
        Ok(())
    }

    pub fn sealed_config(
        &self,
        session_id: &SessionId,
    ) -> Result<SealedSessionConfig, KernelError> {
        let row = self
            .inner
            .store
            .session_row(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        serde_json::from_value(row.configuration)
            .map_err(|error| KernelError::Journal(error.to_string()))
    }

    pub fn record_external_intent<T: serde::Serialize>(
        &self,
        session_id: &SessionId,
        kind: &str,
        request: &T,
    ) -> Result<(OperationId, Identity), KernelError> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?
            .remove(session_id);
        let row = self
            .inner
            .store
            .session_summary(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        if !matches!(row.status, SessionStatus::Idle) {
            return Err(KernelError::InvalidState("session is not idle".into()));
        }
        let operation_id = operation_id(&row.journal_id, row.last_sequence + 1);
        let identity =
            Identity::of(request).map_err(|error| KernelError::InvalidState(error.to_string()))?;
        self.inner.store.append(
            session_id,
            row.last_sequence,
            &[AppendRecord::new(
                format!("{kind}_intent"),
                serde_json::json!({"operation_id":operation_id,"request_identity":identity,"request":request}),
            )],
            SessionUpdate::default(),
        )?;
        Ok((operation_id, identity))
    }

    pub fn record_external_result<T: serde::Serialize>(
        &self,
        session_id: &SessionId,
        kind: &str,
        operation_id: &OperationId,
        result: &T,
    ) -> Result<(), KernelError> {
        let row = self
            .inner
            .store
            .session_summary(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        self.inner.store.append(
            session_id,
            row.last_sequence,
            &[AppendRecord::new(
                format!("{kind}_result"),
                serde_json::json!({"operation_id":operation_id,"result":result}),
            )],
            SessionUpdate::default(),
        )?;
        Ok(())
    }

    pub fn end_after_lifecycle(&self, session_id: &SessionId) -> Result<Session, KernelError> {
        let mut row = self
            .inner
            .store
            .session_summary(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        if matches!(row.status, SessionStatus::Ended) {
            return Ok(row);
        }
        if !matches!(row.status, SessionStatus::Idle) {
            return Err(KernelError::InvalidState("session is not idle".into()));
        }
        self.inner.store.append(
            session_id,
            row.last_sequence,
            &[AppendRecord::new("session_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Ended),
                context: None,
                configuration: None,
            },
        )?;
        row.last_sequence += 1;
        row.status = SessionStatus::Ended;
        self.inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?
            .remove(session_id);
        Ok(row)
    }

    pub fn idempotency_get<T: serde::Serialize>(
        &self,
        scope: &str,
        key: &str,
        request: &T,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        let identity =
            Identity::of(request).map_err(|error| KernelError::InvalidState(error.to_string()))?;
        self.inner.store.idempotency_get(scope, key, &identity)
    }

    pub fn idempotency_put<T: serde::Serialize>(
        &self,
        scope: &str,
        key: &str,
        request: &T,
        response: &serde_json::Value,
    ) -> Result<(), KernelError> {
        let identity =
            Identity::of(request).map_err(|error| KernelError::InvalidState(error.to_string()))?;
        self.inner
            .store
            .idempotency_put(scope, key, &identity, response)
    }

    /// Writes the caller's events into the new session's journal, and hands back what the
    /// agentloop should be told.
    ///
    /// The sequences the caller supplies are checked but not kept: they name positions in
    /// the session those events came from, and this is a different session, whose records
    /// are numbered densely from its own beginning. What they are good for is catching a
    /// caller that has lost or reordered part of the conversation, which is worth failing
    /// on rather than silently writing down.
    fn replay_history(
        &self,
        row: &mut SessionRow,
        history: &[HistoryEvent],
    ) -> Result<Vec<serde_json::Value>, KernelError> {
        if history.is_empty() {
            return Ok(Vec::new());
        }
        let mut previous = 0;
        for event in history {
            // A caller's history cannot wear a lifecycle kind. These are the records the
            // journal is read back by -- a restart derives a session's status from them --
            // so accepting `session_ended` or `turn_started` from a client would let it
            // describe a session that never happened, and a later restart would believe it.
            if RESERVED_KINDS.contains(&event.event_type.as_str()) {
                return Err(KernelError::InvalidState(format!(
                    "session history may not contain `{}`: lifecycle records are Brain's own, \
                     and a restart reads a session's state back out of them",
                    event.event_type
                )));
            }
            if event.sequence <= previous {
                return Err(KernelError::InvalidState(format!(
                    "session history is out of order at sequence {}: events must ascend, and \
                     one that does not is a conversation with a piece missing or moved",
                    event.sequence
                )));
            }
            previous = event.sequence;
        }

        let records: Vec<AppendRecord> = history
            .iter()
            .map(|event| AppendRecord::new(&event.event_type, event.data.clone()))
            .collect();
        let saved = self.inner.store.append(
            &row.session_id,
            row.through_sequence,
            &records,
            SessionUpdate::default(),
        )?;
        row.through_sequence += saved.len() as u64;
        Ok(history.iter().map(|event| event.data.clone()).collect())
    }

    fn spawn(
        &self,
        row: SessionRow,
        history: Vec<serde_json::Value>,
    ) -> Result<SessionHandle, KernelError> {
        let mut sessions = self
            .inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?;
        if let Some(runtime) = sessions.get(&row.session_id).cloned() {
            return Ok(SessionHandle {
                session_id: row.session_id,
                sender: runtime.sender,
                cancelled: runtime.cancelled,
            });
        }
        let (sender, receiver) = mpsc::channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let actor = SessionActor::new(
            row.clone(),
            self.inner.store.clone(),
            self.inner.config.loop_executor.clone(),
            self.inner.config.model_executor.clone(),
            self.inner.config.tool_executor.clone(),
            self.inner.config.max_decisions_per_turn,
            self.inner.config.tool_deadline_ms,
            receiver,
            cancelled.clone(),
            self.inner.store.live_sender(),
            history,
        )?;
        tokio::spawn(actor.run());
        sessions.insert(
            row.session_id.clone(),
            SessionRuntime {
                sender: sender.clone(),
                cancelled: cancelled.clone(),
            },
        );
        Ok(SessionHandle {
            session_id: row.session_id,
            sender,
            cancelled,
        })
    }
}

impl CreatingSession {
    pub fn session_id(&self) -> &SessionId {
        &self.row.session_id
    }
    pub fn journal_id(&self) -> &JournalId {
        &self.row.journal_id
    }

    pub fn record_intent<T: serde::Serialize>(
        &mut self,
        kind: &str,
        request: &T,
    ) -> Result<(OperationId, Identity), KernelError> {
        self.record_intent_redacted(kind, request, request)
    }

    /// Journals the intent as `journal_view` while the operation identity still covers
    /// the wire request. This is how a request carrying values the journal must not
    /// hold — attach binding values — is recorded: the view replaces each value with
    /// its identity.
    pub fn record_intent_redacted<T: serde::Serialize, V: serde::Serialize>(
        &mut self,
        kind: &str,
        request: &T,
        journal_view: &V,
    ) -> Result<(OperationId, Identity), KernelError> {
        let operation_id = operation_id(&self.row.journal_id, self.row.through_sequence + 1);
        let identity =
            Identity::of(request).map_err(|error| KernelError::InvalidState(error.to_string()))?;
        let saved = self.kernel.inner.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(format!("{kind}_intent"), serde_json::json!({"operation_id":operation_id,"request_identity":identity,"request":journal_view}))],
            SessionUpdate::default(),
        )?;
        self.row.through_sequence += saved.len() as u64;
        Ok((operation_id, identity))
    }

    pub fn record_result<T: serde::Serialize>(
        &mut self,
        kind: &str,
        operation_id: &OperationId,
        result: &T,
    ) -> Result<(), KernelError> {
        let saved = self.kernel.inner.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(
                format!("{kind}_result"),
                serde_json::json!({"operation_id":operation_id,"result":result}),
            )],
            SessionUpdate::default(),
        )?;
        self.row.through_sequence += saved.len() as u64;
        Ok(())
    }

    pub fn complete(mut self, sealed: SealedSessionConfig) -> Result<SessionHandle, KernelError> {
        validate_session_contract(&sealed)?;
        let configuration = serde_json::to_value(&sealed).map_err(json_error)?;
        let context = self.row.context.clone();
        let saved = self.kernel.inner.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(
                "session_created",
                serde_json::json!({"configuration":sealed}),
            )],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                context: Some(&context),
                configuration: Some(&configuration),
            },
        )?;
        self.row.through_sequence += saved.len() as u64;
        self.row.status = SessionStatus::Idle;
        self.row.configuration = configuration;
        self.kernel.spawn(self.row, self.history)
    }

    pub fn fail(mut self, code: &str, message: &str) -> Result<(), KernelError> {
        let saved = self.kernel.inner.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(
                "session_creation_failed",
                serde_json::json!({"code":code,"message":message}),
            )],
            SessionUpdate {
                status: Some(SessionStatus::Failed),
                context: None,
                configuration: None,
            },
        )?;
        self.row.through_sequence += saved.len() as u64;
        Ok(())
    }
}

impl SessionHandle {
    pub fn id(&self) -> &SessionId {
        &self.session_id
    }

    pub async fn message(&self, request: MessageRequest) -> Result<Session, KernelError> {
        if request.input.message.is_empty() {
            return Err(KernelError::InvalidState("message cannot be empty".into()));
        }
        if serde_json::to_vec(&request).map_err(json_error)?.len() > 2 * 1024 * 1024 {
            return Err(KernelError::InvalidState(
                "message request exceeds 2 MiB".into(),
            ));
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::Message { request, reply })
            .await
            .map_err(|_| KernelError::InvalidState("session actor stopped".into()))?;
        response
            .await
            .map_err(|_| KernelError::InvalidState("session actor stopped".into()))?
    }

    pub async fn cancel(&self) -> Result<(), KernelError> {
        self.cancelled.store(true, Ordering::Release);
        match self.sender.try_send(SessionCommand::Cancel) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(KernelError::InvalidState("session actor stopped".into()))
            }
        }
    }

    pub async fn end(&self) -> Result<Session, KernelError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::End { reply })
            .await
            .map_err(|_| KernelError::InvalidState("session actor stopped".into()))?;
        response
            .await
            .map_err(|_| KernelError::InvalidState("session actor stopped".into()))?
    }
}

/// The two request shapes the kernel admits differ only in where an Environment identity
/// lives: a resolved request names it directly, a sealed configuration carries it inside
/// the binding it resolved to. Every other term of the contract is identical, and core
/// principle 3 requires that authority be bounded identically however a session is
/// created, so the bounds are stated once and both shapes are read through these views.
trait EnvironmentView {
    fn environment_id(&self) -> &EnvironmentId;
    /// What this environment provides, when that is already known. A resolved request
    /// predates the setup/attach receipts that report it, so only a sealed attachment
    /// answers — and only a sealed configuration is capability-checked.
    fn provides(&self) -> Option<&[Capability]>;
}

impl EnvironmentView for ResolvedEnvironment {
    fn environment_id(&self) -> &EnvironmentId {
        &self.environment_id
    }

    fn provides(&self) -> Option<&[Capability]> {
        None
    }
}

impl EnvironmentView for EnvironmentAttachment {
    fn environment_id(&self) -> &EnvironmentId {
        &self.binding.environment_id
    }

    fn provides(&self) -> Option<&[Capability]> {
        Some(&self.provides)
    }
}

trait ToolBindingView {
    fn name(&self) -> &str;
    fn environment_id(&self) -> &EnvironmentId;
    fn requires(&self) -> &[Capability];
    fn binding_names(&self) -> &[String];
    fn hosting(&self) -> ToolHosting;
    fn payload(&self) -> Option<&ToolPayload>;
}

impl ToolBindingView for RequestedToolBinding {
    fn name(&self) -> &str {
        &self.name
    }

    fn environment_id(&self) -> &EnvironmentId {
        &self.environment_id
    }

    fn requires(&self) -> &[Capability] {
        &self.requires
    }

    fn binding_names(&self) -> &[String] {
        &self.binding_names
    }

    fn hosting(&self) -> ToolHosting {
        self.hosting
    }

    fn payload(&self) -> Option<&ToolPayload> {
        self.payload.as_ref()
    }
}

impl ToolBindingView for ToolBinding {
    fn name(&self) -> &str {
        &self.name
    }

    fn environment_id(&self) -> &EnvironmentId {
        &self.environment.environment_id
    }

    fn requires(&self) -> &[Capability] {
        &self.requires
    }

    fn binding_names(&self) -> &[String] {
        &self.binding_names
    }

    fn hosting(&self) -> ToolHosting {
        self.hosting
    }

    fn payload(&self) -> Option<&ToolPayload> {
        self.payload.as_ref()
    }
}

trait SessionContract: serde::Serialize {
    type Environment: EnvironmentView;
    type ToolBinding: ToolBindingView;

    fn agentloop_identity(&self) -> &AgentloopIdentity;
    fn model(&self) -> &ModelBinding;
    fn presentation(&self) -> &ModelPresentation;
    fn environments(&self) -> &[Self::Environment];
    fn tool_bindings(&self) -> &[Self::ToolBinding];
}

impl SessionContract for ResolvedSessionRequest {
    type Environment = ResolvedEnvironment;
    type ToolBinding = RequestedToolBinding;

    fn agentloop_identity(&self) -> &AgentloopIdentity {
        &self.agentloop_identity
    }

    fn model(&self) -> &ModelBinding {
        &self.model
    }

    fn presentation(&self) -> &ModelPresentation {
        &self.presentation
    }

    fn environments(&self) -> &[Self::Environment] {
        &self.environments
    }

    fn tool_bindings(&self) -> &[Self::ToolBinding] {
        &self.tool_bindings
    }
}

impl SessionContract for SealedSessionConfig {
    type Environment = EnvironmentAttachment;
    type ToolBinding = ToolBinding;

    fn agentloop_identity(&self) -> &AgentloopIdentity {
        &self.agentloop_identity
    }

    fn model(&self) -> &ModelBinding {
        &self.model
    }

    fn presentation(&self) -> &ModelPresentation {
        &self.presentation
    }

    fn environments(&self) -> &[Self::Environment] {
        &self.environments
    }

    fn tool_bindings(&self) -> &[Self::ToolBinding] {
        &self.tool_bindings
    }
}

fn validate_session_contract(request: &impl SessionContract) -> Result<(), KernelError> {
    if serde_json::to_vec(request).map_err(json_error)?.len() > 2 * 1024 * 1024 {
        return Err(KernelError::InvalidState(
            "session request exceeds 2 MiB".into(),
        ));
    }
    if !identity_valid(request.agentloop_identity().as_str())
        || !identifier_valid(&request.model().binding_id)
        || request.model().model.is_empty()
        || request.model().model.len() > 256
        || request.presentation().system.len() > 131_072
        || request.presentation().tools.len() > 128
        || request.environments().len() > 128
        || request.tool_bindings().len() > 128
    {
        return Err(KernelError::InvalidState(
            "session request violates a contract size or identity bound".into(),
        ));
    }
    for tool in &request.presentation().tools {
        if !identifier_valid(&tool.name)
            || tool.description.len() > 8_192
            || !tool.input_schema.is_object()
            || tool
                .output_schema
                .as_ref()
                .is_some_and(|value| !value.is_object())
        {
            return Err(KernelError::InvalidState(
                "Tool definition violates the session contract".into(),
            ));
        }
    }
    if request
        .environments()
        .iter()
        .any(|environment| !identifier_valid(environment.environment_id().as_str()))
        || request.tool_bindings().iter().any(|binding| {
            !identifier_valid(binding.name())
                || !identifier_valid(binding.environment_id().as_str())
                || binding
                    .binding_names()
                    .iter()
                    .any(|name| !identifier_valid(name))
        })
    {
        return Err(KernelError::InvalidState(
            "Environment or Tool binding has an invalid identity".into(),
        ));
    }
    // A callback tool's code stays in the author's process; a payload beside it would
    // be an artifact nothing is allowed to run.
    if request.tool_bindings().iter().any(|binding| {
        matches!(binding.hosting(), ToolHosting::Callback) && binding.payload().is_some()
    }) {
        return Err(KernelError::InvalidState(
            "a callback Tool binding cannot carry a payload".into(),
        ));
    }
    let mut definitions: Vec<&str> = request
        .presentation()
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    let mut bindings: Vec<&str> = request
        .tool_bindings()
        .iter()
        .map(ToolBindingView::name)
        .collect();
    definitions.sort_unstable();
    bindings.sort_unstable();
    if definitions.windows(2).any(|pair| pair[0] == pair[1])
        || bindings.windows(2).any(|pair| pair[0] == pair[1])
        || definitions != bindings
    {
        return Err(KernelError::InvalidState(
            "every unique Tool definition must have exactly one binding".into(),
        ));
    }
    let environment_ids: std::collections::HashSet<_> = request
        .environments()
        .iter()
        .map(EnvironmentView::environment_id)
        .collect();
    if environment_ids.len() != request.environments().len() {
        return Err(KernelError::InvalidState(
            "Environment identities must be unique".into(),
        ));
    }
    if request
        .tool_bindings()
        .iter()
        .any(|binding| !environment_ids.contains(binding.environment_id()))
    {
        return Err(KernelError::InvalidState(
            "every Tool binding must name a bound Environment".into(),
        ));
    }
    // The bind-time capability check: what a tool requires must be provided by the
    // environment it is bound to, so a mismatch is a create-time rejection naming all
    // three parties instead of a runtime mystery. A tool with empty `requires` binds
    // anywhere. Provides is known only once the environment has attached, so only the
    // sealed shape is checked — every admitted session passes through it.
    for binding in request.tool_bindings() {
        let provides = request
            .environments()
            .iter()
            .find(|environment| environment.environment_id() == binding.environment_id())
            .and_then(EnvironmentView::provides);
        let Some(provides) = provides else { continue };
        if let Some(missing) = binding
            .requires()
            .iter()
            .find(|capability| !provides.contains(capability))
        {
            return Err(KernelError::InvalidState(format!(
                "Tool `{}` requires capability `{missing}` that Environment `{}` does not provide",
                binding.name(),
                binding.environment_id()
            )));
        }
    }
    Ok(())
}

fn identifier_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn identity_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    format!("{prefix}_{}", hex::encode(bytes))
}

fn json_error(error: serde_json::Error) -> KernelError {
    KernelError::InvalidState(error.to_string())
}
#[cfg(test)]
mod tests {
    use brain_protocol::{AttachmentId, EnvironmentBinding, LifecyclePolicy, ToolDefinition};

    use super::*;

    fn digest() -> String {
        "a".repeat(64)
    }

    fn tool() -> ToolDefinition {
        ToolDefinition {
            name: "search".into(),
            description: "search the workspace".into(),
            input_schema: serde_json::json!({"type":"object"}),
            output_schema: None,
        }
    }

    fn environment_binding() -> EnvironmentBinding {
        EnvironmentBinding {
            environment_id: EnvironmentId::new("workspace"),
            configuration_identity: Identity::of(&"configuration").unwrap(),
            directory_generation: 1,
            lifecycle_policy: LifecyclePolicy::Shared,
        }
    }

    fn resolved() -> ResolvedSessionRequest {
        ResolvedSessionRequest {
            history: Vec::new(),
            agentloop_identity: AgentloopIdentity::new(digest()),
            brain_configuration: serde_json::json!({}),
            model: ModelBinding {
                binding_id: "gateway".into(),
                model: "openai/test".into(),
            },
            presentation: ModelPresentation {
                system: "test".into(),
                tools: vec![tool()],
                response_format: None,
            },
            environments: vec![ResolvedEnvironment {
                environment_id: EnvironmentId::new("workspace"),
                configuration: serde_json::json!({}),
                lifecycle_policy: LifecyclePolicy::Shared,
                grants: Default::default(),
                binding_identities: Default::default(),
            }],
            tool_bindings: vec![RequestedToolBinding {
                name: "search".into(),
                environment_id: EnvironmentId::new("workspace"),
                requires: Vec::new(),
                binding_names: Vec::new(),
                hosting: ToolHosting::Provisioned,
                payload: None,
            }],
        }
    }

    fn sealed() -> SealedSessionConfig {
        SealedSessionConfig {
            agentloop_identity: AgentloopIdentity::new(digest()),
            brain_configuration: serde_json::json!({}),
            model: ModelBinding {
                binding_id: "gateway".into(),
                model: "openai/test".into(),
            },
            presentation: ModelPresentation {
                system: "test".into(),
                tools: vec![tool()],
                response_format: None,
            },
            environments: vec![EnvironmentAttachment {
                binding: environment_binding(),
                attachment_id: AttachmentId::new("attachment"),
                provides: vec![Capability::Exec, Capability::Fs],
            }],
            tool_bindings: vec![ToolBinding {
                name: "search".into(),
                environment: environment_binding(),
                attachment_id: AttachmentId::new("attachment"),
                requires: vec![Capability::Exec],
                binding_names: Vec::new(),
                hosting: ToolHosting::Provisioned,
                payload: None,
            }],
        }
    }

    /// A rejection case: the name of the breach, the smallest edit that commits it, and
    /// the bound that must reject it.
    type Breach<T> = (&'static str, fn(&mut T), &'static str);

    /// Each case names the bound it breaches, so a case that starts passing for some
    /// other reason fails rather than quietly stops testing what it was written for.
    fn assert_rejected<T: SessionContract>(subject: &str, case: &str, request: &T, bound: &str) {
        let error = validate_session_contract(request)
            .expect_err(&format!("{subject} with {case} must be rejected"));
        let message = error.to_string();
        assert!(
            message.contains(bound),
            "{subject} with {case} must be rejected by the {bound:?} bound, not by {message:?}"
        );
    }

    #[test]
    fn a_request_within_every_bound_is_admitted() {
        validate_session_contract(&resolved()).unwrap();
        validate_session_contract(&sealed()).unwrap();
    }

    /// Principle 3 fixes authority at create, so every bound below is the difference
    /// between a session that can only do what it was granted and one that cannot be
    /// reasoned about at all. Each case is the smallest edit that breaches one bound.
    #[test]
    fn every_bound_rejects_a_resolved_request_that_breaches_it() {
        let cases: Vec<Breach<ResolvedSessionRequest>> = vec![
            (
                "configuration over 2 MiB",
                |request| {
                    request.brain_configuration = serde_json::json!("x".repeat(3 * 1024 * 1024));
                },
                "exceeds 2 MiB",
            ),
            (
                "an Agentloop digest of the wrong length",
                |request| request.agentloop_identity = AgentloopIdentity::new("a".repeat(63)),
                "size or identity bound",
            ),
            (
                "an Agentloop digest that is not hex",
                |request| request.agentloop_identity = AgentloopIdentity::new("g".repeat(64)),
                "size or identity bound",
            ),
            (
                "an empty model binding",
                |request| request.model.binding_id = String::new(),
                "size or identity bound",
            ),
            (
                "a model binding holding a path traversal",
                |request| request.model.binding_id = "gateway/../root".into(),
                "size or identity bound",
            ),
            (
                "an empty model name",
                |request| request.model.model = String::new(),
                "size or identity bound",
            ),
            (
                "a model name over 256 bytes",
                |request| request.model.model = "m".repeat(257),
                "size or identity bound",
            ),
            (
                "a system prompt over 128 KiB",
                |request| request.presentation.system = "s".repeat(131_073),
                "size or identity bound",
            ),
            (
                "more than 128 Tool definitions",
                |request| {
                    let binding = request.tool_bindings[0].clone();
                    request.presentation.tools = (0..129)
                        .map(|index| ToolDefinition {
                            name: format!("tool{index}"),
                            ..tool()
                        })
                        .collect();
                    request.tool_bindings = (0..129)
                        .map(|index| RequestedToolBinding {
                            name: format!("tool{index}"),
                            ..binding.clone()
                        })
                        .collect();
                },
                "size or identity bound",
            ),
            (
                "more than 128 Environments",
                |request| {
                    let environment = request.environments[0].clone();
                    request.environments = (0..129)
                        .map(|index| ResolvedEnvironment {
                            environment_id: EnvironmentId::new(format!("env{index}")),
                            ..environment.clone()
                        })
                        .collect();
                    request.tool_bindings[0].environment_id = EnvironmentId::new("env0");
                },
                "size or identity bound",
            ),
            (
                "a Tool name that is not an identifier",
                |request| {
                    request.presentation.tools[0].name = "../escape".into();
                    request.tool_bindings[0].name = "../escape".into();
                },
                "Tool definition violates",
            ),
            (
                "a Tool description over 8 KiB",
                |request| request.presentation.tools[0].description = "d".repeat(8_193),
                "Tool definition violates",
            ),
            (
                "a Tool input schema that is not an object",
                |request| request.presentation.tools[0].input_schema = serde_json::json!("string"),
                "Tool definition violates",
            ),
            (
                "a Tool output schema that is not an object",
                |request| request.presentation.tools[0].output_schema = Some(serde_json::json!([])),
                "Tool definition violates",
            ),
            (
                "an Environment identity that is not an identifier",
                |request| {
                    request.environments[0].environment_id = EnvironmentId::new("../escape");
                    request.tool_bindings[0].environment_id = EnvironmentId::new("../escape");
                },
                "invalid identity",
            ),
            (
                "a binding name that is not an identifier",
                |request| request.tool_bindings[0].binding_names = vec!["../escape".into()],
                "invalid identity",
            ),
            (
                "a callback Tool carrying a payload",
                |request| {
                    request.tool_bindings[0].hosting = ToolHosting::Callback;
                    request.tool_bindings[0].payload = Some(ToolPayload {
                        kind: brain_protocol::PayloadKind::Esm,
                        identity: Identity::of(&"payload").unwrap(),
                    });
                },
                "cannot carry a payload",
            ),
            (
                "two Tool definitions sharing one name",
                |request| {
                    request.presentation.tools.push(tool());
                    let binding = request.tool_bindings[0].clone();
                    request.tool_bindings.push(binding);
                },
                "exactly one binding",
            ),
            (
                "a Tool definition with no binding",
                |request| request.tool_bindings.clear(),
                "exactly one binding",
            ),
            (
                "a binding with no Tool definition",
                |request| request.presentation.tools.clear(),
                "exactly one binding",
            ),
            (
                "two Environments sharing one identity",
                |request| {
                    let environment = request.environments[0].clone();
                    request.environments.push(environment);
                },
                "Environment identities must be unique",
            ),
            (
                "a binding naming an Environment that was not granted",
                |request| request.tool_bindings[0].environment_id = EnvironmentId::new("elsewhere"),
                "must name a bound Environment",
            ),
        ];
        for (case, breach, bound) in cases {
            let mut request = resolved();
            breach(&mut request);
            assert_rejected("a resolved request", case, &request, bound);
        }
    }

    /// A sealed configuration reaches the journal through `CreatingSession::complete`,
    /// the only other way a session is admitted. It is held to the same bounds, so the
    /// kernel does not become more permissive by being driven from inside a service.
    #[test]
    fn every_bound_rejects_a_sealed_configuration_that_breaches_it() {
        let cases: Vec<Breach<SealedSessionConfig>> = vec![
            (
                "configuration over 2 MiB",
                |sealed| {
                    sealed.brain_configuration = serde_json::json!("x".repeat(3 * 1024 * 1024));
                },
                "exceeds 2 MiB",
            ),
            (
                "an Agentloop digest that is not hex",
                |sealed| sealed.agentloop_identity = AgentloopIdentity::new("g".repeat(64)),
                "size or identity bound",
            ),
            (
                "an empty model binding",
                |sealed| sealed.model.binding_id = String::new(),
                "size or identity bound",
            ),
            (
                "a system prompt over 128 KiB",
                |sealed| sealed.presentation.system = "s".repeat(131_073),
                "size or identity bound",
            ),
            (
                "a Tool name that is not an identifier",
                |sealed| {
                    sealed.presentation.tools[0].name = "../escape".into();
                    sealed.tool_bindings[0].name = "../escape".into();
                },
                "Tool definition violates",
            ),
            (
                "an Environment identity that is not an identifier",
                |sealed| {
                    sealed.environments[0].binding.environment_id = EnvironmentId::new("../escape");
                    sealed.tool_bindings[0].environment.environment_id =
                        EnvironmentId::new("../escape");
                },
                "invalid identity",
            ),
            (
                "a binding name that is not an identifier",
                |sealed| sealed.tool_bindings[0].binding_names = vec!["../escape".into()],
                "invalid identity",
            ),
            (
                "a Tool requiring a capability its Environment does not provide",
                |sealed| sealed.tool_bindings[0].requires = vec![Capability::Page],
                "does not provide",
            ),
            (
                "a Tool definition with no binding",
                |sealed| sealed.tool_bindings.clear(),
                "exactly one binding",
            ),
            (
                "two Environments sharing one identity",
                |sealed| {
                    let environment = sealed.environments[0].clone();
                    sealed.environments.push(environment);
                },
                "Environment identities must be unique",
            ),
            (
                "a binding naming an Environment that was not sealed",
                |sealed| {
                    sealed.tool_bindings[0].environment.environment_id =
                        EnvironmentId::new("elsewhere");
                },
                "must name a bound Environment",
            ),
        ];
        for (case, breach, bound) in cases {
            let mut configuration = sealed();
            breach(&mut configuration);
            assert_rejected("a sealed configuration", case, &configuration, bound);
        }
    }

    #[test]
    fn an_identifier_admits_only_the_characters_the_contract_names() {
        assert!(identifier_valid("a"));
        assert!(identifier_valid("workspace.tool_1:read-only"));
        assert!(identifier_valid(&"a".repeat(128)));
        assert!(!identifier_valid(""));
        assert!(!identifier_valid(&"a".repeat(129)));
        assert!(!identifier_valid(".leading"));
        assert!(!identifier_valid("-leading"));
        assert!(!identifier_valid("has space"));
        assert!(!identifier_valid("has/slash"));
        assert!(!identifier_valid("../escape"));
        assert!(!identifier_valid("na\u{ef}ve"));
    }

    #[test]
    fn a_digest_is_exactly_sixty_four_lowercase_hex_characters() {
        assert!(identity_valid(&"a".repeat(64)));
        assert!(identity_valid(&"0123456789abcdef".repeat(4)));
        assert!(!identity_valid(&"a".repeat(63)));
        assert!(!identity_valid(&"a".repeat(65)));
        assert!(!identity_valid(&"A".repeat(64)));
        assert!(!identity_valid(&"g".repeat(64)));
        assert!(!identity_valid(""));
    }
}
