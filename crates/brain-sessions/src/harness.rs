//! JSON-RPC process adapter. The process is deployment code, never session input.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use brain::{AgentHarness, Error, TurnServices, environment::ExecutionServices};
use brain_protocol::{
    HARNESS_PROTOCOL, HarnessCallback, HarnessCommand, HarnessIdentity, HarnessInitialize,
    HarnessRun, ModelDef, SessionConfig, SessionId, TurnInput, TurnOutput,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex as AsyncMutex, RwLock, oneshot},
};

use crate::SessionServices;

type Reply = oneshot::Sender<Result<Value, Error>>;

struct Activation {
    services: SessionServices,
    fatal: Mutex<Option<Error>>,
    calls: RwLock<()>,
    closed: AtomicBool,
}

#[derive(Default)]
struct State {
    pending: HashMap<u64, Reply>,
    active: HashMap<(SessionId, u64), Arc<Activation>>,
    failed: Option<String>,
}

struct Pipe {
    stdin: AsyncMutex<ChildStdin>,
    child: AsyncMutex<Child>,
    state: Mutex<State>,
    next: AtomicU64,
}

impl Pipe {
    async fn write(&self, frame: Value) -> Result<(), Error> {
        let mut bytes = serde_json::to_vec(&frame).map_err(json_error)?;
        bytes.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|error| Error::Ambiguous(error.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|error| Error::Ambiguous(error.to_string()))
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.state.lock().expect("harness state lock");
            if let Some(message) = &state.failed {
                return Err(Error::Ambiguous(message.clone()));
            }
            state.pending.insert(id, tx);
        }
        if let Err(error) = self
            .write(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .await
        {
            self.fail(&error.to_string());
        }
        rx.await
            .map_err(|_| Error::Ambiguous("harness response channel closed".into()))?
    }

    fn fail(&self, message: &str) {
        let mut state = self.state.lock().expect("harness state lock");
        state.failed.get_or_insert_with(|| message.into());
        for (_, reply) in state.pending.drain() {
            let _ = reply.send(Err(Error::Ambiguous(message.into())));
        }
    }

