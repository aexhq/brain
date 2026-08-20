//! Brain-side client for the Brain↔Hand ABI v1: one WebSocket per Hand, requests
//! multiplexed by id, `hand_status` events on a broadcast channel.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use brain_protocol::abi::{
    AbiError, CancelRequest, CancelResponse, GenerationId, HandFrame, HandStatusEvent,
    HelloRequest, HelloResponse, LaneCloseRequest, LaneCloseResponse, PersistRequest,
    PersistResponse, PollRequest, PollResponse, PutRequest, PutResponse, ReleaseRequest,
    ReleaseResponse, Reply, Request, RequestCall, RequestId, ResponseResult, StartRequest,
    StartResponse, SyncRequest, SyncResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The hand answered with an ABI error.
    #[error("hand refused: {0:?}")]
    Abi(AbiError),
    #[error("transport: {0}")]
    Transport(String),
    #[error("connection closed")]
    Closed,
    #[error("the hand answered {got} to a {expected} request")]
    WrongReply {
        expected: &'static str,
        got: &'static str,
    },
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<ResponseResult>>>>;

pub struct HandClient {
    tx: mpsc::Sender<String>,
    pending: Pending,
    status_tx: broadcast::Sender<HandStatusEvent>,
    next_id: AtomicU64,
    fence: AtomicU64,
    closed: Arc<AtomicBool>,
    generation: Mutex<Option<GenerationId>>,
    id_prefix: String,
    /// Wall-clock cap for any single request (a poll/start with wait_ms must fit under it).
    pub request_timeout: Duration,
    _reader: tokio::task::JoinHandle<()>,
    _writer: tokio::task::JoinHandle<()>,
}

impl HandClient {
    /// Connects to `ws://host:port/` (or `wss://`). No request is sent; call [`hello`] next.
    pub async fn connect(url: &str, fence: u64) -> Result<Self, ClientError> {
        Self::connect_with_headers(url, fence, &[]).await
    }

