use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{future::Future, time::Duration};

use brain_protocol::{
    ActivationInput, ContextEnvelope, Decision, EnvironmentRequest, Event, EventId, Identity,
    LiveEvent, MessageRequest, ModelStreamEvent, Observation, OperationId, Outcome, Presentation,
    RuntimeEnvelope, SealedSessionConfig, Session, SessionId, SessionStatus, StreamingEvent,
    ToolCancellation, ToolDispatch, ToolHosting, ToolResult, operation_id,
};
use futures_util::future::join_all;
use tokio::sync::{mpsc, oneshot};

use super::PendingToolCalls;
use crate::{
    KernelError, LoopExecutor, ModelExecutor, ToolExecutor,
    journal::{AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate},
};

pub enum SessionCommand {
    Message {
        request: MessageRequest,
        reply: oneshot::Sender<Result<Session, KernelError>>,
    },
    Cancel,
    End {
        reply: oneshot::Sender<Result<Session, KernelError>>,
    },
}

pub struct SessionActor {
    row: SessionRow,
    sealed: SealedSessionConfig,
    context: ContextEnvelope,
    presentation: Presentation,
    store: Arc<dyn JournalStore>,
    loop_executor: Arc<dyn LoopExecutor>,
    model_executor: Arc<dyn ModelExecutor>,
    tool_executor: Arc<dyn ToolExecutor>,
    max_decisions_per_turn: usize,
    tool_deadline_ms: u64,
    receiver: mpsc::Receiver<SessionCommand>,
    cancel_requested: Arc<AtomicBool>,
    /// Events the session opened with, waiting to be handed to the agentloop. Taken once,
    /// before the first message.
    opening_history: Vec<serde_json::Value>,
    /// Where model output goes while a turn is still running. Not the journal.
    live: tokio::sync::broadcast::Sender<(SessionId, brain_protocol::LiveEvent)>,
    /// How many of the model request's messages the journal already holds.
    ///
    /// A model request carries the whole conversation, and recording it whole on every
    /// decision wrote the transcript again each turn -- the journal grew with the square
    /// of the turn count, 5 MiB at 250 turns and 73 MiB at 1000. Only the messages past
    /// this point are recorded now.
    journalled_messages: usize,
    /// Where client-hosted tool calls wait for their POSTed outcome.
    pending_tools: Arc<PendingToolCalls>,
}

