//! The task behind a session: one turn at a time, with the agentloop in charge of the
//! turn and Brain in charge of everything the turn does.
//!
//! A turn is one call into the loop. While it runs, the loop reaches Brain through
//! [`TurnHost`]: model calls, tool dispatch, its own records, telemetry. Each service
//! journals before it acts, so the feed says what happened whether or not the loop
//! comes back. Transcript and kv writes commit inline; turn return carries only a result.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use brain_protocol::{
    Message, MessageRequest, ModelRequest, ModelResult, ModelStreamEvent, RuntimeEnvelope,
    SessionConfig, SessionStatus, SessionSummary, StreamingEvent, ToolDispatch, ToolInvocation,
    ToolReturn, TurnInput, TurnOutput,
    codes::{self, Failure},
};
use futures_util::future::join_all;
use tokio::sync::{Mutex, RwLock, RwLockReadGuard, mpsc, oneshot};

use super::{SessionRuntime, TurnServices};
use crate::{
    Error,
    journal::{
        AppendRecord, Folded, JournalEntry, SessionRecord, SessionRow, SessionStore, SessionUpdate,
    },
};

/// The durable sequence through which the Agentloop explicitly acknowledged observations.
pub const LAST_ACTIVATION_KEY: &str = "brain.last_activation";

/// Records handed to a loop as "what happened since you last ran". More than this and
/// the loop reads the feed itself.
const EVENTS_PER_TURN: usize = 1_000;

pub enum SessionCommand {
    Message {
        request: MessageRequest,
        started: oneshot::Sender<u64>,
        reply: oneshot::Sender<Result<SessionSummary, Error>>,
    },
    Events {
        started: oneshot::Sender<u64>,
        reply: oneshot::Sender<Result<SessionSummary, Error>>,
    },
    Cancel,
    End {
        reply: oneshot::Sender<Result<SessionSummary, Error>>,
    },
    /// A record the host writes between turns, for an effect it performs on the
    /// session's behalf. Goes through the actor so the sequence stays one counter.
    Append {
        record: AppendRecord,
        reply: oneshot::Sender<Result<u64, Error>>,
    },
}

pub struct SessionActor {
    row: SessionRow,
    config: Arc<SessionConfig>,
    store: Arc<dyn SessionStore>,
    runtime: Arc<SessionRuntime>,
    receiver: mpsc::Receiver<SessionCommand>,
    cancel_requested: Arc<AtomicBool>,
    /// The transcript and kv as the journal holds them.
    folded: Folded,
    tools: Arc<crate::ToolGroup>,
}

impl SessionActor {
    pub fn new(
        mut row: SessionRow,
        store: Arc<dyn SessionStore>,
        runtime: Arc<SessionRuntime>,
        receiver: mpsc::Receiver<SessionCommand>,
        cancel_requested: Arc<AtomicBool>,
        tools: Arc<crate::ToolGroup>,
    ) -> Result<Self, Error> {
        let config: SessionConfig = serde_json::from_value(std::mem::take(&mut row.configuration))
            .map_err(|error| Error::Journal(error.to_string()))?;
        let folded = store.fold()?;
        Ok(Self {
            row,
            config: Arc::new(config),
            store,
            runtime,
            receiver,
            cancel_requested,
            folded,
            tools,
        })
    }

    pub async fn run(mut self) {
        while let Some(command) = self.receiver.recv().await {
            match command {
                SessionCommand::Message {
                    request,
                    started,
                    reply,
                } => {
                    let result = self.turn(Some(request), started).await;
                    let _ = reply.send(result);
                }
                SessionCommand::Events { started, reply } => {
                    let _ = reply.send(self.turn(None, started).await);
                }
                SessionCommand::Cancel => {
                    self.cancel_requested.store(true, Ordering::Release);
                }
                SessionCommand::End { reply } => {
                    self.tools.interrupt().await;
                    self.tools.wait().await;
                    let _ = reply.send(self.end().await);
                }
                SessionCommand::Append { record, reply } => {
                    let _ = reply.send(self.append_between_turns(record).await);
                }
            }
        }
    }

