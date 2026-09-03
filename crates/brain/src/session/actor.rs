use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{future::Future, time::Duration};

use brain_protocol::codes::{self, Failure};
use brain_protocol::{
    ActivationInput, ContextEnvelope, Decision, Event, EventId, LiveEvent, MessageRequest,
    ModelRequest, ModelStreamEvent, Observation, Outcome, RuntimeEnvelope, SealedSessionConfig,
    SessionStatus, SessionSummary, StreamingEvent, ToolCancellation, ToolDefinition, ToolDispatch,
    ToolHosting, ToolResult,
};
use futures_util::future::join_all;
use tokio::sync::{mpsc, oneshot};

use super::{PendingToolCalls, SessionConfig};
use crate::{
    Error,
    journal::{AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate},
};

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

/// The model request the journal last recorded whole or in part: enough to say which
/// prefix of the next one is already written.
struct Journalled {
    system: u64,
    messages: Vec<u64>,
}

pub struct SessionActor {
    row: SessionRow,
    sealed: SealedSessionConfig,
    context: ContextEnvelope,
    store: Arc<dyn JournalStore>,
    config: Arc<SessionConfig>,
    receiver: mpsc::Receiver<SessionCommand>,
    cancel_requested: Arc<AtomicBool>,
    /// Events the session opened with, waiting to be handed to the agentloop. Taken once,
    /// before the first message.
    opening_history: Vec<serde_json::Value>,
    /// What of the last model request the journal already holds.
    ///
    /// A model request carries the whole conversation, and recording it whole on every
    /// decision wrote the transcript again each turn -- the journal grew with the square
    /// of the turn count, 5 MiB at 250 turns and 73 MiB at 1000. Only what changed since
    /// the last record is written: the messages after the longest prefix both share,
    /// with the system prompt as position zero. `None` after a restart, so the first
    /// request is recorded whole and every one after it is a delta again.
    journalled: Option<Journalled>,
    /// Where client-hosted tool calls wait for their POSTed outcome.
    pending_tools: Arc<PendingToolCalls>,
    /// The context the running turn opened with. A turn that ends normally clears it
    /// and journals the terminal context; every abnormal end restores it — under a
    /// residency-holding executor the mid-turn copy is a placeholder, and the opening
    /// state is the honest answer either way.
    turn_opening_context: Option<ContextEnvelope>,
}

impl SessionActor {
    pub fn new(
        mut row: SessionRow,
        store: Arc<dyn JournalStore>,
        config: Arc<SessionConfig>,
        receiver: mpsc::Receiver<SessionCommand>,
        cancel_requested: Arc<AtomicBool>,
        opening_history: Vec<serde_json::Value>,
        pending_tools: Arc<PendingToolCalls>,
    ) -> Result<Self, Error> {
        // The row is owned, and neither value is read again after this: `sealed` and
        // `context` replace them. Cloning first deep-copied the whole configuration and
        // the whole context on every rehydration.
        let sealed: SealedSessionConfig =
            serde_json::from_value(std::mem::take(&mut row.configuration))
                .map_err(|error| Error::Journal(error.to_string()))?;
        let context = serde_json::from_value(std::mem::take(&mut row.context))
            .map_err(|error| Error::Journal(error.to_string()))?;
        Ok(Self {
            row,
            sealed,
            context,
            store,
            config,
            receiver,
            cancel_requested,
            opening_history,
            journalled: None,
            pending_tools,
            turn_opening_context: None,
        })
    }

