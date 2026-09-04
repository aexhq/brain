mod actor;
mod config;

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use brain_protocol::codes::{self, Failure};
use brain_protocol::{
    AgentloopIdentity, ContextEnvelope, EnvironmentAttachment, EnvironmentId, HistoryEvent,
    MessageRequest, ModelBinding, Outcome, Program, RequestedToolBinding, ResolvedEnvironment,
    ResolvedSessionRequest, Resources, Runtime, SealedSessionConfig, SessionId, SessionStatus,
    SessionSummary, ToolBinding, ToolDefinition, ToolHosting, resource_name_valid,
};
use rand::RngCore;
use tokio::sync::{mpsc, oneshot};

use crate::{
    Error,
    journal::{AppendRecord, JournalStore, SessionRow, SessionUpdate},
};
use actor::{SessionActor, SessionCommand, failure_of, failure_payload};

pub use config::{DEFAULT_TOOL_DEADLINE_MS, SessionConfig};

/// One running session: a task that drives its turns, and this handle to it.
///
/// A session owns nothing across sessions. It is given its store, its executors, and its
/// limits, it numbers its own records, and it journals everything it does. Which sessions
/// exist, which are running, and what to do with them after a restart is the host's.
#[derive(Clone)]
pub struct Session {
    session_id: SessionId,
    sender: mpsc::Sender<SessionCommand>,
    cancelled: Arc<AtomicBool>,
    pending_tools: Arc<PendingToolCalls>,
}

/// Client-hosted tool calls parked mid-turn, waiting for an outcome the session's
/// creator POSTs back. In-memory on purpose: a restart interrupts the turn exactly like
/// any other in-flight tool call, and the `tool_call_started` record on the feed is the
/// durable statement of what was asked.
pub(crate) struct PendingToolCalls {
    inner: Mutex<HashMap<u64, oneshot::Sender<Outcome>>>,
}

impl PendingToolCalls {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Registers a parked call and hands back the receiver the turn awaits. Called
    /// before the `tool_call_started` commit so a client that answers off the live feed
    /// can never race an empty map.
    pub(crate) fn park(&self, sequence: u64) -> oneshot::Receiver<Outcome> {
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut inner) = self.inner.lock() {
            inner.insert(sequence, sender);
        }
        receiver
    }

    /// Delivers an outcome to a parked call. A call that is not pending — unknown,
    /// already answered, or expired — is a conflict the caller hears about; the
    /// idempotency layer above turns retries into replays.
    pub(crate) fn resolve(&self, sequence: u64, outcome: Outcome) -> Result<(), Error> {
        let entry = self
            .inner
            .lock()
            .map_err(|_| Error::InvalidState("pending Tool map poisoned".into()))?
            .remove(&sequence);
        let Some(sender) = entry else {
            return Err(Error::InvalidState(
                "no client Tool call is pending under this sequence".into(),
            ));
        };
        // A dropped receiver means the turn stopped waiting (timeout or cancellation)
        // between our lookup and the send; the caller is told the same thing.
        sender.send(outcome).map_err(|_| {
            Error::InvalidState("no client Tool call is pending under this sequence".into())
        })
    }

    /// Forgets a parked call without answering it (timeout, cancellation, failed commit).
    pub(crate) fn discard(&self, sequence: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.remove(&sequence);
        }
    }
}

/// Record kinds a restart reads a session's state out of, and which a caller therefore
/// cannot supply as history.
const RESERVED_KINDS: [&str; 7] = [
    "session_creation_started",
    "session_creation_ended",
    "session_creation_failed",
    "session_ended",
    "turn_started",
    "turn_ended",
    "turn_failed",
];

/// A session between its first record and its admission. The host attaches the
/// environments the request named, journalling each step here, and then seals what was
/// actually granted with [`CreatingSession::complete`].
pub struct CreatingSession {
    store: Arc<dyn JournalStore>,
    config: Arc<SessionConfig>,
    row: SessionRow,
    /// The events this session opened with, carried in memory to the actor so the
    /// agentloop can be told about them. They are already in the journal; this is not a
    /// second copy of the record, it is the one delivery of it.
    history: Vec<serde_json::Value>,
}

