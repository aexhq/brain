//! The two fixtures every session-kernel subject is wired to.
//!
//! They exist so the benchmark measures engines rather than models. A real provider's
//! latency is hundreds of milliseconds and varies by the minute; against numbers in the
//! low single digits it would be the only thing the result showed. Both fixtures are
//! shared by every subject that can accept them, which is what makes those subjects
//! comparable to each other at all.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::Notify;

use std::time::Duration;

use anyhow::Result;
use axum::body::Body;
use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};

/// What one scripted-provider call cost the fixture itself, server side.
///
/// The scripted provider is handed the whole message history on every turn, so its own
/// parse and serialise cost grows with the conversation exactly as the subject's does.
/// Without this the fixture's growth is indistinguishable from the subject's, and a
/// growth curve measured at the client would be charged entirely to the subject.
#[derive(Clone, Copy, Debug)]
pub struct CallTiming {
    /// Request arrival to response ready: body read, JSON parse, the scripted answer, and
    /// serialising it. Everything the fixture does, and nothing the subject does.
    pub service_ns: u64,
    /// Of that, the part spent reading the body off the socket. Held separately because it
    /// is the only part that can be waiting on the subject to finish sending rather than
    /// the fixture doing work, so a curve that must not flatter the subject can drop it.
    pub read_ns: u64,
    /// Bytes of request body the subject sent. Grows with the transcript, which is why
    /// the fixture's own service time grows too.
    pub request_bytes: u64,
    /// Messages in the request, so a turn's transcript length is on the record.
    pub messages: usize,
}

/// A running fixture, and the loopback URL to point a subject at.
pub struct Fixture {
    pub base_url: String,
    pub calls: Arc<AtomicU64>,
    last_call: Arc<Mutex<Option<Instant>>>,
    /// Woken the moment a tool call arrives, so the dispatch probe waits on an event
    /// rather than polling. Polling would add its own interval to a number expected to
    /// land under a millisecond.
    pub arrival: Arc<Notify>,
    /// One entry per call served, in arrival order. Only the scripted provider fills it.
    timings: Arc<Mutex<Vec<CallTiming>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Fixture {
    /// How many times the fixture was actually hit. Checked after every subject: a turn
    /// count that does not match the model calls served means the subject answered from a
    /// cache or a replay path, and the latency measured is not the one under test.
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    /// Nanoseconds, on this process's monotonic clock, when the environment last received
    /// a tool call. The dispatch probe reads this rather than the session log: the
    /// benchmark owns this server, so it can timestamp arrival without asking the subject
    /// to report on itself.
    pub fn last_call_at(&self) -> Option<Instant> {
        *self.last_call.lock().ok()?
    }

    /// Every call the fixture has served so far, in arrival order.
    ///
    /// Read rather than drained, so a caller that wants only the calls a single turn
    /// provoked slices from a mark it took before the turn.
    pub fn timings(&self) -> Vec<CallTiming> {
        self.timings
            .lock()
            .map(|timings| timings.clone())
            .unwrap_or_default()
    }

    /// Stops the fixture. Takes `&self` so a driver can hold a shared handle to it.
    pub fn shutdown(&self) {
        self.handle.abort();
    }
}

#[derive(Clone)]
struct ProviderState {
    /// What the scripted assistant says. Fixed length, because output length is a real
    /// input to turn cost and must not vary between subjects.
    text: Arc<String>,
    /// Tool calls to emit before the text, for the tool-dispatch probe.
    tool_calls: Arc<Vec<Value>>,
    calls: Arc<AtomicU64>,
    timings: Arc<Mutex<Vec<CallTiming>>>,
    /// How long the provider thinks before its first token, and between tokens after
    /// that. A real provider takes hundreds of milliseconds to begin and then streams;
    /// answering instantly is the one behaviour that makes first-token time impossible to
    /// separate from turn time. Identical for every subject, so it cancels from a ranking.
    first_token_delay: Duration,
    inter_token_delay: Duration,
}

/// An OpenAI-compatible `/chat/completions` endpoint that answers instantly.
///
/// It streams the same shape a gateway does — content deltas, then a terminal frame with
/// `finish_reason` and usage, then `[DONE]` — because the subject's SSE decoding is part
/// of what is being measured and a shortcut here would flatter it.
/// The prompt that makes the scripted provider answer with a tool call instead of text,
/// so one fixture serves both the turn probes and the dispatch probe.
pub const TOOL_PROMPT: &str = "bench-tool";

pub async fn scripted_provider(text: &str, tool_calls: Vec<Value>) -> Result<Fixture> {
    // Instant by default, because every latency already published was measured that way
    // and pacing the provider would silently move all of them. `ttfb` is the one probe
    // that cannot be answered by an instant provider, so it is run separately with these
    // set, and the delay it used is recorded with the number.
    let delay = |name: &str| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map_or(Duration::ZERO, Duration::from_millis)
    };
    scripted_provider_paced(
        text,
        tool_calls,
        delay("BENCH_FIRST_TOKEN_DELAY_MS"),
        delay("BENCH_INTER_TOKEN_DELAY_MS"),
    )
    .await
}

