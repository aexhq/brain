//! The task behind a session: one turn at a time, with the agentloop in charge of the
//! turn and Brain in charge of everything the turn does.
//!
//! A turn is one call into the loop. While it runs, the loop reaches Brain through
//! [`TurnHost`]: model calls, tool dispatch, its own records, telemetry. Each service
//! journals before it acts, so the feed says what happened whether or not the loop
//! comes back. When it does come back, the transcript and slots it hands over are
//! diffed against what the journal already holds and only the difference is written.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use brain_protocol::{
    Event, EventId, LiveEvent, Message, MessageRequest, ModelRequest, ModelResult,
    ModelStreamEvent, Outcome, RuntimeEnvelope, SessionConfig, SessionStatus, SessionSummary,
    StreamingEvent, ToolCancellation, ToolDefinition, ToolDispatch, ToolInvocation, ToolResult,
    TurnInput, TurnOutput,
    codes::{self, Failure},
};
use futures_util::future::join_all;
use tokio::sync::{Mutex, mpsc, oneshot};

use super::{SessionRuntime, TurnServices};
use crate::{
    Error, ToolServices,
    journal::{
        AppendRecord, Folded, JournalEntry, SessionRecord, SessionRow, SessionStore, SessionUpdate,
    },
};

/// The slot Brain keeps for itself: the sequence of the loop's last activation, so the
/// next turn can hand it every record since.
pub const LAST_ACTIVATION_SLOT: &str = "brain.last_activation";

/// Records handed to a loop as "what happened since you last ran". More than this and
/// the loop reads the feed itself.
const EVENTS_PER_TURN: usize = 1_000;

const MAX_EMITS_PER_TURN: usize = 128;
const MAX_EMITTED_BYTES_PER_TURN: usize = 1024 * 1024;

