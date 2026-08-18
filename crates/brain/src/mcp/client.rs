//! The MCP client: 2026-07-28 stateless transport as the primary path, and the thin legacy
//! adapter (`initialize` + `Mcp-Session-Id`, Streamable HTTP 2025-03-26..2025-11-25, no SSE
//! resumability) behind the same call surface.
//!
//! Scope is deliberately narrow (ARCHITECTURE-v1 D11): `tools/list` and `tools/call`, plus
//! the version negotiation needed to reach them. No subscriptions, no sampling/roots (we
//! declare zero client capabilities), no resumability. MRTR `input_required` results are
//! surfaced as structured tool failures. The legacy adapter is the first thing to cut.
//!
//! Every request goes over the caller-supplied client -- compose it from
//! [`crate::outbound::Outbound`] so the SSRF guard is on the wire, not on trust.

use super::wire::{self, Reply};
use crate::provider::sse::SseDecoder;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Hard cap on any single MCP HTTP response we will buffer (tool lists included). Tool
/// RESULTS are additionally truncated to the configured per-result bound.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Pagination safety for `tools/list`.
const MAX_LIST_PAGES: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("http {status}: {body}")]
    Status { status: u16, body: String },
    #[error("rpc error {code}: {message}")]
    Rpc {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    #[error("protocol: {0}")]
    Protocol(String),
    #[error(
        "the server requires interactive input (MRTR input_required); this client declares no input capabilities"
    )]
    InputRequired,
    #[error("timed out")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
}