/// A scripted provider that takes time to answer, the way a real one does.
pub async fn scripted_provider_paced(
    text: &str,
    tool_calls: Vec<Value>,
    first_token_delay: Duration,
    inter_token_delay: Duration,
) -> Result<Fixture> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    let calls = Arc::new(AtomicU64::new(0));
    let timings = Arc::new(Mutex::new(Vec::new()));
    let state = ProviderState {
        text: Arc::new(text.to_owned()),
        tool_calls: Arc::new(tool_calls),
        calls: Arc::clone(&calls),
        timings: Arc::clone(&timings),
        first_token_delay,
        inter_token_delay,
    };

    let app = Router::new()
        .route("/v1/chat/completions", post(completions))
        // Part of being an OpenAI-compatible endpoint, and not optional: a subject that
        // discovers models rather than being told one asks here first. Letta called it,
        // got a 404, and refused every agent with "must be one of []" — which reads as a
        // configuration mistake rather than a missing route on our side.
        .route("/v1/models", get(models))
        // Also not optional. A subject that embeds its memory calls this on every agent
        // it creates, and a 404 here surfaced as "An unknown error occurred" from the
        // subject rather than as a missing route on ours.
        .route("/v1/embeddings", post(embeddings))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok(Fixture {
        // Loopback HTTP is the one non-HTTPS model base URL Brain accepts, and the
        // reason a scripted provider can be used without weakening the server.
        base_url: format!("http://{address}/v1"),
        calls,
        last_call: Arc::new(Mutex::new(None)),
        arrival: Arc::new(Notify::new()),
        timings,
        handle,
    })
}

/// The models this endpoint serves. One, because the benchmark pins the model exactly as
/// it pins everything else.
async fn models() -> impl IntoResponse {
    Json(json!({
        "object": "list",
        "data": [
            { "id": "gpt-4o-mini", "object": "model", "created": 0, "owned_by": "bench" },
            { "id": "text-embedding-3-small", "object": "model", "created": 0, "owned_by": "bench" },
        ],
    }))
}

/// A fixed-width zero vector. The benchmark measures engines, not embedding quality, and
/// a real embedding would put a model's latency inside every agent create.
async fn embeddings(Json(body): Json<Value>) -> impl IntoResponse {
    let inputs = match body.get("input") {
        Some(Value::Array(values)) => values.len().max(1),
        _ => 1,
    };
    Json(json!({
        "object": "list",
        "model": body.get("model").cloned().unwrap_or(json!("text-embedding-3-small")),
        "data": (0..inputs)
            .map(|index| json!({
                "object": "embedding",
                "index": index,
                "embedding": vec![0.0_f32; 1536],
            }))
            .collect::<Vec<_>>(),
        "usage": {"prompt_tokens": 0, "total_tokens": 0},
    }))
}

