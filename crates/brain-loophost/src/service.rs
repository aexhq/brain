//! What the worker process does with a request, independent of the socket it arrived on.
//!
//! One worker holds every admitted component, because compiling one costs seconds and the
//! compiled form is what makes an activation cost milliseconds. It therefore has to serve
//! more than one session at a time: reading one request, running it to completion and only
//! then reading the next made the whole of Brain as concurrent as a single agent loop,
//! whatever the supervisor allowed through.
//!
//! Concurrency stays explicitly bounded. Each activation is a live Wasm instance, so the
//! number running at once is what sets the worker's memory ceiling.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use tokio::sync::Semaphore;

use crate::{
    AdmissionEngine, AdmittedAgentloop, LoopLimits, ResidentContexts, WarmInstances, WorkerRequest,
    WorkerResponse,
};

pub struct WorkerService {
    engine: Arc<AdmissionEngine>,
    /// Read on every activation, written only by admission.
    admitted: RwLock<HashMap<String, Arc<AdmittedAgentloop>>>,
    /// The memory ceiling, expressed as instances rather than bytes. Requests wait here
    /// rather than being refused: the supervisor already refused everything beyond its
    /// own queue, so whatever reaches this point is worth a slot.
    running: Semaphore,
    /// Warm instances, one per recently active session, so a turn's activations do not
    /// pay instantiation and state re-parsing for a conversation this worker just held.
    warm: Arc<WarmInstances>,
    /// The context of every mid-flight turn, held between a turn's activation legs.
    resident: Arc<ResidentContexts>,
}

impl WorkerService {
    pub fn new(limits: LoopLimits, allowed_imports: Vec<String>) -> Result<Self, String> {
        let running = Semaphore::new(limits.concurrent_activations_per_worker.max(1));
        Ok(Self {
            engine: Arc::new(AdmissionEngine::new(limits, allowed_imports)?),
            admitted: RwLock::new(HashMap::new()),
            running,
            warm: Arc::new(WarmInstances::default()),
            resident: Arc::new(ResidentContexts::default()),
        })
    }

    /// Read one request from `stream`, answer it, and write the answer back.
    pub async fn serve<S>(self: Arc<Self>, stream: &mut S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let response = match crate::worker_read(stream).await {
            Ok(request) => self.handle(request).await,
            Err(message) => failed("invalid_frame", message),
        };
        // The caller is waiting on this connection and nothing else. A failure to answer
        // is its timeout to notice, not something this task can repair.
        let _ = crate::worker_write(stream, &response).await;
    }

    pub async fn handle(self: Arc<Self>, request: WorkerRequest) -> WorkerResponse {
        match request {
            WorkerRequest::Ping => WorkerResponse::Pong,
            WorkerRequest::Admit { package_json } => self.admit(package_json).await,
            WorkerRequest::Activate {
                digest,
                session,
                context_attached,
                input,
            } => {
                self.activate(
                    digest.as_str().to_owned(),
                    session,
                    context_attached,
                    *input,
                )
                .await
            }
        }
    }

    async fn admit(&self, package_json: String) -> WorkerResponse {
        let engine = self.engine.clone();
        // Compiling a component is seconds of CPU. It does not belong on the runtime's
        // threads any more than an activation does.
        let compiled =
            tokio::task::spawn_blocking(move || engine.admit(package_json.as_bytes())).await;
        let component = match compiled {
            Ok(Ok(component)) => component,
            Ok(Err(message)) => return failed("admission_failed", message),
            Err(_) => return failed("admission_failed", "the worker stopped compiling".into()),
        };
        let digest = component.digest.clone();
        match self.admitted.write() {
            Ok(mut admitted) => {
                admitted.insert(digest.as_str().to_owned(), Arc::new(component));
                WorkerResponse::Admitted { digest }
            }
            Err(_) => failed("admission_failed", "the worker lost its state".into()),
        }
    }

