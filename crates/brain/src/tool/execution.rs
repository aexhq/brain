use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use brain_protocol::{
    Event, EventOrigin, ModelRequest, ModelResult, Outcome, OutcomeError, SessionId,
    ToolCancellation, ToolDispatch, ToolOutput, ToolResult, ToolReturn, codes,
};
use brain_telemetry::{TelemetryKind, TelemetryPublisher, TelemetryRecord};
use tokio::sync::{Mutex, Notify, oneshot, watch};
use tokio::time::Instant;

use super::{ToolExecutor, ToolServices};
use crate::{
    Error,
    journal::{AppendRecord, JournalEntry, SessionRecord, SessionStore, SessionUpdate},
};

pub(crate) struct EmissionBudget {
    used: AtomicUsize,
    maximum: usize,
}

impl EmissionBudget {
    pub(crate) fn new(maximum: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            maximum: crate::limits::ceiling(maximum),
        }
    }

    pub(crate) fn reserve(&self, bytes: usize) -> Result<(), Error> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.maximum)
            })
            .map(|_| ())
            .map_err(|_| {
                Error::EmitLimit(format!(
                    "activation exceeded its limit of {} emitted Event bytes",
                    self.maximum
                ))
            })
    }
}

#[derive(Default)]
pub struct ToolExecutions {
    groups: StdMutex<HashMap<SessionId, Weak<ToolGroup>>>,
}

impl ToolExecutions {
    pub fn group(&self, session: &SessionId) -> Arc<ToolGroup> {
        let mut groups = self.groups.lock().expect("Tool execution table poisoned");
        if let Some(group) = groups.get(session).and_then(Weak::upgrade) {
            return group;
        }
        groups.retain(|_, group| group.strong_count() > 0);
        let group = Arc::new(ToolGroup::default());
        groups.insert(session.clone(), Arc::downgrade(&group));
        group
    }
}

#[derive(Default)]
pub struct ToolGroup {
    calls: StdMutex<HashMap<u64, Arc<ToolCall>>>,
    pending: StdMutex<Option<(u64, Instant)>>,
    changed: Notify,
    generation: AtomicU64,
}

pub struct ToolWakeup {
    pub through: u64,
    generation: u64,
}

impl ToolGroup {
    pub fn accepts(&self, wakeup: &ToolWakeup) -> bool {
        self.generation.load(Ordering::Acquire) == wakeup.generation
    }

    pub fn is_active(&self) -> bool {
        !self
            .calls
            .lock()
            .expect("Tool execution table poisoned")
            .is_empty()
    }

    pub fn has_work_after(&self, processed: u64) -> bool {
        let active = self.is_active();
        let mut pending = self.pending.lock().expect("Tool wakeup state poisoned");
        if pending.is_some_and(|(through, _)| through <= processed) {
            *pending = None;
        }
        active || pending.is_some()
    }

    /// The first pending commit fixes the collection deadline; later commits only advance its sequence.
    fn wake(&self, sequence: u64, interrupted: &AtomicBool) {
        let mut pending = self.pending.lock().expect("Tool wakeup state poisoned");
        if interrupted.load(Ordering::Acquire) {
            return;
        }
        match pending.as_mut() {
            Some((through, _)) => *through = (*through).max(sequence),
            None => *pending = Some((sequence, Instant::now() + Duration::from_millis(5))),
        }
        self.changed.notify_waiters();
    }