pub fn empty_context() -> ContextEnvelope {
    ContextEnvelope {
        protocol_version: "agentloop/v1".into(),
        items: Vec::new(),
        state: None,
    }
}

/// The sealed configuration a session was admitted with, read from its row.
pub fn sealed_config(
    store: &dyn JournalStore,
    session_id: &SessionId,
) -> Result<SealedSessionConfig, Error> {
    let row = store
        .session_row(session_id)?
        .ok_or_else(|| Error::NotFound("session not found".into()))?;
    serde_json::from_value(row.configuration).map_err(|error| Error::Journal(error.to_string()))
}

impl Session {
    /// Writes a session's first record and hands back the creation to finish.
    ///
    /// Admitting the request bounds its authority before anything is journalled: the
    /// contract is checked here and again at `complete`, so a session can only ever do
    /// what it was granted.
    pub fn begin(
        store: Arc<dyn JournalStore>,
        config: Arc<SessionConfig>,
        request: &ResolvedSessionRequest,
    ) -> Result<CreatingSession, Error> {
        validate_session_contract(request)?;
        let row = SessionRow {
            session_id: SessionId::new(random_id("ses")),
            status: SessionStatus::Creating,
            through_sequence: 1,
            configuration: serde_json::to_value(request).map_err(json_error)?,
            context: serde_json::to_value(empty_context()).map_err(json_error)?,
        };
        // The creation record is the session's own genesis and comes first; the events the
        // caller handed back are what happened before it, and follow.
        store.create_session(
            &row,
            AppendRecord::new(
                codes::event::SESSION_CREATION_STARTED,
                serde_json::to_value(request).map_err(json_error)?,
            ),
        )?;
        let mut row = row;
        let history = replay_history(&*store, &mut row, &request.history)?;
        Ok(CreatingSession {
            store,
            config,
            row,
            history,
        })
    }

    /// Starts a session that is already in the store.
    ///
    /// A session rebuilt from the journal after a restart has an agentloop that has never
    /// seen any of it, so it is handed its own records once, before its first message. A
    /// session this process created was told at creation and is not told again.
    pub fn open(
        store: Arc<dyn JournalStore>,
        config: Arc<SessionConfig>,
        session_id: &SessionId,
    ) -> Result<Self, Error> {
        let row = store
            .session_row(session_id)?
            .ok_or_else(|| Error::NotFound("session not found".into()))?;
        let history = if store.take_restored(session_id)? {
            restored_history(&*store, session_id)?
        } else {
            Vec::new()
        };
        Self::spawn(store, config, row, history)
    }

    fn spawn(
        store: Arc<dyn JournalStore>,
        config: Arc<SessionConfig>,
        row: SessionRow,
        history: Vec<serde_json::Value>,
    ) -> Result<Self, Error> {
        let session_id = row.session_id.clone();
        let (sender, receiver) = mpsc::channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let pending_tools = Arc::new(PendingToolCalls::new());
        let actor = SessionActor::new(
            row,
            store,
            config,
            receiver,
            cancelled.clone(),
            history,
            pending_tools.clone(),
        )?;
        tokio::spawn(actor.run());
        Ok(Self {
            session_id,
            sender,
            cancelled,
            pending_tools,
        })
    }

    pub fn id(&self) -> &SessionId {
        &self.session_id
    }

    /// Runs one turn and returns when it is finished.
    pub async fn message(&self, request: MessageRequest) -> Result<SessionSummary, Error> {
        if request.input.message.is_empty() {
            return Err(Error::InvalidState("message cannot be empty".into()));
        }
        if serde_json::to_vec(&request).map_err(json_error)?.len() > 2 * 1024 * 1024 {
            return Err(Error::InvalidState("message request exceeds 2 MiB".into()));
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::Message { request, reply })
            .await
            .map_err(|_| stopped())?;
        response.await.map_err(|_| stopped())?
    }

