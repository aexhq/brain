//! The SDK / configuration surface, and the sealed prefix.
//!
//! Platform invariant (`design-decisions.md` §1.12): **the prefix is immutable
//! for a session's life.** Tools, system prompt, MCP set and model cannot change
//! mid-session; changing any of them forks a new session. One appended tool
//! definition destroyed a 6,103-token cache entry, so this is enforced by
//! construction rather than by convention: there is no `&mut` path to a
//! `SealedPrefix` anywhere in this crate, and the digest is computed once.

use crate::message::Usage;
use crate::{BrainError, Result, Shared};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A BYOK provider credential. Per-session and frozen.
///
/// We never hold provider keys as our own (`design-decisions.md` §3). The
/// redacting `Debug` is not politeness: a key that reaches a log, a span
/// attribute or a journal entry has leaked into storage we control, which is
/// precisely the thing BYOK promises does not happen.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderKey(String);

impl ProviderKey {
    pub fn new(s: impl Into<String>) -> Self {
        ProviderKey(s.into())
    }
    /// The only accessor. Deliberately named so a grep for `expose` finds every
    /// place a credential is read.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ProviderKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProviderKey(<redacted {} bytes>)", self.0.len())
    }
}

/// Which wire dialect a provider speaks. The seam is a two-variant enum today
/// and a `Box<dyn Provider>` at the call site, so a third dialect is additive:
/// one new module, one new match arm in `Provider::for_dialect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// Anthropic Messages API (`/v1/messages`, `content` blocks, `tool_use`).
    AnthropicMessages,
    /// OpenAI-compatible Chat Completions (`/v1/chat/completions`, `tool_calls`).
    OpenAiChat,
}

/// A tool as the *model* sees it. This is prefix content: it is digested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDecl {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool input. Rendered verbatim into the request.
    pub input_schema: serde_json::Value,
    /// JSON Schema for the successful tool result. This is part of the sealed
    /// definition even though model providers currently only receive the input schema.
    pub output_schema: serde_json::Value,
    /// Where dispatch goes. Not digested -- it is our routing, not model-visible
    /// prefix content.
    #[serde(skip)]
    pub route: ToolRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRoute {
    /// A deliberately selected Brain engine capability. The model-visible name is
    /// unrelated to this stable capability identifier.
    Intrinsic(String),
    /// Forwarded over the Brain->Hand ABI using this exact execution seal.
    Hand(HandToolSeal),
    /// A sealed remote MCP tool: the brain makes the outbound call itself
    /// behind the one `Outbound` seam with its SSRF guard (ARCHITECTURE-v1
    /// D14 -- which supersedes §1.11's connector tier; there is no separate
    /// connector in MVP).
    Mcp { server: String, remote_name: String },
    /// A host-owned trusted executor registered under a stable capability.
    Server(ServerToolPolicy),
    /// An explicitly attached SDK callback. Calls are never replayed after dispatch.
    Attached { callback_id: String },
}