/// Times the fixture's own service and records it, then answers.
///
/// The body is taken as a raw `Request` rather than through the `Json` extractor so the
/// clock starts before the body is read and parsed. Both grow with the transcript the
/// subject sends, and both are the fixture's cost, not the subject's — timing from after
/// the extractor would hide exactly the growth this measurement exists to separate.
async fn completions(
    State(state): State<ProviderState>,
    request: axum::extract::Request,
) -> impl IntoResponse {
    let arrived = Instant::now();
    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let read_ns = arrived.elapsed().as_nanos() as u64;
    let request_bytes = bytes.len() as u64;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let answer = answer(&state, body);
    if let Ok(mut timings) = state.timings.lock() {
        timings.push(CallTiming {
            service_ns: arrived.elapsed().as_nanos() as u64,
            read_ns,
            request_bytes,
            messages,
        });
    }
    answer
}

fn answer(state: &ProviderState, body: Value) -> impl IntoResponse + use<> {
    state.calls.fetch_add(1, Ordering::Relaxed);
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("scripted")
        .to_owned();

    // The driver asks for a tool call by sending TOOL_PROMPT. Deciding here rather than
    // at construction lets one provider serve every probe in a subject's run.
    // Looked for anywhere in the request, not at a fixed path. A kernel is free to carry
    // a message's content as a string, as an array of parts, or wrapped in a system
    // preamble, and reading one shape meant the marker was never found: the provider
    // answered with text, no tool was ever called, and the dispatch probe timed out
    // blaming the environment.
    let wants_tool = body.to_string().contains(TOOL_PROMPT);
    let tool_calls: &[Value] = if wants_tool { &state.tool_calls } else { &[] };

    // Honoured, not assumed. The fixture answered every request with SSE whatever was
    // asked for, so a client that requested a plain completion — which is what
    // langchain-openai's `invoke` does — got event-stream frames and rejected them as an
    // unexpected response type. Brain streams and never noticed.
    let streaming = body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let id = format!("chatcmpl-bench-{}", state.calls.load(Ordering::Relaxed));
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();

    if !streaming {
        let message = if tool_calls.is_empty() {
            json!({ "role": "assistant", "content": state.text.as_str() })
        } else {
            json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": tool_calls
                    .iter()
                    .enumerate()
                    .map(|(index, call)| json!({
                        "id": call.get("id").cloned().unwrap_or(json!(format!("call_{index}"))),
                        "type": "function",
                        "function": {
                            "name": call.get("name").cloned().unwrap_or(json!("echo")),
                            "arguments": call.get("arguments").and_then(Value::as_str).unwrap_or("{}"),
                        },
                    }))
                    .collect::<Vec<_>>(),
            })
        };
        let finish = if tool_calls.is_empty() { "stop" } else { "tool_calls" };
        return (
            [("content-type", "application/json"), ("cache-control", "no-cache")],
            json!({
                "id": id,
                "object": "chat.completion",
                "created": created,
                "model": model,
                "choices": [{ "index": 0, "message": message, "finish_reason": finish }],
                "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
            })
            .to_string()
            .into(),
        );
    }

    let mut frames = String::new();
    let mut push = |value: Value| {
        frames.push_str("data: ");
        frames.push_str(&value.to_string());
        frames.push_str("\n\n");
    };

    // Tool calls first, one delta each, matching how providers fragment them.
    for (index, call) in tool_calls.iter().enumerate() {
        push(json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{"index": 0, "delta": {"tool_calls": [{
                "index": index,
                "id": call.get("id").cloned().unwrap_or(json!(format!("call_{index}"))),
                "type": "function",
                "function": {
                    "name": call.get("name").cloned().unwrap_or(json!("echo")),
                    "arguments": call.get("arguments").and_then(Value::as_str).unwrap_or("{}"),
                },
            }]}}],
        }));
    }
    if !state.text.is_empty() && !wants_tool {
        push(json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{"index": 0, "delta": {"content": state.text.as_str()}}],
        }));
    }
    let finish = if tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    push(json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
    }));
    frames.push_str("data: [DONE]\n\n");

    // Paced, not dumped. Emitting every frame in one write is what a fixture does and no
    // provider does, and it makes first-token time unmeasurable for every subject at once:
    // the first token and the end of the turn leave within the same microsecond, so the gap
    // a `ttfb` probe exists to measure is smaller than the round trip that reports it.
    //
    // The delay is identical for every subject and sits on both sides of the comparison, so
    // it cancels out of any ranking while making the two moments distinguishable at all.
    let first_delay = state.first_token_delay;
    let inter_delay = state.inter_token_delay;
    let body = Body::from_stream(futures_util::stream::unfold(
        (frames.into_bytes(), true),
        move |(remaining, first)| async move {
            if remaining.is_empty() {
                return None;
            }
            // One SSE frame per tick, split on the blank line that terminates a frame.
            let terminator = *b"\n\n";
            let split = remaining
                .windows(2)
                .position(|pair| pair == terminator)
                .map_or(remaining.len(), |at| at + 2);
            let (frame, rest) = remaining.split_at(split);
            let delay = if first { first_delay } else { inter_delay };
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            Some((
                Ok::<_, std::convert::Infallible>(frame.to_vec()),
                (rest.to_vec(), false),
            ))
        },
    ));

    (
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
        ],
        body,
    )
}

