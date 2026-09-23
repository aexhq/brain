use std::{
    collections::HashSet,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::actor::{failure_of, failure_payload, streaming_event};
use crate::{
    Error, SessionRuntime,
    journal::{AppendRecord, SessionStore, SessionUpdate},
};
use brain_protocol::{
    EventOrigin, LiveEvent, ModelRequest, ModelResult, ModelStreamEvent, SessionConfig, SessionId,
    ToolDefinition, Usage, codes,
};

/// Model authority and accounting shared by an activation and the Tools it starts.
/// It holds no Agentloop services or conversation state.
#[derive(Clone)]
pub(crate) struct ModelService {
    pub session_id: SessionId,
    pub config: Arc<SessionConfig>,
    pub runtime: Arc<SessionRuntime>,
    pub store: Arc<dyn SessionStore>,
    pub calls: Arc<AtomicUsize>,
    pub origin: EventOrigin,
}

impl ModelService {
    pub fn prepare(&self, request: &mut ModelRequest) -> Result<Vec<ToolDefinition>, Error> {
        let conversation = matches!(self.origin, EventOrigin::Agentloop { .. });
        if request.system.is_none() {
            request.system = Some(if conversation {
                self.config.system.clone()
            } else {
                String::new()
            });
        }
        if request.tools.is_none() {
            request.tools = Some(if conversation {
                self.config.definitions()
            } else {
                Vec::new()
            });
        }
        if conversation && request.response_format.is_none() {
            request.response_format = self.config.response_format.clone();
        }
        if request.messages.is_empty() {
            return Err(Error::InvalidState(
                "model request must carry at least one message".into(),
            ));
        }
        let tools = request.tools.clone().expect("model tools supplied");
        let mut seen = HashSet::with_capacity(tools.len());
        for definition in &tools {
            super::validate_tool_definition(definition)?;
            if !seen.insert(&definition.name) {
                return Err(Error::InvalidState(format!(
                    "model request offers Tool `{}` twice",
                    definition.name
                )));
            }
        }
        if self.calls.fetch_add(1, Ordering::AcqRel)
            >= crate::limits::ceiling(self.runtime.limits.max_model_calls)
        {
            return Err(Error::Budget(format!(
                "activation and its Tools exceeded the budget of {} model calls",
                self.runtime.limits.max_model_calls
            )));
        }
        Ok(tools)
    }

    pub async fn append(&self, mut record: AppendRecord) -> Result<u64, Error> {
        if let EventOrigin::Tool { sequence } = self.origin {
            record.payload["tool_sequence"] = sequence.into();
        }
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            Ok(store.append_sync(&[record], SessionUpdate::default())?[0].sequence)
        })
        .await
        .map_err(|error| Error::Journal(error.to_string()))?
    }

    pub async fn execute(
        &self,
        sequence: u64,
        request: ModelRequest,
        tools: Vec<ToolDefinition>,
        cancelled: impl Future<Output = ()>,
    ) -> Result<ModelResult, Error> {
        let live = self.runtime.live.clone();
        let live_session = self.session_id.clone();
        let mut observed_usage = Usage::default();
        let mut usage_error = None;
        let result = {
            let mut on_event = |event: ModelStreamEvent| {
                if let ModelStreamEvent::Usage { usage }
                | ModelStreamEvent::MessageDone { usage, .. } = &event
                    && let Err(error) = observed_usage.observe(usage)
                {
                    usage_error = Some(error);
                    return;
                }
                let event = if matches!(self.origin, EventOrigin::Tool { .. }) {
                    match event {
                        ModelStreamEvent::TextDelta { text, .. }
                        | ModelStreamEvent::RefusalDelta { text, .. }
                        | ModelStreamEvent::ToolInputDelta {
                            partial_json: text, ..
                        } => ModelStreamEvent::NativeDelta {
                            index: 0,
                            format: String::new(),
                            field: String::new(),
                            text,
                        },
                        ModelStreamEvent::ToolUseStart { .. } => return,
                        event => event,
                    }
                } else {
                    event
                };
                if let Some(streaming) = streaming_event(sequence, &event) {
                    live.send((live_session.clone(), LiveEvent::Streaming(streaming)));
                }
            };
            let call = self.runtime.model_executor.execute(
                &self.session_id,
                &self.config.model,
                request,
                &tools,
                &mut on_event,
            );
            tokio::select! {
                biased;
                () = cancelled => Err(Error::Ambiguous("execution closed during a model call".into())),
                result = call => result,
            }
        };
        let result = match usage_error {
            Some(error) => Err(Error::Ambiguous(error.into())),
            None => result,
        };
        let record = match &result {
            Ok(result) => AppendRecord::new(
                codes::event::MODEL_CALL_ENDED,
                serde_json::json!({"sequence": sequence, "result": result}),
            ),
            Err(error) => {
                let mut payload = failure_payload(Some(sequence), &failure_of(error))?;
                if let Error::ModelOutput {
                    stop_reason, usage, ..
                } = error
                {
                    payload["response"] = serde_json::json!({ "stop_reason": stop_reason, "usage": usage, "usage_complete": true });
                } else if observed_usage != Usage::default() {
                    payload["response"] =
                        serde_json::json!({ "usage": observed_usage, "usage_complete": false });
                }
                AppendRecord::new(codes::event::MODEL_CALL_FAILED, payload)
            }
        };
        self.append(record).await?;
        result
    }
}