    pub async fn run(mut self) {
        // Before anything is asked of it. A session created with history has records in
        // its journal that this agentloop has never seen, and it cannot answer a message
        // sensibly without them. The events are Brain's; what to make of them is the
        // loop's, so they are handed over and the context that comes back is taken as-is.
        if let Err(error) = self.announce_history().await {
            // Recorded and then abandoned: a session that could not be told what it is
            // continuing cannot answer for that conversation, and pretending otherwise
            // would answer as though it had just begun.
            let _ = self.fail_turn(codes::failure::SESSION_HISTORY_REJECTED, &error.to_string());
            return;
        }
        while let Some(command) = self.receiver.recv().await {
            match command {
                SessionCommand::Message { request, reply } => {
                    let _ = reply.send(self.turn(request).await);
                }
                SessionCommand::Cancel => {
                    self.cancel_requested.store(true, Ordering::Release);
                }
                SessionCommand::End { reply } => {
                    let _ = reply.send(self.end());
                }
                SessionCommand::Append { record, reply } => {
                    let _ = reply.send(self.append_between_turns(record));
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
            SessionUpdate {
                status: Some(SessionStatus::Running),
                context: None,
                configuration: None,
            },
        )?;
        let mut observation = Observation::UserMessage {
            input: request.input,
        };
        self.turn_opening_context = Some(self.context.clone());
        for decision_index in 0..self.config.max_decisions_per_turn {
            if self.cancel_requested.load(Ordering::Acquire) {
                return self.cancel_turn();
            }
            let runtime = RuntimeEnvelope::at(
                &self.row.session_id,
                self.row.through_sequence,
                decision_index,
            );
            let observation_kind = observation_kind(&observation);
            // Moved, not cloned: nothing reads `self.context` while the activation is in
            // flight, and every executor returns the turn's context in its output — the
            // worker-backed one echoes what it holds resident, a scripted one passes the
            // input through — so the envelope makes the round trip without a copy.
            let context = std::mem::take(&mut self.context);
            let activation = ActivationInput {
                context,
                observation,
                configuration: self.sealed.brain_configuration.clone(),
                system: self.sealed.system.clone(),
                tools: self.sealed.tools.clone(),
                runtime,
            };
            self.commit(
                vec![AppendRecord::new(
                    codes::event::ACTIVATION_STARTED,
                    serde_json::json!({"observation": observation_kind}),
                )],
                SessionUpdate::default(),
            )?;
            let loop_executor = self.config.loop_executor.clone();
            let agentloop_identity = self.sealed.agentloop_identity.clone();
            let session_id = self.row.session_id.clone();
            let output = match self
                .interruptible(loop_executor.activate(&session_id, &agentloop_identity, activation))
                .await
            {
                Err(()) => return self.cancel_turn(),
                Ok(Ok(output)) => output,
                Ok(Err(error)) => {
                    let failure = Failure::new(codes::failure::AGENTLOOP_FAILED, error.to_string());
                    return self.finish_turn(vec![
                        AppendRecord::new(
                            codes::event::ACTIVATION_FAILED,
                            failure_payload(None, &failure)?,
                        ),
                        AppendRecord::new(
                            codes::event::TURN_FAILED,
                            failure_payload(None, &failure)?,
                        ),
                    ]);
                }
            };
            if output.context.protocol_version != "agentloop/v1" {
                return self.fail_turn(
                    codes::failure::INVALID_CONTEXT_VERSION,
                    "Agentloop returned an unsupported context version",
                );
            }
            self.context = output.context;
            self.commit(
                vec![AppendRecord::new(
                    codes::event::ACTIVATION_ENDED,
                    serde_json::json!({"decision": decision_kind(&output.decision)}),
                )],
                SessionUpdate::default(),
            )?;
            observation = match output.decision {
                Decision::Model { mut request } => {
                    // What the loop left unsaid is what the session was created with.
                    if request.system.is_none() {
                        request.system = Some(self.sealed.system.clone());
                    }
                    if request.tools.is_none() {
                        request.tools = Some(
                            self.sealed
                                .tools
                                .iter()
                                .map(|tool| tool.name.clone())
                                .collect(),
                        );
                    }
                    if request.response_format.is_none() {
                        request.response_format = self.sealed.response_format.clone();
                    }
                    let tools = match self.offered_tools(&request) {
                        Ok(tools) => tools,
                        Err(message) => {
                            return self
                                .fail_turn(codes::failure::INVALID_MODEL_DECISION, &message);
                        }
                    };
                    let sequence = self.row.through_sequence + 1;
                    let record = self.model_call_record(&request)?;
                    self.commit(
                        vec![AppendRecord::new(codes::event::MODEL_CALL_STARTED, record)],
                        SessionUpdate::default(),
                    )?;
                    // Model output is streamed and not stored. `model_call_ended` used
                    // to carry every delta beside the assembled response, so a turn wrote
                    // its own output twice -- once in pieces and once whole -- and nothing
                    // ever read the pieces. The assembled response is the durable truth; a
                    // client that wants the pieces takes them off the stream as they
                    // arrive. Sending is non-blocking and drops for a subscriber that has
                    // fallen behind, so watching a turn cannot slow it down.
                    let live = self.config.live.clone();
                    let live_session = self.row.session_id.clone();
                    let model_executor = self.config.model_executor.clone();
                    let model_binding = self.sealed.model.clone();
                    match self
                        .interruptible(model_executor.execute(
                            &model_binding,
                            request,
                            &tools,
                            &mut |event| {
                                if let Some(streaming) = streaming_event(sequence, &event) {
                                    let _ = live.send((
                                        live_session.clone(),
                                        LiveEvent::Streaming(streaming),
                                    ));
                                }
                            },
                        ))
                        .await
                    {
                        Err(()) => return self.cancel_turn(),
                        Ok(Ok(result)) => {
                            self.commit(
                                vec![AppendRecord::new(
                                    codes::event::MODEL_CALL_ENDED,
                                    serde_json::json!({"sequence":sequence,"result":result}),
                                )],
                                SessionUpdate::default(),
                            )?;
                            Observation::ModelCompleted {
                                response: serde_json::to_value(result).map_err(json_error)?,
                            }
                        }
                        Ok(Err(error)) => {
                            return self.executor_failure(
                                codes::event::call::MODEL_CALL,
                                sequence,
                                error,
                            );
                        }
                    }
                }
                Decision::Tools { calls } => {
                    if calls.is_empty() {
                        return self.fail_turn(
                            codes::failure::INVALID_TOOLS_DECISION,
                            "Agentloop returned no Tool calls",
                        );
                    }
                    let mut dispatches = Vec::with_capacity(calls.len());
                    let mut started = Vec::with_capacity(calls.len());
                    for (offset, invocation) in calls.into_iter().enumerate() {
                        let binding = self
                            .sealed
                            .tool_bindings
                            .iter()
                            .find(|binding| binding.name == invocation.name)
                            .cloned()
                            .ok_or_else(|| {
                                Error::InvalidState(format!("unsealed Tool {}", invocation.name))
                            })?;
                        let dispatch = ToolDispatch {
                            sequence: self.row.through_sequence + offset as u64 + 1,
                            session_id: self.row.session_id.clone(),
                            binding,
                            invocation,
                            deadline_ms: self.config.tool_deadline_ms,
                        };
                        started.push(AppendRecord::new(
                            codes::event::TOOL_CALL_STARTED,
                            serde_json::to_value(&dispatch).map_err(json_error)?,
                        ));
                        dispatches.push(dispatch);
                    }
                    // A client-hosted call parks before the started commit: the commit is
                    // what puts `tool_call_started` on the live feed, so a client
                    // answering off that feed must never find the park missing.
                    let mut receivers: Vec<Option<oneshot::Receiver<Outcome>>> = dispatches
                        .iter()
                        .map(|dispatch| {
                            matches!(dispatch.binding.hosting, ToolHosting::Client)
                                .then(|| self.pending_tools.park(dispatch.sequence))
                        })
                        .collect();
                    if let Err(error) = self.commit(started, SessionUpdate::default()) {
                        for dispatch in &dispatches {
                            self.pending_tools.discard(dispatch.sequence);
                        }
                        return Err(error);
                    }
                    let futures =
                        dispatches
                            .iter()
                            .cloned()
                            .enumerate()
                            .map(|(index, dispatch)| {
                                let executor = self.config.tool_executor.clone();
                                let pending = self.pending_tools.clone();
                                let receiver = receivers[index].take();
                                async move {
                                    let sequence = dispatch.sequence;
                                    let call_id = dispatch.invocation.call_id.clone();
                                    let deadline = Duration::from_millis(dispatch.deadline_ms);
                                    // The deadline is enforced here, on the calling side: the
                                    // remote cannot be trusted to, so an overdue call is
                                    // dropped and recorded as its own distinguished outcome.
                                    let result = match receiver {
                                        Some(receiver) => {
                                            match tokio::time::timeout(deadline, receiver).await {
                                                Ok(Ok(outcome)) => Ok((outcome, false)),
                                                // The sender is gone without an answer: the
                                                // park was discarded under us, which only a
                                                // cancellation does.
                                                Ok(Err(_)) => Ok((Outcome::Cancelled, false)),
                                                Err(_) => {
                                                    pending.discard(sequence);
                                                    Ok((Outcome::Timeout, true))
                                                }
                                            }
                                        }
                                        None => match tokio::time::timeout(
                                            deadline,
                                            executor.execute(dispatch),
                                        )
                                        .await
                                        {
                                            Ok(result) => result.map(|outcome| (outcome, false)),
                                            Err(_) => Ok((Outcome::Timeout, true)),
                                        },
                                    };
                                    (index, sequence, call_id, result)
                                }
                            });
                    let completed = match self.interruptible(join_all(futures)).await {
                        Ok(completed) => completed,
                        Err(()) => {
                            self.cancel_tools(&dispatches).await?;
                            return self.cancel_turn();
                        }
                    };
                    let mut terminal = Vec::with_capacity(completed.len());
                    let mut results = Vec::with_capacity(completed.len());
                    let mut expired = Vec::new();
                    for (index, sequence, call_id, result) in completed {
                        let result = match result {
                            Ok((outcome, timed_out)) => {
                                if timed_out {
                                    expired.push(dispatches[index].clone());
                                }
                                ToolResult::from_outcome(call_id, outcome)
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
                            serde_json::json!({"sequence":sequence,"result":result}),
                        ));
                        results.push(serde_json::to_value(result).map_err(json_error)?);
                    }
                    self.commit(terminal, SessionUpdate::default())?;
                    // The call was abandoned locally; tell its environment to stop the
                    // work too, best effort like every cancellation.
                    if !expired.is_empty() {
                        self.cancel_tools(&expired).await?;
                    }
                    Observation::ToolsCompleted { results }
                }
                Decision::Emit { event } => {
                    let record = self
                        .commit(
                            vec![AppendRecord::new(codes::event::OUTPUT_EMITTED, event)],
                            SessionUpdate::default(),
                        )?
                        .remove(0);
                    Observation::Emitted {
                        event: Event {
                            event_id: EventId::new(format!(
                                "evt_{}_{}",
                                self.row.session_id, record.sequence
                            )),
                            sequence: record.sequence,
                            recorded_at_ms: record.recorded_at_ms,
                            event_type: record.kind,
                            data: record.payload,
                        },
                    }
                }
                Decision::Finish { result } => {
                    self.turn_opening_context = None;
                    return self.finish_turn(vec![AppendRecord::new(
                        codes::event::TURN_ENDED,
                        serde_json::json!({"result":result}),
                    )]);
                }
                Decision::Fail {
                    code,
                    message,
                    retryable,
                } => {
                    self.turn_opening_context = None;
                    return self.finish_turn(vec![AppendRecord::new(
                        codes::event::TURN_FAILED,
                        failure_payload(None, &Failure::new(code, message).retryable(retryable))?,
                    )]);
                }
            };
        }
        self.fail_turn(
            codes::failure::DECISION_LIMIT,
            "Agentloop exceeded the turn decision limit",
        )
    }

    /// The definitions of the tools a model request offers, in the request's order.
    ///
    /// The loop chooses by name from what the session was created with: a name outside
    /// that set would be a tool nothing admitted and nothing can dispatch, and a repeated
    /// name would offer the model the same tool twice. Both fail the decision.
    fn offered_tools(&self, request: &ModelRequest) -> Result<Vec<ToolDefinition>, String> {
        if request
            .system
            .as_deref()
            .is_some_and(|system| system.len() > 131_072)
        {
            return Err("system prompt exceeds 128 KiB".into());
        }
        let names = request.tools.as_deref().unwrap_or(&[]);
        let mut tools = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            if names[..index].contains(name) {
                return Err(format!("Tool `{name}` is offered twice"));
            }
            let definition = self
                .sealed
                .tools
                .iter()
                .find(|tool| &tool.name == name)
                .ok_or_else(|| format!("Tool `{name}` is not one the session was created with"))?;
            tools.push(definition.clone());
        }
        Ok(tools)
    }

    /// The `model_call_started` record: the request, less the prefix the journal holds.
    ///
    /// The system prompt is position zero and the messages follow it. The record carries
    /// everything from the first position that differs from the last recorded request to
    /// the end, and `messages_from` says where that is. A record starting at zero carries
    /// the system prompt; a later one is continuing the system prompt already written.
    /// Nothing already written is touched, and a reader rebuilds the request by keeping
    /// the previous one up to `messages_from` and appending this record's messages.
    fn model_call_record(&mut self, request: &ModelRequest) -> Result<serde_json::Value, Error> {
        let system = xxhash_rust::xxh3::xxh3_64(request.system.as_deref().unwrap_or("").as_bytes());
        let messages = request
            .messages
            .iter()
            .map(|message| {
                serde_json::to_vec(message).map(|bytes| xxhash_rust::xxh3::xxh3_64(&bytes))
            })
            .collect::<Result<Vec<u64>, _>>()
            .map_err(json_error)?;
        let from = match &self.journalled {
            Some(journalled) if journalled.system == system => journalled
                .messages
                .iter()
                .zip(&messages)
                .take_while(|(previous, current)| previous == current)
                .count(),
            _ => 0,
        };
        let mut record = serde_json::json!({
            "tools": request.tools,
            "messages_from": from,
            "messages_total": request.messages.len(),
            "messages": &request.messages[from..],
            "response_format": request.response_format,
            "max_output_tokens": request.max_output_tokens,
        });
        if from == 0 {
            record["system"] =
                serde_json::Value::String(request.system.clone().unwrap_or_default());
        }
        self.journalled = Some(Journalled { system, messages });
        Ok(record)
    }

    /// The effect failed, so the turn does. `ambiguous` says whether the effect may have
    /// happened anyway: a stream that broke mid-answer, a receipt that never came.
    fn executor_failure(
        &mut self,
        kind: &str,
        sequence: u64,
        error: Error,
    ) -> Result<SessionSummary, Error> {
        let effect = failure_of(&error);
        let turn = Failure::new(format!("{kind}_failed"), error.to_string())
            .retryable(effect.retryable)
            .ambiguous(effect.ambiguous);
        self.finish_turn(vec![
            AppendRecord::new(
                format!("{kind}_failed"),
                failure_payload(Some(sequence), &effect)?,
            ),
            AppendRecord::new(codes::event::TURN_FAILED, failure_payload(None, &turn)?),
        ])
    }

    fn fail_turn(&mut self, code: &str, message: &str) -> Result<SessionSummary, Error> {
        self.finish_turn(vec![AppendRecord::new(
            codes::event::TURN_FAILED,
            failure_payload(None, &Failure::new(code, message))?,
        )])
    }

    /// Commits a turn's terminal record and returns the session to Idle.
    ///
    /// This is the only point at which the session row's context is written. Within a
    /// turn the context is held in memory: it grows with every decision, so writing it
    /// per decision costs the sum of every intermediate size rather than the final one,
    /// and nothing ever reads those intermediate values. A restart closes an in-flight
    /// turn with `turn_failed` (code `interrupted`) rather than resuming it, and the row is read back only
    /// when an Idle session is rehydrated — which is to say, here.
    fn finish_turn(&mut self, records: Vec<AppendRecord>) -> Result<SessionSummary, Error> {
        // An abnormal end rolls the context back to the turn's opening state; the
        // decisions the turn did make stay journaled as events. Normal completion
        // cleared this and keeps the terminal context.
        if let Some(opening) = self.turn_opening_context.take() {
            self.context = opening;
        }
        let context = serde_json::to_value(&self.context).map_err(json_error)?;
        self.commit(
            records,
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                context: Some(&context),
                configuration: None,
            },
        )?;
        Ok(self.public())
    }