    async fn activate(
        &self,
        digest: String,
        session: String,
        context_attached: bool,
        input: brain_protocol::ActivationInput,
    ) -> WorkerResponse {
        let component = self
            .admitted
            .read()
            .ok()
            .and_then(|admitted| admitted.get(&digest).cloned());
        let Some(component) = component else {
            return failed(
                "not_admitted",
                "Agentloop digest is not admitted in this worker".into(),
            );
        };
        // Held across the whole activation, so what it counts is instances alive rather
        // than requests started.
        let Ok(_slot) = self.running.acquire().await else {
            return failed("activation_failed", "the worker is shutting down".into());
        };
        let engine = self.engine.clone();
        let warm = self.warm.clone();
        let resident = self.resident.clone();
        // Wasmtime executes synchronously. Left on a runtime thread it blocks every other
        // connection this worker is serving for as long as the guest runs.
        let activated = tokio::task::spawn_blocking(move || {
            component.activate(
                engine.engine(),
                engine.limits(),
                &warm,
                &resident,
                &session,
                context_attached,
                input,
            )
        })
        .await;
        match activated {
            Ok(Ok((output, context_attached))) => WorkerResponse::Activated {
                output,
                context_attached,
            },
            Ok(Err(message)) if message.starts_with("context_required") => {
                failed("context_required", message)
            }
            Ok(Err(message)) => failed("activation_failed", message),
            Err(_) => failed("activation_failed", "the activation was lost".into()),
        }
    }
}

fn failed(code: &str, message: String) -> WorkerResponse {
    WorkerResponse::Error {
        code: code.to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn service() -> Arc<WorkerService> {
        Arc::new(WorkerService::new(LoopLimits::default(), Vec::new()).unwrap())
    }

    fn input() -> brain_protocol::ActivationInput {
        brain_protocol::ActivationInput {
            context: brain_protocol::ContextEnvelope {
                protocol_version: "agentloop/v1".into(),
                items: Vec::new(),
                state: None,
            },
            observation: brain_protocol::Observation::SessionStarted {
                history: Vec::new(),
            },
            configuration: serde_json::json!({}),
            presentation: brain_protocol::Presentation {
                bytes: Vec::new(),
                identity: brain_protocol::Identity::of_bytes(b"presentation"),
            },
            runtime: brain_protocol::RuntimeEnvelope::at(
                &brain_protocol::JournalId::new("jrn_test"),
                1,
                0,
            ),
        }
    }

    /// Reading one request, running it to completion and only then reading the next made
    /// every session in the process wait for the one in front of it. Requests that do not
    /// depend on each other must not.
    #[tokio::test]
    async fn requests_are_answered_without_waiting_for_each_other() {
        let service = service();
        let answers = async {
            tokio::join!(
                service.clone().handle(WorkerRequest::Ping),
                service.clone().handle(WorkerRequest::Ping),
                service.clone().handle(WorkerRequest::Activate {
                    session: "ses_test".into(),
                    context_attached: true,
                    digest: brain_protocol::AgentloopIdentity::new("agl_missing"),
                    input: Box::new(input()),
                }),
            )
        };
        let (first, second, third) = tokio::time::timeout(Duration::from_secs(5), answers)
            .await
            .expect("concurrent requests must not deadlock on each other");

        assert!(matches!(first, WorkerResponse::Pong));
        assert!(matches!(second, WorkerResponse::Pong));
        assert!(
            matches!(third, WorkerResponse::Error { ref code, .. } if code == "not_admitted"),
            "expected a miss, got {third:?}"
        );
    }

    /// A request that arrives over a connection is answered over the same connection.
    #[tokio::test]
    async fn a_request_on_a_connection_is_answered_on_it() {
        let service = service();
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn(async move { service.serve(&mut server).await });

        crate::wire::write_frame(
            &mut client,
            &WorkerRequest::Ping,
            crate::wire::MAX_RESPONSE_FRAME_BYTES,
        )
        .await
        .unwrap();
        let response: WorkerResponse =
            crate::wire::read_frame(&mut client, crate::wire::MAX_RESPONSE_FRAME_BYTES)
                .await
                .unwrap();

        assert!(matches!(response, WorkerResponse::Pong));
        serving.await.unwrap();
    }
}