    async fn turn(
        &mut self,
        request: Option<MessageRequest>,
        started: oneshot::Sender<u64>,
    ) -> Result<SessionSummary, Error> {
        if !matches!(self.row.status, SessionStatus::Idle) {
            return Err(Error::InvalidState("session is not idle".into()));
        }
        self.cancel_requested.store(false, Ordering::Release);
        let payload = match &request {
            Some(request) => serde_json::to_value(request).map_err(json_error)?,
            None => serde_json::json!({"trigger": "events"}),
        };
        let sequence = self
            .commit(
                vec![AppendRecord::new(codes::event::TURN_STARTED, payload)],
                Some(SessionStatus::Running),
            )
            .await?[0]
            .sequence;
        let _ = started.send(sequence);
        let since = self
            .folded
            .kv
            .get(LAST_ACTIVATION_KEY)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let event_records = self.store.records_after(since, EVENTS_PER_TURN)?;
        let events = event_records
            .into_iter()
            .map(SessionRecord::into_event)
            .collect();
        let input = TurnInput {
            input: request.map(|request| request.input),
            transcript: self.folded.transcript.clone(),
            kv: self.folded.kv.clone(),
            events,
            configuration: self.config.agentloop.configuration.clone(),
            system: self.config.system.clone(),
            tools: self
                .config
                .tools
                .iter()
                .map(|tool| brain_protocol::ActivationTool {
                    definition: tool.definition(),
                    environments: tool.placements.keys().cloned().collect(),
                })
                .collect(),
            runtime: RuntimeEnvelope::at(&self.row.session_id, self.row.through_sequence),
        };
        let activation = self
            .commit(
                vec![AppendRecord::new(
                    codes::event::ACTIVATION_STARTED,
                    serde_json::json!({"since": since}),
                )],
                None,
            )
            .await?[0]
            .sequence;
        let host = Arc::new(TurnHost {
            active: Arc::new(RwLock::new(true)),
            tools: self.tools.clone(),
            emissions: Arc::new(crate::tool::EmissionBudget::new(
                self.runtime.limits.max_emitted_bytes,
            )),
            origin: brain_protocol::EventOrigin::Agentloop {
                sequence: activation,
            },
            session_id: self.row.session_id.clone(),
            store: self.store.clone(),
            runtime: self.runtime.clone(),
            config: self.config.clone(),
            cancel_requested: self.cancel_requested.clone(),
            model_calls: Arc::new(AtomicUsize::new(0)),
            cursor: Arc::new(Mutex::new(Cursor {
                through_sequence: self.row.through_sequence,
                transcript: std::mem::take(&mut self.folded.transcript),
                kv: std::mem::take(&mut self.folded.kv),
            })),
        });
        let agentloop_environment = self
            .config
            .environment(&self.config.agentloop.environment)
            .cloned()
            .ok_or_else(|| Error::InvalidState("Agentloop Environment is missing".into()))?;
        let outcome = {
            let running = self.runtime.loop_executor.turn(
                &self.row.session_id,
                activation,
                &self.config.agentloop,
                &agentloop_environment,
                input,
                host.clone(),
            );
            tokio::pin!(running);
            match self.runtime.limits.max_turn() {
                None => running.await,
                Some(max_turn) => match tokio::time::timeout(max_turn, &mut running).await {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        self.cancel_requested.store(true, Ordering::Release);
                        self.tools.interrupt().await;
                        // Keep mediated calls alive through their terminal commit and bounded
                        // cancellation. Dropping the executor here would abandon those records.
                        let _ = running.await;
                        Err(Error::Cancelled(
                            "turn exceeded its wall-time budget".into(),
                        ))
                    }
                },
            }
        };
        // Whatever the loop did, the host's cursor is the truth about what reached the
        // journal.
        let cursor = {
            let mut active = host.active.write().await;
            *active = false;
            let mut cursor = host.cursor.lock().await;
            Cursor {
                through_sequence: cursor.through_sequence,
                transcript: std::mem::take(&mut cursor.transcript),
                kv: std::mem::take(&mut cursor.kv),
            }
        };
        self.row.through_sequence = cursor.through_sequence;
        self.folded.transcript = cursor.transcript;
        self.folded.kv = cursor.kv;
        // A turn that was cancelled does not get to finish, whatever the loop brought
        // back: what the services already recorded stands, the rest is discarded.
        let outcome = match outcome {
            Ok(_) if self.cancel_requested.load(Ordering::Acquire) => {
                Err(Error::Cancelled("turn cancelled".into()))
            }
            outcome => outcome,
        };
        match outcome {
            Ok(output) => self.finish_turn(output).await,
            Err(error) => {
                self.tools.interrupt().await;
                let failure = failure_of(&error);
                let failure = if self.cancel_requested.load(Ordering::Acquire)
                    && !matches!(error, Error::Cancelled(_))
                {
                    Failure::new(codes::failure::CANCELLED, error.to_string())
                } else {
                    failure
                };
                // Records the loop's tools started are their own; a cancellation the
                // loop did not see through to its tools is told to them here.
                self.close_turn(vec![
                    AppendRecord::new(
                        codes::event::ACTIVATION_FAILED,
                        failure_payload(None, &failure)?,
                    ),
                    AppendRecord::new(codes::event::TURN_FAILED, failure_payload(None, &failure)?),
                ])
                .await
            }
        }
    }

    /// Close the turn without overwriting state saved by its services.
    async fn finish_turn(&mut self, output: TurnOutput) -> Result<SessionSummary, Error> {
        self.commit(
            vec![AppendRecord::new(
                codes::event::ACTIVATION_ENDED,
                serde_json::json!({}),
            )],
            None,
        )
        .await?;
        self.close_turn(vec![AppendRecord::new(
            codes::event::TURN_ENDED,
            serde_json::json!({"result": output.result}),
        )])
        .await
    }

    /// Commits a turn's terminal records and returns the session to Idle.
    async fn close_turn(&mut self, records: Vec<AppendRecord>) -> Result<SessionSummary, Error> {
        self.commit(records, Some(SessionStatus::Idle)).await?;
        Ok(self.public())
    }

    async fn end(&mut self) -> Result<SessionSummary, Error> {
        if matches!(self.row.status, SessionStatus::Running) {
            return Err(Error::InvalidState("cannot end a running session".into()));
        }
        if !matches!(self.row.status, SessionStatus::Ended) {
            self.commit(
                vec![AppendRecord::new(
                    codes::event::SESSION_ENDED,
                    serde_json::json!({}),
                )],
                Some(SessionStatus::Ended),
            )
            .await?;
        }
        Ok(self.public())
    }

    /// A record for something the host did to the session between turns. Refused while
    /// a turn is running: the turn owns the sequence until it ends.
    async fn append_between_turns(&mut self, record: AppendRecord) -> Result<u64, Error> {
        if !matches!(self.row.status, SessionStatus::Idle | SessionStatus::Ending) {
            return Err(Error::InvalidState("session is not idle".into()));
        }
        let status =
            (record.kind == codes::event::SESSION_END_STARTED).then_some(SessionStatus::Ending);
        self.commit(vec![record], status).await?;
        Ok(self.row.through_sequence)
    }

    async fn commit(
        &mut self,
        records: Vec<AppendRecord>,
        status: Option<SessionStatus>,
    ) -> Result<Vec<SessionRecord>, Error> {
        let saved = append_records(self.store.clone(), records, status.clone()).await?;
        if let Some(last) = saved.last() {
            self.row.through_sequence = last.sequence;
        }
        if let Some(status) = status {
            self.row.status = status;
        }
        Ok(saved)
    }

    fn public(&self) -> SessionSummary {
        SessionSummary {
            session_id: self.row.session_id.clone(),
            status: self.row.status.clone(),
            last_sequence: self.row.through_sequence,
        }
    }
}