    fn cancel_turn(&mut self) -> Result<SessionSummary, Error> {
        self.fail_turn(codes::failure::CANCELLED, "turn cancelled")
    }

    async fn cancel_tools(&mut self, dispatches: &[ToolDispatch]) -> Result<(), Error> {
        let mut cancellations = Vec::with_capacity(dispatches.len());
        let mut started = Vec::with_capacity(dispatches.len());
        for (offset, dispatch) in dispatches.iter().enumerate() {
            let cancellation = ToolCancellation {
                sequence: self.row.through_sequence + offset as u64 + 1,
                target_sequence: dispatch.sequence,
                session_id: dispatch.session_id.clone(),
                binding: dispatch.binding.clone(),
            };
            started.push(AppendRecord::new(
                codes::event::TOOL_CANCEL_STARTED,
                serde_json::to_value(&cancellation).map_err(json_error)?,
            ));
            cancellations.push(cancellation);
        }
        self.commit(started, SessionUpdate::default())?;
        let sequences: Vec<u64> = cancellations
            .iter()
            .map(|cancellation| cancellation.sequence)
            .collect();
        let futures = cancellations.into_iter().map(|cancellation| {
            let executor = self.config.tool_executor.clone();
            let pending = self.pending_tools.clone();
            async move {
                let sequence = cancellation.sequence;
                // A client-hosted call has no environment to tell: dropping the park is
                // the cancellation, and the journaled `tool_cancel_started` above is the
                // signal the client aborts its local handler on.
                let result = if matches!(cancellation.binding.hosting, ToolHosting::Client) {
                    pending.discard(cancellation.target_sequence);
                    Ok(())
                } else {
                    executor.cancel(cancellation).await
                };
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
                        serde_json::json!({"sequence":sequence}),
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
        self.commit(records, SessionUpdate::default())?;
        Ok(())
    }

    async fn interruptible<T>(&self, future: impl Future<Output = T>) -> Result<T, ()> {
        tokio::pin!(future);
        let mut poll = tokio::time::interval(Duration::from_millis(10));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                result = &mut future => return Ok(result),
                _ = poll.tick() => {
                    if self.cancel_requested.load(Ordering::Acquire) {
                        return Err(());
                    }
                }
            }
        }
    }

    fn end(&mut self) -> Result<SessionSummary, Error> {
        if matches!(self.row.status, SessionStatus::Running) {
            return Err(Error::InvalidState("cannot end a running session".into()));
        }
        if !matches!(self.row.status, SessionStatus::Ended) {
            self.commit(
                vec![AppendRecord::new(
                    codes::event::SESSION_ENDED,
                    serde_json::json!({}),
                )],
                SessionUpdate {
                    status: Some(SessionStatus::Ended),
                    context: None,
                    configuration: None,
                },
            )?;
        }
        Ok(self.public())
    }

    /// A record for something the host did to the session between turns. Refused while
    /// a turn is running: the turn owns the sequence until it ends.
    fn append_between_turns(&mut self, record: AppendRecord) -> Result<u64, Error> {
        if !matches!(self.row.status, SessionStatus::Idle) {
            return Err(Error::InvalidState("session is not idle".into()));
        }
        self.commit(vec![record], SessionUpdate::default())?;
        Ok(self.row.through_sequence)
    }

    fn commit(
        &mut self,
        records: Vec<AppendRecord>,
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, Error> {
        let status = update.status.clone();
        let saved = self.store.append(
            &self.row.session_id,
            self.row.through_sequence,
            &records,
            update,
        )?;
        self.row.through_sequence += saved.len() as u64;
        if let Some(status) = status {
            self.row.status = status;
        }
        // `row.context` is not refreshed here: the actor reads it once, in `new`, to seed
        // `self.context`, and never again. Re-serialising the envelope on every commit
        // only to discard it cost a full context serialisation per record.
        Ok(saved)
    }

    /// Hands the opening history to the agentloop and keeps whatever context it builds.
    ///
    /// One activation, journalled like any other, and no decision is acted on: this is not
    /// a turn, it is the loop reading what came before one.
    async fn announce_history(&mut self) -> Result<(), Error> {
        let history = std::mem::take(&mut self.opening_history);
        if history.is_empty() {
            return Ok(());
        }
        // Derived the same way every other activation's is, so an agentloop that depends
        // on it sees nothing unusual about this one.
        let runtime = RuntimeEnvelope::at(&self.row.session_id, self.row.through_sequence, 0);
        let activation = ActivationInput {
            context: self.context.clone(),
            observation: Observation::SessionStarted { history },
            configuration: self.sealed.brain_configuration.clone(),
            system: self.sealed.system.clone(),
            tools: self.sealed.tools.clone(),
            runtime,
        };
        let output = self
            .config
            .loop_executor
            .activate(
                &self.row.session_id,
                &self.sealed.agentloop_identity,
                activation,
            )
            .await?;
        if output.context.protocol_version != self.context.protocol_version {
            return Err(Error::InvalidState(
                "Agentloop returned an unsupported context version".into(),
            ));
        }
        self.context = output.context;
        let context = serde_json::to_value(&self.context).map_err(json_error)?;
        self.commit(
            vec![AppendRecord::new(
                codes::event::SESSION_HISTORY_REPLAYED,
                serde_json::json!({"events": self.row.through_sequence}),
            )],
            SessionUpdate {
                status: None,
                context: Some(&context),
                configuration: None,
            },
        )?;
        Ok(())
    }

    fn public(&self) -> SessionSummary {
        SessionSummary {
            session_id: self.row.session_id.clone(),
            status: self.row.status.clone(),
            last_sequence: self.row.through_sequence,
            share_key: String::new(),
        }
    }
}

/// What the agentloop decided, without what it decided it with.
///
/// This record carried the decision whole, and a model decision holds the entire request:
/// the conversation was written here on every turn, and then written again by the
/// `model_call_started` that followed it. Between them the journal grew with the square
/// of the turn count. Every variant is followed by a record carrying its detail -- a model
/// decision by `model_call_started`, an emit by `output_emitted`, a failure by
/// `turn_failed` -- so the kind is all this one has to add.
fn decision_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::Model { .. } => "model",
        Decision::Tools { .. } => "tools",
        Decision::Emit { .. } => "emit",
        Decision::Finish { .. } => "finish",
        Decision::Fail { .. } => "fail",
    }
}

