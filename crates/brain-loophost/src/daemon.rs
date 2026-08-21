//! The loop-host daemon: a per-tenant process serving guest activations over the [`crate::wire`]
//! protocol. The daemon is a pure relay around the wasm engine — every ctx op a guest issues is
//! forwarded to the brain, which services it against the turn's `TurnCtx`; the daemon holds no
//! kernel state and no credentials.
//!
//! Guest instances are resident per session and shared across brain connections: a fresh
//! instance receives its `session_start` hydration (fetched from the driving turn over the
//! wire) before its first message activation, and idle instances are swept independently of
//! brain actor residency.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::wire::{self, Frame};
use crate::{SessionInstance, SessionInstances, WasmLoopEngine, check_session_start};

/// Accept brain connections forever. Each connection authenticates with the shared token and
/// then multiplexes activations; per-tenant isolation is the process boundary around this call.
pub async fn serve(
    listener: TcpListener,
    engine: Arc<WasmLoopEngine>,
    token: String,
) -> anyhow::Result<()> {
    let instances = Arc::new(SessionInstances::new(engine));
    let sweeper = instances.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            sweeper.sweep();
        }
    });
    loop {
        let (stream, peer) = listener.accept().await?;
        let instances = instances.clone();
        let token = token.clone();
        tokio::spawn(async move {
            match handle_connection(stream, instances, &token).await {
                Ok(()) => tracing::info!(%peer, "brain connection closed"),
                Err(error) => tracing::warn!(%peer, %error, "brain connection failed"),
            }
        });
    }
}

struct ConnectionState {
    /// Ctx ops forwarded to the brain and awaiting their `ctx_result`, tagged with the
    /// activation that issued them so an abort can sweep its orphans.
    pending_ctx: Mutex<HashMap<u64, (u64, oneshot::Sender<String>)>>,
    activations: Mutex<HashMap<u64, AbortHandle>>,
    next_ctx_id: AtomicU64,
}

async fn handle_connection(
    stream: TcpStream,
    instances: Arc<SessionInstances>,
    token: &str,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    match wire::read_frame(&mut reader).await? {
        Frame::Hello { token: presented } if presented == token => {}
        Frame::Hello { .. } => anyhow::bail!("connection presented a wrong token"),
        other => anyhow::bail!("expected hello, got a {} frame", other.kind_name()),
    }
    let (out, mut out_rx) = mpsc::channel::<Frame>(64);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if wire::write_frame(&mut writer, &frame).await.is_err() {
                break;
            }
        }
    });
    out.send(Frame::HelloAck).await.ok();

    let state = Arc::new(ConnectionState {
        pending_ctx: Mutex::new(HashMap::new()),
        activations: Mutex::new(HashMap::new()),
        next_ctx_id: AtomicU64::new(1),
    });
    let result = connection_loop(&mut reader, &instances, &out, &state).await;
    // The brain is gone: stop every activation this connection was running. Dropping the
    // pending replies answers any in-flight guest hostcall with "aborted" on its way down;
    // resident instances stay warm for a reconnecting brain.
    for (_, abort) in state.activations.lock().expect("activations").drain() {
        abort.abort();
    }
    state.pending_ctx.lock().expect("pending ctx").clear();
    drop(out);
    let _ = writer_task.await;
    result
}

async fn connection_loop(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    instances: &Arc<SessionInstances>,
    out: &mpsc::Sender<Frame>,
    state: &Arc<ConnectionState>,
) -> anyhow::Result<()> {
    loop {
        let frame = match wire::read_frame(reader).await {
            Ok(frame) => frame,
            // A clean disconnect between frames is the normal end of a connection.
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(anyhow::anyhow!("wire read: {error}")),
        };
        match frame {
            Frame::Activate {
                activation,
                activation_kind,
                payload,
            } => {
                if state
                    .activations
                    .lock()
                    .expect("activations")
                    .contains_key(&activation)
                {
                    anyhow::bail!("activation {activation} is already running");
                }
                let task = tokio::spawn(run_activation(
                    activation,
                    activation_kind,
                    payload,
                    instances.clone(),
                    out.clone(),
                    state.clone(),
                ));
                // The task may already have finished; aborting a finished task is a no-op and
                // the entry is reaped by the task itself or at connection teardown.
                state
                    .activations
                    .lock()
                    .expect("activations")
                    .insert(activation, task.abort_handle());
            }
            Frame::Abort { activation } => {
                if let Some(abort) = state
                    .activations
                    .lock()
                    .expect("activations")
                    .remove(&activation)
                {
                    abort.abort();
                }
                state
                    .pending_ctx
                    .lock()
                    .expect("pending ctx")
                    .retain(|_, (owner, _)| *owner != activation);
            }
            Frame::CtxResult { id, payload } => {
                if let Some((_, reply)) = state.pending_ctx.lock().expect("pending ctx").remove(&id)
                {
                    let _ = reply.send(payload);
                }
            }
            other => anyhow::bail!("unexpected {} frame from the brain", other.kind_name()),
        }
    }
}

