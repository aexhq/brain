//! The SDK / configuration surface, and the sealed prefix.
//!
//! Core invariant: **the prefix is immutable for a session's life.** Tools, system prompt,
//! and model cannot change
//! mid-session; changing any of them forks a new session. One appended tool
//! definition destroyed a 6,103-token cache entry, so this is enforced by
//! construction rather than by convention: there is no `&mut` path to a
//! `SealedPrefix` anywhere in this crate, and the digest is computed once.

use crate::message::Usage;
use crate::{BrainError, Result, Shared};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// A BYOK provider credential. Per-session and frozen.
///
/// Provider keys remain per-session customer credentials. The redacting `Debug` is not politeness:
/// a key that reaches a log, a span
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
    /// Immutable authoring contract digest, used to fence customer and managed executors.
    pub contract_digest: String,
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
    /// Routed through the typed Hand operation/receipt seam using this exact execution seal.
    Hand(HandToolSeal),
    /// A host-owned trusted executor registered under a stable capability.
    Server(ServerToolPolicy),
    /// A customer-app registration routed through the durable CustomerCoordinator seam.
    Customer { registration: String },
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
    pub required_env: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerToolPolicy {
    pub capability: String,
    pub scope: brain_protocol::session::ExternalToolScope,
    pub completion: brain_protocol::session::ExternalToolCompletion,
    pub effect: brain_protocol::session::ExternalToolEffect,
    pub max_input_bytes: usize,
}

/// Sampling parameters. Digested: they are part of what makes a request
/// reproducible, though not part of the cache prefix proper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTokenParameter {
    MaxTokens,
    MaxCompletionTokens,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenOpts {
    pub max_tokens: u32,
    pub output_token_parameter: OutputTokenParameter,
    pub temperature: Option<f32>,
    pub reasoning_effort: Option<String>,
    pub stop_sequences: Vec<String>,
}

impl Default for GenOpts {
    fn default() -> Self {
        GenOpts {
            max_tokens: 4096,
            output_token_parameter: OutputTokenParameter::MaxCompletionTokens,
            temperature: None,
            reasoning_effort: None,
            stop_sequences: Vec::new(),
        }
    }
}

