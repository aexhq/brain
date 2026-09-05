//! One bounded Wasmtime worker. It caches compiled Components only; every invocation
//! receives a fresh Store and instance.

use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use brain_protocol::{TurnError, TurnInput, codes};
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::{
    AdmissionEngine, AdmittedAgentloop, AdmittedTool, ComponentKind, GuestHost, HostCall,
    LoopLimits, NativeEnvironment, NativeToolInput, WorkerRequest, WorkerResponse,
    wire::{MAX_RESPONSE_FRAME_BYTES, read_frame, write_frame},
};

pub struct WorkerService {
    engine: Arc<AdmissionEngine>,
    agentloops: RwLock<HashMap<String, Arc<AdmittedAgentloop>>>,
    tools: RwLock<HashMap<String, Arc<AdmittedTool>>>,
    running: Semaphore,
}

type Outbound = (HostCall, Option<oneshot::Sender<Result<String, TurnError>>>);

struct ConnectionBridge {
    outbound: mpsc::Sender<Outbound>,
    cancelled: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl GuestHost for ConnectionBridge {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        if matches!(call, HostCall::Telemetry { .. }) {
            let _ = self.outbound.send((call, None)).await;
            return Ok(String::new());
        }
        let (reply, answer) = oneshot::channel();
        self.outbound
            .send((call, Some(reply)))
            .await
            .map_err(|_| TurnError::new("connection_lost", "the invocation connection is gone"))?;
        answer.await.unwrap_or_else(|_| Err(cancelled()))
    }
}

fn cancelled() -> TurnError {
    TurnError::new(codes::failure::CANCELLED, "the invocation was cancelled")
}

impl WorkerService {
    pub fn new(limits: LoopLimits, allowed_imports: Vec<String>) -> Result<Self, String> {
        let running = Semaphore::new(limits.concurrent_turns_per_worker.max(1));
        Ok(Self {
            engine: Arc::new(AdmissionEngine::new(limits, allowed_imports)?),
            agentloops: RwLock::new(HashMap::new()),
            tools: RwLock::new(HashMap::new()),
            running,
        })
    }

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
            WorkerRequest::Admit {
                kind,
                component_base64,
            } => {
                let response = self.admit(kind, component_base64).await;
                let _ = crate::worker_write(stream, &response).await;
            }
            WorkerRequest::Turn {
                digest,
                environment,
                input,
            } => {
                self.turn(stream, digest.as_str(), environment, *input)
                    .await;
            }
            WorkerRequest::Tool {
                digest,
                environment,
                call_id,
                input,
                configuration,
                deadline_at_ms,
            } => {
                self.tool(
                    stream,
                    digest.as_str(),
                    environment,
                    NativeToolInput {
                        call_id,
                        input,
                        configuration,
                        deadline_at_ms,
                    },
                )
                .await;
            }
            WorkerRequest::HostResult { .. } | WorkerRequest::Cancel => {
                let _ = crate::worker_write(
                    stream,
                    &failed(
                        "invalid_frame",
                        "an invocation has not been opened on this connection".into(),
                    ),
                )
                .await;
            }
        }
    }

    async fn admit(&self, kind: ComponentKind, component_base64: String) -> WorkerResponse {
        use base64::Engine as _;
        let component = match base64::engine::general_purpose::STANDARD.decode(component_base64) {
            Ok(component) => component,
            Err(error) => return failed("admission_failed", error.to_string()),
        };
        let engine = self.engine.clone();
        match kind {
            ComponentKind::Agentloop => {
                let compiled = tokio::task::spawn_blocking(move || engine.admit(&component)).await;
                let component = match compiled {
                    Ok(Ok(component)) => component,
                    Ok(Err(message)) => return failed("admission_failed", message),
                    Err(_) => {
                        return failed("admission_failed", "the worker stopped compiling".into());
                    }
                };
                let digest = component.digest.as_str().to_owned();
                match self.agentloops.write() {
                    Ok(mut admitted) => {
                        admitted.insert(digest.clone(), Arc::new(component));
                        WorkerResponse::Admitted { digest }
                    }
                    Err(_) => failed("admission_failed", "the worker lost its state".into()),
                }
            }
            ComponentKind::Tool => {
                let compiled =
                    tokio::task::spawn_blocking(move || engine.admit_tool(&component)).await;
                let component = match compiled {
                    Ok(Ok(component)) => component,
                    Ok(Err(message)) => return failed("admission_failed", message),
                    Err(_) => {
                        return failed("admission_failed", "the worker stopped compiling".into());
                    }
                };
                let digest = component.digest.as_str().to_owned();
                match self.tools.write() {
                    Ok(mut admitted) => {
                        admitted.insert(digest.clone(), Arc::new(component));
                        WorkerResponse::Admitted { digest }
                    }
                    Err(_) => failed("admission_failed", "the worker lost its state".into()),
                }
            }
        }
    }

    async fn turn<S>(
        &self,
        stream: &mut S,
        digest: &str,
        environment: NativeEnvironment,
        input: TurnInput,
    ) where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let component = self
            .agentloops
            .read()
            .ok()
            .and_then(|admitted| admitted.get(digest).cloned());
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
        let Ok(_slot) = self.running.acquire().await else {
            let _ = crate::worker_write(
                stream,
                &failed("turn_failed", "the worker is shutting down".into()),
            )
            .await;
            return;
        };
        let engine = self.engine.clone();
        self.converse(stream, move |bridge| async move {
            match component
                .turn(engine.engine(), engine.limits(), environment, input, bridge)
                .await
            {
                Ok(output) => WorkerResponse::Turned { output },
                Err(error) => WorkerResponse::TurnFailed { error },
            }
        })
        .await;
    }

    async fn tool<S>(
        &self,
        stream: &mut S,
        digest: &str,
        environment: NativeEnvironment,
        input: NativeToolInput,
    ) where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let component = self
            .tools
            .read()
            .ok()
            .and_then(|admitted| admitted.get(digest).cloned());
        let Some(component) = component else {
            let _ = crate::worker_write(
                stream,
                &failed(
                    "not_admitted",
                    "Tool digest is not admitted in this worker".into(),
                ),
            )
            .await;
            return;
        };
        let Ok(_slot) = self.running.acquire().await else {
            let _ = crate::worker_write(
                stream,
                &failed("tool_failed", "the worker is shutting down".into()),
            )
            .await;
            return;
        };
        let engine = self.engine.clone();
        self.converse(stream, move |bridge| async move {
            match component
                .run(engine.engine(), engine.limits(), environment, input, bridge)
                .await
            {
                Ok(output) => WorkerResponse::ToolRan { output },
                Err(error) => WorkerResponse::TurnFailed { error },
            }
        })
        .await;
    }

    async fn converse<S, Make, Running>(&self, stream: &mut S, make: Make)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        Make: FnOnce(Arc<dyn GuestHost>) -> Running,
        Running: Future<Output = WorkerResponse>,
    {
        let (outbound, mut inbound) = mpsc::channel::<Outbound>(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let bridge: Arc<dyn GuestHost> = Arc::new(ConnectionBridge {
            outbound,
            cancelled: cancelled.clone(),
        });
        let mut running = Box::pin(make(bridge));
        let pending: Mutex<HashMap<u64, oneshot::Sender<Result<String, TurnError>>>> =
            Mutex::new(HashMap::new());
        let mut next_id = 0_u64;
        let (mut reader, mut writer) = tokio::io::split(&mut *stream);
        let response = 'invocation: loop {
            let mut next_frame = std::pin::pin!(read_frame::<_, WorkerRequest>(
                &mut reader,
                crate::MAX_TURN_INPUT_BYTES + 1_024,
            ));
            loop {
                tokio::select! {
                    Some((call, reply)) = inbound.recv() => {
                        next_id += 1;
                        if let (Some(reply), Ok(mut pending)) = (reply, pending.lock()) {
                            pending.insert(next_id, reply);
                        }
                        if write_frame(&mut writer, &WorkerResponse::HostCall { id: next_id, call }, MAX_RESPONSE_FRAME_BYTES).await.is_err() {
                            cancelled.store(true, Ordering::Release);
                            fail_pending(&pending);
                            return;
                        }
                    }
                    frame = &mut next_frame => {
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
                                break 'invocation WorkerResponse::TurnFailed {
                                    error: crate::service::cancelled(),
                                };
                            }
                            Ok(_) => break 'invocation failed("invalid_frame", "unexpected frame during an invocation".into()),
                            Err(_) => {
                                cancelled.store(true, Ordering::Release);
                                fail_pending(&pending);
                                return;
                            }
                        }
                        continue 'invocation;
                    }
                    finished = &mut running => break 'invocation finished,
                }
            }
        };
        drop(running);
        drop(reader);
        drop(writer);
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