impl Default for ToolRoute {
    fn default() -> Self {
        Self::Intrinsic("brain.invalid".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandToolSeal {
    pub protocol: i64,
    pub checksum: String,
    pub source: brain_protocol::session::HandToolSource,
    pub required_env: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerToolPolicy {
    pub capability: String,
    pub scope: brain_protocol::session::ExternalToolScope,
    pub completion: brain_protocol::session::ExternalToolCompletion,
    pub effect: brain_protocol::session::ExternalToolEffect,
    pub max_input_bytes: usize,
}

/// An MCP server as declared at session start. Digested, because attaching one
/// changes the tool list and therefore the prefix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerDecl {
    pub name: String,
    pub url: String,
    /// The NEGOTIATED protocol revision, sealed at create: `2026-07-28` for the
    /// stateless spec, an initialization-era date for the legacy adapter. All
    /// three fields reach the digest.
    pub spec_version: String,
}

/// Sampling parameters. Digested: they are part of what makes a request
/// reproducible, though not part of the cache prefix proper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenOpts {
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub stop_sequences: Vec<String>,
}

impl Default for GenOpts {
    fn default() -> Self {
        GenOpts {
            max_tokens: 4096,
            temperature: None,
            stop_sequences: Vec::new(),
        }
    }
}

/// Caps that bound a session's blast radius. Not model-visible, not digested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum subagent nesting. Pi's example spawns an OS process per child
    /// with **unbounded nesting**; this is the direct answer to that.
    pub max_subagent_depth: u32,
    /// Maximum children in one fan-out.
    pub max_fanout: usize,
    /// Maximum model rounds in one turn before the loop is declared runaway.
    pub max_rounds: u32,
    /// Maximum tool calls dispatched concurrently from one assistant message.
    /// p90 batch size is 4; this is not sized for 64.
    pub max_parallel_tools: usize,
    /// Maximum `task` subagent identities minted over the session's LIFETIME (D11).
    /// Counted from journaled `task` tool calls, so the cap survives re-materialise.
    pub max_subagent_identities: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_subagent_depth: 3,
            max_fanout: 16,
            max_rounds: 128,
            max_parallel_tools: 8,
            max_subagent_identities: 12,
        }
    }
}

/// What a caller declares *before* serving. This is the SDK surface.
///
/// It is deliberately a plain struct with a builder rather than a registry with
/// runtime mutation: everything a session needs must exist before the session
/// exists, which is what makes the seal cheap.
#[derive(Debug, Clone)]
pub struct AgentDef {
    pub system_prompt: String,
    pub tools: Vec<ToolDecl>,
    pub mcp_servers: Vec<McpServerDecl>,
    pub model: String,
    pub dialect: Dialect,
    pub sampling: GenOpts,
    pub limits: Limits,
    /// Named child agent definitions this agent may dispatch to. A child gets
    /// its own sealed prefix -- a subagent is a session, and every session
    /// seals.
    pub subagents: BTreeMap<String, Arc<AgentDef>>,
}

impl AgentDef {
    pub fn new(
        system_prompt: impl Into<String>,
        model: impl Into<String>,
        dialect: Dialect,
    ) -> Self {
        AgentDef {
            system_prompt: system_prompt.into(),
            tools: Vec::new(),
            mcp_servers: Vec::new(),
            model: model.into(),
            dialect,
            sampling: GenOpts::default(),
            limits: Limits::default(),
            subagents: BTreeMap::new(),
        }
    }
    pub fn tool(mut self, t: ToolDecl) -> Self {
        self.tools.push(t);
        self
    }
    pub fn mcp(mut self, m: McpServerDecl) -> Self {
        self.mcp_servers.push(m);
        self
    }
    pub fn subagent(mut self, name: impl Into<String>, def: AgentDef) -> Self {
        self.subagents.insert(name.into(), Arc::new(def));
        self
    }
    pub fn limits(mut self, l: Limits) -> Self {
        self.limits = l;
        self
    }
    pub fn sampling(mut self, g: GenOpts) -> Self {
        self.sampling = g;
        self
    }

    /// Seal. Consumes the definition into a shared, immutable prefix.
    pub fn seal(self) -> Shared<SealedPrefix> {
        let digest = prefix_digest(&self);
        Arc::new(SealedPrefix {
            digest,
            system_prompt: self.system_prompt,
            tools: self.tools,
            mcp_servers: self.mcp_servers,
            model: self.model,
            dialect: self.dialect,
            sampling: self.sampling,
            limits: self.limits,
            subagents: self.subagents,
        })
    }
}

/// The frozen prefix. There is no public constructor other than `AgentDef::seal`
/// and no `&mut self` method: the type system is the enforcement.
#[derive(Debug)]
pub struct SealedPrefix {
    digest: String,
    pub system_prompt: String,
    pub tools: Vec<ToolDecl>,
    pub mcp_servers: Vec<McpServerDecl>,
    pub model: String,
    pub dialect: Dialect,
    pub sampling: GenOpts,
    pub limits: Limits,
    pub subagents: BTreeMap<String, Arc<AgentDef>>,
}