pub enum SessionCommand {
    Message {
        request: MessageRequest,
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
    /// The transcript and slots as the journal holds them.
    folded: Folded,
}

impl SessionActor {
    pub fn new(
        mut row: SessionRow,
        store: Arc<dyn SessionStore>,
        runtime: Arc<SessionRuntime>,
        receiver: mpsc::Receiver<SessionCommand>,
        cancel_requested: Arc<AtomicBool>,
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
        })
    }

    pub async fn run(mut self) {
        while let Some(command) = self.receiver.recv().await {
            match command {
                SessionCommand::Message { request, reply } => {
                    let result = self.turn(request).await;
                    let _ = reply.send(result);
                }
                SessionCommand::Cancel => {
                    self.cancel_requested.store(true, Ordering::Release);
                }
                SessionCommand::End { reply } => {
                    let _ = reply.send(self.end().await);
                }
                SessionCommand::Append { record, reply } => {
                    let _ = reply.send(self.append_between_turns(record).await);
                }
            }
        }
    }

    async fn turn(&mut self, request: MessageRequest) -> Result<SessionSummary, Error> {
        if !matches!(self.row.status, SessionStatus::Idle) {
            return Err(Error::InvalidState("session is not idle".into()));
        }
        self.cancel_requested.store(false, Ordering::Release);
        self.commit(
            vec![AppendRecord::new(
                codes::event::TURN_STARTED,
                serde_json::to_value(&request).map_err(json_error)?,
            )],
            Some(SessionStatus::Running),
        )
        .await?;
        let since = self
            .folded
            .slots
            .get(LAST_ACTIVATION_SLOT)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let event_records = self.store.records_after(since, EVENTS_PER_TURN)?;
        let events_through = event_records.last().map_or(since, |record| record.sequence);
        let events = event_records.into_iter().map(event_of).collect();
        let input = TurnInput {
            input: request.input,
            transcript: self.folded.transcript.clone(),
            slots: self.folded.slots.clone(),
            events,
            configuration: self.config.brain_configuration.clone(),
            system: self.config.system.clone(),
            tools: self.config.tools.clone(),
            runtime: RuntimeEnvelope::at(&self.row.session_id, self.row.through_sequence),
        };
        self.commit(
            vec![AppendRecord::new(
                codes::event::ACTIVATION_STARTED,
                serde_json::json!({"since": since}),
            )],
            None,
        )
        .await?;
        let host = Arc::new(TurnHost {
            session_id: self.row.session_id.clone(),
            store: self.store.clone(),
            runtime: self.runtime.clone(),
            config: self.config.clone(),
            cancel_requested: self.cancel_requested.clone(),
            model_calls: AtomicUsize::new(0),
            cursor: Mutex::new(Cursor {
                through_sequence: self.row.through_sequence,
                transcript: std::mem::take(&mut self.folded.transcript),
                emitted: 0,
                emitted_bytes: 0,
            }),
        });
        let agentloop_environment = self
            .config
            .environments
            .iter()
            .find(|environment| environment.environment_id == self.config.agentloop_environment_id)
            .map(|environment| environment.configuration.clone())
            .ok_or_else(|| Error::InvalidState("Agentloop Environment is missing".into()))?;
        let running = self.runtime.loop_executor.turn(
            &self.row.session_id,
            &self.config.agentloop_identity,
            agentloop_environment,
            input,
            host.clone(),
        );
        let outcome = if self.runtime.max_turn_ms == 0 {
            running.await
        } else {
            match tokio::time::timeout(Duration::from_millis(self.runtime.max_turn_ms), running)
                .await
            {
                Ok(outcome) => outcome,
                Err(_) => {
                    // The loop is told through its next host call; whatever it was
                    // doing in between is bounded by the executor's own guest budget.
                    self.cancel_requested.store(true, Ordering::Release);
                    Err(Error::Cancelled(
                        "turn exceeded its wall-time budget".into(),
                    ))
                }
            }
        };
        // Whatever the loop did, the host's cursor is the truth about what reached the
        // journal.
        let cursor = {
            let mut cursor = host.cursor.lock().await;
            Cursor {
                through_sequence: cursor.through_sequence,
                transcript: std::mem::take(&mut cursor.transcript),
                emitted: cursor.emitted,
                emitted_bytes: cursor.emitted_bytes,
            }
        };
        self.row.through_sequence = cursor.through_sequence;
        self.folded.transcript = cursor.transcript;
        // A turn that was cancelled does not get to finish, whatever the loop brought
        // back: what the services already recorded stands, the rest is discarded.
        let outcome = match outcome {
            Ok(_) if self.cancel_requested.load(Ordering::Acquire) => {
                Err(Error::Cancelled("turn cancelled".into()))
            }
            outcome => outcome,
        };
        match outcome {
            Ok(output) => self.finish_turn(output, events_through).await,
            Err(error) => {
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

    /// The loop came back: keep what it handed over, then close the turn.
    async fn finish_turn(
        &mut self,
        output: TurnOutput,
        events_through: u64,
    ) -> Result<SessionSummary, Error> {
        if output.transcript.len() > brain_protocol::MAX_TRANSCRIPT_ITEMS {
            let failure = Failure::new(
                codes::failure::INVALID_TRANSCRIPT,
                format!(
                    "Agentloop returned a transcript of {} items; the most is {}",
                    output.transcript.len(),
                    brain_protocol::MAX_TRANSCRIPT_ITEMS
                ),
            );
            return self
                .close_turn(vec![
                    AppendRecord::new(
                        codes::event::ACTIVATION_FAILED,
                        failure_payload(None, &failure)?,
                    ),
                    AppendRecord::new(codes::event::TURN_FAILED, failure_payload(None, &failure)?),
                ])
                .await;
        }
        let mut entries = Vec::new();
        if let Some(delta) = delta(&self.folded.transcript, &output.transcript) {
            entries.push(delta);
        }
        for (name, value) in output.slots {
            if name == LAST_ACTIVATION_SLOT {
                continue;
            }
            if self.folded.slots.get(&name) != Some(&value) {
                entries.push(JournalEntry::StateSet {
                    name: name.clone(),
                    value: value.clone(),
                });
                self.folded.slots.insert(name, value);
            }
        }
        self.folded.transcript = output.transcript;
        self.row.through_sequence = append_journal(self.store.clone(), entries).await?;
        self.commit(
            vec![AppendRecord::new(
                codes::event::ACTIVATION_ENDED,
                serde_json::json!({}),
            )],
            None,
        )
        .await?;
        self.folded.slots.insert(
            LAST_ACTIVATION_SLOT.into(),
            serde_json::json!(events_through),
        );
        let entries = vec![JournalEntry::StateSet {
            name: LAST_ACTIVATION_SLOT.into(),
            value: serde_json::json!(events_through),
        }];
        self.row.through_sequence = append_journal(self.store.clone(), entries).await?;
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
        if !matches!(self.row.status, SessionStatus::Idle) {
            return Err(Error::InvalidState("session is not idle".into()));
        }
        self.commit(vec![record], None).await?;
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
    emitted: usize,
    emitted_bytes: usize,
}

impl Cursor {
    fn reserve_emit(&mut self, bytes: usize) -> Result<(), Error> {
        if self.emitted >= MAX_EMITS_PER_TURN {
            return Err(Error::EmitLimit(format!(
                "turn exceeded its limit of {MAX_EMITS_PER_TURN} emitted Events"
            )));
        }
        if self.emitted_bytes.saturating_add(bytes) > MAX_EMITTED_BYTES_PER_TURN {
            return Err(Error::EmitLimit(format!(
                "turn exceeded its limit of {MAX_EMITTED_BYTES_PER_TURN} emitted Event bytes"
            )));
        }
        self.emitted += 1;
        self.emitted_bytes += bytes;
        Ok(())
    }
}

/// Brain's side of a running turn.
pub struct TurnHost {
    session_id: brain_protocol::SessionId,
    store: Arc<dyn SessionStore>,
    runtime: Arc<SessionRuntime>,
    config: Arc<SessionConfig>,
    cancel_requested: Arc<AtomicBool>,
    model_calls: AtomicUsize,
    cursor: Mutex<Cursor>,
}

impl TurnHost {
    fn check_cancelled(&self) -> Result<(), Error> {
        if self.cancel_requested.load(Ordering::Acquire) {
            return Err(Error::Cancelled("turn cancelled".into()));
        }
        Ok(())
    }

    /// The definitions of the tools a model request offers, in the request's order.
    ///
    /// The loop chooses by name from what the session was created with: a name outside
    /// that set would be a tool nothing admitted and nothing can dispatch, and a repeated
    /// name would offer the model the same tool twice. Both fail the call.
    fn offered_tools(&self, request: &ModelRequest) -> Result<Vec<ToolDefinition>, Error> {
        let Some(names) = &request.tools else {
            return Ok(self.config.tools.clone());
        };
        let mut seen = HashSet::with_capacity(names.len());
        names
            .iter()
            .map(|name| {
                if !seen.insert(name.as_str()) {
                    return Err(Error::InvalidState(format!(
                        "model request offers Tool `{name}` twice"
                    )));
                }
                self.config
                    .tools
                    .iter()
                    .find(|tool| &tool.name == name)
                    .cloned()
                    .ok_or_else(|| {
                        Error::InvalidState(format!(
                            "model request offers Tool `{name}`, which the session was not created with"
                        ))
                    })
            })
            .collect()
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
    async fn model(&self, mut request: ModelRequest) -> Result<ModelResult, Error> {
        self.check_cancelled()?;
        let calls = self.model_calls.fetch_add(1, Ordering::AcqRel) + 1;
        if calls > self.runtime.max_model_calls_per_turn {
            return Err(Error::Budget(format!(
                "turn exceeded its budget of {} model calls",
                self.runtime.max_model_calls_per_turn
            )));
        }
        // What the loop left unsaid is what the session was created with.
        if request.system.is_none() {
            request.system = Some(self.config.system.clone());
        }
        if request.tools.is_none() {
            request.tools = Some(
                self.config
                    .tools
                    .iter()
                    .map(|tool| tool.name.clone())
                    .collect(),
            );
        }
        if request.response_format.is_none() {
            request.response_format = self.config.response_format.clone();
        }
        if request
            .system
            .as_ref()
            .is_some_and(|system| system.len() > 131_072)
        {
            return Err(Error::InvalidState(
                "model request system prompt exceeds 128 KiB".into(),
            ));
        }
        if request.messages.is_empty()
            || request.messages.len() > brain_protocol::MAX_TRANSCRIPT_ITEMS
        {
            return Err(Error::InvalidState(format!(
                "model request must carry 1..={} messages",
                brain_protocol::MAX_TRANSCRIPT_ITEMS
            )));
        }
        let tools = self.offered_tools(&request)?;
        // The messages are the transcript as the loop wants the model to see it; the
        // journal keeps how they differ from what it last recorded, then the call.
        let sequence = {
            let mut cursor = self.cursor.lock().await;
            if let Some(entry) = delta(&cursor.transcript, &request.messages) {
                cursor.through_sequence = append_journal(self.store.clone(), vec![entry]).await?;
                cursor.transcript = request.messages.clone();
            }
            let saved = self
                .append(
                    &mut cursor,
                    vec![AppendRecord::new(
                        codes::event::MODEL_CALL_STARTED,
                        serde_json::json!({
                            "system": request.system,
                            "tools": request.tools,
                            "messages": request.messages.len(),
                            "response_format": request.response_format,
                            "max_output_tokens": request.max_output_tokens,
                        }),
                    )],
                )
                .await?;
            saved[0].sequence
        };
        // Model output is streamed and not stored: the assembled response is the durable
        // truth, and a client that wants the pieces takes them off the stream.
        let live = self.runtime.live.clone();
        let live_session = self.session_id.clone();
        let cancel = self.cancel_requested.clone();
        let mut on_event = move |event: ModelStreamEvent| {
            if let Some(streaming) = streaming_event(sequence, &event) {
                let _ = live.send((live_session.clone(), LiveEvent::Streaming(streaming)));
            }
        };
        let call =
            self.runtime
                .model_executor
                .execute(&self.config.model, request, &tools, &mut on_event);
        // A cancellation ends the wait, not the provider's work: the stream is dropped and
        // the record says the outcome is unknown.
        let cancelled = async {
            while !cancel.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        let result = tokio::select! {
            result = call => result,
            () = cancelled => Err(Error::Cancelled("turn cancelled during a model call".into())),
        };
        let mut cursor = self.cursor.lock().await;
        match result {
            Ok(result) => {
                self.append(
                    &mut cursor,
                    vec![AppendRecord::new(
                        codes::event::MODEL_CALL_ENDED,
                        serde_json::json!({"sequence": sequence, "result": result}),
                    )],
                )
                .await?;
                Ok(result)
            }
            Err(error) => {
                self.append(
                    &mut cursor,
                    vec![AppendRecord::new(
                        codes::event::MODEL_CALL_FAILED,
                        failure_payload(Some(sequence), &failure_of(&error))?,
                    )],
                )
                .await?;
                Err(error)
            }
        }
    }

    async fn dispatch(&self, calls: Vec<ToolInvocation>) -> Result<Vec<ToolResult>, Error> {
        self.check_cancelled()?;
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        if calls.len() > 128 {
            return Err(Error::InvalidState(
                "a dispatch carries at most 128 Tool calls".into(),
            ));
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
                let binding = self
                    .config
                    .tool_bindings
                    .iter()
                    .find(|binding| binding.name == invocation.name)
                    .cloned()
                    .ok_or_else(|| {
                        Error::InvalidState(format!("unbound Tool `{}`", invocation.name))
                    })?;
                let dispatch = ToolDispatch {
                    sequence: 0,
                    session_id: self.session_id.clone(),
                    binding,
                    invocation,
                    deadline_ms: self.runtime.tool_deadline_ms,
                };
                started.push(AppendRecord::new(
                    codes::event::TOOL_CALL_STARTED,
                    serde_json::json!({
                        "binding": &dispatch.binding,
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
        let futures = dispatches
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, dispatch)| {
                let executor = self.runtime.tool_executor.clone();
                let cancel = self.cancel_requested.clone();
                async move {
                    let sequence = dispatch.sequence;
                    let call_id = dispatch.invocation.call_id.clone();
                    let deadline = Duration::from_millis(dispatch.deadline_ms);
                    // The deadline is enforced here, on the calling side: the remote
                    // cannot be trusted to, so an overdue call is dropped and recorded
                    // as its own distinguished outcome. A cancellation ends the wait the
                    // same way.
                    let call = executor.execute(dispatch, self);
                    let cancelled = async {
                        while !cancel.load(Ordering::Acquire) {
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    };
                    let result = tokio::select! {
                        outcome = tokio::time::timeout(deadline, call) => match outcome {
                            Ok(result) => result.map(|outcome| (outcome, false)),
                            Err(_) => Ok((Outcome::Unknown {
                                message: "Tool deadline elapsed after the call was sent".into(),
                            }, true)),
                        },
                        () = cancelled => Ok((Outcome::Unknown {
                            message: "Tool cancellation was requested after the call was sent".into(),
                        }, true)),
                    };
                    (index, sequence, call_id, result)
                }
            });
        let completed = join_all(futures).await;
        let mut terminal = Vec::with_capacity(completed.len());
        let mut results = Vec::with_capacity(completed.len());
        let mut abandoned = Vec::new();
        for (index, sequence, call_id, result) in completed {
            let result = match result {
                Ok((outcome, dropped)) => {
                    if dropped {
                        abandoned.push(dispatches[index].clone());
                    }
                    ToolResult::from_outcome(call_id, outcome)
                }
                Err(Error::Ambiguous(message)) => {
                    ToolResult::from_outcome(call_id, Outcome::Unknown { message })
                }
                Err(error) => ToolResult {
                    call_id,
                    output: serde_json::to_value(Failure::new(
                        codes::failure::TOOL_ERROR,
                        error.to_string(),
                    ))
                    .map_err(json_error)?,
                    is_error: true,
                },
            };
            terminal.push(AppendRecord::new(
                codes::event::TOOL_CALL_ENDED,
                serde_json::json!({"sequence": sequence, "result": result}),
            ));
            results.push(result);
        }
        {
            let mut cursor = self.cursor.lock().await;
            self.append(&mut cursor, terminal).await?;
        }
        // A call abandoned locally is told to stop where it runs, best effort like every
        // cancellation.
        if !abandoned.is_empty() {
            self.cancel_tools(&abandoned).await?;
        }
        // The results are recorded either way; a cancelled turn hears it here rather
        // than on its next call.
        self.check_cancelled()?;
        Ok(results)
    }

    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error> {
        self.check_cancelled()?;
        if !valid_kind(&kind)
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
        cursor.reserve_emit(bytes)?;
        let saved =
            TurnHost::append(self, &mut cursor, vec![AppendRecord::new(kind, payload)]).await?;
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

#[async_trait::async_trait]
impl ToolServices for TurnHost {
    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error> {
        TurnServices::emit(self, kind, payload).await
    }

    fn telemetry(&self, record: serde_json::Value) {
        self.publish_telemetry(
            brain_telemetry::TelemetryKind::Log,
            "tool_telemetry",
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
                event_id: None,
            });
    }

    async fn cancel_tools(&self, dispatches: &[ToolDispatch]) -> Result<(), Error> {
        let (cancellations, sequences) = {
            let mut cursor = self.cursor.lock().await;
            let mut cancellations = Vec::with_capacity(dispatches.len());
            let mut started = Vec::with_capacity(dispatches.len());
            for dispatch in dispatches {
                let cancellation = ToolCancellation {
                    sequence: 0,
                    target_sequence: dispatch.sequence,
                    session_id: dispatch.session_id.clone(),
                    binding: dispatch.binding.clone(),
                };
                started.push(AppendRecord::new(
                    codes::event::TOOL_CANCEL_STARTED,
                    serde_json::json!({
                        "target_sequence": cancellation.target_sequence,
                        "binding": &cancellation.binding,
                    }),
                ));
                cancellations.push(cancellation);
            }
            let saved = self.append(&mut cursor, started).await?;
            for (cancellation, record) in cancellations.iter_mut().zip(saved) {
                cancellation.sequence = record.sequence;
            }
            let sequences: Vec<u64> = cancellations.iter().map(|c| c.sequence).collect();
            (cancellations, sequences)
        };
        let futures = cancellations.into_iter().map(|cancellation| {
            let executor = self.runtime.tool_executor.clone();
            async move {
                let sequence = cancellation.sequence;
                let result = executor.cancel(cancellation).await;
                (sequence, result)
            }
        });
        let results = tokio::time::timeout(Duration::from_secs(5), join_all(futures)).await;
        let records = match results {
            Ok(results) => results
                .into_iter()
                .map(|(sequence, result)| match result {
                    Ok(()) => Ok(AppendRecord::new(
                        codes::event::TOOL_CANCEL_ENDED,
                        serde_json::json!({"sequence": sequence}),
                    )),
                    Err(error) => Ok(AppendRecord::new(
                        codes::event::TOOL_CANCEL_FAILED,
                        failure_payload(Some(sequence), &failure_of(&error).ambiguous(false))?,
                    )),
                })
                .collect::<Result<Vec<_>, Error>>()?,
            // Nothing answered in time: whether the environment stopped the work is not
            // known, and the record says so.
            Err(_) => sequences
                .into_iter()
                .map(|sequence| {
                    Ok(AppendRecord::new(
                        codes::event::TOOL_CANCEL_FAILED,
                        failure_payload(
                            Some(sequence),
                            &Failure::new(
                                codes::failure::TIMEOUT,
                                "Environment cancellation deadline exceeded",
                            )
                            .ambiguous(true),
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, Error>>()?,
        };
        let mut cursor = self.cursor.lock().await;
        self.append(&mut cursor, records).await?;
        Ok(())
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

fn event_of(record: SessionRecord) -> Event {
    Event {
        event_id: EventId::new(format!("evt_{}_{}", record.session_id, record.sequence)),
        sequence: record.sequence,
        recorded_at_ms: record.recorded_at_ms,
        event_type: record.kind,
        data: record.payload,
    }
}

fn valid_kind(kind: &str) -> bool {
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
fn streaming_event(sequence: u64, event: &ModelStreamEvent) -> Option<StreamingEvent> {
    let (event_type, data) = match event {
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
        ModelStreamEvent::BlockDone { .. }
        | ModelStreamEvent::Usage { .. }
        | ModelStreamEvent::MessageDone { .. } => return None,
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
    fn emitted_events_have_count_and_aggregate_byte_limits() {
        let mut cursor = Cursor {
            through_sequence: 0,
            transcript: Vec::new(),
            emitted: MAX_EMITS_PER_TURN - 1,
            emitted_bytes: 0,
        };
        cursor.reserve_emit(1).unwrap();
        assert_eq!(
            cursor.reserve_emit(1).unwrap_err().code(),
            codes::failure::EMIT_LIMIT
        );

        cursor.emitted = 0;
        cursor.emitted_bytes = MAX_EMITTED_BYTES_PER_TURN - 1;
        cursor.reserve_emit(1).unwrap();
        assert_eq!(
            cursor.reserve_emit(1).unwrap_err().code(),
            codes::failure::EMIT_LIMIT
        );
    }
}