fn failed(code: &str, message: String) -> WorkerResponse {
    WorkerResponse::Error {
        code: code.to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::pending,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

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

    #[tokio::test]
    async fn an_unknown_loop_is_refused_without_blocking_others() {
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
                environment: NativeEnvironment {
                    scratch: false,
                    workspace: None,
                    network_allow: Vec::new(),
                    secrets: Default::default(),
                },
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

    struct DropNotice(Arc<AtomicBool>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn cancelling_an_invocation_drops_its_guest() {
        let service = service();
        let started = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let (mut client, mut server) = tokio::io::duplex(1024);
        let serving = tokio::spawn({
            let service = service.clone();
            let started = started.clone();
            let dropped = dropped.clone();
            async move {
                service
                    .converse(&mut server, move |_| async move {
                        let _notice = DropNotice(dropped);
                        started.store(true, Ordering::Release);
                        pending::<()>().await;
                        WorkerResponse::Pong
                    })
                    .await;
            }
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            while !started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        write_frame(&mut client, &WorkerRequest::Cancel, 1024)
            .await
            .unwrap();
        let response: WorkerResponse = tokio::time::timeout(
            Duration::from_secs(5),
            read_frame(&mut client, MAX_RESPONSE_FRAME_BYTES),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            matches!(response, WorkerResponse::TurnFailed { ref error }
                if error.code == codes::failure::CANCELLED),
            "expected cancellation, got {response:?}"
        );
        tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap();
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn losing_an_invocation_connection_drops_its_guest() {
        let service = service();
        let started = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let (client, mut server) = tokio::io::duplex(1024);
        let serving = tokio::spawn({
            let service = service.clone();
            let started = started.clone();
            let dropped = dropped.clone();
            async move {
                service
                    .converse(&mut server, move |_| async move {
                        let _notice = DropNotice(dropped);
                        started.store(true, Ordering::Release);
                        pending::<()>().await;
                        WorkerResponse::Pong
                    })
                    .await;
            }
        });

        tokio::time::timeout(Duration::from_secs(5), async {
            while !started.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(client);
        tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap();
        assert!(dropped.load(Ordering::Acquire));
    }
}