impl SealedPrefix {
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn tool(&self, name: &str) -> Option<&ToolDecl> {
        self.tools.iter().find(|t| t.name == name)
    }
    /// The rejection path an SDK caller hits if they try to mutate mid-session.
    /// Present so the error is typed and attributable rather than a panic or a
    /// silently-ignored write.
    pub fn reject_mutation(&self, what: &'static str) -> BrainError {
        BrainError::PrefixSealed {
            digest: self.digest.clone(),
            what,
        }
    }
    /// The uniform child prefix for subagents: the parent's prefix minus trusted server tools
    /// explicitly scoped to the root. The child keeps the subagent capability itself -- depth is enforced at
    /// dispatch, so one uniform child prefix serves every depth. Derived
    /// deterministically from the sealed prefix: the SESSION digest is unchanged
    /// and replay reconstructs children exactly.
    pub(crate) fn task_child(&self) -> SealedPrefix {
        SealedPrefix {
            digest: self.digest.clone(),
            system_prompt: self.system_prompt.clone(),
            tools: self
                .tools
                .iter()
                .filter(|tool| {
                    !matches!(
                        &tool.route,
                        ToolRoute::Server(policy)
                            if policy.scope
                                == brain_protocol::session::ExternalToolScope::Root
                    )
                })
                .cloned()
                .collect(),
            mcp_servers: self.mcp_servers.clone(),
            model: self.model.clone(),
            dialect: self.dialect,
            sampling: self.sampling.clone(),
            limits: self.limits,
            subagents: self.subagents.clone(),
        }
    }

    /// Heap bytes owned by the prefix. Shared across every session that seals
    /// the same definition, so it is charged once to the host, not per session.
    pub fn heap_bytes(&self) -> usize {
        self.system_prompt.capacity()
            + self
                .tools
                .iter()
                .map(|t| {
                    t.name.capacity()
                        + t.description.capacity()
                        + serde_json::to_string(&t.input_schema)
                            .map(|s| s.len())
                            .unwrap_or(0)
                        + serde_json::to_string(&t.output_schema)
                            .map(|s| s.len())
                            .unwrap_or(0)
                })
                .sum::<usize>()
            + self.model.capacity()
    }
}

/// Canonical digest over exactly the model-visible prefix plus the parameters
/// that make a request reproducible. Deliberately excludes:
///  - the BYOK key (a credential is not prefix content),
///  - tool routing (our dispatch, not the model's view),
///  - limits (our blast radius, not the model's view).
///
/// `serde_json::Map` is a `BTreeMap` in this build (the `preserve_order` feature
/// is off), so object keys serialize sorted and user-supplied schemas digest
/// stably regardless of the order they were written in.
pub fn prefix_digest(def: &AgentDef) -> String {
    #[derive(Serialize)]
    struct Canon<'a> {
        v: u8,
        system_prompt: &'a str,
        model: &'a str,
        dialect: Dialect,
        tools: Vec<CanonTool<'a>>,
        mcp: Vec<CanonMcp<'a>>,
        max_tokens: u32,
        temperature: Option<f32>,
        stop_sequences: &'a [String],
        subagents: Vec<(&'a str, String)>,
    }
    #[derive(Serialize)]
    struct CanonTool<'a> {
        name: &'a str,
        description: &'a str,
        input_schema: &'a serde_json::Value,
        output_schema: &'a serde_json::Value,
    }
    #[derive(Serialize)]
    struct CanonMcp<'a> {
        name: &'a str,
        url: &'a str,
        spec_version: &'a str,
    }

    // Tool order is model-visible (it is the literal render order) so it is NOT
    // sorted here. Two definitions that differ only in tool order are two
    // different prefixes, because they are two different cache keys.
    let canon = Canon {
        v: 1,
        system_prompt: &def.system_prompt,
        model: &def.model,
        dialect: def.dialect,
        tools: def
            .tools
            .iter()
            .map(|t| CanonTool {
                name: &t.name,
                description: &t.description,
                input_schema: &t.input_schema,
                output_schema: &t.output_schema,
            })
            .collect(),
        mcp: def
            .mcp_servers
            .iter()
            .map(|m| CanonMcp {
                name: &m.name,
                url: &m.url,
                spec_version: &m.spec_version,
            })
            .collect(),
        max_tokens: def.sampling.max_tokens,
        temperature: def.sampling.temperature,
        stop_sequences: &def.sampling.stop_sequences,
        // Children are digested by their own digest: a change deep in a subagent
        // definition changes the parent's digest too, which is what stops a
        // "same prefix" claim from being true only at the top level.
        subagents: def
            .subagents
            .iter()
            .map(|(k, v)| (k.as_str(), prefix_digest(v)))
            .collect(),
    };

    let bytes = serde_json::to_vec(&canon).expect("canonical prefix is always serializable");
    let mut h = Sha256::new();
    h.update(&bytes);
    hex::encode(h.finalize())
}