/// Where the turn's records go and what the journal already holds, shared between the
/// actor and the services the loop calls.
struct Cursor {
    through_sequence: u64,
    /// The transcript as last recorded, so the next delta is against it.
    transcript: Vec<Message>,
    kv: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone)]
pub struct TurnHost {
    active: Arc<RwLock<bool>>,
    tools: Arc<crate::ToolGroup>,
    emissions: Arc<crate::tool::EmissionBudget>,
    origin: brain_protocol::EventOrigin,
    session_id: brain_protocol::SessionId,
    store: Arc<dyn SessionStore>,
    runtime: Arc<SessionRuntime>,
    config: Arc<SessionConfig>,
    cancel_requested: Arc<AtomicBool>,
    model_calls: Arc<AtomicUsize>,
    cursor: Arc<Mutex<Cursor>>,
}

impl TurnHost {
    async fn admit(&self) -> Result<RwLockReadGuard<'_, bool>, Error> {
        let active = self.active.read().await;
        if !*active {
            return Err(Error::InvalidState("Agentloop activation is closed".into()));
        }
        self.check_cancelled()?;
        Ok(active)
    }

    fn check_cancelled(&self) -> Result<(), Error> {
        if self.cancel_requested.load(Ordering::Acquire) {
            return Err(Error::Cancelled("turn cancelled".into()));
        }
        Ok(())
    }

    fn model_service(&self, origin: brain_protocol::EventOrigin) -> super::model::ModelService {
        super::model::ModelService {
            session_id: self.session_id.clone(),
            config: self.config.clone(),
            runtime: self.runtime.clone(),
            store: self.store.clone(),
            calls: self.model_calls.clone(),
            origin,
        }
    }

    async fn append(
        &self,
        cursor: &mut Cursor,
        records: Vec<AppendRecord>,
    ) -> Result<Vec<SessionRecord>, Error> {
        let saved = append_records(self.store.clone(), records, None).await?;
        if let Some(last) = saved.last() {
            cursor.through_sequence = last.sequence;
        }
        Ok(saved)
    }
}

