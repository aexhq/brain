//! The loop-host daemon: a per-tenant process serving guest activations over the [`crate::wire`]
//! protocol. The daemon is a pure relay around the wasm engine — every ctx op a guest issues is
//! forwarded to the brain, which services it against the turn's `TurnCtx`; the daemon holds no
//! kernel state and no credentials.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::wire::{self, Frame};
use crate::{ActivationFuture, CtxRequest, WasmLoopEngine};

/// Accept brain connections forever. Each connection authenticates with the shared token and
/// then multiplexes activations; per-tenant isolation is the process boundary around this call.
pub async fn serve(
    listener: TcpListener,
    engine: Arc<WasmLoopEngine>,
    token: String,
) -> anyhow::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let engine = engine.clone();
        let token = token.clone();
        tokio::spawn(async move {
            match handle_connection(stream, engine, &token).await {
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
    engine: Arc<WasmLoopEngine>,
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
    let result = connection_loop(&mut reader, &engine, &out, &state).await;
    // The brain is gone: kill every guest this connection was running. Dropping the pending
    // replies answers any in-flight guest hostcall with "aborted" on its way down.
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
    engine: &Arc<WasmLoopEngine>,
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
                let (rx, future) = engine.start_activation(&activation_kind, &payload);
                let task = tokio::spawn(run_activation(
                    activation,
                    rx,
                    future,
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

/// Drive one guest activation, relaying its ctx ops to the brain and reporting the outcome.
async fn run_activation(
    activation: u64,
    mut rx: mpsc::Receiver<CtxRequest>,
    mut future: ActivationFuture,
    out: mpsc::Sender<Frame>,
    state: Arc<ConnectionState>,
) {
    let result = loop {
        tokio::select! {
            biased;
            Some((payload, reply)) = rx.recv() => {
                let id = state.next_ctx_id.fetch_add(1, Ordering::Relaxed);
                state
                    .pending_ctx
                    .lock()
                    .expect("pending ctx")
                    .insert(id, (activation, reply));
                if out.send(Frame::Ctx { id, activation, payload }).await.is_err() {
                    break Err("the brain connection closed mid-activation".to_string());
                }
            }
            result = &mut future => break result,
        }
    };
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
