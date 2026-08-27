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
    CreateSessionRequest, EventPage, JournalId, MessageRequest, OperationId, SealedSessionConfig,
    Session, SessionId, SessionStatus, operation_id, request_digest,
};
use brain_telemetry::TelemetryPublisher;
use rand::RngCore;
use tokio::sync::{mpsc, oneshot};

use crate::{
    KernelError, context,
    journal::{AppendRecord, JournalStore, SessionRow, SessionUpdate, SqliteJournal, event_page},
};
use actor::{SessionActor, SessionCommand};

pub use config::KernelConfig;

#[derive(Clone)]
pub struct Kernel {
    inner: Arc<KernelInner>,
}

struct KernelInner {
    store: Arc<dyn JournalStore>,
    config: KernelConfig,
    telemetry: TelemetryPublisher,
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

pub struct CreatingSession {
    kernel: Kernel,
    row: SessionRow,
}

impl Kernel {
    pub fn open(config: KernelConfig, telemetry: TelemetryPublisher) -> Result<Self, KernelError> {
        fs::create_dir_all(&config.data_dir)
            .map_err(|error| KernelError::Journal(error.to_string()))?;
        let store = Arc::new(SqliteJournal::open(&config.data_dir.join("brain.sqlite3"))?);
        Self::with_store(config, store, telemetry)
    }

    pub fn with_store(
        config: KernelConfig,
        store: Arc<dyn JournalStore>,
        telemetry: TelemetryPublisher,
    ) -> Result<Self, KernelError> {
        let kernel = Self {
            inner: Arc::new(KernelInner {
                store,
                config,
                telemetry,
                sessions: Mutex::new(HashMap::new()),
            }),
        };
        kernel.recover_interrupted()?;
        Ok(kernel)
    }

    pub async fn create_session(
        &self,
        request: SealedSessionConfig,
    ) -> Result<SessionHandle, KernelError> {
        validate_bindings(&request)?;
        let session_id = SessionId::new(random_id("ses"));
        let journal_id = JournalId::new(random_id("jrn"));
        let presentation = context::presentation(&request.presentation)?;
        let context = context::empty_context();
        let row = SessionRow {
            session_id: session_id.clone(),
            journal_id,
            status: SessionStatus::Idle,
            through_sequence: 1,
            configuration: serde_json::to_value(&request).map_err(json_error)?,
            context: serde_json::to_value(&context).map_err(json_error)?,
            presentation_digest: presentation.digest.clone(),
            metadata: request.metadata.clone(),
        };
        self.inner.store.create_session(
            &row,
            AppendRecord::new(
                "session_created",
                serde_json::json!({
                    "configuration": request,
                    "presentation_bytes": presentation.bytes,
                    "presentation_digest": presentation.digest,
                }),
            ),
        )?;
        self.spawn(row)
    }

    pub fn begin_session(
        &self,
        request: &CreateSessionRequest,
    ) -> Result<CreatingSession, KernelError> {
        validate_create_request(request)?;
        validate_requested_bindings(request)?;
        let session_id = SessionId::new(random_id("ses"));
        let journal_id = JournalId::new(random_id("jrn"));
        let presentation = context::presentation(&request.presentation)?;
        let context = context::empty_context();
        let row = SessionRow {
            session_id,
            journal_id,
            status: SessionStatus::Creating,
            through_sequence: 1,
            configuration: serde_json::to_value(request).map_err(json_error)?,
            context: serde_json::to_value(context).map_err(json_error)?,
            presentation_digest: presentation.digest,
            metadata: request.metadata.clone(),
        };
        self.inner.store.create_session(
            &row,
            AppendRecord::new(
                "session_creation_started",
                serde_json::to_value(request).map_err(json_error)?,
            ),
        )?;
        Ok(CreatingSession {
            kernel: self.clone(),
            row,
        })
    }