/// A remote tool as `tools/list` served it (pre-namespacing).
#[derive(Debug, Clone)]
pub struct RemoteTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// One tool call's mapped result.
#[derive(Debug)]
pub struct McpCallResult {
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn is_v2(version: &str) -> bool {
    version >= wire::V2_VERSION
}

/// A negotiated connection to one MCP server. Static headers ride every request; the legacy
/// session id is minted lazily and re-minted once if the server forgets it (a restart).
pub struct ServerConn {
    pub name: String,
    client: reqwest::Client,
    url: reqwest::Url,
    headers: Vec<(String, String)>,
    version: String,
    legacy_session: tokio::sync::Mutex<Option<String>>,
}

impl ServerConn {
    pub fn new(
        name: impl Into<String>,
        client: reqwest::Client,
        url: reqwest::Url,
        headers: Vec<(String, String)>,
        version: impl Into<String>,
    ) -> Self {
        ServerConn {
            name: name.into(),
            client,
            url,
            headers,
            version: version.into(),
            legacy_session: tokio::sync::Mutex::new(None),
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    /// Seeds the legacy session id from a handshake that has already run, so the first
    /// `rpc` reuses it instead of re-initializing. The probe path calls this after
    /// `legacy_initialize`; a connection built this way must carry the same negotiated
    /// version.
    pub fn with_legacy_session(mut self, session: Option<String>) -> Self {
        self.legacy_session = tokio::sync::Mutex::new(session);
        self
    }

    /// One RPC with negotiation-era handling: v2 sends the metadata headers and `_meta`;
    /// legacy ensures the handshake ran and retries ONCE through a fresh handshake when the
    /// server answers 404 (it lost or expired the session).
    pub async fn rpc(
        &self,
        method: &str,
        params: Value,
        mcp_name: Option<&str>,
        param_headers: &[(String, String)],
        timeout: Duration,
        cancel: Option<&CancellationToken>,
    ) -> Result<Value, McpError> {
        if is_v2(&self.version) {
            let body = wire::request(next_id(), method, params, true);
            let mut hs = self.wire_headers(method, mcp_name, None);
            hs.extend_from_slice(param_headers);
            return rpc_once(
                &self.client,
                &self.url,
                &self.headers,
                &hs,
                body,
                timeout,
                cancel,
            )
            .await
            .and_then(reply_to_result);
        }
        // Legacy: handshake, call, one retry on a lost session.
        for attempt in 0..2 {
            let session = self.ensure_legacy_session().await?;
            let body = wire::request(next_id(), method, params.clone(), false);
            let hs = self.wire_headers(method, mcp_name, session.as_deref());
            match rpc_once(
                &self.client,
                &self.url,
                &self.headers,
                &hs,
                body,
                timeout,
                cancel,
            )
            .await
            {
                Ok(reply) => return reply_to_result(reply),
                Err(McpError::Status { status: 404, .. }) if attempt == 0 => {
                    // The server no longer knows our session; re-initialize once.
                    *self.legacy_session.lock().await = None;
                }
                Err(e) => return Err(e),
            }
        }
        Err(McpError::Protocol(
            "legacy session could not be re-established".into(),
        ))
    }

    fn wire_headers(
        &self,
        method: &str,
        mcp_name: Option<&str>,
        legacy_session: Option<&str>,
    ) -> Vec<(String, String)> {
        let mut hs = vec![("MCP-Protocol-Version".to_string(), self.version.clone())];
        if is_v2(&self.version) {
            hs.push(("Mcp-Method".into(), method.to_string()));
            if let Some(n) = mcp_name {
                hs.push(("Mcp-Name".into(), wire::header_value(n)));
            }
        } else if let Some(s) = legacy_session {
            hs.push(("Mcp-Session-Id".into(), s.to_string()));
        }
        hs
    }

    /// Runs (or reuses) the legacy `initialize` handshake; returns the session id the server
    /// minted, if any.
    async fn ensure_legacy_session(&self) -> Result<Option<String>, McpError> {
        let mut guard = self.legacy_session.lock().await;
        if let Some(s) = guard.as_ref() {
            return Ok(Some(s.clone()));
        }
        let (version, session) = legacy_initialize(
            &self.client,
            &self.url,
            &self.headers,
            Duration::from_secs(10),
        )
        .await?;
        if version != self.version {
            // The server changed its negotiated version since create. Tools were sealed at
            // create; a version change alone does not fork the prefix, but it is loud.
            tracing::warn!(server = %self.name, sealed = %self.version, now = %version,
                "legacy MCP server renegotiated a different protocol version");
        }
        *guard = session.clone();
        Ok(session)
    }

    /// `tools/list`, following cursor pagination to the end.
    pub async fn list_tools(&self, timeout: Duration) -> Result<Vec<RemoteTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let result = self
                .rpc("tools/list", params, None, &[], timeout, None)
                .await?;
            for t in result
                .get("tools")
                .and_then(|t| t.as_array())
                .ok_or_else(|| McpError::Protocol("tools/list result has no tools array".into()))?
            {
                tools.push(RemoteTool {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| McpError::Protocol("tool without a name".into()))?
                        .to_string(),
                    description: t
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    input_schema: t
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object"})),
                });
            }
            match result.get("nextCursor").and_then(|c| c.as_str()) {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => return Ok(tools),
            }
        }
        Err(McpError::Protocol(format!(
            "tools/list did not terminate within {MAX_LIST_PAGES} pages"
        )))
    }