impl SessionActor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mut row: SessionRow,
        store: Arc<dyn JournalStore>,
        loop_executor: Arc<dyn LoopExecutor>,
        model_executor: Arc<dyn ModelExecutor>,
        tool_executor: Arc<dyn ToolExecutor>,
        max_decisions_per_turn: usize,
        tool_deadline_ms: u64,
        receiver: mpsc::Receiver<SessionCommand>,
        cancel_requested: Arc<AtomicBool>,
        live: tokio::sync::broadcast::Sender<(SessionId, brain_protocol::LiveEvent)>,
        opening_history: Vec<serde_json::Value>,
        pending_tools: Arc<PendingToolCalls>,
    ) -> Result<Self, KernelError> {
        // The row is owned, and neither value is read again after this: `sealed` and
        // `context` replace them. Cloning first deep-copied the whole configuration and
        // the whole context on every rehydration.
        let sealed: SealedSessionConfig =
            serde_json::from_value(std::mem::take(&mut row.configuration))
                .map_err(|error| KernelError::Journal(error.to_string()))?;
        let context = serde_json::from_value(std::mem::take(&mut row.context))
            .map_err(|error| KernelError::Journal(error.to_string()))?;
        let presentation_bytes = brain_protocol::canonical_json(&serde_json::json!({
            "brain_configuration": sealed.brain_configuration,
            "presentation": sealed.presentation,
        }))
        .map_err(|error| KernelError::Journal(error.to_string()))?;
        Ok(Self {
            presentation: Presentation {
                bytes: presentation_bytes,
                identity: row.presentation_identity,
            },
            row,
            sealed,
            context,
            store,
            loop_executor,
            model_executor,
            tool_executor,
            max_decisions_per_turn,
            tool_deadline_ms,
            receiver,
            cancel_requested,
            live,
            opening_history,
            // A restored session has journalled messages it cannot count without reading
            // its own history back, so the first request after a restart records its
            // messages whole and every one after that is a delta again.
            journalled_messages: 0,
            pending_tools,
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
            let _ = self.fail_turn("session_history_rejected", &error.to_string());
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
            }
        }
    }

    async fn turn(&mut self, request: MessageRequest) -> Result<Session, KernelError> {
        if !matches!(self.row.status, SessionStatus::Idle) {
            return Err(KernelError::InvalidState("session is not idle".into()));
        }
        self.cancel_requested.store(false, Ordering::Release);
        self.commit(
            vec![AppendRecord::new(
                "turn_started",
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
        for decision_index in 0..self.max_decisions_per_turn {
            if self.cancel_requested.load(Ordering::Acquire) {
                return self.finish_turn(vec![AppendRecord::new(
                    "turn_failed",
                    serde_json::json!({"code":"cancelled"}),
                )]);
            }
            let runtime = RuntimeEnvelope::at(
                &self.row.journal_id,
                self.row.through_sequence,
                decision_index,
            );
            // The sealed presentation is the same on every decision of the session, so
            // take the identity it was sealed with instead of reading its bytes again.
            // What is left is hashed from Brain's own encoding: nothing outside Brain
            // recomputes this one, so it does not have to be canonical.
            let activation_identity = Identity::over(&[
                self.presentation.identity,
                Identity::of_bytes(
                    &serde_json::to_vec(&(&self.context, &observation, &runtime))
                        .map_err(json_error)?,
                ),
            ]);
            let activation = ActivationInput {
                context: self.context.clone(),
                observation,
                configuration: self.sealed.brain_configuration.clone(),
                presentation: self.presentation.clone(),
                runtime,
            };
            self.commit(
                vec![AppendRecord::new(
                    "activation_intent",
                    serde_json::json!({"request_identity":activation_identity}),
                )],
                SessionUpdate::default(),
            )?;
            let loop_executor = self.loop_executor.clone();
            let agentloop_identity = self.sealed.agentloop_identity.clone();
            let output = match self
                .interruptible(loop_executor.activate(&agentloop_identity, activation))
                .await
            {
                Err(()) => return self.cancel_turn(),
                Ok(Ok(output)) => output,
                Ok(Err(error)) => {
                    return self.finish_turn(vec![AppendRecord::new("activation_result", serde_json::json!({"error":error.to_string()})), AppendRecord::new("turn_failed", serde_json::json!({"code":"agentloop_failed","message":error.to_string()}))]);
                }
            };
            if output.context.protocol_version != "agentloop/v1" {
                return self.fail_turn(
                    "invalid_context_version",
                    "Agentloop returned an unsupported context version",
                );
            }
            self.context = output.context;
            self.commit(
                vec![AppendRecord::new(
                    "activation_result",
                    serde_json::json!({"decision": decision_kind(&output.decision)}),
                )],
                SessionUpdate::default(),
            )?;
            observation = match output.decision {
                Decision::Model { request } => {
                    let intent_sequence = self.row.through_sequence + 1;
                    let operation_id = operation_id(&self.row.journal_id, intent_sequence);
                    let identity = Identity::of(&request).map_err(identity_error)?;
                    // Only the messages this decision added. `request_identity` still
                    // covers the whole request, so a reader that rebuilds the messages by
                    // concatenation can check that what it rebuilt is what was sent: if an
                    // agentloop rewrote what it had already said, the identity will not
                    // match and the reader knows it, rather than being handed a
                    // conversation that never happened.
                    let sent = request.messages.len();
                    let from = self.journalled_messages.min(sent);
                    self.commit(
                        vec![AppendRecord::new(
                            "model_intent",
                            serde_json::json!({
                                "operation_id": operation_id,
                                "request_identity": identity,
                                "messages_from": from,
                                "messages_total": sent,
                                "messages": &request.messages[from..],
                                "response_format": request.response_format,
                                "max_output_tokens": request.max_output_tokens,
                            }),
                        )],
                        SessionUpdate::default(),
                    )?;
                    self.journalled_messages = sent;
                    // Model output is streamed and not stored. `model_result` used to
                    // carry every delta beside the assembled response, so a turn wrote its
                    // own output twice -- once in pieces and once whole -- and nothing ever
                    // read the pieces. The assembled response is the durable truth; a
                    // client that wants the pieces takes them off the stream as they
                    // arrive. Sending is non-blocking and drops for a subscriber that has
                    // fallen behind, so watching a turn cannot slow it down.
                    let live = self.live.clone();
                    let live_session = self.row.session_id.clone();
                    let live_operation = operation_id.clone();
                    let model_executor = self.model_executor.clone();
                    let model_binding = self.sealed.model.clone();
                    let model_presentation = self.sealed.presentation.clone();
                    match self
                        .interruptible(model_executor.execute(
                            &operation_id,
                            &identity,
                            &model_binding,
                            &model_presentation,
                            request,
                            &mut |event| {
                                if let Some(streaming) = streaming_event(&live_operation, &event) {
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
                            self.commit(vec![AppendRecord::new("model_result", serde_json::json!({"operation_id":operation_id,"result":result}))], SessionUpdate::default())?;
                            Observation::ModelCompleted {
                                response: serde_json::to_value(result).map_err(json_error)?,
                            }
                        }
                        Ok(Err(error)) => {
                            return self.executor_failure("model", operation_id, error);
                        }
                    }
                }
                Decision::Tools { calls } => {
                    if calls.is_empty() {
                        return self.fail_turn(
                            "invalid_tools_decision",
                            "Agentloop returned no Tool calls",
                        );
                    }
                    let mut dispatches = Vec::with_capacity(calls.len());
                    let mut intents = Vec::with_capacity(calls.len());
                    for (offset, invocation) in calls.into_iter().enumerate() {
                        let binding = self
                            .sealed
                            .tool_bindings
                            .iter()
                            .find(|binding| binding.name == invocation.name)
                            .cloned()
                            .ok_or_else(|| {
                                KernelError::InvalidState(format!(
                                    "unsealed Tool {}",
                                    invocation.name
                                ))
                            })?;
                        let operation_id = operation_id(
                            &self.row.journal_id,
                            self.row.through_sequence + offset as u64 + 1,
                        );
                        // The Environment recomputes this to decide whether a
                        // redelivery is the same effect, so it must be canonical.
                        let identity = Identity::of(&EnvironmentRequest::Invoke {
                            call_id: invocation.call_id.clone(),
                            tool: binding.name.clone(),
                            input: invocation.input.clone(),
                            deadline_ms: self.tool_deadline_ms,
                        })
                        .map_err(identity_error)?;
                        let dispatch = ToolDispatch {
                            operation_id: operation_id.clone(),
                            request_identity: identity,
                            session_id: self.row.session_id.clone(),
                            binding,
                            invocation,
                            deadline_ms: self.tool_deadline_ms,
                        };
                        intents.push(AppendRecord::new(
                            "tool_intent",
                            serde_json::to_value(&dispatch).map_err(json_error)?,
                        ));
                        dispatches.push(dispatch);
                    }
                    // A client-hosted call parks before the intent commit: the commit is
                    // what puts `tool_intent` on the live feed, so a client answering off
                    // that feed must never find the park missing.
                    let mut receivers: Vec<Option<oneshot::Receiver<Outcome>>> = dispatches
                        .iter()
                        .map(|dispatch| {
                            matches!(dispatch.binding.hosting, ToolHosting::Client).then(|| {
                                self.pending_tools.park(
                                    dispatch.session_id.clone(),
                                    dispatch.operation_id.clone(),
                                )
                            })
                        })
                        .collect();
                    if let Err(error) = self.commit(intents, SessionUpdate::default()) {
                        for dispatch in &dispatches {
                            self.pending_tools.discard(&dispatch.operation_id);
                        }
                        return Err(error);
                    }
                    let futures =
                        dispatches
                            .iter()
                            .cloned()
                            .enumerate()
                            .map(|(index, dispatch)| {
                                let executor = self.tool_executor.clone();
                                let pending = self.pending_tools.clone();
                                let receiver = receivers[index].take();
                                async move {
                                    let operation_id = dispatch.operation_id.clone();
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
                                                    pending.discard(&operation_id);
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
                                    (index, operation_id, call_id, result)
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
                    for (index, operation_id, call_id, result) in completed {
                        let result = match result {
                            Ok((outcome, timed_out)) => {
                                if timed_out {
                                    expired.push(dispatches[index].clone());
                                }
                                ToolResult::from_outcome(call_id, outcome)
                            }
                            Err(error) => ToolResult {
                                call_id,
                                output: serde_json::json!({"code":"tool_error","message":error.to_string()}),
                                is_error: true,
                            },
                        };
                        terminal.push(AppendRecord::new(
                            "tool_result",
                            serde_json::json!({"operation_id":operation_id,"result":result}),
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
                            vec![AppendRecord::new("output_emitted", event)],
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
                    return self.finish_turn(vec![AppendRecord::new(
                        "turn_finished",
                        serde_json::json!({"result":result}),
                    )]);
                }
                Decision::Fail {
                    code,
                    message,
                    retryable,
                } => {
                    return self.finish_turn(vec![AppendRecord::new(
                        "turn_failed",
                        serde_json::json!({"code":code,"message":message,"retryable":retryable}),
                    )]);
                }
            };
        }
        self.fail_turn(
            "decision_limit",
            "Agentloop exceeded the turn decision limit",
        )
    }

    fn executor_failure(
        &mut self,
        kind: &str,
        operation_id: brain_protocol::OperationId,
        error: KernelError,
    ) -> Result<Session, KernelError> {
        let outcome = if matches!(error, KernelError::Ambiguous(_)) {
            format!("{kind}_ambiguous")
        } else {
            format!("{kind}_result")
        };
        self.finish_turn(vec![
            AppendRecord::new(
                outcome,
                serde_json::json!({"operation_id":operation_id,"error":error.to_string()}),
            ),
            AppendRecord::new(
                "turn_failed",
                serde_json::json!({"code":format!("{kind}_failed"),"message":error.to_string()}),
            ),
        ])
    }

    fn fail_turn(&mut self, code: &str, message: &str) -> Result<Session, KernelError> {
        self.finish_turn(vec![AppendRecord::new(
            "turn_failed",
            serde_json::json!({"code":code,"message":message}),
        )])
    }

    /// Commits a turn's terminal record and returns the session to Idle.
    ///
    /// This is the only point at which the session row's context is written. Within a
    /// turn the context is held in memory: it grows with every decision, so writing it
    /// per decision costs the sum of every intermediate size rather than the final one,
    /// and nothing ever reads those intermediate values. A restart closes an in-flight
    /// turn with `turn_interrupted` rather than resuming it, and the row is read back only
    /// when an Idle session is rehydrated — which is to say, here.
    fn finish_turn(&mut self, records: Vec<AppendRecord>) -> Result<Session, KernelError> {
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

    fn cancel_turn(&mut self) -> Result<Session, KernelError> {
        self.fail_turn("cancelled", "turn cancelled")
    }

    async fn cancel_tools(&mut self, dispatches: &[ToolDispatch]) -> Result<(), KernelError> {
        let mut cancellations = Vec::with_capacity(dispatches.len());
        let mut intents = Vec::with_capacity(dispatches.len());
        for (offset, dispatch) in dispatches.iter().enumerate() {
            let request = EnvironmentRequest::Cancel {
                target_operation_id: dispatch.operation_id.clone(),
            };
            let cancellation = ToolCancellation {
                operation_id: operation_id(
                    &self.row.journal_id,
                    self.row.through_sequence + offset as u64 + 1,
                ),
                request_identity: Identity::of(&request).map_err(identity_error)?,
                target_operation_id: dispatch.operation_id.clone(),
                session_id: dispatch.session_id.clone(),
                binding: dispatch.binding.clone(),
            };
            intents.push(AppendRecord::new(
                "tool_cancel_intent",
                serde_json::to_value(&cancellation).map_err(json_error)?,
            ));
            cancellations.push(cancellation);
        }
        self.commit(intents, SessionUpdate::default())?;
        let cancellation_ids: Vec<_> = cancellations
            .iter()
            .map(|cancellation| cancellation.operation_id.clone())
            .collect();
        let futures = cancellations.into_iter().map(|cancellation| {
            let executor = self.tool_executor.clone();
            let pending = self.pending_tools.clone();
            async move {
                let operation_id = cancellation.operation_id.clone();
                // A client-hosted call has no environment to tell: dropping the park is
                // the cancellation, and the journaled `tool_cancel_intent` above is the
                // signal the client aborts its local handler on.
                let result = if matches!(cancellation.binding.hosting, ToolHosting::Client) {
                    pending.discard(&cancellation.target_operation_id);
                    Ok(())
                } else {
                    executor.cancel(cancellation).await
                };
                (operation_id, result)
            }
        });
        let results = tokio::time::timeout(Duration::from_secs(5), join_all(futures)).await;
        let records = match results {
            Ok(results) => results
                .into_iter()
                .map(|(operation_id, result)| {
                    AppendRecord::new(
                        "tool_cancel_result",
                        match result {
                            Ok(()) => serde_json::json!({"operation_id":operation_id,"cancelled":true}),
                            Err(error) => serde_json::json!({"operation_id":operation_id,"error":error.to_string()}),
                        },
                    )
                })
                .collect(),
            Err(_) => cancellation_ids
                .into_iter()
                .map(|operation_id| {
                    AppendRecord::new(
                        "tool_cancel_ambiguous",
                        serde_json::json!({
                            "operation_id":operation_id,
                            "error":"Environment cancellation deadline exceeded"
                        }),
                    )
                })
                .collect(),
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

    fn end(&mut self) -> Result<Session, KernelError> {
        if matches!(self.row.status, SessionStatus::Running) {
            return Err(KernelError::InvalidState(
                "cannot end a running session".into(),
            ));
        }
        if !matches!(self.row.status, SessionStatus::Ended) {
            self.commit(
                vec![AppendRecord::new("session_ended", serde_json::json!({}))],
                SessionUpdate {
                    status: Some(SessionStatus::Ended),
                    context: None,
                    configuration: None,
                },
            )?;
        }
        Ok(self.public())
    }

    fn commit(
        &mut self,
        records: Vec<AppendRecord>,
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, KernelError> {
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
    async fn announce_history(&mut self) -> Result<(), KernelError> {
        let history = std::mem::take(&mut self.opening_history);
        if history.is_empty() {
            return Ok(());
        }
        // Derived the same way every other activation's is, so an agentloop that depends
        // on it sees nothing unusual about this one.
        let runtime = RuntimeEnvelope::at(&self.row.journal_id, self.row.through_sequence, 0);
        let activation = ActivationInput {
            context: self.context.clone(),
            observation: Observation::SessionStarted { history },
            configuration: self.sealed.brain_configuration.clone(),
            presentation: self.presentation.clone(),
            runtime,
        };
        let output = self
            .loop_executor
            .activate(&self.sealed.agentloop_identity, activation)
            .await?;
        if output.context.protocol_version != self.context.protocol_version {
            return Err(KernelError::InvalidState(
                "Agentloop returned an unsupported context version".into(),
            ));
        }
        self.context = output.context;
        let context = serde_json::to_value(&self.context).map_err(json_error)?;
        self.commit(
            vec![AppendRecord::new(
                "session_history_replayed",
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

    fn public(&self) -> Session {
        Session {
            session_id: self.row.session_id.clone(),
            journal_id: self.row.journal_id.clone(),
            status: self.row.status.clone(),
            last_sequence: self.row.through_sequence,
            config_hash: self.row.presentation_identity,
            share_key: String::new(),
        }
    }
}

/// What the agentloop decided, without what it decided it with.
///
/// This record carried the decision whole, and a model decision holds the entire request:
/// the conversation was written here on every turn, and then written again by the
/// `model_intent` that followed it. Between them the journal grew with the square of the
/// turn count. Every variant is followed by a record carrying its detail -- a model
/// decision by `model_intent`, an emit by `output_emitted`, a failure by `turn_failed` --
/// so the kind is all this one has to add.
fn decision_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::Model { .. } => "model",
        Decision::Tools { .. } => "tools",
        Decision::Emit { .. } => "emit",
        Decision::Finish { .. } => "finish",
        Decision::Fail { .. } => "fail",
    }
}

/// One piece of model output, as a client watching the turn should see it.
///
/// Usage is left out: it is an accounting total that arrives at the end and is already in
/// `model_result`, and it is not something a reader is watching the stream for.
fn streaming_event(operation_id: &OperationId, event: &ModelStreamEvent) -> Option<StreamingEvent> {
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
        operation_id: operation_id.clone(),
        event_type: event_type.to_owned(),
        data,
    })
}

fn json_error(error: serde_json::Error) -> KernelError {
    KernelError::InvalidState(error.to_string())
}
fn identity_error(error: brain_protocol::IdentityError) -> KernelError {
    KernelError::InvalidState(error.to_string())
}