#[async_trait::async_trait]
impl TurnServices for TurnHost {
    async fn acknowledge(&self, sequence: u64) -> Result<u64, Error> {
        let _active = self.admit().await?;
        let mut cursor = self.cursor.lock().await;
        let processed = cursor
            .kv
            .get(LAST_ACTIVATION_KEY)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if sequence < processed || sequence > self.store.session_row()?.through_sequence {
            return Err(Error::InvalidState(
                "processed sequence must advance within committed history".into(),
            ));
        }
        if sequence > processed {
            cursor.through_sequence = append_journal(
                self.store.clone(),
                vec![JournalEntry::KvSet {
                    key: LAST_ACTIVATION_KEY.into(),
                    value: serde_json::json!(sequence),
                }],
            )
            .await?;
            cursor
                .kv
                .insert(LAST_ACTIVATION_KEY.into(), serde_json::json!(sequence));
        }
        Ok(cursor.through_sequence)
    }

    async fn set_transcript(&self, messages: Vec<Message>) -> Result<u64, Error> {
        let _active = self.admit().await?;
        let mut cursor = self.cursor.lock().await;
        self.check_cancelled()?;
        if let Some(entry) = delta(&cursor.transcript, &messages) {
            cursor.through_sequence = append_journal(self.store.clone(), vec![entry]).await?;
            cursor.transcript = messages;
        }
        Ok(cursor.through_sequence)
    }

    async fn kv_put(&self, request: brain_protocol::KvPutRequest) -> Result<u64, Error> {
        let _active = self.admit().await?;
        validate_kv_key(&request.key)?;
        let mut cursor = self.cursor.lock().await;
        self.check_cancelled()?;
        if cursor.kv.get(&request.key) != Some(&request.value) {
            let entry = JournalEntry::KvSet {
                key: request.key.clone(),
                value: request.value.clone(),
            };
            cursor.through_sequence = append_journal(self.store.clone(), vec![entry]).await?;
            cursor.kv.insert(request.key, request.value);
        }
        Ok(cursor.through_sequence)
    }

    async fn kv_read(&self, key: String) -> Result<Option<serde_json::Value>, Error> {
        let _active = self.admit().await?;
        validate_kv_key(&key)?;
        let cursor = self.cursor.lock().await;
        self.check_cancelled()?;
        Ok(cursor.kv.get(&key).cloned())
    }