    /// `tools/call`, with `Mcp-Name` and any `x-mcp-header` mirrors, mapped to one bounded
    /// text result.
    pub async fn call_tool(
        &self,
        remote_name: &str,
        args: &Value,
        header_params: &[wire::HeaderParam],
        max_result_bytes: usize,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<McpCallResult, McpError> {
        let param_headers = wire::param_headers(header_params, args).map_err(McpError::Protocol)?;
        let result = self
            .rpc(
                "tools/call",
                json!({"name": remote_name, "arguments": args}),
                Some(remote_name),
                &param_headers,
                timeout,
                Some(cancel),
            )
            .await?;
        if wire::result_kind(&result) == wire::ResultKind::InputRequired {
            return Err(McpError::InputRequired);
        }
        Ok(map_call_result(&result, max_result_bytes))
    }

    /// Best-effort legacy session teardown (HTTP DELETE with the session id). v2 has no
    /// session to tear down. Never an error path.
    pub async fn close(&self) {
        if is_v2(&self.version) {
            return;
        }
        let session = self.legacy_session.lock().await.take();
        if let Some(s) = session {
            let mut rb = self.client.delete(self.url.clone());
            for (k, v) in &self.headers {
                rb = rb.header(k.as_str(), v.as_str());
            }
            let _ = rb
                .header("Mcp-Session-Id", s)
                .timeout(Duration::from_secs(5))
                .send()
                .await;
        }
    }
}

/// Flattens an MCP `tools/call` result into one text block. Non-text content becomes a
/// placeholder note rather than silence; an error with no content still carries text (an
/// empty `tool_result` with `is_error` is wire-invalid at the provider -- learned the hard
/// way in slice 3).
fn map_call_result(result: &Value, max_bytes: usize) -> McpCallResult {
    let is_error = result
        .get("isError")
        .and_then(|e| e.as_bool())
        .unwrap_or(false);
    let mut parts: Vec<String> = Vec::new();
    if let Some(items) = result.get("content").and_then(|c| c.as_array()) {
        for item in items {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        parts.push(t.to_string());
                    }
                }
                Some(kind @ ("image" | "audio")) => {
                    let mime = item
                        .get("mimeType")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown");
                    parts.push(format!("[{kind} content ({mime}) omitted]"));
                }
                Some("resource_link") => {
                    let uri = item.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                    parts.push(format!("[resource link: {uri}]"));
                }
                Some("resource") => {
                    let res = item.get("resource").cloned().unwrap_or_default();
                    let uri = res.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                    match res.get("text").and_then(|t| t.as_str()) {
                        Some(t) => parts.push(format!("[resource {uri}]\n{t}")),
                        None => parts.push(format!("[binary resource {uri} omitted]")),
                    }
                }
                other => parts.push(format!("[unsupported content type {other:?} omitted]")),
            }
        }
    }
    let mut content = parts.join("\n");
    if content.is_empty()
        && let Some(s) = result.get("structuredContent")
    {
        content = s.to_string();
    }
    if content.is_empty() {
        content = if is_error {
            "MCP server reported an error with no content".to_string()
        } else {
            "(empty result)".to_string()
        };
    }
    let truncated = content.len() > max_bytes;
    if truncated {
        let mut cut = max_bytes;
        while cut > 0 && !content.is_char_boundary(cut) {
            cut -= 1;
        }
        content.truncate(cut);
        content.push_str("\n[result truncated]");
    }
    McpCallResult {
        content,
        is_error,
        truncated,
    }
}

// ---------------------------------------------------------------------------------------------
// Negotiation (create time)
// ---------------------------------------------------------------------------------------------

/// What `auto` may fall back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requested {
    Auto,
    V2Only,
    LegacyOnly,
}

/// Probes the server and lists its tools in one motion. The modern request IS the probe
/// (the spec's backward-compat rule: attempt a modern request first; fall back to
/// `initialize` only when the failure is not a recognized modern JSON-RPC error).
/// `server/discover` exists for up-front version selection, but `tools/list` is the request
/// we need anyway, so probing with it saves the round trip.
///
/// Returns the sealed spec version string and the tool list.
pub async fn negotiate_and_list(
    client: &reqwest::Client,
    url: &reqwest::Url,
    headers: &[(String, String)],
    requested: Requested,
    timeout: Duration,
) -> Result<(String, Vec<RemoteTool>), McpError> {
    if requested != Requested::LegacyOnly {
        let conn = ServerConn::new(
            "probe",
            client.clone(),
            url.clone(),
            headers.to_vec(),
            wire::V2_VERSION,
        );
        match conn.list_tools(timeout).await {
            Ok(tools) => return Ok((wire::V2_VERSION.to_string(), tools)),
            Err(e) => {
                if requested == Requested::V2Only || !legacy_fallback_applies(&e) {
                    return Err(e);
                }
                tracing::debug!(error = %e, "modern MCP probe fell back to the legacy adapter");
            }
        }
    }
    // Legacy: initialize, list, best-effort teardown. The session the handshake minted rides
    // into the listing connection so `tools/list` reuses it rather than re-initializing (a
    // second handshake would leak the first session on the server, and some servers reject a
    // second initialize for the same client).
    let (version, session) = legacy_initialize(client, url, headers, timeout).await?;
    let conn = ServerConn::new(
        "probe",
        client.clone(),
        url.clone(),
        headers.to_vec(),
        &version,
    )
    .with_legacy_session(session);
    let tools = conn.list_tools(timeout).await?;
    conn.close().await;
    Ok((version, tools))
}