/// Caps that bound a session's blast radius. Not model-visible, not digested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum model rounds in one turn before the loop is declared runaway.
    pub max_rounds: u32,
    /// Maximum tool calls dispatched concurrently from one assistant message.
    /// p90 batch size is 4; this is not sized for 64.
    pub max_parallel_tools: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            // A high runaway ceiling, not working policy: the field (pi, codex) ships no cap
            // at all and opencode defaults to infinity. Real work must never be truncated by a
            // default; the graceful closing round handles the pathological case.
            max_rounds: 512,
            max_parallel_tools: 8,
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
    pub model: String,
    pub dialect: Dialect,
    pub sampling: GenOpts,
    pub limits: Limits,
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
            model: model.into(),
            dialect,
            sampling: GenOpts::default(),
            limits: Limits::default(),
        }
    }
    pub fn tool(mut self, t: ToolDecl) -> Self {
        self.tools.push(t);
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
        if self.limits.max_rounds < 8 {
            // The cap is kernel runaway authorization, not a work budget; a tight cap usually
            // truncates legitimate multi-tool turns. Sealed as requested regardless.
            tracing::warn!(
                max_rounds = self.limits.max_rounds,
                "sealing a session with a round cap below 8; real turns are likely to hit it"
            );
        }
        let digest = prefix_digest(&self);
        Arc::new(SealedPrefix {
            digest,
            system_prompt: self.system_prompt,
            tools: self.tools,
            model: self.model,
            dialect: self.dialect,
            sampling: self.sampling,
            limits: self.limits,
            rendered_base: None,
            prompt_cache_key: None,
            tool_choice_none: false,
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
    pub model: String,
    pub dialect: Dialect,
    pub sampling: GenOpts,
    pub limits: Limits,
    rendered_base: Option<serde_json::Value>,
    prompt_cache_key: Option<String>,
    /// Per-call request shaping, never part of the sealed identity: render `tool_choice: none`
    /// so the model must answer in text. Set only on derived views for the closing round.
    pub tool_choice_none: bool,
}

impl SealedPrefix {
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn tool(&self, name: &str) -> Option<&ToolDecl> {
        self.tools.iter().find(|t| t.name == name)
    }
    /// A per-call view for the graceful at-cap closing round: the same sealed presentation with
    /// `tool_choice: none`, so the model wraps up in text instead of requesting more work.
    pub(crate) fn closing_view(&self) -> SealedPrefix {
        SealedPrefix {
            digest: self.digest.clone(),
            system_prompt: self.system_prompt.clone(),
            tools: self.tools.clone(),
            model: self.model.clone(),
            dialect: self.dialect,
            sampling: self.sampling.clone(),
            limits: self.limits,
            rendered_base: None,
            prompt_cache_key: None,
            tool_choice_none: true,
        }
    }
    /// A per-call view of this sealed prefix for a loop-composed `model_stream` request.
    ///
    /// Presentation (system text, which sealed tools are shown and how, output/temperature
    /// sampling) is loop policy; authority is not — the provider, model, dialect and every
    /// executable tool binding stay exactly as sealed.
    ///
    /// The frozen provider base segment and prompt-cache key survive whenever the call's
    /// presentation is byte-identical to the seal — the common case for every official loop,
    /// which re-presents the sealed system/tools verbatim. Sampling scalars ride outside the
    /// cached segment, so overriding them never invalidates it. Only a genuinely changed
    /// presentation re-renders (and pays its own cache economics — that is the loop's call).
    pub(crate) fn loop_call_view(
        &self,
        system_prompt: Option<String>,
        tools: Option<Vec<ToolDecl>>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> SealedPrefix {
        let mut sampling = self.sampling.clone();
        if let Some(max_tokens) = max_tokens {
            sampling.max_tokens = max_tokens;
        }
        if let Some(temperature) = temperature {
            sampling.temperature = Some(temperature);
        }
        let system_prompt = system_prompt.unwrap_or_else(|| self.system_prompt.clone());
        let tools = tools.unwrap_or_else(|| self.tools.clone());
        let sealed_presentation = system_prompt == self.system_prompt && tools == self.tools;
        SealedPrefix {
            digest: self.digest.clone(),
            system_prompt,
            tools,
            model: self.model.clone(),
            dialect: self.dialect,
            sampling,
            limits: self.limits,
            rendered_base: if sealed_presentation {
                self.rendered_base.clone()
            } else {
                None
            },
            prompt_cache_key: if sealed_presentation {
                self.prompt_cache_key.clone()
            } else {
                None
            },
            tool_choice_none: false,
        }
    }
    pub fn rendered_base(&self) -> Option<&serde_json::Value> {
        self.rendered_base.as_ref()
    }
    pub fn prompt_cache_key(&self) -> Option<&str> {
        self.prompt_cache_key.as_deref()
    }
    pub(crate) fn with_provider_base(
        mut self: Arc<Self>,
        rendered_base: Option<serde_json::Value>,
        prompt_cache_key: Option<String>,
    ) -> Arc<Self> {
        let prefix = Arc::get_mut(&mut self).expect("newly sealed prefix is uniquely owned");
        prefix.rendered_base = rendered_base;
        prefix.prompt_cache_key = prompt_cache_key;
        self
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
        max_tokens: u32,
        output_token_parameter: OutputTokenParameter,
        temperature: Option<f32>,
        reasoning_effort: &'a Option<String>,
        stop_sequences: &'a [String],
    }
    #[derive(Serialize)]
    struct CanonTool<'a> {
        name: &'a str,
        description: &'a str,
        input_schema: &'a serde_json::Value,
        output_schema: &'a serde_json::Value,
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
        max_tokens: def.sampling.max_tokens,
        output_token_parameter: def.sampling.output_token_parameter,
        temperature: def.sampling.temperature,
        reasoning_effort: &def.sampling.reasoning_effort,
        stop_sequences: &def.sampling.stop_sequences,
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
            contract_digest: "a".repeat(64),
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
            contract_digest: "b".repeat(64),
            input_schema: schema(),
            output_schema: serde_json::json!({"type":"object"}),
            route: ToolRoute::Intrinsic("brain.test.write".into()),
        });
        assert_ne!(
            prefix_digest(&more),
            base,
            "one appended tool definition must fork the prefix"
        );
    }

    #[test]
    fn tool_order_is_significant() {
        let mut swapped = def().tool(ToolDecl {
            name: "write".into(),
            description: "write a file".into(),
            contract_digest: "b".repeat(64),
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
        d.tools[0].route = ToolRoute::Intrinsic("brain.test.other-route".into());
        d.limits.max_parallel_tools = 999;
        assert_eq!(
            prefix_digest(&d),
            base,
            "our routing is not model-visible prefix"
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
    }

    #[test]
    fn loop_view_with_sealed_presentation_keeps_the_frozen_base_and_cache_key() {
        // The D3 gate: a loop that re-presents the sealed system/tools verbatim (every
        // official loop) must reuse the byte-frozen provider base and cache key — dropping
        // them silently forfeits prompt caching on every ctx-composed round.
        let sealed = def().seal().with_provider_base(
            Some(serde_json::json!({"frozen": "base"})),
            Some("aex:ses_test".into()),
        );
        let echoed = sealed.loop_call_view(None, Some(sealed.tools.clone()), Some(512), Some(0.25));
        assert_eq!(
            echoed.rendered_base(),
            Some(&serde_json::json!({"frozen": "base"})),
            "sampling overrides ride outside the cached segment and must not invalidate it"
        );
        assert_eq!(echoed.prompt_cache_key(), Some("aex:ses_test"));
        assert_eq!(echoed.sampling.max_tokens, 512);
    }

    #[test]
    fn loop_view_with_changed_presentation_drops_the_frozen_base() {
        let sealed = def().seal().with_provider_base(
            Some(serde_json::json!({"frozen": "base"})),
            Some("aex:ses_test".into()),
        );
        let new_system = sealed.loop_call_view(Some("different".into()), None, None, None);
        assert!(new_system.rendered_base().is_none());
        assert!(new_system.prompt_cache_key().is_none());
        let mut tools = sealed.tools.clone();
        tools[0].description = "changed".into();
        let new_tools = sealed.loop_call_view(None, Some(tools), None, None);
        assert!(new_tools.rendered_base().is_none());
        assert!(new_tools.prompt_cache_key().is_none());
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
