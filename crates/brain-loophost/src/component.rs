use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use brain::agentloop::{Agentloop, LoopVerdict, TurnCtx};
use brain::{BrainError, Result};
use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, CapabilityRouter, ComponentSource,
    WorkerPool, WorkerRequest, agentloop,
};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::{check_session_start, payload_session_id, resolve_verdict, service_ctx_op};

pub struct ComponentAgentloop {
    pool: Arc<WorkerPool>,
    router: Arc<CapabilityRouter>,
    component: ComponentSource,
    config_json: String,
    started: Mutex<HashSet<String>>,
}

impl ComponentAgentloop {
    pub fn new(
        pool: Arc<WorkerPool>,
        router: Arc<CapabilityRouter>,
        component: ComponentSource,
        config_json: String,
    ) -> Self {
        Self {
            pool,
            router,
            component,
            config_json,
            started: Mutex::new(HashSet::new()),
        }
    }

    async fn activate(
        &self,
        instance_id: &str,
        kind: &str,
        payload: Value,
        ctx: &mut dyn TurnCtx,
        receiver: &mut mpsc::Receiver<PendingCapability>,
        first_error: &mut Option<BrainError>,
    ) -> Result<String> {
        let operation_id = payload
            .get("activation_id")
            .and_then(Value::as_str)
            .unwrap_or(kind)
            .to_owned();
        let request = WorkerRequest::Agentloop {
            instance_id: instance_id.to_owned(),
            component: ComponentSource {
                path: self.component.path.clone(),
                sha256: self.component.sha256.clone(),
            },
            request: agentloop::aex::agentloop::types::Activation {
                operation_id,
                session_id: instance_id.to_owned(),
                kind: kind.to_owned(),
                payload_json: payload.to_string(),
                config_json: self.config_json.clone(),
                deadline_at_ms: u64::MAX,
            },
        };
        let mut activation = Box::pin(self.pool.call(request));
        let value = loop {
            tokio::select! {
                pending = receiver.recv() => {
                    let Some(pending) = pending else {
                        return Err(BrainError::Agentloop("component capability channel closed".into()));
                    };
                    let payload = pending.call.request.to_string();
                    let response = service_ctx_op(ctx, &payload, first_error).await;
                    let _ = pending.reply.send(Ok(Value::String(response)));
                }
                result = &mut activation => break result.map_err(|error| BrainError::Agentloop(error.to_string()))?,
            }
        };
        value
            .get("payload_json")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                BrainError::Agentloop("component activation returned no payload_json".into())
            })
    }
}

#[async_trait]
impl Agentloop for ComponentAgentloop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict> {
        let message = ctx.activation_message()?;
        let session_id = payload_session_id(&message)?;
        let (sender, mut receiver) = mpsc::channel(16);
        let binding = self
            .router
            .bind(
                brain_component_host::AGENTLOOP_COMPONENT,
                session_id.clone(),
                Arc::new(ChannelCapabilities { sender }),
            )
            .map_err(|error| BrainError::Agentloop(error.to_string()))?;
        let mut first_error = None;
        let needs_start = !self
            .started
            .lock()
            .expect("started sessions")
            .contains(&session_id);
        if needs_start {
            let hydration = ctx.session_start_payload().await?;
            let returned = self
                .activate(
                    &session_id,
                    "session_start",
                    hydration,
                    ctx,
                    &mut receiver,
                    &mut first_error,
                )
                .await?;
            check_session_start(&returned)?;
            self.started
                .lock()
                .expect("started sessions")
                .insert(session_id.clone());
        }
        let returned = self
            .activate(
                &session_id,
                "message",
                message,
                ctx,
                &mut receiver,
                &mut first_error,
            )
            .await;
        drop(binding);
        if let Some(error) = first_error {
            self.started
                .lock()
                .expect("started sessions")
                .remove(&session_id);
            return Err(error);
        }
        resolve_verdict(&returned?, ctx)
    }
}

struct PendingCapability {
    call: CapabilityCall,
    reply: oneshot::Sender<std::result::Result<Value, CapabilityFailure>>,
}

struct ChannelCapabilities {
    sender: mpsc::Sender<PendingCapability>,
}

#[async_trait]
impl CapabilityHandler for ChannelCapabilities {
    async fn call(&self, call: CapabilityCall) -> std::result::Result<Value, CapabilityFailure> {
        if call.capability != "agentloop.call" {
            return Err(CapabilityFailure {
                code: "capability_denied".into(),
                message: format!("an Agentloop cannot call {}", call.capability),
                retryable: false,
            });
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .send(PendingCapability { call, reply })
            .await
            .map_err(|_| channel_failure())?;
        response.await.map_err(|_| channel_failure())?
    }
}

fn channel_failure() -> CapabilityFailure {
    CapabilityFailure {
        code: "interrupted".into(),
        message: "the Agentloop activation is no longer running".into(),
        retryable: true,
    }
}