    /// One pending burst, or no further work once all executions have been released.
    pub async fn next_wakeup(&self) -> Option<ToolWakeup> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let pending = *self.pending.lock().expect("Tool wakeup state poisoned");
            if let Some((_, deadline)) = pending {
                tokio::time::sleep_until(deadline).await;
                return self
                    .pending
                    .lock()
                    .expect("Tool wakeup state poisoned")
                    .take()
                    .map(|(through, _)| ToolWakeup {
                        through,
                        generation: self.generation.load(Ordering::Acquire),
                    });
            }
            if !self.has_work_after(0) {
                return None;
            }
            changed.await;
        }
    }

    pub async fn interrupt(&self) {
        let calls = self
            .calls
            .lock()
            .expect("Tool execution table poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for call in &calls {
            call.cancelled.store(true, Ordering::Release);
        }
        {
            let mut pending = self.pending.lock().expect("Tool wakeup state poisoned");
            self.generation.fetch_add(1, Ordering::AcqRel);
            *pending = None;
        }
        for call in &calls {
            call.stop.send_replace(true);
        }
        for call in calls {
            call.closed().await;
        }
    }

    pub async fn wait(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.is_active() {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn start(
        self: &Arc<Self>,
        dispatch: ToolDispatch,
        store: Arc<dyn SessionStore>,
        executor: Arc<dyn ToolExecutor>,
        budget: Arc<EmissionBudget>,
        telemetry: TelemetryPublisher,
        model: crate::session::model::ModelService,
    ) -> oneshot::Receiver<ToolReturn> {
        let (reply, returned) = oneshot::channel();
        let (closed, _) = watch::channel(false);
        let (stop, _) = watch::channel(false);
        let output_schema =
            dispatch.tool.output_schema.as_ref().map(|schema| {
                jsonschema::validator_for(schema).expect("admitted Tool output schema")
            });
        let call = Arc::new(ToolCall {
            output_schema,
            dispatch,
            store,
            executor,
            budget,
            telemetry,
            model,
            group: Arc::downgrade(self),
            state: Mutex::new(CallState {
                events: Vec::new(),
                models: Vec::new(),
                returned: Some(reply),
                finished: false,
                sequence: 0,
            }),
            closed,
            stop,
            cancelled: AtomicBool::new(false),
        });
        self.calls
            .lock()
            .expect("Tool execution table poisoned")
            .insert(call.dispatch.sequence, call.clone());
        let group = self.clone();
        tokio::spawn(async move {
            let result = call.run().await;
            if let Err(error) = result {
                tracing::error!(session_id = %call.dispatch.session_id, sequence = call.dispatch.sequence, %error, "Tool execution could not be recorded");
                let mut state = call.state.lock().await;
                state.finished = true;
                state.returned.take();
                call.cancelled.store(true, Ordering::Release);
                call.closed.send_replace(true);
            }
            let models = std::mem::take(&mut call.state.lock().await.models);
            for model in models {
                if let Err(error) = model.await {
                    tracing::error!(%error, "Tool model task failed");
                }
            }
            group
                .calls
                .lock()
                .expect("Tool execution table poisoned")
                .remove(&call.dispatch.sequence);
            group.changed.notify_waiters();
        });
        returned
    }
}

struct CallState {
    models: Vec<tokio::task::JoinHandle<()>>,
    events: Vec<Event>,
    returned: Option<oneshot::Sender<ToolReturn>>,
    finished: bool,
    sequence: u64,
}

struct ToolCall {
    output_schema: Option<jsonschema::Validator>,
    dispatch: ToolDispatch,
    store: Arc<dyn SessionStore>,
    executor: Arc<dyn ToolExecutor>,
    budget: Arc<EmissionBudget>,
    telemetry: TelemetryPublisher,
    model: crate::session::model::ModelService,
    group: Weak<ToolGroup>,
    state: Mutex<CallState>,
    closed: watch::Sender<bool>,
    stop: watch::Sender<bool>,
    cancelled: AtomicBool,
}

impl ToolCall {
    async fn append(
        &self,
        state: &mut CallState,
        mut records: Vec<AppendRecord>,
    ) -> Result<u64, Error> {
        let wake = records.iter().any(|record| {
            !codes::event::ALL.contains(&record.kind.as_str())
                || matches!(
                    record.kind.as_str(),
                    codes::event::TOOL_RESULT_EMITTED
                        | codes::event::TOOL_CALL_ENDED
                        | codes::event::OUTPUT_EMITTED
                )
        });
        for record in &mut records {
            if !codes::event::ALL.contains(&record.kind.as_str())
                || record.kind == codes::event::OUTPUT_EMITTED
            {
                record.origin = Some(EventOrigin::Tool {
                    sequence: self.dispatch.sequence,
                });
            }
        }
        let store = self.store.clone();
        let saved: Vec<SessionRecord> = tokio::task::spawn_blocking(move || {
            store.append_sync(&records, SessionUpdate::default())
        })
        .await
        .map_err(|error| Error::Journal(error.to_string()))??;
        if let Some(last) = saved.last() {
            state.sequence = last.sequence;
        }
        if state.returned.is_some() {
            state
                .events
                .extend(saved.into_iter().map(SessionRecord::into_event));
        }
        if wake && let Some(group) = self.group.upgrade() {
            group.wake(state.sequence, &self.cancelled);
        }
        Ok(state.sequence)
    }

    fn ensure_open(state: &CallState) -> Result<(), Error> {
        if state.finished {
            return Err(Error::InvalidState("Tool execution is finished".into()));
        }
        Ok(())
    }

    fn release_caller(&self, state: &mut CallState) {
        if let Some(reply) = state.returned.take() {
            let _ = reply.send(ToolReturn {
                call_id: self.dispatch.invocation.call_id.clone(),
                sequence: self.dispatch.sequence,
                events: std::mem::take(&mut state.events),
                finished: state.finished,
            });
        }
    }

    fn result_record(&self, output: ToolOutput) -> Result<AppendRecord, Error> {
        if let (Outcome::Ok { value }, Some(validator)) = (&output.outcome, &self.output_schema) {
            validator
                .validate(value)
                .map_err(|error| Error::InvalidState(format!("invalid Tool output: {error}")))?;
        }
        let mut result =
            ToolResult::from_outcome(self.dispatch.invocation.call_id.clone(), output.outcome);
        result.content = output.content;
        let payload =
            serde_json::to_value(result).map_err(|error| Error::InvalidState(error.to_string()))?;
        self.budget.reserve(
            codes::event::TOOL_RESULT_EMITTED
                .len()
                .saturating_add(payload.to_string().len()),
        )?;
        Ok(AppendRecord::new(
            codes::event::TOOL_RESULT_EMITTED,
            serde_json::json!({"sequence": self.dispatch.sequence, "result": payload}),
        ))
    }

    async fn complete(&self, outcome: Outcome, cancel: bool) -> Result<(), Error> {
        let mut state = self.state.lock().await;
        if state.finished {
            return Ok(());
        }
        let mut records = Vec::new();
        if cancel {
            records.push(AppendRecord::new(codes::event::TOOL_CANCEL_STARTED, serde_json::json!({"target_sequence": self.dispatch.sequence, "tool": self.dispatch.tool.name, "environment": self.dispatch.environment.name})));
        }
        records.push(AppendRecord::new(
            codes::event::TOOL_CALL_ENDED,
            serde_json::json!({"sequence": self.dispatch.sequence, "outcome": outcome}),
        ));
        let sequence = self.append(&mut state, records).await?;
        state.finished = true;
        self.release_caller(&mut state);
        self.closed.send_replace(true);
        if !cancel {
            return Ok(());
        }
        self.cancelled.store(true, Ordering::Release);
        drop(state);
        let sequence = sequence - 1;
        let cancellation = ToolCancellation {
            sequence,
            target_sequence: self.dispatch.sequence,
            session_id: self.dispatch.session_id.clone(),
            environment: self.dispatch.environment.clone(),
        };
        let result =
            tokio::time::timeout(Duration::from_secs(5), self.executor.cancel(cancellation)).await;
        let record = match result {
            Ok(Ok(())) => AppendRecord::new(
                codes::event::TOOL_CANCEL_ENDED,
                serde_json::json!({"sequence": sequence}),
            ),
            result => AppendRecord::new(
                codes::event::TOOL_CANCEL_FAILED,
                serde_json::json!({"sequence": sequence, "code": "unknown", "ambiguous": true, "message": format!("Environment cancellation failed: {result:?}")}),
            ),
        };
        let mut state = self.state.lock().await;
        self.append(&mut state, vec![record]).await?;
        Ok(())
    }

    async fn run(self: &Arc<Self>) -> Result<(), Error> {
        let validator = jsonschema::validator_for(&self.dispatch.tool.input_schema)
            .map_err(|error| Error::InvalidState(error.to_string()))?;
        if let Err(error) = validator.validate(&self.dispatch.invocation.input) {
            return self
                .complete(error_outcome("invalid_input", error.to_string()), false)
                .await;
        }
        let mut stop = self.stop.subscribe();
        let deadline = async {
            match self.dispatch.deadline_ms {
                Some(milliseconds) => tokio::time::sleep(Duration::from_millis(milliseconds)).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(deadline);
        let execute = self.executor.execute(self.dispatch.clone(), self.clone());
        tokio::pin!(execute);
        let returned = tokio::select! {
            result = &mut execute => Some(result),
            () = async { let _ = stop.wait_for(|stop| *stop).await; } => None,
            () = &mut deadline => return self.complete(Outcome::Timeout, true).await,
        };
        let Some(returned) = returned.filter(|_| !self.cancelled.load(Ordering::Acquire)) else {
            return self.complete(Outcome::Cancelled, true).await;
        };
        let finished = *self.closed.borrow();
        if !finished {
            match returned {
                Ok(Some(outcome)) if !matches!(outcome, Outcome::Ok { .. }) => {
                    return self.complete(outcome, false).await;
                }
                Ok(outcome) => {
                    let is_returned = self.state.lock().await.returned.is_none();
                    let result = if is_returned {
                        match outcome {
                            Some(outcome) => self.result(outcome.into()).await.map(|_| ()),
                            None => Ok(()),
                        }
                    } else {
                        self.returned(outcome.map(Into::into)).await.map(|_| ())
                    };
                    if let Err(error) = result {
                        return self
                            .complete(error_outcome("invalid_output", error.to_string()), false)
                            .await;
                    }
                }
                Err(Error::Ambiguous(message)) => {
                    return self.complete(Outcome::Unknown { message }, false).await;
                }
                Err(error) => {
                    return self
                        .complete(error_outcome("tool_error", error.to_string()), false)
                        .await;
                }
            }
        }
        tokio::select! {
            () = self.closed() => Ok(()),
            () = async { let _ = stop.wait_for(|stop| *stop).await; } => self.complete(Outcome::Cancelled, true).await,
            () = &mut deadline => self.complete(Outcome::Timeout, true).await,
        }
    }
}

#[async_trait::async_trait]
impl ToolServices for ToolCall {
    async fn model(&self, mut request: ModelRequest) -> Result<ModelResult, Error> {
        let mut state = self.state.lock().await;
        Self::ensure_open(&state)?;
        if self.cancelled() {
            return Err(Error::Cancelled("Tool execution cancelled".into()));
        }
        let tools = self.model.prepare(&mut request)?;
        let service = self.model.clone();
        let mut closed = self.closed.subscribe();
        let mut stop = self.stop.subscribe();
        let (reply, answer) = oneshot::channel();
        // A disconnected callback must not abandon a committed model intent.
        state.models.push(tokio::spawn(async move {
            let result = async {
                if *closed.borrow() || *stop.borrow() {
                    return Err(Error::Cancelled("Tool execution closed".into()));
                }
        let sequence = service.append(AppendRecord::new(
            codes::event::MODEL_CALL_STARTED,
            serde_json::json!({
                "system": request.system, "tools": request.tools,
                "messages": request.messages.len(),
                "context": {"through_sequence": 0, "delta": JournalEntry::TranscriptDelta { keep: 0, append: request.messages.clone() }},
                "response_format": request.response_format, "max_output_tokens": request.max_output_tokens,
                "options": request.options,
            }),
        )).await?;
            let cancelled = async {
                tokio::select! {
                    _ = closed.wait_for(|value| *value) => {},
                    _ = stop.wait_for(|value| *value) => {},
                }
            };
            service.execute(sequence, request, tools, cancelled).await
            }.await;
            let _ = reply.send(result);
        }));
        drop(state);
        answer
            .await
            .map_err(|error| Error::Executor(error.to_string()))?
    }

    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error> {
        if kind == "_extension_event"
            || !crate::session::valid_kind(&kind)
            || JournalEntry::is_kind(&kind)
            || (codes::event::ALL.contains(&kind.as_str()) && kind != codes::event::OUTPUT_EMITTED)
        {
            return Err(Error::InvalidState(
                "event kind is reserved or invalid".into(),
            ));
        }
        let mut state = self.state.lock().await;
        Self::ensure_open(&state)?;
        self.budget
            .reserve(kind.len().saturating_add(payload.to_string().len()))?;
        self.append(&mut state, vec![AppendRecord::new(kind, payload)])
            .await
    }

    async fn result(&self, outcome: ToolOutput) -> Result<u64, Error> {
        let mut state = self.state.lock().await;
        Self::ensure_open(&state)?;
        let record = self.result_record(outcome)?;
        self.append(&mut state, vec![record]).await
    }

    async fn returned(&self, outcome: Option<ToolOutput>) -> Result<u64, Error> {
        let mut state = self.state.lock().await;
        Self::ensure_open(&state)?;
        if state.returned.is_none() {
            return Err(Error::InvalidState("Tool has already returned".into()));
        }
        let mut records = Vec::new();
        if let Some(outcome) = outcome {
            records.push(self.result_record(outcome)?);
        }
        records.push(AppendRecord::new(
            codes::event::TOOL_CALL_RETURNED,
            serde_json::json!({"sequence": self.dispatch.sequence}),
        ));
        let sequence = self.append(&mut state, records).await?;
        self.release_caller(&mut state);
        Ok(sequence)
    }

    async fn finish(&self, outcome: Option<ToolOutput>) -> Result<u64, Error> {
        let mut state = self.state.lock().await;
        Self::ensure_open(&state)?;
        let mut records = Vec::new();
        let terminal = match outcome {
            Some(output) => {
                let outcome = output.outcome.clone();
                if matches!(outcome, Outcome::Ok { .. }) || output.content.is_some() {
                    records.push(self.result_record(output)?);
                }
                match outcome {
                    Outcome::Ok { .. } => Outcome::Ok {
                        value: serde_json::Value::Null,
                    },
                    outcome => outcome,
                }
            }
            None => Outcome::Ok {
                value: serde_json::Value::Null,
            },
        };
        records.push(AppendRecord::new(
            codes::event::TOOL_CALL_ENDED,
            serde_json::json!({"sequence": self.dispatch.sequence, "outcome": terminal}),
        ));
        let sequence = self.append(&mut state, records).await?;
        state.finished = true;
        self.release_caller(&mut state);
        self.closed.send_replace(true);
        Ok(sequence)
    }

    async fn closed(&self) {
        let _ = self.closed.subscribe().wait_for(|closed| *closed).await;
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn telemetry(&self, record: serde_json::Value) {
        let _ = self.telemetry.try_publish(TelemetryRecord {
            kind: TelemetryKind::Log,
            name: "tool_telemetry".into(),
            payload: record.to_string().into_bytes(),
            session_id: Some(self.dispatch.session_id.clone()),
            sequence: Some(self.dispatch.sequence),
        });
    }
}

fn error_outcome(code: &str, message: String) -> Outcome {
    Outcome::Error {
        error: OutcomeError {
            code: code.into(),
            message,
            retryable: false,
            details: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_burst_keeps_its_first_deadline_and_acknowledged_work_needs_no_wakeup() {
        let group = ToolGroup::default();
        let interrupted = AtomicBool::new(false);
        group.wake(1, &interrupted);
        let deadline = group.pending.lock().unwrap().unwrap().1;
        for sequence in 2..=1000 {
            group.wake(sequence, &interrupted);
        }
        assert_eq!(group.pending.lock().unwrap().unwrap(), (1000, deadline));
        assert_eq!(group.next_wakeup().await.unwrap().through, 1000);
        assert!(group.next_wakeup().await.is_none());
        group.wake(1001, &interrupted);
        assert!(!group.has_work_after(1001));
        assert!(group.next_wakeup().await.is_none());
    }

    #[tokio::test]
    async fn interruption_invalidates_a_wakeup_already_waiting_for_the_session() {
        let group = ToolGroup::default();
        let interrupted = AtomicBool::new(false);
        group.wake(1, &interrupted);
        let wakeup = group.next_wakeup().await.unwrap();
        group.interrupt().await;
        assert!(!group.accepts(&wakeup));
        interrupted.store(true, Ordering::Release);
        group.wake(2, &interrupted);
        assert!(group.next_wakeup().await.is_none());
    }
}
