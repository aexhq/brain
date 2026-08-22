//! The brain-side client for a loop-host process. [`RemoteAgentloop`] drives turns whose guest
//! runs in the daemon: activations go out over one multiplexed [`crate::wire`] connection, and
//! every guest ctx op comes back to be serviced locally against the turn's `TurnCtx` — so the
//! kernel-error latch and verdict rules are byte-identical to the in-process host.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use brain::BrainError;
use brain::agentloop::{Agentloop, LoopVerdict, TurnCtx};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use crate::wire::{self, Frame};
use crate::{resolve_verdict, service_ctx_op};

const CONNECTION_LOST: &str = "loop host connection lost";

struct Route {
    ctx: mpsc::Sender<(u64, String)>,
    done: oneshot::Sender<Result<String, String>>,
}

/// One duplex connection to a loop-host process, multiplexing every activation the brain runs
/// there. There is no reconnect: when the connection drops, in-flight turns fail honestly and
/// the composition must build a fresh client (drain-before-stop deploys make that the rare path).
pub struct WireClient {
    out: mpsc::Sender<Frame>,
    routes: Arc<Mutex<HashMap<u64, Route>>>,
    next_activation: AtomicU64,
    closed: Arc<AtomicBool>,
}

impl WireClient {
    /// Connect and authenticate. Fails fast on a wrong token (the daemon answers `hello_ack`
    /// only after accepting the token; a refusal closes the connection).
    pub async fn connect(addr: SocketAddr, token: &str) -> anyhow::Result<Arc<Self>> {
        let stream = TcpStream::connect(addr).await?;
        let (mut reader, mut writer) = stream.into_split();
        wire::write_frame(
            &mut writer,
            &Frame::Hello {
                token: token.to_string(),
            },
        )
        .await?;
        match wire::read_frame(&mut reader).await {
            Ok(Frame::HelloAck) => {}
            Ok(other) => anyhow::bail!(
                "loop host answered hello with a {} frame",
                other.kind_name()
            ),
            Err(error) => {
                anyhow::bail!("loop host refused the connection (wrong token?): {error}")
            }
        }

        let (out, mut out_rx) = mpsc::channel::<Frame>(64);
        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if wire::write_frame(&mut writer, &frame).await.is_err() {
                    break;
                }
            }
        });

        let routes: Arc<Mutex<HashMap<u64, Route>>> = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let reader_routes = routes.clone();
        let reader_closed = closed.clone();
        tokio::spawn(async move {
            loop {
                match wire::read_frame(&mut reader).await {
                    Ok(Frame::Ctx {
                        id,
                        activation,
                        payload,
                    }) => {
                        // Capacity 1 never overflows: the guest ABI allows one op in flight
                        // per activation. A frame that would overflow is a daemon bug.
                        let delivered = reader_routes
                            .lock()
                            .expect("routes")
                            .get(&activation)
                            .map(|route| route.ctx.try_send((id, payload)));
                        match delivered {
                            Some(Ok(())) => {}
                            Some(Err(_)) => {
                                tracing::error!(activation, "ctx frame overflowed its route");
                                break;
                            }
                            None => tracing::warn!(
                                activation,
                                "ctx frame for an unknown activation; dropped"
                            ),
                        }
                    }
                    Ok(Frame::ActivationResult {
                        activation,
                        verdict,
                        error,
                    }) => {
                        if let Some(route) =
                            reader_routes.lock().expect("routes").remove(&activation)
                        {
                            let result = match (verdict, error) {
                                (Some(verdict), None) => Ok(verdict),
                                (_, Some(error)) => Err(error),
                                (None, None) => {
                                    Err("loop host sent an empty activation result".to_string())
                                }
                            };
                            let _ = route.done.send(result);
                        }
                    }
                    Ok(other) => {
                        tracing::warn!(
                            kind = other.kind_name(),
                            "unexpected frame from the loop host; dropped"
                        );
                    }
                    Err(_) => break,
                }
            }
            // The connection is gone: fail every in-flight activation honestly. `closed` is
            // set first so a drive_turn registering concurrently notices after its insert.
            reader_closed.store(true, Ordering::SeqCst);
            for (_, route) in reader_routes.lock().expect("routes").drain() {
                let _ = route.done.send(Err(CONNECTION_LOST.to_string()));
            }
        });

        Ok(Arc::new(Self {
            out,
            routes,
            next_activation: AtomicU64::new(1),
            closed,
        }))
    }
}

/// Removes the activation's route on drop; if the route was still registered, the daemon-side
/// guest is still running, so a best-effort `abort` follows it out.
struct ActivationGuard {
    client: Arc<WireClient>,
    activation: u64,
}

