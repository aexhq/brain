//! MCP for the brain (ARCHITECTURE-v1 D11/D14): remote MCP servers become ordinary sealed
//! tools.
//!
//! Shape of the feature:
//! - **Resolved once at create, sealed forever.** `tools/list` runs at session create; the
//!   full tool schemas (namespaced `server__tool`) are persisted in the HEAD prefix doc, so
//!   rehydration does zero network I/O and a server-side schema drift can never silently fork
//!   the prefix digest. Changing the MCP set forks a new session (§1.12).
//! - **Brain-side dispatch behind the `Outbound` SSRF guard.** MCP URLs are user-controlled;
//!   every request goes through [`crate::outbound::Outbound`]. Per-server headers (the
//!   credentials) live in key custody, never in the journal plaintext and never in the hand.
//! - **2026-07-28 primary, thin legacy adapter** ([`client`]); the negotiated version is
//!   sealed and digested.
//!
//! stdio MCP servers are out of scope by design: users install those into their own hand and
//! the brain reaches them over the existing tool channel.

pub mod client;
pub mod wire;

use crate::journal::{McpServerDoc, McpToolDoc};
use crate::outbound::Outbound;
use crate::{BrainError, Result};
use aex_contracts::session::McpServerConfig;
use client::{McpError, Requested, ServerConn};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const MAX_SERVERS: usize = 8;
pub const MAX_TOOLS_PER_SERVER: usize = 128;
/// Bound on one tool's description; a tool schema is prefix content and prefix bytes are
/// paid by every request.
pub const MAX_DESCRIPTION_BYTES: usize = 4096;
pub const MAX_SCHEMA_BYTES: usize = 64 * 1024;

/// Provider tool-name rule (the strictest of the certified dialects): `[a-zA-Z0-9_-]`, at
/// most 64 chars. The namespaced `server__tool` name must fit it.
fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Header names a per-server credential map may not set: transport-owned or wire-protocol
/// headers. Everything is compared lowercased.
fn reserved_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("mcp-")
        || matches!(
            n.as_str(),
            "host"
                | "content-length"
                | "content-type"
                | "accept"
                | "connection"
                | "transfer-encoding"
                | "te"
                | "upgrade"
        )
}

fn requested_of(cfg: &McpServerConfig) -> Requested {
    use aex_contracts::session::McpProtocol;
    match cfg.protocol {
        None | Some(McpProtocol::Auto) => Requested::Auto,
        Some(McpProtocol::X202607) => Requested::V2Only,
        Some(McpProtocol::Legacy) => Requested::LegacyOnly,
    }
}

/// What create seals: the server records (digested) and the namespaced tool declarations
/// (digested, rendered to the model verbatim).
#[derive(Debug)]
pub struct ResolvedMcp {
    pub servers: Vec<McpServerDoc>,
    pub tools: Vec<McpToolDoc>,
}