    /// [`connect`], with extra request headers. This is how a brain reaches a hand behind the
    /// AWS Lambda MicroVM endpoint: `wss://<endpoint-host>/` plus
    /// `("X-aws-proxy-auth", <JWE token from CreateMicrovmAuthToken>)`.
    pub async fn connect_with_headers(
        url: &str,
        fence: u64,
        headers: &[(&str, &str)],
    ) -> Result<Self, ClientError> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
        let mut request = url
            .into_client_request()
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        for (name, value) in headers {
            let name = tokio_tungstenite::tungstenite::http::HeaderName::try_from(*name)
                .map_err(|e| ClientError::Transport(e.to_string()))?;
            let value = tokio_tungstenite::tungstenite::http::HeaderValue::try_from(*value)
                .map_err(|e| ClientError::Transport(e.to_string()))?;
            request.headers_mut().insert(name, value);
        }
        // disable_nagle: ABI frames are small; Nagle + delayed ACK adds ~40 ms to every
        // request/response pair (the slice-5 latency gate measured a 50 ms tool-call floor
        // against a 2 ms network baseline before this).
        let (ws, _) = tokio_tungstenite::connect_async_with_config(request, None, true)
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;
        let (mut sink, mut source) = ws.split();
        let (tx, mut rx) = mpsc::channel::<String>(256);
        let pending: Pending = Default::default();
        let closed = Arc::new(AtomicBool::new(false));
        let (status_tx, _) = broadcast::channel(256);
        let writer = tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let pending2 = pending.clone();
        let status_tx2 = status_tx.clone();
        let closed2 = closed.clone();
        let reader = tokio::spawn(async move {
            while let Some(msg) = source.next().await {
                let text = match msg {
                    Ok(Message::Text(t)) => t.to_string(),
                    Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                };
                match serde_json::from_str::<HandFrame>(&text) {
                    Ok(HandFrame::Response(r)) => {
                        if let Some(waiter) = pending2.lock().await.remove(&*r.id) {
                            let _ = waiter.send(r.result);
                        } else {
                            tracing::debug!(id = %*r.id, "response for unknown request");
                        }
                    }
                    Ok(HandFrame::HandStatus(ev)) => {
                        let _ = status_tx2.send(ev);
                    }
                    Err(e) => tracing::warn!(error = %e, "unparseable frame from hand"),
                }
            }
            // Connection gone: refuse future calls and fail every pending request.
            closed2.store(true, Ordering::SeqCst);
            pending2.lock().await.clear();
        });
        Ok(Self {
            tx,
            pending,
            status_tx,
            next_id: AtomicU64::new(1),
            fence: AtomicU64::new(fence),
            closed,
            generation: Mutex::new(None),
            id_prefix: format!("c{:x}", std::process::id()),
            request_timeout: Duration::from_secs(120),
            _reader: reader,
            _writer: writer,
        })
    }

    /// True once the underlying WebSocket has closed. A closed client never recovers; the
    /// owner must reconnect (and re-hello) or reclassify the hand.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn set_fence(&self, fence: u64) {
        self.fence.store(fence, Ordering::SeqCst);
    }

    pub async fn generation(&self) -> Option<GenerationId> {
        self.generation.lock().await.clone()
    }

    /// Overrides the generation the client stamps on requests (tests use this to provoke a
    /// generation_mismatch).
    pub async fn set_generation(&self, g: Option<GenerationId>) {
        *self.generation.lock().await = g;
    }

    pub fn status_events(&self) -> broadcast::Receiver<HandStatusEvent> {
        self.status_tx.subscribe()
    }

    /// Sends one request and awaits its reply.
    pub async fn call(&self, call: RequestCall) -> Result<Reply, ClientError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ClientError::Closed);
        }
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id: RequestId = format!("{}-{n}", self.id_prefix).parse().expect("id");
        let generation_id = if matches!(call, RequestCall::Hello(_)) {
            None
        } else {
            self.generation.lock().await.clone()
        };
        let req = Request {
            id: id.clone(),
            fence: self.fence.load(Ordering::SeqCst),
            generation_id,
            call,
        };
        let (otx, orx) = oneshot::channel();
        self.pending.lock().await.insert(id.to_string(), otx);
        let text =
            serde_json::to_string(&req).map_err(|e| ClientError::Transport(e.to_string()))?;
        self.tx.send(text).await.map_err(|_| ClientError::Closed)?;
        let result = match tokio::time::timeout(self.request_timeout, orx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => return Err(ClientError::Closed),
            Err(_) => {
                self.pending.lock().await.remove(&*id);
                return Err(ClientError::Timeout(self.request_timeout));
            }
        };
        match result {
            ResponseResult::Ok { reply } => Ok(reply),
            ResponseResult::Error { error } => Err(ClientError::Abi(error)),
        }
    }

    pub async fn hello(&self, a: HelloRequest) -> Result<HelloResponse, ClientError> {
        match self.call(RequestCall::Hello(a)).await? {
            Reply::Hello(r) => {
                *self.generation.lock().await = Some(r.generation_id.clone());
                Ok(r)
            }
            other => Err(wrong("hello", &other)),
        }
    }
    pub async fn start(&self, a: StartRequest) -> Result<StartResponse, ClientError> {
        match self.call(RequestCall::Start(a)).await? {
            Reply::Start(r) => Ok(r),
            other => Err(wrong("start", &other)),
        }
    }
    pub async fn poll(&self, a: PollRequest) -> Result<PollResponse, ClientError> {
        match self.call(RequestCall::Poll(a)).await? {
            Reply::Poll(r) => Ok(r),
            other => Err(wrong("poll", &other)),
        }
    }
    pub async fn cancel(&self, a: CancelRequest) -> Result<CancelResponse, ClientError> {
        match self.call(RequestCall::Cancel(a)).await? {
            Reply::Cancel(r) => Ok(r),
            other => Err(wrong("cancel", &other)),
        }
    }
    pub async fn release(&self, a: ReleaseRequest) -> Result<ReleaseResponse, ClientError> {
        match self.call(RequestCall::Release(a)).await? {
            Reply::Release(r) => Ok(r),
            other => Err(wrong("release", &other)),
        }
    }
    pub async fn lane_close(&self, a: LaneCloseRequest) -> Result<LaneCloseResponse, ClientError> {
        match self.call(RequestCall::LaneClose(a)).await? {
            Reply::LaneClose(r) => Ok(r),
            other => Err(wrong("lane_close", &other)),
        }
    }
    pub async fn put(&self, a: PutRequest) -> Result<PutResponse, ClientError> {
        match self.call(RequestCall::Put(a)).await? {
            Reply::Put(r) => Ok(r),
            other => Err(wrong("put", &other)),
        }
    }
    pub async fn persist(&self, a: PersistRequest) -> Result<PersistResponse, ClientError> {
        match self.call(RequestCall::Persist(a)).await? {
            Reply::Persist(r) => Ok(r),
            other => Err(wrong("persist", &other)),
        }
    }
    pub async fn sync(&self, a: SyncRequest) -> Result<SyncResponse, ClientError> {
        match self.call(RequestCall::Sync(a)).await? {
            Reply::Sync(r) => Ok(r),
            other => Err(wrong("sync", &other)),
        }
    }
}

fn reply_name(r: &Reply) -> &'static str {
    match r {
        Reply::Hello(_) => "hello",
        Reply::Start(_) => "start",
        Reply::Poll(_) => "poll",
        Reply::Cancel(_) => "cancel",
        Reply::Release(_) => "release",
        Reply::LaneClose(_) => "lane_close",
        Reply::Put(_) => "put",
        Reply::Persist(_) => "persist",
        Reply::Sync(_) => "sync",
    }
}

fn wrong(expected: &'static str, got: &Reply) -> ClientError {
    ClientError::WrongReply {
        expected,
        got: reply_name(got),
    }
}

/// Convenience: build a `start` request for a tool call with the identity hash filled in.
#[allow(clippy::too_many_arguments)]
pub fn start_request(
    operation_id: &str,
    tool: &str,
    input: serde_json::Value,
    lane: brain_protocol::abi::LaneRef,
    cwd: Option<String>,
    detach: bool,
    wait_ms: u64,
    max_bytes: u64,
) -> StartRequest {
    let mut req = StartRequest {
        operation_id: operation_id.parse().expect("operation id"),
        call_hash: "0".repeat(64).parse().expect("placeholder"),
        batch_id: None,
        tool: tool.to_string(),
        input,
        lane,
        cwd,
        detach,
        wait_ms,
        max_bytes,
        bounds: None,
        correlation: Default::default(),
    };
    req.call_hash = brain_protocol::tools::call_hash(&req);
    req
}

/// Convenience: the root lane.
pub fn root_lane() -> brain_protocol::abi::LaneRef {
    brain_protocol::abi::LaneRef {
        id: "0".parse().unwrap(),
        mode: brain_protocol::abi::LaneMode::Persistent,
        parent: None,
    }
}
