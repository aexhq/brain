//! What the worker process does with a connection, independent of the socket it arrived
//! on.
//!
//! One worker holds every admitted component, because compiling one costs seconds and the
//! compiled form is what makes a turn cost milliseconds. It serves many sessions at once,
//! each turn on its own connection and its own blocking thread; what runs at once stays
//! explicitly bounded, because each turn is a live Wasm instance.
//!
//! A turn's connection is a conversation: the guest's host calls go out as frames and
//! wait for their results; a cancel from the server fails every pending call.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock, atomic::AtomicBool, atomic::Ordering},
};

use brain_protocol::{TurnError, TurnInput, codes};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::{
    AdmissionEngine, AdmittedAgentloop, GuestHost, HostCall, LoopLimits, WarmInstances,
    WorkerRequest, WorkerResponse,
    wire::{MAX_RESPONSE_FRAME_BYTES, read_frame, write_frame},
};

pub struct WorkerService {
    engine: Arc<AdmissionEngine>,
    /// Read on every turn, written only by admission.
    admitted: RwLock<HashMap<String, Arc<AdmittedAgentloop>>>,
    /// The memory ceiling, expressed as instances rather than bytes. Requests wait here
    /// rather than being refused: the supervisor already refused everything beyond its
    /// own queue, so whatever reaches this point is worth a slot.
    running: Semaphore,
    /// Warm instances, one per recently active session, so a turn does not pay
    /// instantiation for a conversation this worker just held.
    warm: Arc<WarmInstances>,
}

/// A host call on its way from the guest's thread to the connection, with the channel
/// its answer comes back on. `None` for a call that wants no answer.
type Outbound = (HostCall, Option<oneshot::Sender<Result<String, TurnError>>>);

/// The guest's side of the bridge: a channel to the connection task, and the cancel flag
/// that fails every call once the server has said stop.
struct ConnectionBridge {
    outbound: mpsc::UnboundedSender<Outbound>,
    cancelled: Arc<AtomicBool>,
}

impl GuestHost for ConnectionBridge {
    fn call(&self, call: HostCall) -> Result<String, TurnError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        if matches!(call, HostCall::Telemetry { .. }) {
            let _ = self.outbound.send((call, None));
            return Ok(String::new());
        }
        let (reply, answer) = oneshot::channel();
        self.outbound
            .send((call, Some(reply)))
            .map_err(|_| TurnError::new("connection_lost", "the turn's connection is gone"))?;
        answer.blocking_recv().unwrap_or_else(|_| Err(cancelled()))
    }
}

fn cancelled() -> TurnError {
    TurnError::new(codes::failure::CANCELLED, "the turn was cancelled")
}

impl WorkerService {
    pub fn new(limits: LoopLimits, allowed_imports: Vec<String>) -> Result<Self, String> {
        let running = Semaphore::new(limits.concurrent_turns_per_worker.max(1));
        Ok(Self {
            engine: Arc::new(AdmissionEngine::new(limits, allowed_imports)?),
            admitted: RwLock::new(HashMap::new()),
            running,
            warm: Arc::new(WarmInstances::default()),
        })
    }

    /// Serve one connection: a ping or an admission is one answer; a turn holds the
    /// connection until the guest is done.
    pub async fn serve<S>(self: Arc<Self>, stream: &mut S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let request = match crate::worker_read(stream).await {
            Ok(request) => request,
            Err(message) => {
                let _ = crate::worker_write(stream, &failed("invalid_frame", message)).await;
                return;
            }
        };
        match request {
            WorkerRequest::Ping => {
                let _ = crate::worker_write(stream, &WorkerResponse::Pong).await;
            }
            WorkerRequest::Admit { package_json } => {
                let response = self.admit(package_json).await;
                let _ = crate::worker_write(stream, &response).await;
            }
            WorkerRequest::Turn {
                digest,
                session,
                input,
            } => {
                self.turn(stream, digest.as_str().to_owned(), session, *input)
                    .await;
            }
            WorkerRequest::HostResult { .. } | WorkerRequest::Cancel => {
                let _ = crate::worker_write(
                    stream,
                    &failed(
                        "invalid_frame",
                        "a turn has not been opened on this connection".into(),
                    ),
                )
                .await;
            }
        }
    }