    pub async fn cancel(&self) -> Result<(), Error> {
        self.cancelled.store(true, Ordering::Release);
        match self.sender.try_send(SessionCommand::Cancel) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(stopped()),
        }
    }

    pub async fn end(&self) -> Result<SessionSummary, Error> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::End { reply })
            .await
            .map_err(|_| stopped())?;
        response.await.map_err(|_| stopped())?
    }

    /// Answers a parked client-hosted tool call. Deliberately lock-free at the session
    /// level: the turn holding the park is inside `message` — this is the call that lets
    /// it finish.
    pub fn resolve_tool_call(&self, sequence: u64, outcome: Outcome) -> Result<(), Error> {
        self.pending_tools.resolve(sequence, outcome)
    }

    /// Journals an effect the host is about to perform on the session's behalf outside a
    /// turn, such as calling one of its environments. Returns the record's sequence,
    /// which names the operation; the host records what came of it with
    /// [`Session::record_call_ended`] or [`Session::record_call_failed`]. Refused while a
    /// turn is running.
    pub async fn record_call_started<T: serde::Serialize>(
        &self,
        kind: &str,
        request: &T,
    ) -> Result<u64, Error> {
        self.append(AppendRecord::new(
            format!("{kind}_started"),
            serde_json::json!({"request": request}),
        ))
        .await
    }

    pub async fn record_call_ended<T: serde::Serialize>(
        &self,
        kind: &str,
        sequence: u64,
        result: &T,
    ) -> Result<(), Error> {
        self.append(AppendRecord::new(
            format!("{kind}_ended"),
            serde_json::json!({"sequence": sequence, "result": result}),
        ))
        .await
        .map(|_| ())
    }

    pub async fn record_call_failed(
        &self,
        kind: &str,
        sequence: u64,
        error: &Error,
    ) -> Result<(), Error> {
        self.append(failed_record(kind, sequence, error)?)
            .await
            .map(|_| ())
    }

    async fn append(&self, record: AppendRecord) -> Result<u64, Error> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::Append { record, reply })
            .await
            .map_err(|_| stopped())?;
        response.await.map_err(|_| stopped())?
    }
}

impl CreatingSession {
    pub fn session_id(&self) -> &SessionId {
        &self.row.session_id
    }

    /// Journals an effect performed while the session is being created, before its
    /// actor exists. Returns the record's sequence, which names the operation.
    pub fn record_call_started<T: serde::Serialize>(
        &mut self,
        kind: &str,
        request: &T,
    ) -> Result<u64, Error> {
        self.append(AppendRecord::new(
            format!("{kind}_started"),
            serde_json::json!({"request": request}),
        ))
    }

    pub fn record_call_ended<T: serde::Serialize>(
        &mut self,
        kind: &str,
        sequence: u64,
        result: &T,
    ) -> Result<(), Error> {
        self.append(AppendRecord::new(
            format!("{kind}_ended"),
            serde_json::json!({"sequence": sequence, "result": result}),
        ))
        .map(|_| ())
    }

    pub fn record_call_failed(
        &mut self,
        kind: &str,
        sequence: u64,
        error: &Error,
    ) -> Result<(), Error> {
        self.append(failed_record(kind, sequence, error)?)
            .map(|_| ())
    }