impl Drop for ActivationGuard {
    fn drop(&mut self) {
        let live = self
            .client
            .routes
            .lock()
            .expect("routes")
            .remove(&self.activation)
            .is_some();
        if live {
            let _ = self.client.out.try_send(Frame::Abort {
                activation: self.activation,
            });
        }
    }
}

/// An agentloop whose guest runs in a loop-host process reached through `client`.
pub struct RemoteAgentloop {
    client: Arc<WireClient>,
}

impl RemoteAgentloop {
    pub fn new(client: Arc<WireClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Agentloop for RemoteAgentloop {
    async fn drive_turn(&self, ctx: &mut dyn TurnCtx) -> Result<LoopVerdict, BrainError> {
        let activation = self.client.next_activation.fetch_add(1, Ordering::Relaxed);
        let (ctx_tx, mut ctx_rx) = mpsc::channel(1);
        let (done_tx, mut done_rx) = oneshot::channel();
        self.client.routes.lock().expect("routes").insert(
            activation,
            Route {
                ctx: ctx_tx,
                done: done_tx,
            },
        );
        let _guard = ActivationGuard {
            client: self.client.clone(),
            activation,
        };
        // Insert-then-check pairs with the reader's set-then-drain: whichever side runs second
        // sees the other's write, so a connection lost around registration cannot strand a turn.
        if self.client.closed.load(Ordering::SeqCst) {
            return Err(BrainError::Agentloop(CONNECTION_LOST.into()));
        }
        let payload = ctx.activation_message()?.to_string();
        let sent = self
            .client
            .out
            .send(Frame::Activate {
                activation,
                activation_kind: "message".into(),
                payload,
            })
            .await;
        if sent.is_err() {
            return Err(BrainError::Agentloop(CONNECTION_LOST.into()));
        }

        let mut first_error: Option<BrainError> = None;
        let outcome = loop {
            tokio::select! {
                biased;
                Some((id, payload)) = ctx_rx.recv() => {
                    // The daemon hosts the composition's official aex component only (its one
                    // LOOPHOST_COMPONENT), so the wire serves the engine vocabulary.
                    let response =
                        service_ctx_op(ctx, &payload, crate::EngineOps::Trusted, &mut first_error)
                            .await;
                    if self
                        .client
                        .out
                        .send(Frame::CtxResult { id, payload: response })
                        .await
                        .is_err()
                    {
                        break Err(CONNECTION_LOST.to_string());
                    }
                }
                done = &mut done_rx => {
                    break done.unwrap_or_else(|_| Err(CONNECTION_LOST.to_string()));
                }
            }
        };
        if let Some(error) = first_error {
            return Err(error);
        }
        let verdict_json = outcome.map_err(BrainError::Agentloop)?;
        resolve_verdict(&verdict_json, ctx)
    }
}

/// Convenience for compositions: install a remote loop reached through `client` into
/// [`brain::session::BrainServices`].
pub fn services_with_remote_loop(client: Arc<WireClient>) -> brain::session::BrainServices {
    brain::session::BrainServices {
        agentloop: Some(Arc::new(RemoteAgentloop::new(client))),
        ..brain::session::BrainServices::default()
    }
}

/// A loop-host daemon process this composition spawned and owns. Killed on drop.
///
/// Startup blocks the calling thread until the daemon reports its bound address (which includes
/// compiling the component) — call it before entering the async runtime, or from a test.
pub struct SpawnedLoopHost {
    pub addr: SocketAddr,
    pub token: String,
    child: std::process::Child,
}

impl SpawnedLoopHost {
    pub fn spawn(daemon_exe: &Path, component: &Path) -> anyhow::Result<Self> {
        use rand::Rng;
        use std::io::BufRead;
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let mut command = std::process::Command::new(daemon_exe);
        // The loop host gets no ambient environment: no cloud credentials, no provider keys,
        // no composition secrets ever reach guest-adjacent code. Only the process plumbing the
        // OS needs survives the scrub. Every daemon-backed test runs under this discipline,
        // which is the executable form of the no-secrets gate.
        command.env_clear();
        for keep in ["PATH", "SYSTEMROOT", "TEMP", "TMP", "HOME", "USERPROFILE"] {
            if let Ok(value) = std::env::var(keep) {
                command.env(keep, value);
            }
        }
        let mut child = command
            .env("LOOPHOST_COMPONENT", component)
            .env("LOOPHOST_TOKEN", &token)
            .env("LOOPHOST_LISTEN", "127.0.0.1:0")
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let mut line = String::new();
        std::io::BufReader::new(stdout).read_line(&mut line)?;
        let addr = line
            .strip_prefix("listening ")
            .and_then(|addr| addr.trim().parse().ok());
        match addr {
            Some(addr) => Ok(Self { addr, token, child }),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("loop host did not report its address (got {line:?})")
            }
        }
    }
}

impl Drop for SpawnedLoopHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