/// The spec's fallback rule: a 400/404/405 whose body is NOT a recognized modern JSON-RPC
/// error means the server predates the stateless revision. A recognized modern error means a
/// modern server refusing us -- falling back would mask a real misconfiguration. -32601 on
/// 404 is treated as legacy: `tools/list` is mandatory in every revision, so a modern server
/// cannot honestly answer it with method-not-found.
fn legacy_fallback_applies(e: &McpError) -> bool {
    match e {
        McpError::Rpc { code, .. } => !wire::is_modern_error(*code),
        McpError::Status { status, body } => {
            if !matches!(status, 400 | 404 | 405) {
                return false;
            }
            let code = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| v.get("error")?.get("code")?.as_i64());
            match code {
                Some(c) => !wire::is_modern_error(c),
                None => true,
            }
        }
        _ => false,
    }
}

/// The legacy handshake: `initialize` -> validate the negotiated version -> capture
/// `Mcp-Session-Id` -> `notifications/initialized`.
async fn legacy_initialize(
    client: &reqwest::Client,
    url: &reqwest::Url,
    headers: &[(String, String)],
    timeout: Duration,
) -> Result<(String, Option<String>), McpError> {
    let body = wire::request(
        next_id(),
        "initialize",
        json!({
            "protocolVersion": wire::LEGACY_OFFER,
            "capabilities": {},
            "clientInfo": {"name": wire::CLIENT_NAME, "version": env!("CARGO_PKG_VERSION")}
        }),
        false,
    );
    let outcome = rpc_once(client, url, headers, &[], body, timeout, None).await?;
    let session = outcome.session_id.clone();
    let result = reply_to_result(outcome)?;
    let version = result
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::Protocol("initialize result has no protocolVersion".into()))?
        .to_string();
    if !wire::LEGACY_ACCEPTED.contains(&version.as_str()) {
        return Err(McpError::Protocol(format!(
            "server negotiated protocol {version}, which this adapter does not speak"
        )));
    }
    // notifications/initialized completes the handshake. 202 expected; body-less.
    let mut hs = vec![("MCP-Protocol-Version".to_string(), version.clone())];
    if let Some(s) = &session {
        hs.push(("Mcp-Session-Id".into(), s.clone()));
    }
    notify_once(
        client,
        url,
        headers,
        &hs,
        wire::notification("notifications/initialized", json!({})),
        timeout,
    )
    .await?;
    Ok((version, session))
}

// ---------------------------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------------------------

struct RpcOutcome {
    reply: Reply,
    session_id: Option<String>,
}

fn reply_to_result(o: RpcOutcome) -> Result<Value, McpError> {
    match o.reply {
        Reply::Result(v) => Ok(v),
        Reply::Error {
            code,
            message,
            data,
        } => Err(McpError::Rpc {
            code,
            message,
            data,
        }),
    }
}