    async fn kv_delete(&self, key: String) -> Result<u64, Error> {
        let _active = self.admit().await?;
        validate_kv_key(&key)?;
        let mut cursor = self.cursor.lock().await;
        self.check_cancelled()?;
        if cursor.kv.contains_key(&key) {
            cursor.through_sequence = append_journal(
                self.store.clone(),
                vec![JournalEntry::KvDelete { key: key.clone() }],
            )
            .await?;
            cursor.kv.remove(&key);
        }
        Ok(cursor.through_sequence)
    }

    async fn events(&self, after: u64) -> Result<brain_protocol::EventPage, Error> {
        let _active = self.admit().await?;
        self.check_cancelled()?;
        let store = self.store.clone();
        let records =
            tokio::task::spawn_blocking(move || store.records_after(after, EVENTS_PER_TURN))
                .await
                .map_err(|error| Error::Journal(error.to_string()))??;
        Ok(crate::event_page(records, after))
    }

    async fn model(&self, mut request: ModelRequest) -> Result<ModelResult, Error> {
        let _active = self.admit().await?;
        self.check_cancelled()?;
        let model = self.model_service(self.origin.clone());
        let tools = model.prepare(&mut request)?;
        // Auxiliary model views are auditable without replacing conversation state.
        let sequence = {
            let mut cursor = self.cursor.lock().await;
            let context = delta(&cursor.transcript, &request.messages).unwrap_or(
                JournalEntry::TranscriptDelta {
                    keep: request.messages.len() as u64,
                    append: Vec::new(),
                },
            );
            let through_sequence = cursor.through_sequence;
            let saved = self
                .append(
                    &mut cursor,
                    vec![AppendRecord::new(
                        codes::event::MODEL_CALL_STARTED,
                        serde_json::json!({
                            "system": request.system,
                            "tools": request.tools,
                            "messages": request.messages.len(),
                            "context": {"through_sequence": through_sequence, "delta": context},
                            "response_format": request.response_format,
                            "max_output_tokens": request.max_output_tokens,
                            "options": request.options,
                        }),
                    )],
                )
                .await?;
            saved[0].sequence
        };
        let cancelled = async {
            while !self.cancel_requested.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        let result = model.execute(sequence, request, tools, cancelled).await;
        let mut cursor = self.cursor.lock().await;
        cursor.through_sequence = self.store.session_row()?.through_sequence;
        result
    }

    async fn dispatch(&self, calls: Vec<ToolInvocation>) -> Result<Vec<ToolReturn>, Error> {
        let _active = self.admit().await?;
        self.check_cancelled()?;
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = HashSet::with_capacity(calls.len());
        for call in &calls {
            if !seen.insert(call.call_id.as_str()) {
                return Err(Error::InvalidState(format!(
                    "Tool call id `{}` repeats within one dispatch",
                    call.call_id
                )));
            }
        }
        let dispatches = {
            let mut cursor = self.cursor.lock().await;
            let mut dispatches = Vec::with_capacity(calls.len());
            let mut started = Vec::with_capacity(calls.len());
            for invocation in calls {
                let tool = self.config.tool(&invocation.name).cloned().ok_or_else(|| {
                    Error::InvalidState(format!(
                        "Tool `{}` is not one the session was created with",
                        invocation.name
                    ))
                })?;
                let environment = self
                    .config
                    .environment(&invocation.environment)
                    .cloned()
                    .ok_or_else(|| {
                        Error::InvalidState(format!(
                            "Tool `{}` names Environment `{}`, which this session does not have",
                            tool.name, invocation.environment
                        ))
                    })?;
                let placement = tool
                    .placements
                    .get(&invocation.environment)
                    .cloned()
                    .ok_or_else(|| {
                        Error::InvalidState(format!(
                            "Tool `{}` is not authorized in Environment `{}`",
                            tool.name, invocation.environment
                        ))
                    })?;
                let dispatch = ToolDispatch {
                    sequence: 0,
                    session_id: self.session_id.clone(),
                    tool,
                    placement,
                    environment,
                    invocation,
                    deadline_ms: self.runtime.limits.tool_deadline_ms(),
                };
                // A reference, not a copy: the Tool and its Environment live once, in
                // the configuration recorded at creation.
                started.push(AppendRecord::new(
                    codes::event::TOOL_CALL_STARTED,
                    serde_json::json!({
                        "tool": &dispatch.tool.name,
                        "environment": &dispatch.environment.name,
                        "invocation": &dispatch.invocation,
                        "deadline_ms": dispatch.deadline_ms,
                    }),
                ));
                dispatches.push(dispatch);
            }
            let saved = self.append(&mut cursor, started).await?;
            for (dispatch, record) in dispatches.iter_mut().zip(saved) {
                dispatch.sequence = record.sequence;
            }
            dispatches
        };
        let waits = dispatches
            .into_iter()
            .map(|dispatch| {
                let model = self.model_service(brain_protocol::EventOrigin::Tool {
                    sequence: dispatch.sequence,
                });
                self.tools.start(
                    dispatch,
                    self.store.clone(),
                    self.runtime.tool_executor.clone(),
                    self.emissions.clone(),
                    self.runtime.telemetry.clone(),
                    model,
                )
            })
            .collect::<Vec<_>>();
        let results = join_all(waits)
            .await
            .into_iter()
            .map(|result| {
                result
                    .map_err(|_| Error::Executor("Tool execution stopped before returning".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.check_cancelled()?;
        Ok(results)
    }

    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error> {
        let _active = self.admit().await?;
        self.check_cancelled()?;
        if kind == "_extension_event"
            || !valid_kind(&kind)
            || JournalEntry::is_kind(&kind)
            || (codes::event::ALL.contains(&kind.as_str()) && kind != codes::event::OUTPUT_EMITTED)
        {
            return Err(Error::InvalidState(format!(
                "record kind `{kind}` is Brain's own; a loop may not append it"
            )));
        }
        let bytes = kind
            .len()
            .checked_add(serde_json::to_vec(&payload).map_err(json_error)?.len())
            .ok_or_else(|| Error::EmitLimit("emitted Event size overflowed".into()))?;
        let mut cursor = self.cursor.lock().await;
        self.emissions.reserve(bytes)?;
        let mut record = AppendRecord::new(kind, payload);
        record.origin = Some(self.origin.clone());
        let saved = TurnHost::append(self, &mut cursor, vec![record]).await?;
        Ok(saved[0].sequence)
    }

    fn cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::Acquire)
    }

    fn telemetry(&self, record: serde_json::Value) {
        self.publish_telemetry(
            brain_telemetry::TelemetryKind::Event,
            "agentloop_telemetry",
            record,
        );
    }
}

impl TurnHost {
    fn publish_telemetry(
        &self,
        kind: brain_telemetry::TelemetryKind,
        name: &str,
        record: serde_json::Value,
    ) {
        let payload = serde_json::to_vec(&record).unwrap_or_default();
        let _ = self
            .runtime
            .telemetry
            .try_publish(brain_telemetry::TelemetryRecord {
                kind,
                name: name.into(),
                payload,
                session_id: Some(self.session_id.clone()),
                sequence: None,
            });
    }
}

async fn append_records(
    store: Arc<dyn SessionStore>,
    records: Vec<AppendRecord>,
    status: Option<SessionStatus>,
) -> Result<Vec<SessionRecord>, Error> {
    tokio::task::spawn_blocking(move || {
        store.append_sync(
            &records,
            SessionUpdate {
                status,
                configuration: None,
            },
        )
    })
    .await
    .map_err(|error| Error::Journal(format!("journal commit task failed: {error}")))?
}

async fn append_journal(
    store: Arc<dyn SessionStore>,
    entries: Vec<JournalEntry>,
) -> Result<u64, Error> {
    tokio::task::spawn_blocking(move || store.append_journal_sync(&entries))
        .await
        .map_err(|error| Error::Journal(format!("journal commit task failed: {error}")))?
}

/// The journal entry that takes `recorded` to `wanted`: keep the longest shared prefix,
/// append the rest. `None` when nothing changed. A change deep in the transcript
/// rewrites the tail from there; rare, because it breaks prompt cache anyway.
pub(crate) fn delta(recorded: &[Message], wanted: &[Message]) -> Option<JournalEntry> {
    let keep = recorded
        .iter()
        .zip(wanted)
        .take_while(|(left, right)| left == right)
        .count();
    if keep == recorded.len() && keep == wanted.len() {
        return None;
    }
    Some(JournalEntry::TranscriptDelta {
        keep: keep as u64,
        append: wanted[keep..].to_vec(),
    })
}

fn validate_kv_key(key: &str) -> Result<(), Error> {
    if !valid_kind(key) || key == LAST_ACTIVATION_KEY {
        return Err(Error::InvalidState(
            "kv key must be an identifier not reserved by Brain".into(),
        ));
    }
    Ok(())
}

pub(crate) fn valid_kind(kind: &str) -> bool {
    let bytes = kind.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

/// Model output as the live feed carries it: only what a client watching the turn can
/// use, keyed to the `model_call_started` record it belongs to.
pub(super) fn streaming_event(sequence: u64, event: &ModelStreamEvent) -> Option<StreamingEvent> {
    let (event_type, data) = match event {
        ModelStreamEvent::Request {
            input_bytes,
            media_inputs,
        } => (
            "model_request",
            serde_json::json!({"input_bytes": input_bytes, "media_inputs": media_inputs}),
        ),
        ModelStreamEvent::Usage { usage } | ModelStreamEvent::MessageDone { usage, .. } => {
            ("model_usage", serde_json::json!({"usage": usage}))
        }
        ModelStreamEvent::NativeDelta { text, .. } => (
            "model_output",
            serde_json::json!({"output_bytes": text.len()}),
        ),
        ModelStreamEvent::TextDelta { index, text } => (
            "assistant_delta",
            serde_json::json!({"index": index, "text": text}),
        ),
        ModelStreamEvent::RefusalDelta { index, text } => (
            "refusal_delta",
            serde_json::json!({"index": index, "text": text}),
        ),
        ModelStreamEvent::ToolUseStart { index, id, name } => (
            "tool_call_delta",
            serde_json::json!({"index": index, "id": id, "name": name}),
        ),
        ModelStreamEvent::ToolInputDelta {
            index,
            partial_json,
        } => (
            "tool_call_delta",
            serde_json::json!({"index": index, "partial_json": partial_json}),
        ),
        ModelStreamEvent::NativeStart { .. } | ModelStreamEvent::BlockDone { .. } => return None,
    };
    Some(StreamingEvent {
        sequence,
        event_type: event_type.into(),
        data,
    })
}

/// The failure a runtime error records: its API code, its message, and whether the
/// effect may have happened anyway.
pub(crate) fn failure_of(error: &Error) -> Failure {
    Failure::new(error.code(), error.to_string())
        .retryable(error.retryable())
        .ambiguous(matches!(error, Error::Ambiguous(_)))
}

/// The one shape every `*_failed` record carries; `sequence` names the effect record
/// when there is one.
pub(crate) fn failure_payload(
    sequence: Option<u64>,
    failure: &Failure,
) -> Result<serde_json::Value, Error> {
    let mut payload = serde_json::to_value(failure).map_err(json_error)?;
    if let (Some(sequence), Some(object)) = (sequence, payload.as_object_mut()) {
        object.insert("sequence".into(), serde_json::json!(sequence));
    }
    Ok(payload)
}

pub(crate) fn json_error(error: serde_json::Error) -> Error {
    Error::InvalidState(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Message {
        Message::user_text(text)
    }

    #[test]
    fn a_delta_keeps_the_shared_prefix() {
        let recorded = vec![user("a"), user("b"), user("c")];
        let wanted = vec![user("a"), user("b"), user("d"), user("e")];
        assert_eq!(
            delta(&recorded, &wanted),
            Some(JournalEntry::TranscriptDelta {
                keep: 2,
                append: vec![user("d"), user("e")],
            })
        );
        assert_eq!(delta(&recorded, &recorded), None);
        assert_eq!(
            delta(&recorded, &[]),
            Some(JournalEntry::TranscriptDelta {
                keep: 0,
                append: Vec::new()
            })
        );
    }

    #[test]
    fn emitted_events_have_an_aggregate_byte_limit() {
        let budget = crate::tool::EmissionBudget::new(1024);
        budget.reserve(1023).unwrap();
        budget.reserve(1).unwrap();
        assert_eq!(
            budget.reserve(1).unwrap_err().code(),
            codes::failure::EMIT_LIMIT
        );
    }
}