/// What the agentloop was shown, without what it was shown. The detail is in the record
/// before this one: the message in `turn_started`, the response in `model_call_ended`,
/// the results in `tool_call_ended`.
fn observation_kind(observation: &Observation) -> &'static str {
    match observation {
        Observation::SessionStarted { .. } => "session_started",
        Observation::UserMessage { .. } => "user_message",
        Observation::ModelCompleted { .. } => "model_completed",
        Observation::ToolsCompleted { .. } => "tools_completed",
        Observation::Emitted { .. } => "emitted",
        Observation::Cancelled => "cancelled",
    }
}

/// One piece of model output, as a client watching the turn should see it.
///
/// Usage is left out: it is an accounting total that arrives at the end and is already in
/// `model_call_ended`, and it is not something a reader is watching the stream for.
fn streaming_event(sequence: u64, event: &ModelStreamEvent) -> Option<StreamingEvent> {
    let (event_type, data) = match event {
        ModelStreamEvent::TextDelta { index, text } => (
            "assistant_delta",
            serde_json::json!({ "index": index, "text": text }),
        ),
        ModelStreamEvent::RefusalDelta { index, text } => (
            "refusal_delta",
            serde_json::json!({ "index": index, "text": text }),
        ),
        ModelStreamEvent::ToolUseStart { index, id, name } => (
            "tool_call_delta",
            serde_json::json!({ "index": index, "id": id, "name": name }),
        ),
        ModelStreamEvent::ToolInputDelta {
            index,
            partial_json,
        } => (
            "tool_call_delta",
            serde_json::json!({ "index": index, "partial_json": partial_json }),
        ),
        ModelStreamEvent::BlockDone { .. }
        | ModelStreamEvent::Usage { .. }
        | ModelStreamEvent::MessageDone { .. } => return None,
    };
    Some(StreamingEvent {
        sequence,
        event_type: event_type.to_owned(),
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
