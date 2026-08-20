//! Explicit SDK-attached Tool callbacks.
//!
//! Brain remains authoritative for the session and journal. This hub only binds one live worker
//! to a session and carries calls whose intents are already durable. A disconnected or cancelled
//! call is terminally interrupted and is never assigned or replayed to a later worker.

use crate::adapter::CallOutcome;
use crate::{BrainError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const MAX_FRAME_BYTES: usize = 128 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    Hello {
        token: String,
        callbacks: Vec<String>,
    },
    Result {
        call_id: String,
        ok: bool,
        #[serde(default)]
        output: Option<Value>,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    Ready,
    Call {
        call_id: String,
        callback_id: String,
        name: String,
        input: Value,
    },
    Abort {
        call_id: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug)]
pub struct AttachedResult {
    pub ok: bool,
    pub output: Option<Value>,
    pub error: Option<String>,
}

struct Worker {
    id: String,
    callbacks: HashSet<String>,
    outbound: mpsc::Sender<ServerFrame>,
    pending: Mutex<HashMap<String, oneshot::Sender<AttachedResult>>>,
}

#[derive(Default)]
pub struct AttachedHub {
    workers: Mutex<HashMap<String, Arc<Worker>>>,
}

pub struct Registration {
    pub outbound: mpsc::Receiver<ServerFrame>,
    session_id: String,
    worker_id: String,
    hub: Arc<AttachedHub>,
}

impl Registration {
    pub fn disconnect(self) {
        self.hub.unregister(&self.session_id, &self.worker_id);
    }
}

impl AttachedHub {
    pub fn register(
        self: &Arc<Self>,
        session_id: &str,
        callbacks: HashSet<String>,
        expected: &HashSet<String>,
    ) -> Result<Registration> {
        if &callbacks != expected {
            return Err(BrainError::Invalid(
                "attached worker callback identities do not match the session seal".into(),
            ));
        }
        let mut workers = self.workers.lock().expect("attached workers");
        if workers.contains_key(session_id) {
            return Err(BrainError::Invalid(
                "an attached worker is already connected for this session".into(),
            ));
        }
        let (outbound, receiver) = mpsc::channel(32);
        let worker_id = crate::mint_id("wrk", 20);
        workers.insert(
            session_id.to_string(),
            Arc::new(Worker {
                id: worker_id.clone(),
                callbacks,
                outbound,
                pending: Mutex::new(HashMap::new()),
            }),
        );
        Ok(Registration {
            outbound: receiver,
            session_id: session_id.to_string(),
            worker_id,
            hub: self.clone(),
        })
    }

    fn unregister(&self, session_id: &str, worker_id: &str) {
        let worker = {
            let mut workers = self.workers.lock().expect("attached workers");
            match workers.get(session_id) {
                Some(worker) if worker.id == worker_id => workers.remove(session_id),
                _ => None,
            }
        };
        if let Some(worker) = worker {
            worker.pending.lock().expect("attached pending").clear();
        }
    }

    pub fn complete(&self, session_id: &str, call_id: &str, result: AttachedResult) -> bool {
        let worker = self
            .workers
            .lock()
            .expect("attached workers")
            .get(session_id)
            .cloned();
        let Some(worker) = worker else {
            return false;
        };
        let sender = worker
            .pending
            .lock()
            .expect("attached pending")
            .remove(call_id);
        sender.is_some_and(|sender| sender.send(result).is_ok())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn call(
        &self,
        session_id: &str,
        callback_id: &str,
        call_id: &str,
        name: &str,
        input: Value,
        output_schema: &Value,
        cancel: CancellationToken,
    ) -> CallOutcome {
        let worker = self
            .workers
            .lock()
            .expect("attached workers")
            .get(session_id)
            .cloned();
        let Some(worker) = worker else {
            return CallOutcome::failed(format!(
                "attached callback {callback_id} is not connected"
            ));
        };
        if !worker.callbacks.contains(callback_id) {
            return CallOutcome::failed(format!(
                "attached callback {callback_id} is not advertised by this worker"
            ));
        }
        if serde_json::to_vec(&input).map_or(true, |bytes| bytes.len() > MAX_FRAME_BYTES) {
            return CallOutcome::failed("attached callback input exceeds 128 KiB");
        }
        let (result, wait) = oneshot::channel();
        if worker
            .pending
            .lock()
            .expect("attached pending")
            .insert(call_id.to_string(), result)
            .is_some()
        {
            return CallOutcome::failed("attached callback call id was already pending");
        }
        let frame = ServerFrame::Call {
            call_id: call_id.to_string(),
            callback_id: callback_id.to_string(),
            name: name.to_string(),
            input,
        };
        if worker.outbound.send(frame).await.is_err() {
            worker
                .pending
                .lock()
                .expect("attached pending")
                .remove(call_id);
            return interrupted("attached worker disconnected before dispatch");
        }
        let result = tokio::select! {
            result = wait => result.ok(),
            () = cancel.cancelled() => {
                worker.pending.lock().expect("attached pending").remove(call_id);
                let _ = worker.outbound.send(ServerFrame::Abort { call_id: call_id.to_string() }).await;
                return CallOutcome {
                    outcome: "cancelled".into(),
                    value: None,
                    content: "attached callback cancelled".into(),
                    is_error: true,
                    exit_code: None,
                    duration_ms: 0,
                    truncated: false,
                    terminal: None,
                };
            }
        };
        let Some(result) = result else {
            return interrupted("attached worker disconnected during the callback");
        };
        if !result.ok {
            return CallOutcome::failed(
                result
                    .error
                    .unwrap_or_else(|| "attached callback failed".into()),
            );
        }
        let Some(output) = result.output else {
            return CallOutcome::failed("attached callback returned no output");
        };
        if serde_json::to_vec(&output).map_or(true, |bytes| bytes.len() > MAX_FRAME_BYTES) {
            return CallOutcome::failed("attached callback output exceeds 128 KiB");
        }
        let validator = match jsonschema::draft202012::new(output_schema) {
            Ok(validator) => validator,
            Err(error) => return CallOutcome::failed(format!("attached output schema: {error}")),
        };
        if let Some(error) = validator.iter_errors(&output).next() {
            return CallOutcome::failed(format!(
                "attached callback output{}: {error}",
                error.instance_path()
            ));
        }
        CallOutcome {
            outcome: "completed".into(),
            value: Some(output.clone()),
            content: output.to_string(),
            is_error: false,
            exit_code: None,
            duration_ms: 0,
            truncated: false,
            terminal: None,
        }
    }
}

fn interrupted(message: &str) -> CallOutcome {
    CallOutcome {
        outcome: "interrupted".into(),
        value: None,
        content: message.into(),
        is_error: true,
        exit_code: None,
        duration_ms: 0,
        truncated: false,
        terminal: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;
    use std::time::Duration;

    fn callbacks() -> HashSet<String> {
        HashSet::from(["callback.echo".to_string()])
    }

    fn output_schema() -> Value {
        json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
            "additionalProperties": false
        })
    }

    #[test]
    fn registration_requires_the_exact_seal_and_one_worker() {
        let hub = Arc::new(AttachedHub::default());
        assert!(hub.register("ses", HashSet::new(), &callbacks()).is_err());
        let registration = hub.register("ses", callbacks(), &callbacks()).unwrap();
        assert!(hub.register("ses", callbacks(), &callbacks()).is_err());
        registration.disconnect();
        assert!(hub.register("ses", callbacks(), &callbacks()).is_ok());
    }

    #[tokio::test]
    async fn completion_is_exactly_once_and_output_is_validated() {
        let hub = Arc::new(AttachedHub::default());
        let mut registration = hub.register("ses", callbacks(), &callbacks()).unwrap();
        let caller = {
            let hub = hub.clone();
            tokio::spawn(async move {
                hub.call(
                    "ses",
                    "callback.echo",
                    "call-1",
                    "renamed_echo",
                    json!({"text": "in"}),
                    &output_schema(),
                    CancellationToken::new(),
                )
                .await
            })
        };
        assert!(matches!(
            registration.outbound.recv().await,
            Some(ServerFrame::Call { call_id, name, .. })
                if call_id == "call-1" && name == "renamed_echo"
        ));
        assert!(hub.complete(
            "ses",
            "call-1",
            AttachedResult {
                ok: true,
                output: Some(json!({"text": "out"})),
                error: None,
            },
        ));
        assert!(!hub.complete(
            "ses",
            "call-1",
            AttachedResult {
                ok: true,
                output: Some(json!({"text": "duplicate"})),
                error: None,
            },
        ));
        let outcome = caller.await.unwrap();
        assert_eq!(outcome.outcome, "completed");
        assert_eq!(outcome.value, Some(json!({"text": "out"})));
    }

    #[tokio::test]
    async fn disconnect_interrupts_an_ambiguous_call_and_never_reassigns_it() {
        let hub = Arc::new(AttachedHub::default());
        let mut registration = hub.register("ses", callbacks(), &callbacks()).unwrap();
        let caller = {
            let hub = hub.clone();
            tokio::spawn(async move {
                hub.call(
                    "ses",
                    "callback.echo",
                    "call-ambiguous",
                    "echo",
                    json!({}),
                    &output_schema(),
                    CancellationToken::new(),
                )
                .await
            })
        };
        assert!(matches!(
            registration.outbound.recv().await,
            Some(ServerFrame::Call { call_id, .. }) if call_id == "call-ambiguous"
        ));
        registration.disconnect();
        let outcome = caller.await.unwrap();
        assert_eq!(outcome.outcome, "interrupted");
        assert!(!hub.complete(
            "ses",
            "call-ambiguous",
            AttachedResult {
                ok: true,
                output: Some(json!({"text": "late"})),
                error: None
            },
        ));

        let mut replacement = hub.register("ses", callbacks(), &callbacks()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), replacement.outbound.recv())
                .await
                .is_err(),
            "an ambiguous call must not be assigned to a replacement worker"
        );
    }

    #[tokio::test]
    async fn cancellation_removes_the_call_and_sends_abort() {
        let hub = Arc::new(AttachedHub::default());
        let mut registration = hub.register("ses", callbacks(), &callbacks()).unwrap();
        let cancel = CancellationToken::new();
        let caller = {
            let hub = hub.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                hub.call(
                    "ses",
                    "callback.echo",
                    "call-cancel",
                    "echo",
                    json!({}),
                    &output_schema(),
                    cancel,
                )
                .await
            })
        };
        assert!(matches!(
            registration.outbound.recv().await,
            Some(ServerFrame::Call { .. })
        ));
        cancel.cancel();
        assert!(matches!(
            registration.outbound.recv().await,
            Some(ServerFrame::Abort { call_id }) if call_id == "call-cancel"
        ));
        assert_eq!(caller.await.unwrap().outcome, "cancelled");
        assert!(!hub.complete(
            "ses",
            "call-cancel",
            AttachedResult {
                ok: true,
                output: Some(json!({"text": "late"})),
                error: None
            },
        ));
    }
}