/// An environment that returns immediately, so a tool-dispatch number is the kernel's
/// dispatch and journal cost with nothing else in it.
///
/// It speaks the remote environment contract: `POST /v1/operations` carrying a binding and
/// an operation, answered with a receipt echoing the operation id and request identity.
pub async fn echo_environment() -> Result<Fixture> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    let calls = Arc::new(AtomicU64::new(0));
    let last_call = Arc::new(Mutex::new(None));
    let arrival = Arc::new(Notify::new());
    let state = EnvironmentState {
        calls: Arc::clone(&calls),
        last_call: Arc::clone(&last_call),
        arrival: Arc::clone(&arrival),
    };

    let app = Router::new()
        .route("/v1/operations", post(operations))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok(Fixture {
        base_url: format!("http://{address}"),
        calls,
        last_call,
        arrival,
        // The echo environment serves lifecycle and tool operations, whose cost does not
        // grow with the conversation; only the provider's does, and only that is timed.
        timings: Arc::new(Mutex::new(Vec::new())),
        handle,
    })
}

#[derive(Clone)]
struct EnvironmentState {
    calls: Arc<AtomicU64>,
    last_call: Arc<Mutex<Option<Instant>>>,
    arrival: Arc<Notify>,
}

async fn operations(
    State(state): State<EnvironmentState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Stamped before any parsing, so the reading is arrival and not arrival plus the
    // fixture's own work.
    let arrived = Instant::now();
    state.calls.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = state.last_call.lock() {
        *last = Some(arrived);
    }
    state.arrival.notify_one();
    let operation = body.get("operation").cloned().unwrap_or(Value::Null);
    // The receipt has to answer the *kind* of operation that arrived. A tool dispatch
    // wants an `outcome`; a lifecycle operation — setup, attach, detach, teardown —
    // only accepts `accepted` or `result`, and answering one of those with a tool receipt
    // fails the session with "Environment returned a nonterminal lifecycle receipt".
    let receipt = match operation
        .pointer("/request/type")
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "invoke" => json!({
            "type": "outcome",
            "outcome": {"status": "ok", "value": {"content": "echo"}},
        }),
        "call" => json!({ "type": "result", "output": {"content": "echo"} }),
        // setup, attach, detach, teardown, cancel: nothing to return but that it is done.
        _ => json!({ "type": "accepted" }),
    };
    Json(json!({
        "contract": "environment/v2",
        "operation_id": operation.get("operation_id").cloned().unwrap_or(json!("op_unknown")),
        // Echoed back, not invented: Brain checks the receipt names the request it sent,
        // and a receipt that carries the wrong field fails the whole session with
        // "operation outcome is ambiguous" rather than anything about this fixture.
        "request_identity": operation
            .get("request_identity")
            .cloned()
            .unwrap_or(json!("")),
        "receipt": receipt,
    }))
}
