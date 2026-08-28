use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{future::Future, time::Duration};

use brain_protocol::{
    ActivationInput, ContextEnvelope, Decision, EnvironmentRequest, Event, EventId, Identity,
    MessageRequest, Observation, Presentation, RuntimeEnvelope, SealedSessionConfig, Session,
    SessionStatus, ToolCancellation, ToolDispatch, ToolResult, operation_id,
};
use futures_util::future::join_all;
use tokio::sync::{mpsc, oneshot};

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
    receiver: mpsc::Receiver<SessionCommand>,
    cancel_requested: Arc<AtomicBool>,
}

impl SessionActor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        row: SessionRow,
        store: Arc<dyn JournalStore>,
        loop_executor: Arc<dyn LoopExecutor>,
        model_executor: Arc<dyn ModelExecutor>,
        tool_executor: Arc<dyn ToolExecutor>,
        max_decisions_per_turn: usize,
        receiver: mpsc::Receiver<SessionCommand>,
        cancel_requested: Arc<AtomicBool>,
    ) -> Result<Self, KernelError> {
        let sealed: SealedSessionConfig = serde_json::from_value(row.configuration.clone())
            .map_err(|error| KernelError::Journal(error.to_string()))?;
        let context = serde_json::from_value(row.context.clone())
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
            receiver,
            cancel_requested,
        })
    }

    pub async fn run(mut self) {
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
            content: request.content,
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
                    serde_json::json!({"decision":output.decision}),
                )],
                SessionUpdate::default(),
            )?;
            observation = match output.decision {
                Decision::Model { request } => {
                    let intent_sequence = self.row.through_sequence + 1;
                    let operation_id = operation_id(&self.row.journal_id, intent_sequence);
                    let identity = Identity::of(&request).map_err(identity_error)?;
                    self.commit(vec![AppendRecord::new("model_intent", serde_json::json!({"operation_id":operation_id,"request_identity":identity,"request":request}))], SessionUpdate::default())?;
                    let mut stream = Vec::new();
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
                            &mut |event| stream.push(event),
                        ))
                        .await
                    {
                        Err(()) => return self.cancel_turn(),
                        Ok(Ok(result)) => {
                            self.commit(vec![AppendRecord::new("model_result", serde_json::json!({"operation_id":operation_id,"stream":stream,"result":result}))], SessionUpdate::default())?;
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
                        let identity = Identity::of(&EnvironmentRequest::Execute {
                            tool: invocation.clone(),
                            remote_tool_id: binding.remote_tool_id.clone(),
                            tool_configuration: binding.tool_configuration.clone(),
                            grant: binding.grant.clone(),
                        })
                        .map_err(identity_error)?;
                        let dispatch = ToolDispatch {
                            operation_id: operation_id.clone(),
                            request_identity: identity,
                            session_id: self.row.session_id.clone(),
                            binding,
                            invocation,
                        };
                        intents.push(AppendRecord::new(
                            "tool_intent",
                            serde_json::to_value(&dispatch).map_err(json_error)?,
                        ));
                        dispatches.push(dispatch);
                    }
                    self.commit(intents, SessionUpdate::default())?;
                    let futures = dispatches.iter().cloned().map(|dispatch| {
                        let executor = self.tool_executor.clone();
                        async move {
                            let operation_id = dispatch.operation_id.clone();
                            let call_id = dispatch.invocation.call_id.clone();
                            let result = executor.execute(dispatch).await;
                            (operation_id, call_id, result)
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
                    for (operation_id, call_id, result) in completed {
                        let result = match result {
                            Ok(result) => result,
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
    /// and nothing ever reads those intermediate values. A restart fails an in-flight
    /// turn outright (`Kernel::recover_interrupted`) rather than resuming it, and the row
    /// is read back only when an Idle session is rehydrated — which is to say, here.
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
            async move {
                let operation_id = cancellation.operation_id.clone();
                (operation_id, executor.cancel(cancellation).await)
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

    fn public(&self) -> Session {
        Session {
            session_id: self.row.session_id.clone(),
            journal_id: self.row.journal_id.clone(),
            status: self.row.status.clone(),
            through_sequence: self.row.through_sequence,
            presentation_identity: self.row.presentation_identity,
        }
    }
}

fn json_error(error: serde_json::Error) -> KernelError {
    KernelError::InvalidState(error.to_string())
}
fn identity_error(error: brain_protocol::IdentityError) -> KernelError {
    KernelError::InvalidState(error.to_string())
}