/// Everything a *session* is opened with. The prefix plus the frozen credential.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub prefix: Shared<SealedPrefix>,
    pub key: ProviderKey,
    pub base_url: String,
}

impl SessionConfig {
    pub fn new(
        prefix: Shared<SealedPrefix>,
        key: ProviderKey,
        base_url: impl Into<String>,
    ) -> Self {
        SessionConfig {
            prefix,
            key,
            base_url: base_url.into(),
        }
    }
    /// The mutation rejection an SDK caller sees. Exists so the invariant has a
    /// test target rather than being an unwritten rule.
    pub fn try_set_model(&self, _m: &str) -> Result<()> {
        Err(self.prefix.reject_mutation("model"))
    }
    pub fn try_add_tool(&self, _t: ToolDecl) -> Result<()> {
        Err(self.prefix.reject_mutation("tools"))
    }
    pub fn try_set_system_prompt(&self, _s: &str) -> Result<()> {
        Err(self.prefix.reject_mutation("system_prompt"))
    }
    pub fn try_add_mcp(&self, _m: McpServerDecl) -> Result<()> {
        Err(self.prefix.reject_mutation("mcp_servers"))
    }
}

/// Aggregate usage, carried per session and per turn. Passed through from the
/// provider; we never estimate it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionUsage {
    pub total: Usage,
    pub rounds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"b":{"type":"string"},"a":{"type":"number"}}})
    }

    fn def() -> AgentDef {
        AgentDef::new("you are a test", "test-model", Dialect::AnthropicMessages).tool(ToolDecl {
            name: "read".into(),
            description: "read a file".into(),
            input_schema: schema(),
            output_schema: serde_json::json!({"type":"object"}),
            route: ToolRoute::Intrinsic("brain.test.read".into()),
        })
    }

    #[test]
    fn digest_is_stable_and_order_independent_within_a_schema() {
        let a = prefix_digest(&def());
        let b = prefix_digest(&def());
        assert_eq!(a, b);

        // Same schema, keys written in the other order -> same digest, because
        // serde_json::Map sorts.
        let mut d2 = def();
        d2.tools[0].input_schema = serde_json::json!(
            {"properties":{"a":{"type":"number"},"b":{"type":"string"}},"type":"object"});
        assert_eq!(
            prefix_digest(&d2),
            a,
            "canonicalisation must be key-order stable"
        );
    }

    #[test]
    fn appending_a_tool_changes_the_digest() {
        let base = prefix_digest(&def());
        let more = def().tool(ToolDecl {
            name: "write".into(),
            description: "write a file".into(),
            input_schema: schema(),
            output_schema: serde_json::json!({"type":"object"}),
            route: ToolRoute::Intrinsic("brain.test.write".into()),
        });
        assert_ne!(
            prefix_digest(&more),
            base,
            "one appended tool definition must fork the prefix (design-decisions §1.12)"
        );
    }

    #[test]
    fn tool_order_is_significant() {
        let mut swapped = def().tool(ToolDecl {
            name: "write".into(),
            description: "write a file".into(),
            input_schema: schema(),
            output_schema: serde_json::json!({"type":"object"}),
            route: ToolRoute::Intrinsic("brain.test.write".into()),
        });
        let forward = prefix_digest(&swapped);
        swapped.tools.swap(0, 1);
        assert_ne!(
            prefix_digest(&swapped),
            forward,
            "render order is cache-visible"
        );
    }

    #[test]
    fn routing_and_limits_are_not_digested() {
        let base = prefix_digest(&def());
        let mut d = def();
        d.tools[0].route = ToolRoute::Mcp {
            server: "test".into(),
            remote_name: "read".into(),
        };
        d.limits.max_fanout = 999;
        assert_eq!(
            prefix_digest(&d),
            base,
            "our routing is not model-visible prefix"
        );
    }

    #[test]
    fn subagent_change_propagates_to_parent_digest() {
        let child = AgentDef::new("child v1", "test-model", Dialect::AnthropicMessages);
        let p1 = def().subagent("explore", child);
        let d1 = prefix_digest(&p1);
        let child2 = AgentDef::new("child v2", "test-model", Dialect::AnthropicMessages);
        let p2 = def().subagent("explore", child2);
        assert_ne!(prefix_digest(&p2), d1);
    }

    #[test]
    fn task_child_keeps_intrinsics_and_removes_root_scoped_server_tools() {
        let sealed = AgentDef::new("parent", "test-model", Dialect::AnthropicMessages)
            .tool(ToolDecl {
                name: "delegate".into(),
                description: "delegate".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: serde_json::json!({"type":"string"}),
                route: ToolRoute::Intrinsic("brain.subagents".into()),
            })
            .tool(ToolDecl {
                name: "root_only".into(),
                description: "root".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: serde_json::json!({"type":"object"}),
                route: ToolRoute::Server(ServerToolPolicy {
                    capability: "test.root".into(),
                    scope: brain_protocol::session::ExternalToolScope::Root,
                    completion: brain_protocol::session::ExternalToolCompletion::Continue,
                    effect: brain_protocol::session::ExternalToolEffect::Opaque,
                    max_input_bytes: 1024,
                }),
            })
            .tool(ToolDecl {
                name: "read".into(),
                description: "read".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: serde_json::json!({"type":"object"}),
                route: ToolRoute::Intrinsic("brain.test.read".into()),
            })
            .seal();
        let child = sealed.task_child();
        assert_eq!(child.digest(), sealed.digest());
        assert_eq!(child.limits, sealed.limits);
        assert_eq!(
            child
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["delegate", "read"]
        );
    }

    #[test]
    fn sealed_prefix_rejects_every_mutation_path() {
        let cfg = SessionConfig::new(def().seal(), ProviderKey::new("sk-test"), "http://x");
        assert!(matches!(
            cfg.try_set_model("other"),
            Err(BrainError::PrefixSealed { .. })
        ));
        assert!(matches!(
            cfg.try_set_system_prompt("other"),
            Err(BrainError::PrefixSealed { .. })
        ));
        assert!(matches!(
            cfg.try_add_mcp(McpServerDecl {
                name: "x".into(),
                url: "http://y".into(),
                spec_version: "2026-07-28".into()
            }),
            Err(BrainError::PrefixSealed { .. })
        ));
    }

    #[test]
    fn provider_key_never_prints_itself() {
        let k = ProviderKey::new("sk-ant-super-secret-value");
        let shown = format!("{:?}", k);
        assert!(
            !shown.contains("secret"),
            "credential leaked into Debug: {shown}"
        );
        let cfg = SessionConfig::new(def().seal(), k, "http://x");
        let shown = format!("{:?}", cfg);
        assert!(
            !shown.contains("secret"),
            "credential leaked via SessionConfig: {shown}"
        );
    }
}