/// Resolves the declared MCP servers at session create: validates the configs, probes each
/// server concurrently (the modern request is the probe), lists its tools, applies the
/// allowlist and the naming rules, and returns what the prefix seals. STRICT: an unreachable
/// server, a failed negotiation, or an explicitly allowlisted tool that is unusable fails the
/// create -- a session sealed with fewer tools than asked for is a silent lie.
pub async fn resolve_at_create(
    outbound: &Outbound,
    cfgs: &[McpServerConfig],
    per_server_timeout: Duration,
) -> Result<ResolvedMcp> {
    if cfgs.len() > MAX_SERVERS {
        return Err(BrainError::Invalid(format!(
            "tools.mcp: at most {MAX_SERVERS} servers"
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for cfg in cfgs {
        let name: &str = &cfg.name;
        if name.contains("__") {
            // "__" is the namespace separator; a server name containing it would make
            // `server__tool` ambiguous.
            return Err(BrainError::Invalid(format!(
                "tools.mcp server {name:?}: name must not contain \"__\""
            )));
        }
        if !seen.insert(name.to_string()) {
            return Err(BrainError::Invalid(format!(
                "tools.mcp server {name:?} is declared twice"
            )));
        }
        for (k, v) in &cfg.headers {
            if reserved_header(k) {
                return Err(BrainError::Invalid(format!(
                    "tools.mcp server {name:?}: header {k:?} is transport-owned"
                )));
            }
            if !k
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || k.is_empty()
            {
                return Err(BrainError::Invalid(format!(
                    "tools.mcp server {name:?}: header name {k:?} is not a valid token"
                )));
            }
            if v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
                return Err(BrainError::Invalid(format!(
                    "tools.mcp server {name:?}: header {k:?} value contains control bytes"
                )));
            }
        }
        outbound.check_url(&cfg.url)?;
    }

    let jobs = cfgs.iter().map(|cfg| async move {
        let url = outbound.check_url(&cfg.url)?;
        let headers: Vec<(String, String)> = cfg
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let (version, remote) = client::negotiate_and_list(
            outbound.client(),
            &url,
            &headers,
            requested_of(cfg),
            per_server_timeout,
        )
        .await
        .map_err(|e| BrainError::Invalid(format!("tools.mcp server {:?}: {e}", cfg.name)))?;
        seal_server_tools(cfg, version, remote)
    });
    let results = futures_util::future::join_all(jobs).await;

    let mut servers = Vec::with_capacity(cfgs.len());
    let mut tools = Vec::new();
    for r in results {
        let (doc, mut ts) = r?;
        servers.push(doc);
        tools.append(&mut ts);
    }
    Ok(ResolvedMcp { servers, tools })
}

/// Applies the allowlist and validity rules to one server's listed tools. Invalid tools are
/// EXCLUDED with a warning (the spec's own rule for bad `x-mcp-header` annotations) -- unless
/// the caller allowlisted them by name, which turns exclusion into a loud create failure.
fn seal_server_tools(
    cfg: &McpServerConfig,
    version: String,
    remote: Vec<client::RemoteTool>,
) -> Result<(McpServerDoc, Vec<McpToolDoc>)> {
    let server: &str = &cfg.name;
    let allow: Option<std::collections::HashSet<&str>> = if cfg.allowed_tools.is_empty() {
        None
    } else {
        Some(cfg.allowed_tools.iter().map(|s| s.as_str()).collect())
    };
    let mut out = Vec::new();
    let mut matched: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for t in &remote {
        if let Some(allow) = &allow {
            if !allow.contains(t.name.as_str()) {
                continue;
            }
            matched.insert(t.name.as_str());
        }
        let namespaced = format!("{server}__{}", t.name);
        let verdict = tool_verdict(&namespaced, t);
        match verdict {
            Ok(header_params) => {
                drop(header_params); // re-derived at hydrate; the schema is the source of truth
                out.push(McpToolDoc {
                    name: namespaced,
                    server: server.to_string(),
                    remote_name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                });
            }
            Err(why) => {
                if allow.is_some() {
                    return Err(BrainError::Invalid(format!(
                        "tools.mcp server {server:?}: allowed tool {:?} is unusable: {why}",
                        t.name
                    )));
                }
                tracing::warn!(server = %server, tool = %t.name, why = %why,
                    "excluding an unusable MCP tool");
            }
        }
        if out.len() > MAX_TOOLS_PER_SERVER {
            return Err(BrainError::Invalid(format!(
                "tools.mcp server {server:?} serves more than {MAX_TOOLS_PER_SERVER} tools; use allowed_tools"
            )));
        }
    }
    if let Some(allow) = &allow {
        let missing: Vec<&str> = allow
            .iter()
            .filter(|n| !matched.contains(**n))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(BrainError::Invalid(format!(
                "tools.mcp server {server:?}: allowed tools {missing:?} were not served by tools/list"
            )));
        }
    }
    Ok((
        McpServerDoc {
            name: server.to_string(),
            url: cfg.url.clone(),
            spec_version: version,
        },
        out,
    ))
}

/// Why one tool is unusable, or its validated header params.
fn tool_verdict(
    namespaced: &str,
    t: &client::RemoteTool,
) -> std::result::Result<Vec<wire::HeaderParam>, String> {
    if !valid_tool_name(namespaced) {
        return Err(format!(
            "namespaced name {namespaced:?} does not fit the provider tool-name rule ([a-zA-Z0-9_-], max 64)"
        ));
    }
    if t.description.len() > MAX_DESCRIPTION_BYTES {
        return Err(format!(
            "description is {} bytes (max {MAX_DESCRIPTION_BYTES}); prefix bytes are paid on every request",
            t.description.len()
        ));
    }
    let schema_bytes = serde_json::to_vec(&t.input_schema)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if schema_bytes > MAX_SCHEMA_BYTES {
        return Err(format!(
            "input schema is {schema_bytes} bytes (max {MAX_SCHEMA_BYTES})"
        ));
    }
    wire::validate_tool_headers(&t.input_schema)
}

// ---------------------------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------------------------

/// Everything one resident session needs to dispatch MCP calls. Built at hydrate from the
/// sealed prefix doc plus the custody-decrypted header map; holds the lazy legacy sessions.
pub struct McpRuntime {
    conns: HashMap<String, Arc<ServerConn>>,
    tools: HashMap<String, RuntimeTool>,
    call_timeout: Duration,
    max_result_bytes: usize,
}

struct RuntimeTool {
    server: String,
    remote_name: String,
    header_params: Vec<wire::HeaderParam>,
}

impl McpRuntime {
    pub fn build(
        outbound: &Outbound,
        servers: &[McpServerDoc],
        tools: &[McpToolDoc],
        secrets: &HashMap<String, HashMap<String, String>>,
        call_timeout: Duration,
        max_result_bytes: usize,
    ) -> Result<Self> {
        let mut conns = HashMap::with_capacity(servers.len());
        for s in servers {
            let url = outbound.check_url(&s.url)?;
            let headers: Vec<(String, String)> = secrets
                .get(&s.name)
                .map(|h| h.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            conns.insert(
                s.name.clone(),
                Arc::new(ServerConn::new(
                    &s.name,
                    outbound.client().clone(),
                    url,
                    headers,
                    &s.spec_version,
                )),
            );
        }
        let mut map = HashMap::with_capacity(tools.len());
        for t in tools {
            let header_params = wire::validate_tool_headers(&t.input_schema).map_err(|e| {
                BrainError::Invalid(format!(
                    "sealed MCP tool {} no longer validates: {e}",
                    t.name
                ))
            })?;
            map.insert(
                t.name.clone(),
                RuntimeTool {
                    server: t.server.clone(),
                    remote_name: t.remote_name.clone(),
                    header_params,
                },
            );
        }
        Ok(McpRuntime {
            conns,
            tools: map,
            call_timeout,
            max_result_bytes,
        })
    }

    /// Dispatches one sealed MCP tool call. Always resolves to a `CallOutcome` -- errors are
    /// the model's to read, not ours to escalate; the existing outcome vocabulary applies
    /// (`completed` / `failed` / `cancelled` / `deadline_exceeded`).
    pub async fn call(
        &self,
        namespaced: &str,
        input: &serde_json::Value,
        cancel: &CancellationToken,
    ) -> crate::adapter::CallOutcome {
        use crate::adapter::CallOutcome;
        let t0 = std::time::Instant::now();
        let Some(tool) = self.tools.get(namespaced) else {
            return CallOutcome::failed(crate::tools::undeclared(namespaced));
        };
        let conn = self
            .conns
            .get(&tool.server)
            .expect("sealed tool names a sealed server");
        let result = conn
            .call_tool(
                &tool.remote_name,
                input,
                &tool.header_params,
                self.max_result_bytes,
                self.call_timeout,
                cancel,
            )
            .await;
        let duration_ms = t0.elapsed().as_millis() as u64;
        match result {
            Ok(r) => CallOutcome {
                outcome: if r.is_error {
                    "failed".into()
                } else {
                    "completed".into()
                },
                content: r.content,
                is_error: r.is_error,
                exit_code: None,
                duration_ms,
                truncated: r.truncated,
                terminal: None,
            },
            Err(McpError::Cancelled) => CallOutcome {
                outcome: "cancelled".into(),
                content: "cancelled".into(),
                is_error: true,
                exit_code: None,
                duration_ms,
                truncated: false,
                terminal: None,
            },
            Err(McpError::Timeout) => CallOutcome {
                outcome: "deadline_exceeded".into(),
                content: format!(
                    "MCP call exceeded the {}ms deadline",
                    self.call_timeout.as_millis()
                ),
                is_error: true,
                exit_code: None,
                duration_ms,
                truncated: false,
                terminal: None,
            },
            Err(e) => {
                let mut o = CallOutcome::failed(format!("MCP server {}: {e}", tool.server));
                o.duration_ms = duration_ms;
                o
            }
        }
    }

    /// Best-effort teardown of any live legacy sessions (session end/delete).
    pub async fn close(&self) {
        for conn in self.conns.values() {
            conn.close().await;
        }
    }
}

impl std::fmt::Debug for McpRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpRuntime")
            .field("servers", &self.conns.len())
            .field("tools", &self.tools.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_rule() {
        assert!(valid_tool_name("github__create_issue"));
        assert!(valid_tool_name("a__b-c_D9"));
        assert!(!valid_tool_name(""));
        assert!(!valid_tool_name("bad name"));
        assert!(!valid_tool_name("emoji__🦀"));
        assert!(!valid_tool_name(&"x".repeat(65)));
    }

    #[test]
    fn reserved_headers_are_refused() {
        for h in [
            "Mcp-Session-Id",
            "MCP-Protocol-Version",
            "mcp-method",
            "Host",
            "Content-Length",
            "connection",
        ] {
            assert!(reserved_header(h), "{h} must be reserved");
        }
        assert!(!reserved_header("Authorization"));
        assert!(!reserved_header("X-Api-Key"));
    }
}