/// POSTs one JSON-RPC request and resolves its reply, whether the server answers with a
/// single JSON object or a request-scoped SSE stream. Cancellation drops the connection --
/// on Streamable HTTP, closing the response stream IS the cancellation signal.
async fn rpc_once(
    client: &reqwest::Client,
    url: &reqwest::Url,
    static_headers: &[(String, String)],
    wire_headers: &[(String, String)],
    body: Value,
    timeout: Duration,
    cancel: Option<&CancellationToken>,
) -> Result<RpcOutcome, McpError> {
    let expect_id = body
        .get("id")
        .and_then(|i| i.as_u64())
        .expect("rpc_once takes requests, not notifications");
    let fut = async {
        let mut rb = client
            .post(url.clone())
            .header("Accept", "application/json, text/event-stream")
            .json(&body);
        for (k, v) in static_headers.iter().chain(wire_headers) {
            rb = rb.header(k.as_str(), v.as_str());
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;
        let status = resp.status();
        let session_id = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if !status.is_success() {
            let bytes = read_bounded(resp, 64 * 1024).await.unwrap_or_default();
            let body = String::from_utf8_lossy(&bytes);
            return Err(McpError::Status {
                status: status.as_u16(),
                body: body.chars().take(2048).collect(),
            });
        }
        let is_sse = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("text/event-stream"));
        let reply = if is_sse {
            sse_reply(resp, expect_id).await?
        } else {
            let bytes = read_bounded(resp, MAX_RESPONSE_BYTES).await?;
            let msg: Value = serde_json::from_slice(&bytes)
                .map_err(|e| McpError::Protocol(format!("response is not JSON: {e}")))?;
            wire::parse_reply(&msg, expect_id)
                .map_err(McpError::Protocol)?
                .ok_or_else(|| McpError::Protocol("response is not a reply".into()))?
        };
        Ok(RpcOutcome { reply, session_id })
    };
    with_deadline(fut, timeout, cancel).await
}

/// POSTs one JSON-RPC notification; any 2xx (202 per spec) is success.
async fn notify_once(
    client: &reqwest::Client,
    url: &reqwest::Url,
    static_headers: &[(String, String)],
    wire_headers: &[(String, String)],
    body: Value,
    timeout: Duration,
) -> Result<(), McpError> {
    let fut = async {
        let mut rb = client
            .post(url.clone())
            .header("Accept", "application/json, text/event-stream")
            .json(&body);
        for (k, v) in static_headers.iter().chain(wire_headers) {
            rb = rb.header(k.as_str(), v.as_str());
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let bytes = read_bounded(resp, 64 * 1024).await.unwrap_or_default();
            return Err(McpError::Status {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).chars().take(2048).collect(),
            });
        }
        Ok(())
    };
    with_deadline(fut, timeout, None).await
}

async fn with_deadline<T>(
    fut: impl Future<Output = Result<T, McpError>>,
    timeout: Duration,
    cancel: Option<&CancellationToken>,
) -> Result<T, McpError> {
    let timed = tokio::time::timeout(timeout, fut);
    match cancel {
        Some(c) => tokio::select! {
            _ = c.cancelled() => Err(McpError::Cancelled),
            r = timed => r.map_err(|_| McpError::Timeout)?,
        },
        None => timed.await.map_err(|_| McpError::Timeout)?,
    }
}