    pub fn session(&self, session_id: &SessionId) -> Result<Session, KernelError> {
        let row = self
            .inner
            .store
            .session(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        Ok(public_session(&row))
    }

    pub fn sessions(&self) -> Result<Vec<Session>, KernelError> {
        Ok(self
            .inner
            .store
            .sessions()?
            .iter()
            .map(public_session)
            .collect())
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
            .session(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        self.spawn(row)
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
            .session(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        serde_json::from_value(row.configuration)
            .map_err(|error| KernelError::Journal(error.to_string()))
    }

    pub fn record_external_intent<T: serde::Serialize>(
        &self,
        session_id: &SessionId,
        kind: &str,
        request: &T,
    ) -> Result<(OperationId, String), KernelError> {
        self.inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?
            .remove(session_id);
        let row = self
            .inner
            .store
            .session(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        if !matches!(row.status, SessionStatus::Idle) {
            return Err(KernelError::InvalidState("session is not idle".into()));
        }
        let operation_id = operation_id(&row.journal_id, row.through_sequence + 1);
        let digest = request_digest(request)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        self.inner.store.append(
            session_id,
            row.through_sequence,
            &[AppendRecord::new(
                format!("{kind}_intent"),
                serde_json::json!({"operation_id":operation_id,"request_digest":digest,"request":request}),
            )],
            SessionUpdate::default(),
        )?;
        Ok((operation_id, digest))
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
            .session(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        self.inner.store.append(
            session_id,
            row.through_sequence,
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
            .session(session_id)?
            .ok_or_else(|| KernelError::InvalidState("session not found".into()))?;
        if matches!(row.status, SessionStatus::Ended) {
            return Ok(public_session(&row));
        }
        if !matches!(row.status, SessionStatus::Idle) {
            return Err(KernelError::InvalidState("session is not idle".into()));
        }
        self.inner.store.append(
            session_id,
            row.through_sequence,
            &[AppendRecord::new("session_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Ended),
                context: None,
                configuration: None,
            },
        )?;
        row.through_sequence += 1;
        row.status = SessionStatus::Ended;
        self.inner
            .sessions
            .lock()
            .map_err(|_| KernelError::InvalidState("session map poisoned".into()))?
            .remove(session_id);
        Ok(public_session(&row))
    }

    pub fn idempotency_get<T: serde::Serialize>(
        &self,
        scope: &str,
        key: &str,
        request: &T,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        let digest = request_digest(request)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        self.inner.store.idempotency_get(scope, key, &digest)
    }

    fn recover_interrupted(&self) -> Result<(), KernelError> {
        for row in self.inner.store.sessions()? {
            let classification = match row.status {
                SessionStatus::Creating => Some("session_creation_interrupted"),
                SessionStatus::Running => Some("operation_outcome_ambiguous"),
                SessionStatus::Idle | SessionStatus::Ended | SessionStatus::Failed => None,
            };
            let Some(classification) = classification else {
                continue;
            };
            self.inner.store.append(
                &row.session_id,
                row.through_sequence,
                &[AppendRecord::new(
                    "recovery_interrupted",
                    serde_json::json!({
                        "classification": classification,
                        "message": "Brain restarted before the in-flight transition reached a terminal record"
                    }),
                )],
                SessionUpdate {
                    status: Some(SessionStatus::Failed),
                    context: None,
                    configuration: None,
                },
            )?;
        }
        Ok(())
    }

    pub fn idempotency_put<T: serde::Serialize>(
        &self,
        scope: &str,
        key: &str,
        request: &T,
        response: &serde_json::Value,
    ) -> Result<(), KernelError> {
        let digest = request_digest(request)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        self.inner
            .store
            .idempotency_put(scope, key, &digest, response)
    }

    fn spawn(&self, row: SessionRow) -> Result<SessionHandle, KernelError> {
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
            self.inner.telemetry.clone(),
            self.inner.config.max_decisions_per_turn,
            receiver,
            cancelled.clone(),
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
    ) -> Result<(OperationId, String), KernelError> {
        let operation_id = operation_id(&self.row.journal_id, self.row.through_sequence + 1);
        let digest = request_digest(request)
            .map_err(|error| KernelError::InvalidState(error.to_string()))?;
        let saved = self.kernel.inner.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &[AppendRecord::new(format!("{kind}_intent"), serde_json::json!({"operation_id":operation_id,"request_digest":digest,"request":request}))],
            SessionUpdate::default(),
        )?;
        self.row.through_sequence += saved.len() as u64;
        Ok((operation_id, digest))
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
        validate_bindings(&sealed)?;
        if sealed.metadata != self.row.metadata {
            return Err(KernelError::InvalidState(
                "sealed session metadata changed during creation".into(),
            ));
        }
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
        self.kernel.spawn(self.row)
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

fn validate_bindings(request: &SealedSessionConfig) -> Result<(), KernelError> {
    let mut definitions: Vec<&str> = request
        .presentation
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    let mut bindings: Vec<&str> = request
        .tool_bindings
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    definitions.sort_unstable();
    bindings.sort_unstable();
    if definitions.windows(2).any(|pair| pair[0] == pair[1])
        || bindings.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err(KernelError::InvalidState(
            "tool names must be unique".into(),
        ));
    }
    if definitions != bindings {
        return Err(KernelError::InvalidState(
            "every Tool definition must have exactly one binding".into(),
        ));
    }
    let environment_ids: std::collections::HashSet<_> = request
        .environments
        .iter()
        .map(|environment| &environment.binding.environment_id)
        .collect();
    if environment_ids.len() != request.environments.len()
        || request
            .tool_bindings
            .iter()
            .any(|binding| !environment_ids.contains(&binding.environment_id))
    {
        return Err(KernelError::InvalidState(
            "sealed Environment bindings are inconsistent".into(),
        ));
    }
    Ok(())
}

fn validate_requested_bindings(request: &CreateSessionRequest) -> Result<(), KernelError> {
    let mut definitions: Vec<&str> = request
        .presentation
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    let mut bindings: Vec<&str> = request
        .tool_bindings
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    definitions.sort_unstable();
    bindings.sort_unstable();
    if definitions.windows(2).any(|pair| pair[0] == pair[1])
        || bindings.windows(2).any(|pair| pair[0] == pair[1])
        || definitions != bindings
    {
        return Err(KernelError::InvalidState(
            "every unique Tool definition must have exactly one requested binding".into(),
        ));
    }
    let environment_ids: std::collections::HashSet<_> = request
        .environments
        .iter()
        .map(|environment| &environment.environment_id)
        .collect();
    if environment_ids.len() != request.environments.len() {
        return Err(KernelError::InvalidState(
            "Environment identities must be unique".into(),
        ));
    }
    if request
        .tool_bindings
        .iter()
        .any(|binding| !environment_ids.contains(&binding.environment_id))
    {
        return Err(KernelError::InvalidState(
            "every Tool binding must name a requested Environment".into(),
        ));
    }
    Ok(())
}

fn validate_create_request(request: &CreateSessionRequest) -> Result<(), KernelError> {
    if serde_json::to_vec(request).map_err(json_error)?.len() > 2 * 1024 * 1024 {
        return Err(KernelError::InvalidState(
            "session request exceeds 2 MiB".into(),
        ));
    }
    if !digest_valid(request.agentloop_digest.as_str())
        || !identifier_valid(&request.model.binding_id)
        || request.model.model.is_empty()
        || request.model.model.len() > 256
        || request.presentation.system.len() > 131_072
        || request.presentation.tools.len() > 128
        || request.environments.len() > 128
        || request.tool_bindings.len() > 128
    {
        return Err(KernelError::InvalidState(
            "session request violates a contract size or identity bound".into(),
        ));
    }
    for tool in &request.presentation.tools {
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
        .environments
        .iter()
        .any(|environment| !identifier_valid(environment.environment_id.as_str()))
        || request.tool_bindings.iter().any(|binding| {
            !identifier_valid(&binding.name)
                || !identifier_valid(binding.environment_id.as_str())
                || !identifier_valid(&binding.remote_tool_id)
        })
    {
        return Err(KernelError::InvalidState(
            "Environment or Tool binding has an invalid identity".into(),
        ));
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

fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn public_session(row: &SessionRow) -> Session {
    Session {
        session_id: row.session_id.clone(),
        journal_id: row.journal_id.clone(),
        status: row.status.clone(),
        through_sequence: row.through_sequence,
        presentation_digest: row.presentation_digest.clone(),
        metadata: row.metadata.clone(),
    }
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    format!("{prefix}_{}", hex::encode(bytes))
}

fn json_error(error: serde_json::Error) -> KernelError {
    KernelError::InvalidState(error.to_string())
}
