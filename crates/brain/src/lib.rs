//! Brain: one long-lived, product-neutral process that owns every session's decisions.
//!
//! The core loop is deliberately small: build a provider request from (sealed
//! prefix, history), stream it, parse tool calls, dispatch them through typed
//! execution ports, append results, repeat. Every decision is made durable in
//! one journal decision before it takes effect anywhere else, and an idle
//! session is nothing but a cached fold of its journal (hydrate-act-commit-
//! discard).
//!
//! This repository owns the session and Brain-to-Hand wire formats exposed by `brain-protocol`;
//! downstream Hands and products consume immutable Brain identities.

pub mod adapter;
pub mod agentloop;
pub mod api;
pub mod compact;
pub mod config;
pub mod customer;
pub mod events;
pub mod external;

pub mod hand;
pub mod journal;
pub mod keys;
pub(crate) mod loopctx;
pub mod message;
pub mod outbound;
pub mod provider;
pub mod reclaim;
pub mod session;
pub mod storage;
pub mod tools;
pub mod turn;

pub use config::{ProviderKey, SealedPrefix, SessionConfig, ToolDecl};
pub use journal::{
    MAX_DECISION_ACTIONS, MAX_DECISION_SERIALIZED_BYTES, MAX_MESSAGE_REQUEST_BYTES,
    MAX_SERIALIZED_RECORD_BYTES,
};
pub use message::{ContentBlock, Message, Role, StopReason, Usage};
pub use session::{
    DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS, DEFAULT_PROVIDER_HEADER_TIMEOUT_MS,
    DEFAULT_PROVIDER_IDLE_TIMEOUT_MS, DEFAULT_PROVIDER_TOTAL_TIMEOUT_MS, EXTERNAL_TOOL_TIMEOUT_ENV,
    MAX_EXTERNAL_TOOL_TIMEOUT_MS, MAX_PROVIDER_HEADER_TIMEOUT_MS, MAX_PROVIDER_IDLE_TIMEOUT_MS,
    MAX_PROVIDER_TOTAL_TIMEOUT_MS, MIN_EXTERNAL_TOOL_TIMEOUT_MS, MIN_PROVIDER_HEADER_TIMEOUT_MS,
    MIN_PROVIDER_IDLE_TIMEOUT_MS, MIN_PROVIDER_TOTAL_TIMEOUT_MS, PROVIDER_HEADER_TIMEOUT_ENV,
    PROVIDER_IDLE_TIMEOUT_ENV, PROVIDER_TOTAL_TIMEOUT_ENV,
};

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

    #[error("Idempotency-Key was already used with a different request")]
    IdempotencyConflict,

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

    #[error("session storage object exceeds the sealed limit of {limit} bytes")]
    StorageObjectTooLarge { limit: u64 },

    #[error("the default sandbox has never been materialized")]
    SandboxNotMaterialized,

    #[error("the requested sandbox generation is gone")]
    SandboxGone,

    #[error("sandbox generation does not match the current target")]
    SandboxGenerationConflict,

    #[error("sandbox capacity is exhausted")]
    SandboxResourceExhausted,

    #[error(
        "session storage quota exceeded (published {published} + reserved {reserved} + requested {requested} > {limit})"
    )]
    StorageQuotaExceeded {
        published: u64,
        reserved: u64,
        requested: u64,
        limit: u64,
    },

    #[error(
        "tenant storage quota exceeded (requested {requested} additional bytes; limit {limit})"
    )]
    TenantStorageQuotaExceeded { requested: u64, limit: u64 },

    #[error(
        "session journal retention quota exceeded (requested {requested} additional bytes; limit {limit})"
    )]
    SessionJournalQuotaExceeded { requested: u64, limit: u64 },

    #[error(
        "tenant journal retention quota exceeded (requested {requested} additional bytes; limit {limit})"
    )]
    TenantJournalQuotaExceeded { requested: u64, limit: u64 },

    #[error("tenant retained-session identity quota exceeded (limit {limit})")]
    TenantRetainedSessionQuotaExceeded { limit: u64 },

    #[error("session already has upload {transfer_id} in progress")]
    StorageUploadInProgress { transfer_id: String },

    #[error("storage upload {0} expired")]
    StorageUploadExpired(String),

    #[error("sandbox transfer {0} is unknown; prepare a fresh transfer")]
    SandboxTransferUnknown(String),

    #[error("sandbox transfer {0} expired; inspect the file and prepare a fresh transfer")]
    SandboxTransferExpired(String),

    #[error("sandbox transfer outcome is ambiguous; inspect the file before retrying")]
    SandboxTransferAmbiguous,

    #[error("the brain is at capacity; retry with backoff")]
    Overloaded,

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

    /// The session's agentloop (or the loop host running it) failed; the kernel and provider
    /// are healthy. Distinct from [`BrainError::Protocol`] so turn failures name the right party.
    #[error("agentloop: {0}")]
    Agentloop(String),

    #[error("provider error {status}: {body}")]
    ProviderStatus { status: u16, body: String },

    #[error("hand unavailable: {0}")]
    HandUnavailable(String),

    #[error("hand operation error: {0}")]
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
/// (`ses_`, `trn_`) require 20..=32 alphanumerics after the prefix; internal operation ids are
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