    async fn stop(&self, message: &str) {
        self.fail(message);
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    async fn callback(&self, frame: Value) {
        let id = frame["id"].clone();
        let result = self.call(frame).await;
        let response = match result {
            Ok(value) => json!({"jsonrpc":"2.0", "id":id, "result":value}),
            Err(error) => json!({"jsonrpc":"2.0", "id":id, "error":{
                "code":-32000, "message":error.to_string(),
                "data":{"code":error.code(), "fatal":fatal(&error)}
            }}),
        };
        if let Err(error) = self.write(response).await {
            self.fail(&error.to_string());
        }
    }

    async fn call(&self, frame: Value) -> Result<Value, Error> {
        if frame["method"] != "brain/call" {
            return Err(Error::InvalidState("unknown harness callback".into()));
        }
        let call: HarnessCallback =
            serde_json::from_value(frame["params"].clone()).map_err(json_error)?;
        let activation = self
            .state
            .lock()
            .expect("harness state lock")
            .active
            .get(&(call.session_id, call.sequence))
            .cloned()
            .ok_or_else(|| Error::InvalidState("harness activation is not active".into()))?;
        let _call = activation.calls.read().await;
        if activation.closed.load(Ordering::Acquire) {
            return Err(Error::InvalidState("harness activation has ended".into()));
        }
        if activation
            .fatal
            .lock()
            .expect("harness failure lock")
            .is_some()
        {
            return Err(Error::Journal(
                "activation stopped after a fatal callback failure".into(),
            ));
        }
        let result = activation.services.call(&call.method, call.input).await;
        match result {
            Err(error) if fatal(&error) => {
                let message = error.to_string();
                *activation.fatal.lock().expect("harness failure lock") = Some(error);
                Err(Error::Journal(message))
            }
            result => result,
        }
    }
}

fn fatal(error: &Error) -> bool {
    matches!(
        error,
        Error::Journal(_) | Error::Ambiguous(_) | Error::Cancelled(_) | Error::Budget(_)
    )
}

/// One harness for the lifetime of this Brain instance. A dead process is never
/// restarted automatically: its last submitted operations may have taken effect.
pub struct StdioHarness {
    pipe: Arc<Pipe>,
    opened: AsyncMutex<HashSet<SessionId>>,
    providers: Arc<brain::model::ProviderRegistry>,
}

impl StdioHarness {
    pub async fn start(
        command: &HarnessCommand,
        directory: &Path,
        providers: Arc<brain::model::ProviderRegistry>,
    ) -> Result<Self, Error> {
        if command.command.trim().is_empty() {
            return Err(Error::InvalidState(
                "harness command cannot be empty".into(),
            ));
        }
        std::fs::create_dir_all(directory).map_err(io_error)?;
        let directory = std::fs::canonicalize(directory).map_err(io_error)?;
        let mut child = Command::new(&command.command)
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(io_error)?;
        let stdout = child.stdout.take().expect("piped harness stdout");
        let pipe = Arc::new(Pipe {
            stdin: AsyncMutex::new(child.stdin.take().expect("piped harness stdin")),
            child: AsyncMutex::new(child),
            state: Mutex::new(State::default()),
            next: AtomicU64::new(1),
        });
        let weak = Arc::downgrade(&pipe);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                let line = lines.next_line().await;
                let Some(pipe) = weak.upgrade() else { break };
                let frame = match line {
                    Ok(Some(line)) => serde_json::from_str::<Value>(&line),
                    Ok(None) => {
                        pipe.fail("harness process exited; submitted operations are not replayed");
                        break;
                    }
                    Err(error) => {
                        pipe.fail(&error.to_string());
                        break;
                    }
                };
                let frame = match frame {
                    Ok(frame) if frame["jsonrpc"] == "2.0" && !frame["id"].is_null() => frame,
                    _ => {
                        pipe.stop("invalid harness JSON-RPC frame").await;
                        break;
                    }
                };
                if frame.get("method").is_some() {
                    tokio::spawn(async move {
                        pipe.callback(frame).await;
                    });
                } else {
                    let reply = frame["id"].as_u64().and_then(|id| {
                        pipe.state
                            .lock()
                            .expect("harness state lock")
                            .pending
                            .remove(&id)
                    });
                    match reply {
                        Some(reply) => {
                            let result = if let Some(error) = frame.get("error") {
                                Err(Error::Loop(brain_protocol::TurnError::new(
                                    "harness_failed",
                                    error["message"]
                                        .as_str()
                                        .unwrap_or("harness request failed"),
                                )))
                            } else if let Some(result) = frame.get("result") {
                                Ok(result.clone())
                            } else {
                                Err(Error::Ambiguous(
                                    "harness response has neither result nor error".into(),
                                ))
                            };
                            let _ = reply.send(result);
                        }
                        None => {
                            pipe.stop("harness returned an unknown request id").await;
                            break;
                        }
                    }
                }
            }
        });
        let initialize = HarnessInitialize {
            protocol: HARNESS_PROTOCOL.into(),
            state_directory: directory.to_string_lossy().into_owned(),
            configuration: command.configuration.clone(),
        };
        let handshake = tokio::time::timeout(
            Duration::from_secs(30),
            pipe.request("initialize", json!(initialize)),
        )
        .await;
        let identity: HarnessIdentity = match handshake {
            Ok(Ok(value)) => serde_json::from_value(value).map_err(json_error)?,
            other => {
                pipe.stop("harness initialization failed").await;
                return Err(Error::Executor(format!(
                    "harness initialization failed: {other:?}"
                )));
            }
        };
        if identity.protocol != HARNESS_PROTOCOL
            || identity.name.is_empty()
            || identity.state_version.is_empty()
        {
            pipe.stop("unsupported harness protocol or identity").await;
            return Err(Error::InvalidState(
                "unsupported harness protocol or identity".into(),
            ));
        }
        // Reopening native state with a different harness would misinterpret it.
        let identity_path = directory.join("identity.json");
        if identity_path.exists() {
            let saved: HarnessIdentity =
                serde_json::from_slice(&std::fs::read(&identity_path).map_err(io_error)?)
                    .map_err(json_error)?;
            if saved != identity {
                pipe.stop("harness identity does not match stored native state")
                    .await;
                return Err(Error::InvalidState(
                    "harness identity does not match stored native state; use a new data directory"
                        .into(),
                ));
            }
        } else {
            std::fs::write(
                identity_path,
                serde_json::to_vec(&identity).map_err(json_error)?,
            )
            .map_err(io_error)?;
        }
        Ok(Self {
            pipe,
            opened: AsyncMutex::new(HashSet::new()),
            providers,
        })
    }
}