/// Reads a request-scoped SSE stream until the reply to `expect_id` arrives. Request-scoped
/// notifications are ignored; a server-initiated JSON-RPC *request* (legacy servers may send
/// sampling/elicitation on this stream) is answered inline with a method-not-found error so
/// the server can finish our call. No `Last-Event-ID`, no resumability -- a broken stream is
/// a failed call, re-issued as a fresh request by whoever owns the retry.
async fn sse_reply(resp: reqwest::Response, expect_id: u64) -> Result<Reply, McpError> {
    use futures_util::StreamExt;
    let mut dec = SseDecoder::default();
    let mut bytes = resp.bytes_stream();
    let mut total = 0usize;
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|e| McpError::Transport(e.to_string()))?;
        total += chunk.len();
        if total > MAX_RESPONSE_BYTES {
            return Err(McpError::Protocol(format!(
                "SSE response exceeded {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        for frame in dec
            .feed(&chunk)
            .map_err(|e| McpError::Protocol(e.to_string()))?
        {
            if frame.data.is_empty() {
                continue;
            }
            let msg: Value = serde_json::from_slice(frame.data.as_bytes())
                .map_err(|e| McpError::Protocol(format!("SSE frame is not JSON: {e}")))?;
            if let Some(reply) = wire::parse_reply(&msg, expect_id).map_err(McpError::Protocol)? {
                return Ok(reply);
            }
            // Not our reply: a notification (ignored) or a server-initiated request
            // (unsupported by declaration; it gets a typed refusal in the error log).
            if msg.get("method").is_some() && msg.get("id").is_some() {
                tracing::warn!(
                    method = %msg["method"],
                    "legacy MCP server sent a server-initiated request; this client declares no such capabilities"
                );
            }
        }
    }
    Err(McpError::Protocol(
        "SSE stream ended without the reply".into(),
    ))
}

async fn read_bounded(resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, McpError> {
    use futures_util::StreamExt;
    let mut out = Vec::new();
    let mut bytes = resp.bytes_stream();
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|e| McpError::Transport(e.to_string()))?;
        if out.len() + chunk.len() > cap {
            return Err(McpError::Protocol(format!("response exceeded {cap} bytes")));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_result_mapping_covers_the_content_zoo() {
        let r = map_call_result(
            &json!({
                "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "image", "mimeType": "image/png", "data": "AAAA"},
                    {"type": "resource", "resource": {"uri": "file:///x", "text": "body"}},
                    {"type": "resource_link", "uri": "https://e.com/r"}
                ]
            }),
            1024,
        );
        assert!(!r.is_error && !r.truncated);
        assert!(r.content.contains("hello"));
        assert!(r.content.contains("[image content (image/png) omitted]"));
        assert!(r.content.contains("[resource file:///x]\nbody"));
        assert!(r.content.contains("[resource link: https://e.com/r]"));
    }

    #[test]
    fn error_results_never_map_to_empty_content() {
        let r = map_call_result(&json!({"isError": true, "content": []}), 1024);
        assert!(r.is_error);
        assert!(
            !r.content.is_empty(),
            "empty error tool_result is wire-invalid"
        );
    }

    #[test]
    fn structured_content_fills_an_empty_body_and_truncation_is_bounded() {
        let r = map_call_result(&json!({"content": [], "structuredContent": {"a": 1}}), 1024);
        assert_eq!(r.content, "{\"a\":1}");
        let big = "x".repeat(4096);
        let r = map_call_result(&json!({"content": [{"type":"text","text": big}]}), 100);
        assert!(r.truncated);
        assert!(r.content.len() <= 100 + "\n[result truncated]".len());
    }

    #[test]
    fn fallback_rule_distinguishes_modern_refusals_from_legacy_servers() {
        // Modern refusals: no fallback.
        for code in [
            wire::HEADER_MISMATCH,
            wire::MISSING_CLIENT_CAPABILITY,
            wire::UNSUPPORTED_PROTOCOL_VERSION,
        ] {
            assert!(!legacy_fallback_applies(&McpError::Rpc {
                code,
                message: "no".into(),
                data: None
            }));
            assert!(!legacy_fallback_applies(&McpError::Status {
                status: 400,
                body: format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{{\"code\":{code},\"message\":\"no\"}}}}"
                ),
            }));
        }
        // Legacy-shaped failures: fall back.
        assert!(legacy_fallback_applies(&McpError::Status {
            status: 400,
            body: "session required".into()
        }));
        assert!(legacy_fallback_applies(&McpError::Status {
            status: 405,
            body: String::new()
        }));
        assert!(legacy_fallback_applies(&McpError::Rpc {
            code: wire::METHOD_NOT_FOUND,
            message: "tools/list?".into(),
            data: None
        }));
        // Transport failures are failures, not era signals.
        assert!(!legacy_fallback_applies(&McpError::Transport(
            "refused".into()
        )));
        assert!(!legacy_fallback_applies(&McpError::Status {
            status: 500,
            body: "oops".into()
        }));
    }
}