    fn append(&mut self, record: AppendRecord) -> Result<u64, Error> {
        let saved = self.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[record],
            SessionUpdate::default(),
        )?;
        self.row.through_sequence += saved.len() as u64;
        Ok(self.row.through_sequence)
    }

    /// Seals what was granted and starts the session.
    pub fn complete(mut self, sealed: SealedSessionConfig) -> Result<Session, Error> {
        validate_session_contract(&sealed)?;
        let configuration = serde_json::to_value(&sealed).map_err(json_error)?;
        let context = self.row.context.clone();
        let saved = self.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(
                codes::event::SESSION_CREATION_ENDED,
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
        Session::spawn(self.store, self.config, self.row, self.history)
    }

    pub fn fail(mut self, code: &str, message: &str) -> Result<(), Error> {
        let saved = self.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(
                codes::event::SESSION_CREATION_FAILED,
                failure_payload(None, &Failure::new(code, message))?,
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

/// The record of an effect that did not come back with a result. `ambiguous` says
/// whether it may have happened anyway.
fn failed_record(kind: &str, sequence: u64, error: &Error) -> Result<AppendRecord, Error> {
    Ok(AppendRecord::new(
        format!("{kind}_failed"),
        failure_payload(Some(sequence), &failure_of(error))?,
    ))
}

fn stopped() -> Error {
    Error::InvalidState("session actor stopped".into())
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
    store: &dyn JournalStore,
    row: &mut SessionRow,
    history: &[HistoryEvent],
) -> Result<Vec<serde_json::Value>, Error> {
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
            return Err(Error::InvalidState(format!(
                "session history may not contain `{}`: lifecycle records are Brain's own, \
                 and a restart reads a session's state back out of them",
                event.event_type
            )));
        }
        if event.sequence <= previous {
            return Err(Error::InvalidState(format!(
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
    let saved = store.append(
        &row.session_id,
        row.through_sequence,
        &records,
        SessionUpdate::default(),
    )?;
    row.through_sequence += saved.len() as u64;
    Ok(history.iter().map(|event| event.data.clone()).collect())
}

/// A restored session's own records, to hand back to its agentloop.
///
/// Bounded at the same count the create API accepts as history. A conversation longer
/// than that restores and can be read, but is not replayed into a loop: handing over
/// part of a conversation as though it were the whole of it would be a worse answer
/// than declining to.
fn restored_history(
    store: &dyn JournalStore,
    session_id: &SessionId,
) -> Result<Vec<serde_json::Value>, Error> {
    const MOST: usize = 10_000;
    let mut history = Vec::new();
    let mut after = 0;
    loop {
        let page = store.records_after(session_id, after, 1_000)?;
        let Some(last) = page.last() else { break };
        after = last.sequence;
        history.extend(page.into_iter().map(|record| record.payload));
        if history.len() > MOST {
            return Ok(Vec::new());
        }
    }
    Ok(history)
}

/// The two request shapes a session admits differ only in where an Environment identity
/// lives: a resolved request names it directly, a sealed configuration carries it inside
/// the binding it resolved to. Every other term of the contract is identical, and core
/// principle 3 requires that authority be bounded identically however a session is
/// created, so the bounds are stated once and both shapes are read through these views.
trait EnvironmentView {
    fn environment_id(&self) -> &EnvironmentId;
    /// What this environment declared it executes and offers, when that is already
    /// known. A resolved request predates the setup/attach receipts that report it, so
    /// only a sealed attachment answers — and only a sealed configuration is bind-checked.
    fn declaration(&self) -> Option<(&[Runtime], &Resources)>;
}

impl EnvironmentView for ResolvedEnvironment {
    fn environment_id(&self) -> &EnvironmentId {
        &self.environment_id
    }

    fn declaration(&self) -> Option<(&[Runtime], &Resources)> {
        None
    }
}

impl EnvironmentView for EnvironmentAttachment {
    fn environment_id(&self) -> &EnvironmentId {
        &self.binding.environment_id
    }

    fn declaration(&self) -> Option<(&[Runtime], &Resources)> {
        Some((&self.runtimes, &self.resources))
    }
}

trait ToolBindingView {
    fn name(&self) -> &str;
    fn environment_id(&self) -> Option<&EnvironmentId>;
    fn needs(&self) -> &[String];
    fn binding_names(&self) -> &[String];
    fn hosting(&self) -> ToolHosting;
    fn program(&self) -> Option<&Program>;
}

impl ToolBindingView for RequestedToolBinding {
    fn name(&self) -> &str {
        &self.name
    }

    fn environment_id(&self) -> Option<&EnvironmentId> {
        self.environment_id.as_ref()
    }

    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn binding_names(&self) -> &[String] {
        &self.binding_names
    }

    fn hosting(&self) -> ToolHosting {
        self.hosting
    }

    fn program(&self) -> Option<&Program> {
        self.program.as_ref()
    }
}

impl ToolBindingView for ToolBinding {
    fn name(&self) -> &str {
        &self.name
    }

    fn environment_id(&self) -> Option<&EnvironmentId> {
        self.environment
            .as_ref()
            .map(|binding| &binding.environment_id)
    }

    fn needs(&self) -> &[String] {
        &self.needs
    }

    fn binding_names(&self) -> &[String] {
        &self.binding_names
    }

    fn hosting(&self) -> ToolHosting {
        self.hosting
    }

    fn program(&self) -> Option<&Program> {
        self.program.as_ref()
    }
}

trait SessionContract: serde::Serialize {
    type Environment: EnvironmentView;
    type ToolBinding: ToolBindingView;

    fn agentloop_identity(&self) -> &AgentloopIdentity;
    fn model(&self) -> &ModelBinding;
    fn system(&self) -> &str;
    fn tools(&self) -> &[ToolDefinition];
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

    fn system(&self) -> &str {
        &self.system
    }

    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
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

    fn system(&self) -> &str {
        &self.system
    }

    fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    fn environments(&self) -> &[Self::Environment] {
        &self.environments
    }

    fn tool_bindings(&self) -> &[Self::ToolBinding] {
        &self.tool_bindings
    }
}

fn validate_session_contract(request: &impl SessionContract) -> Result<(), Error> {
    if serde_json::to_vec(request).map_err(json_error)?.len() > 2 * 1024 * 1024 {
        return Err(Error::InvalidState("session request exceeds 2 MiB".into()));
    }
    if !identity_valid(request.agentloop_identity().as_str())
        || !identifier_valid(&request.model().binding_id)
        || request.model().model.is_empty()
        || request.model().model.len() > 256
        || request.system().len() > 131_072
        || request.tools().len() > 128
        || request.environments().len() > 128
        || request.tool_bindings().len() > 128
    {
        return Err(Error::InvalidState(
            "session request violates a contract size or identity bound".into(),
        ));
    }
    for tool in request.tools() {
        if !identifier_valid(&tool.name)
            || tool.description.len() > 8_192
            || !tool.input_schema.is_object()
            || tool
                .output_schema
                .as_ref()
                .is_some_and(|value| !value.is_object())
        {
            return Err(Error::InvalidState(
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
                || binding
                    .environment_id()
                    .is_some_and(|id| !identifier_valid(id.as_str()))
                || binding
                    .binding_names()
                    .iter()
                    .any(|name| !identifier_valid(name))
        })
    {
        return Err(Error::InvalidState(
            "Environment or Tool binding has an invalid identity".into(),
        ));
    }
    // Resource names are the bind check's vocabulary: an invalid or repeated one is
    // rejected before it can silently match nothing.
    for binding in request.tool_bindings() {
        let needs = binding.needs();
        if needs.iter().any(|name| !resource_name_valid(name))
            || needs
                .iter()
                .enumerate()
                .any(|(index, name)| needs[..index].contains(name))
        {
            return Err(Error::InvalidState(format!(
                "Tool `{}` names an invalid or repeated resource",
                binding.name()
            )));
        }
    }
    // A client-hosted tool's code stays in the author's process; a program beside
    // it would be an artifact nothing is allowed to run.
    if request.tool_bindings().iter().any(|binding| {
        matches!(binding.hosting(), ToolHosting::Client) && binding.program().is_some()
    }) {
        return Err(Error::InvalidState(
            "a client Tool binding cannot carry a program".into(),
        ));
    }
    // A client-hosted tool is served off the event feed by an application process: no
    // environment is on its path, so binding one (or requiring capabilities only an
    // environment could provide) is a contradiction the caller should hear about.
    for binding in request.tool_bindings() {
        let client = matches!(binding.hosting(), ToolHosting::Client);
        if client && binding.environment_id().is_some() {
            return Err(Error::InvalidState(
                "a client-hosted Tool binding cannot name an Environment".into(),
            ));
        }
        if client && !binding.needs().is_empty() {
            return Err(Error::InvalidState(
                "a client-hosted Tool binding cannot need Environment resources".into(),
            ));
        }
        if !client && binding.environment_id().is_none() {
            return Err(Error::InvalidState(
                "every provisioned Tool binding must name a bound Environment".into(),
            ));
        }
    }
    let mut definitions: Vec<&str> = request
        .tools()
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
        return Err(Error::InvalidState(
            "every unique Tool definition must have exactly one binding".into(),
        ));
    }
    let environment_ids: std::collections::HashSet<_> = request
        .environments()
        .iter()
        .map(EnvironmentView::environment_id)
        .collect();
    if environment_ids.len() != request.environments().len() {
        return Err(Error::InvalidState(
            "Environment identities must be unique".into(),
        ));
    }
    if request.tool_bindings().iter().any(|binding| {
        binding
            .environment_id()
            .is_some_and(|id| !environment_ids.contains(id))
    }) {
        return Err(Error::InvalidState(
            "every Tool binding must name a bound Environment".into(),
        ));
    }
    // The bind check: the environment a tool is bound to must launch the tool's
    // program kind and declare every resource the tool needs, so a mismatch is a
    // create-time rejection naming all three parties instead of a runtime mystery. A
    // tool with no program and no needs binds anywhere. The declaration is known only
    // once the environment has attached, so only the sealed shape is checked — every
    // admitted session passes through it.
    for binding in request.tool_bindings() {
        let Some(environment_id) = binding.environment_id() else {
            continue;
        };
        let declaration = request
            .environments()
            .iter()
            .find(|environment| environment.environment_id() == environment_id)
            .and_then(EnvironmentView::declaration);
        let Some((runtimes, resources)) = declaration else {
            continue;
        };
        if let Some(runtime) = binding
            .program()
            .map(Program::runtime)
            .filter(|runtime| !runtimes.contains(runtime))
        {
            return Err(Error::InvalidState(format!(
                "Tool `{}` needs runtime `{runtime}` that Environment `{environment_id}` does not provide",
                binding.name(),
            )));
        }
        if let Some(missing) = binding
            .needs()
            .iter()
            .find(|name| !resources.contains_key(name.as_str()))
        {
            return Err(Error::InvalidState(format!(
                "Tool `{}` needs resource `{missing}` that Environment `{environment_id}` does not provide",
                binding.name(),
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

fn json_error(error: serde_json::Error) -> Error {
    Error::InvalidState(error.to_string())
}

#[cfg(test)]
mod tests {
    use brain_protocol::{AttachmentId, EnvironmentBinding, Identity, LifecyclePolicy};

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
            system: "test".into(),
            response_format: None,
            tools: vec![tool()],
            environments: vec![ResolvedEnvironment {
                environment_id: EnvironmentId::new("workspace"),
                configuration: serde_json::json!({}),
                lifecycle_policy: LifecyclePolicy::Shared,
                binding_identities: Default::default(),
            }],
            tool_bindings: vec![RequestedToolBinding {
                name: "search".into(),
                environment_id: Some(EnvironmentId::new("workspace")),
                needs: Vec::new(),
                binding_names: Vec::new(),
                hosting: ToolHosting::Provisioned,
                program: None,
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
            system: "test".into(),
            response_format: None,
            tools: vec![tool()],
            environments: vec![EnvironmentAttachment {
                binding: environment_binding(),
                attachment_id: AttachmentId::new("attachment"),
                runtimes: vec![Runtime::Esm],
                resources: [
                    ("process".to_string(), serde_json::json!({})),
                    ("fs".to_string(), serde_json::json!({"root": "/workspace"})),
                ]
                .into_iter()
                .collect(),
            }],
            tool_bindings: vec![ToolBinding {
                name: "search".into(),
                environment: Some(environment_binding()),
                attachment_id: Some(AttachmentId::new("attachment")),
                needs: vec!["process".into()],
                binding_names: Vec::new(),
                hosting: ToolHosting::Provisioned,
                program: None,
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
                |request| request.system = "s".repeat(131_073),
                "size or identity bound",
            ),
            (
                "more than 128 Tool definitions",
                |request| {
                    let binding = request.tool_bindings[0].clone();
                    request.tools = (0..129)
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
                    request.tool_bindings[0].environment_id = Some(EnvironmentId::new("env0"));
                },
                "size or identity bound",
            ),
            (
                "a Tool name that is not an identifier",
                |request| {
                    request.tools[0].name = "../escape".into();
                    request.tool_bindings[0].name = "../escape".into();
                },
                "Tool definition violates",
            ),
            (
                "a Tool description over 8 KiB",
                |request| request.tools[0].description = "d".repeat(8_193),
                "Tool definition violates",
            ),
            (
                "a Tool input schema that is not an object",
                |request| request.tools[0].input_schema = serde_json::json!("string"),
                "Tool definition violates",
            ),
            (
                "a Tool output schema that is not an object",
                |request| request.tools[0].output_schema = Some(serde_json::json!([])),
                "Tool definition violates",
            ),
            (
                "an Environment identity that is not an identifier",
                |request| {
                    request.environments[0].environment_id = EnvironmentId::new("../escape");
                    request.tool_bindings[0].environment_id = Some(EnvironmentId::new("../escape"));
                },
                "invalid identity",
            ),
            (
                "a binding name that is not an identifier",
                |request| request.tool_bindings[0].binding_names = vec!["../escape".into()],
                "invalid identity",
            ),
            (
                "a client Tool carrying a program",
                |request| {
                    request.tool_bindings[0].hosting = ToolHosting::Client;
                    request.tool_bindings[0].environment_id = None;
                    request.tool_bindings[0].needs = Vec::new();
                    request.tool_bindings[0].program = Some(Program::Esm {
                        identity: Identity::of(&"payload").unwrap(),
                    });
                },
                "cannot carry a program",
            ),
            (
                "two Tool definitions sharing one name",
                |request| {
                    request.tools.push(tool());
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
                |request| request.tools.clear(),
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
                |request| {
                    request.tool_bindings[0].environment_id = Some(EnvironmentId::new("elsewhere"));
                },
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
    /// the only other way a session is admitted. It is held to the same bounds, so a
    /// session does not become more permissive by being driven from inside a service.
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
                |sealed| sealed.system = "s".repeat(131_073),
                "size or identity bound",
            ),
            (
                "a Tool name that is not an identifier",
                |sealed| {
                    sealed.tools[0].name = "../escape".into();
                    sealed.tool_bindings[0].name = "../escape".into();
                },
                "Tool definition violates",
            ),
            (
                "an Environment identity that is not an identifier",
                |sealed| {
                    sealed.environments[0].binding.environment_id = EnvironmentId::new("../escape");
                    sealed.tool_bindings[0]
                        .environment
                        .as_mut()
                        .unwrap()
                        .environment_id = EnvironmentId::new("../escape");
                },
                "invalid identity",
            ),
            (
                "a binding name that is not an identifier",
                |sealed| sealed.tool_bindings[0].binding_names = vec!["../escape".into()],
                "invalid identity",
            ),
            (
                "a Tool needing a resource its Environment does not declare",
                |sealed| sealed.tool_bindings[0].needs = vec!["dom".into()],
                "does not provide",
            ),
            (
                "a Tool whose program kind its Environment cannot launch",
                |sealed| {
                    sealed.tool_bindings[0].program = Some(Program::Shell {
                        identity: Identity::of(&"script").unwrap(),
                        script: "$command".into(),
                    })
                },
                "does not provide",
            ),
            (
                "a Tool naming a resource that is not a resource name",
                |sealed| sealed.tool_bindings[0].needs = vec!["../fs".into()],
                "invalid or repeated resource",
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
                    sealed.tool_bindings[0]
                        .environment
                        .as_mut()
                        .unwrap()
                        .environment_id = EnvironmentId::new("elsewhere");
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
