//! A stdio-over-HTTP bridge, for subjects whose only integration surface is a process's
//! stdin and stdout.
//!
//! pi's RPC mode and Codex's app-server both speak newline-delimited JSON over stdio and
//! listen on no port. The runner starts every subject from a launch block, waits on an
//! HTTP readiness URL, and samples memory across the tree under the pid it started, so the
//! smallest thing that lets those two in is a process that owns the child and exposes its
//! pipes:
//!
//! * `POST /stdin` writes the body, newline-terminated, to the child's stdin;
//! * `GET /stdout` is a server-sent event stream carrying every line the child writes
//!   from the moment the client connected, one `data:` frame per line;
//! * `GET /health` answers 200 once the child is up.
//!
//! It knows nothing of either protocol. Every command is composed and every event is read
//! by the subject's own driver, so what gets measured is pi's or Codex's handling of its
//! own documented wire plus one loopback HTTP hop — the same hop every HTTP subject pays
//! to its own server. Rust rather than a Node script because the sampler cannot separate
//! the bridge's footprint from the child's, and a runtime's tens of megabytes would be
//! charged to the subject.

use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Parser;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::{Mutex, broadcast},
};

#[derive(Parser)]
#[command(name = "brain-bench-bridge")]
struct Cli {
    /// Loopback port to listen on.
    #[arg(long)]
    port: u16,
    /// Working directory for the child.
    #[arg(long)]
    cwd: Option<std::path::PathBuf>,
    /// A line written to the child's stdin as soon as it starts. When given, `/health`
    /// answers 200 only once the child has written a line back, so a cold-start figure
    /// includes the child's own boot rather than only the bridge's.
    #[arg(long)]
    ready_send: Option<String>,
    /// The child command and its arguments, after `--`.
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

struct Shared {
    stdin: Mutex<ChildStdin>,
    lines: broadcast::Sender<Arc<str>>,
    /// Whether the child has written a line yet.
    spoke: AtomicBool,
    /// Readiness waits for the child to speak, not merely to exist.
    wait_for_speech: bool,
    alive: AtomicBool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let (program, args) = cli
        .command
        .split_first()
        .context("no command was given after --")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // The child's diagnostics go where the bridge's do: the runner's log for the
        // subject, so a failure can be read there.
        .stderr(Stdio::inherit());
    if let Some(cwd) = &cli.cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    let mut stdin = child.stdin.take().context("the child has no stdin")?;
    let stdout = child.stdout.take().context("the child has no stdout")?;

    if let Some(line) = &cli.ready_send {
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
    }

    let (lines, _) = broadcast::channel(1 << 16);
    let shared = Arc::new(Shared {
        stdin: Mutex::new(stdin),
        lines,
        spoke: AtomicBool::new(false),
        wait_for_speech: cli.ready_send.is_some(),
        alive: AtomicBool::new(true),
    });

    let reader = Arc::clone(&shared);
    tokio::spawn(async move {
        // `lines` splits on `\n` and strips a trailing `\r`, which is exactly the framing
        // pi's protocol document asks for; a reader that also split on Unicode line
        // separators would break JSON strings that legitimately contain them.
        let mut lines = BufReader::with_capacity(1 << 20, stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    reader.spoke.store(true, Ordering::Relaxed);
                    let _ = reader.lines.send(Arc::from(line));
                }
                Ok(None) => break,
                Err(error) => {
                    eprintln!("bridge: reading the child's stdout: {error}");
                    break;
                }
            }
        }
        reader.alive.store(false, Ordering::Relaxed);
    });

    let watched = Arc::clone(&shared);
    tokio::spawn(async move {
        let status = child.wait().await;
        watched.alive.store(false, Ordering::Relaxed);
        eprintln!("bridge: the child exited: {status:?}");
        // The bridge has nothing to bridge any more. Exiting is what lets the runner's
        // readiness check and the recovery probe see the death rather than a silent port.
        std::process::exit(status.ok().and_then(|status| status.code()).unwrap_or(1));
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/stdin", post(write_stdin))
        .route("/stdout", get(stream_stdout))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cli.port))
        .await
        .with_context(|| format!("listening on 127.0.0.1:{}", cli.port))?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(shared): State<Arc<Shared>>) -> Response {
    if !shared.alive.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "the child has exited").into_response();
    }
    if shared.wait_for_speech && !shared.spoke.load(Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "the child has not spoken yet",
        )
            .into_response();
    }
    (StatusCode::OK, "ok").into_response()
}

async fn write_stdin(State(shared): State<Arc<Shared>>, body: Bytes) -> Response {
    let mut stdin = shared.stdin.lock().await;
    let written = async {
        stdin.write_all(&body).await?;
        if !body.ends_with(b"\n") {
            stdin.write_all(b"\n").await?;
        }
        stdin.flush().await
    }
    .await;
    match written {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("writing to the child's stdin: {error}"),
        )
            .into_response(),
    }
}

async fn stream_stdout(State(shared): State<Arc<Shared>>) -> Response {
    let receiver = shared.lines.subscribe();
    let frames = futures_util::stream::unfold(
        (receiver, true),
        |(mut receiver, opening)| async move {
            if opening {
                // Sent before anything else, so a client can wait for it and know its
                // subscription is live before it writes the command whose answer it
                // wants.
                return Some((
                    Ok::<Bytes, std::io::Error>(Bytes::from_static(b": connected\n\n")),
                    (receiver, false),
                ));
            }
            match receiver.recv().await {
                Ok(line) => Some((
                    Ok(Bytes::from(format!("data: {line}\n\n"))),
                    (receiver, false),
                )),
                // Said out loud rather than skipped: a client that silently lost frames
                // would wait forever for an answer that already went by.
                Err(broadcast::error::RecvError::Lagged(missed)) => Some((
                    Ok(Bytes::from(format!("event: lagged\ndata: {missed}\n\n"))),
                    (receiver, false),
                )),
                Err(broadcast::error::RecvError::Closed) => None,
            }
        },
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(frames))
        .expect("a static response builds")
}