    async fn admit(&self, package_json: String) -> WorkerResponse {
        let engine = self.engine.clone();
        // Compiling a component is seconds of CPU. It does not belong on the runtime's
        // threads any more than a turn does.
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

    async fn turn<S>(&self, stream: &mut S, digest: String, session: String, input: TurnInput)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let component = self
            .admitted
            .read()
            .ok()
            .and_then(|admitted| admitted.get(&digest).cloned());
        let Some(component) = component else {
            let _ = crate::worker_write(
                stream,
                &failed(
                    "not_admitted",
                    "Agentloop digest is not admitted in this worker".into(),
                ),
            )
            .await;
            return;
        };
        // Held across the whole turn, so what it counts is instances alive rather than
        // requests started.
        let Ok(_slot) = self.running.acquire().await else {
            let _ = crate::worker_write(
                stream,
                &failed("turn_failed", "the worker is shutting down".into()),
            )
            .await;
            return;
        };
        let (outbound, mut inbound) = mpsc::unbounded_channel::<Outbound>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let bridge: Arc<dyn GuestHost> = Arc::new(ConnectionBridge {
            outbound,
            cancelled: cancelled.clone(),
        });
        let engine = self.engine.clone();
        let warm = self.warm.clone();
        // Wasmtime executes synchronously. Left on a runtime thread it blocks every other
        // connection this worker is serving for as long as the guest runs.
        let mut running = tokio::task::spawn_blocking(move || {
            component.turn(
                engine.engine(),
                engine.limits(),
                &warm,
                &session,
                input,
                bridge,
            )
        });
        let pending: Mutex<HashMap<u64, oneshot::Sender<Result<String, TurnError>>>> =
            Mutex::new(HashMap::new());
        let mut next_id = 0_u64;
        let response = loop {
            tokio::select! {
                Some((call, reply)) = inbound.recv() => {
                    next_id += 1;
                    if let (Some(reply), Ok(mut pending)) = (reply, pending.lock()) {
                        pending.insert(next_id, reply);
                    }
                    if write_frame(stream, &WorkerResponse::HostCall { id: next_id, call }, MAX_RESPONSE_FRAME_BYTES).await.is_err() {
                        // The server is gone: the guest's call fails, the turn ends.
                        cancelled.store(true, Ordering::Release);
                        fail_pending(&pending);
                        let _ = (&mut running).await;
                        return;
                    }
                    let _ = error_never(&());
                }
                frame = read_frame::<_, WorkerRequest>(stream, crate::MAX_TURN_INPUT_BYTES + 1_024) => {
                    match frame {
                        Ok(WorkerRequest::HostResult { id, result }) => {
                            let reply = pending.lock().ok().and_then(|mut pending| pending.remove(&id));
                            if let Some(reply) = reply {
                                let _ = reply.send(result);
                            }
                        }
                        Ok(WorkerRequest::Cancel) => {
                            cancelled.store(true, Ordering::Release);
                            fail_pending(&pending);
                        }
                        Ok(_) => {
                            break failed("invalid_frame", "unexpected frame during a turn".into());
                        }
                        Err(_) => {
                            // The connection closed under the turn. Nothing to answer to.
                            cancelled.store(true, Ordering::Release);
                            fail_pending(&pending);
                            let _ = (&mut running).await;
                            return;
                        }
                    }
                }
                finished = &mut running => {
                    break match finished {
                        Ok(Ok(output)) => WorkerResponse::Turned { output },
                        Ok(Err(message)) => failed("turn_failed", message),
                        Err(_) => failed("turn_failed", "the turn was lost".into()),
                    };
                }
            }
        };
        let _ = crate::worker_write(stream, &response).await;
    }
}

fn fail_pending(pending: &Mutex<HashMap<u64, oneshot::Sender<Result<String, TurnError>>>>) {
    if let Ok(mut pending) = pending.lock() {
        for (_, reply) in pending.drain() {
            let _ = reply.send(Err(cancelled()));
        }
    }
}

// Keeps the select arm's `let _ =` shape uniform; optimised away.
fn error_never(_: &()) -> Result<(), ()> {
    Ok(())
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

    fn input() -> TurnInput {
        TurnInput {
            input: "hello".into(),
            transcript: Vec::new(),
            slots: Default::default(),
            events: Vec::new(),
            configuration: serde_json::json!({}),
            system: String::new(),
            tools: Vec::new(),
            runtime: brain_protocol::RuntimeEnvelope::at(
                &brain_protocol::SessionId::new("ses_test"),
                1,
            ),
        }
    }

    /// A connection that opens a turn for a loop nobody admitted is answered, and other
    /// connections are answered meanwhile.
    #[tokio::test]
    async fn a_turn_for_an_unknown_loop_is_refused_without_blocking_others() {
        let service = service();
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let service = service.clone();
            async move { service.serve(&mut server).await }
        });
        write_frame(
            &mut client,
            &WorkerRequest::Turn {
                digest: brain_protocol::AgentloopIdentity::new("agl_missing"),
                session: "ses_test".into(),
                input: Box::new(input()),
            },
            MAX_RESPONSE_FRAME_BYTES,
        )
        .await
        .unwrap();
        let (mut ping_client, mut ping_server) = tokio::io::duplex(1024);
        let pinging = tokio::spawn({
            let service = service.clone();
            async move { service.serve(&mut ping_server).await }
        });
        write_frame(&mut ping_client, &WorkerRequest::Ping, 1024)
            .await
            .unwrap();
        let pong: WorkerResponse = read_frame(&mut ping_client, MAX_RESPONSE_FRAME_BYTES)
            .await
            .unwrap();
        assert!(matches!(pong, WorkerResponse::Pong));
        let refused: WorkerResponse = tokio::time::timeout(
            Duration::from_secs(5),
            read_frame(&mut client, MAX_RESPONSE_FRAME_BYTES),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            matches!(refused, WorkerResponse::Error { ref code, .. } if code == "not_admitted"),
            "expected a miss, got {refused:?}"
        );
        serving.await.unwrap();
        pinging.await.unwrap();
    }
}