/// One daemon-initiated ctx op to the brain (the `session_start` hydration fetch), using the
/// same correlation machinery guest ops ride.
async fn wire_ctx_op(
    activation: u64,
    op_json: &str,
    out: &mpsc::Sender<Frame>,
    state: &Arc<ConnectionState>,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    let id = state.next_ctx_id.fetch_add(1, Ordering::Relaxed);
    state
        .pending_ctx
        .lock()
        .expect("pending ctx")
        .insert(id, (activation, reply_tx));
    if out
        .send(Frame::Ctx {
            id,
            activation,
            payload: op_json.to_string(),
        })
        .await
        .is_err()
    {
        return Err("the brain connection closed mid-activation".to_string());
    }
    reply_rx
        .await
        .map_err(|_| "the brain connection closed mid-activation".to_string())
}

/// Serve one activation on a resident instance, forwarding every guest ctx op to the brain.
async fn serve_forwarding(
    instance: &mut SessionInstance,
    kind: &str,
    payload: &str,
    activation: u64,
    out: &mpsc::Sender<Frame>,
    state: &Arc<ConnectionState>,
) -> Result<String, String> {
    let mut rx = instance.arm();
    let mut future = Box::pin(instance.activate(kind, payload));
    loop {
        tokio::select! {
            biased;
            Some((op, reply)) = rx.recv() => {
                let id = state.next_ctx_id.fetch_add(1, Ordering::Relaxed);
                state
                    .pending_ctx
                    .lock()
                    .expect("pending ctx")
                    .insert(id, (activation, reply));
                if out.send(Frame::Ctx { id, activation, payload: op }).await.is_err() {
                    break Err("the brain connection closed mid-activation".to_string());
                }
            }
            result = &mut future => break result,
        }
    }
}

/// Drive one guest activation: resident instance lookup, first-use `session_start` hydration,
/// then the requested activation, reporting the outcome to the brain.
async fn run_activation(
    activation: u64,
    activation_kind: String,
    payload: String,
    instances: Arc<SessionInstances>,
    out: mpsc::Sender<Frame>,
    state: Arc<ConnectionState>,
) {
    let result = drive_activation(
        activation,
        &activation_kind,
        &payload,
        &instances,
        &out,
        &state,
    )
    .await;
    state
        .activations
        .lock()
        .expect("activations")
        .remove(&activation);
    let frame = match result {
        Ok(verdict) => Frame::ActivationResult {
            activation,
            verdict: Some(verdict),
            error: None,
        },
        Err(error) => Frame::ActivationResult {
            activation,
            verdict: None,
            error: Some(error),
        },
    };
    let _ = out.send(frame).await;
}

async fn drive_activation(
    activation: u64,
    activation_kind: &str,
    payload: &str,
    instances: &Arc<SessionInstances>,
    out: &mpsc::Sender<Frame>,
    state: &Arc<ConnectionState>,
) -> Result<String, String> {
    let parsed: serde_json::Value = serde_json::from_str(payload)
        .map_err(|error| format!("activation payload is not JSON: {error}"))?;
    let session_id = crate::payload_session_id(&parsed).map_err(|error| error.to_string())?;
    let instance = instances.acquire(&session_id).await?;
    let mut guard = instance.lock().await;
    if !guard.started {
        let hydration =
            wire_ctx_op(activation, r#"{"op":"engine.session_start"}"#, out, state).await?;
        let hydrated: serde_json::Value = serde_json::from_str(&hydration)
            .map_err(|error| format!("session_start hydration is not JSON: {error}"))?;
        if let Some(error) = hydrated.get("error") {
            return Err(format!("session_start hydration failed: {error}"));
        }
        let returned = serve_forwarding(
            &mut guard,
            "session_start",
            &hydration,
            activation,
            out,
            state,
        )
        .await
        .inspect_err(|_| instances.remove(&session_id))?;
        if let Err(error) = check_session_start(&returned) {
            instances.remove(&session_id);
            return Err(error.to_string());
        }
        guard.started = true;
    }
    serve_forwarding(&mut guard, activation_kind, payload, activation, out, state)
        .await
        .inspect_err(|_| instances.remove(&session_id))
}