#[async_trait]
impl AgentHarness for StdioHarness {
    fn ready(&self) -> bool {
        self.pipe
            .state
            .lock()
            .expect("harness state lock")
            .failed
            .is_none()
    }
    async fn turn(
        &self,
        session: &SessionId,
        sequence: u64,
        config: &SessionConfig,
        input: TurnInput,
        services: Arc<dyn TurnServices>,
    ) -> Result<TurnOutput, Error> {
        // Upstream recovery may replay an effect whose result never reached native
        // state. Refuse that state instead of allowing upstream at-least-once recovery.
        const ACTIVE: &str = "brain.harness.activation";
        if input.kv.contains_key(ACTIVE) {
            return Err(Error::InvalidState(
                "native harness activation did not complete; start a new session".into(),
            ));
        }
        services
            .kv_put(brain_protocol::KvPutRequest {
                key: ACTIVE.into(),
                value: json!(sequence),
            })
            .await?;
        {
            let mut opened = self.opened.lock().await;
            if !opened.contains(session) {
                self.pipe
                    .request("session/open", json!({"session_id":session}))
                    .await?;
                opened.insert(session.clone());
            }
        }
        let activation = Arc::new(Activation {
            services: SessionServices::Harness(services.clone()),
            fatal: Mutex::new(None),
            calls: RwLock::new(()),
            closed: AtomicBool::new(false),
        });
        self.pipe
            .state
            .lock()
            .expect("harness state lock")
            .active
            .insert((session.clone(), sequence), activation.clone());
        let model = self
            .providers
            .model(&config.model.provider, &config.model.name)
            .cloned()
            .unwrap_or_else(|| ModelDef {
                id: config.model.name.clone(),
                ..Default::default()
            });
        let request = HarnessRun {
            session_id: session.clone(),
            sequence,
            model,
            input,
        };
        let running = self.pipe.request("session/run", json!(request));
        tokio::pin!(running);
        let result = tokio::select! {
            result = &mut running => result,
            () = async { while !services.cancelled() { tokio::time::sleep(Duration::from_millis(25)).await; } } => {
                let cancel = self.pipe.request("session/cancel", json!({"session_id":session}));
                match tokio::time::timeout(Duration::from_secs(5), async { let _ = cancel.await; running.await }).await {
                    Ok(_) => Err(Error::Cancelled("harness activation cancelled".into())),
                    Err(_) => {
                        self.pipe.stop("harness did not stop after cancellation").await;
                        Err(Error::Cancelled("harness did not stop after cancellation".into()))
                    }
                }
            }
        };
        let _calls = activation.calls.write().await;
        activation.closed.store(true, Ordering::Release);
        self.pipe
            .state
            .lock()
            .expect("harness state lock")
            .active
            .remove(&(session.clone(), sequence));
        if let Some(error) = activation
            .fatal
            .lock()
            .expect("harness failure lock")
            .take()
        {
            return Err(error);
        }
        let output = serde_json::from_value(result?).map_err(json_error)?;
        services.kv_delete(ACTIVE.into()).await?;
        Ok(output)
    }

    async fn close(&self, session: &SessionId) -> Result<(), Error> {
        let mut opened = self.opened.lock().await;
        if opened.contains(session) {
            self.pipe
                .request("session/close", json!({"session_id":session}))
                .await?;
            opened.remove(session);
        }
        Ok(())
    }

    async fn shutdown(&self) {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            self.pipe.request("shutdown", json!({})),
        )
        .await;
        self.pipe.stop("Brain shut down").await;
    }

    async fn delete(&self, session: &SessionId, _config: &SessionConfig) -> Result<(), Error> {
        self.close(session).await?;
        self.pipe
            .request("session/delete", json!({"session_id":session}))
            .await?;
        Ok(())
    }
}

fn json_error(error: serde_json::Error) -> Error {
    Error::InvalidState(error.to_string())
}
fn io_error(error: std::io::Error) -> Error {
    Error::Executor(error.to_string())
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;
