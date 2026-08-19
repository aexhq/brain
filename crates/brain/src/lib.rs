//! The aex brain: one long-lived process that owns every session's decisions.
//!
//! The core loop is deliberately small: build a provider request from (sealed
//! prefix, history), stream it, parse tool calls, dispatch them over the
//! brain->hand ABI, append results, repeat. Every decision is made durable in
//! one DynamoDB write before it takes effect anywhere else (D9), and an idle
//! session is nothing but a cached fold of its journal (hydrate-act-commit-
//! discard).
//!
//! Design authority: `aex-research/docs/ARCHITECTURE-v1.md`. Wire formats are
//! owned by `aexhq/aex` (`aex-contracts`, session API + ABI v1) and consumed by
//! tag -- never re-described here.

pub mod adapter;
pub mod api;
pub mod compact;
pub mod config;
pub mod events;

pub mod journal;
pub mod keys;
pub mod local;
pub mod mcp;
pub mod message;
pub mod outbound;
pub mod provider;
pub mod reclaim;
pub mod session;
pub mod subagent;
pub mod tools;
pub mod turn;
pub mod web;

pub use config::{ProviderKey, SealedPrefix, SessionConfig, ToolDecl};
pub use message::{ContentBlock, Message, Role, StopReason, Usage};

use std::sync::Arc;

/// Typed failures. Every variant carries enough evidence to attribute the
/// failure to a layer; nothing is a bare string.
#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("prefix is sealed for the life of the session (digest {digest}); {what} cannot change")]
    PrefixSealed { digest: String, what: &'static str },

    #[error("session {0} not found")]
    NoSuchSession(String),

    #[error("session {0} is already running a turn")]
    TurnInFlight(String),

    #[error("session {0} is deleted")]
    SessionDeleted(String),

    #[error("session failed: {0}")]
    SessionFailed(String),

    #[error("invalid request: {0}")]
    Invalid(String),

    #[error("workspace path not found: {0}")]
    FileNotFound(String),

    #[error("file payload exceeds the configured limit of {limit} bytes")]
    FileTooLarge { limit: usize },

    #[error("the brain is at capacity; retry with backoff")]
    Overloaded,

    #[error("subagent depth cap {cap} exceeded (requested depth {requested})")]
    DepthCap { cap: u32, requested: u32 },

    #[error("subagent fan-out cap {cap} exceeded ({requested} requested)")]
    FanoutCap { cap: usize, requested: usize },

    #[error("tool {name} is not declared in this session's sealed prefix")]
    UndeclaredTool { name: String },

    #[error("tool {name} failed: {source}")]
    Tool {
        name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("provider transport: {0}")]
    Transport(String),

    #[error("provider returned an unparseable stream: {0}")]
    Protocol(String),

    #[error("provider error {status}: {body}")]
    ProviderStatus { status: u16, body: String },

    #[error("hand unavailable: {0}")]
    HandUnavailable(String),

    #[error("hand ABI error: {0}")]
    Hand(String),

    #[error("journal: {0}")]
    Journal(String),

    /// Another owner holds (or took) the session lease. The local fold is stale
    /// and must be discarded, never written from.
    #[error("fenced: journal lease lost to a newer owner")]
    Fenced,

    #[error("key custody: {0}")]
    Custody(String),

    #[error("turn exceeded the {cap}-round tool loop cap")]
    RoundCap { cap: u32 },

    #[error("cancelled")]
    Cancelled,

    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, BrainError>;

/// Shared, immutable, reference-counted. N sessions on one sealed prefix hold
/// ONE copy of the system prompt and tool schemas, not N.
pub type Shared<T> = Arc<T>;

/// Mints an id: `prefix_` + `n` random alphanumeric characters. Session API ids
/// (`ses_`, `trn_`) require 20..=32 alphanumerics after the prefix; ABI ids are
/// free-form up to 64 bytes.
pub fn mint_id(prefix: &str, n: usize) -> String {
    use rand::Rng;
    use rand::distr::Alphanumeric;
    let mut rng = rand::rng();
    let tail: String = (0..n).map(|_| rng.sample(Alphanumeric) as char).collect();
    format!("{prefix}_{tail}")
}

/// Wall-clock milliseconds since the epoch. The journal orders by `seq`, never
/// by this; it exists for humans and events.
pub fn wall_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
