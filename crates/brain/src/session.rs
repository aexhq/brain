//! Sessions as spawned tasks: one actor per resident session, hydrate-act-commit-discard.
//!
//! An idle session is nothing but its journal. The actor holds the cached fold (history,
//! head and lease); after `idle_discard` without traffic it releases the lease and exits -- the
//! next message hydrates from the journal (measured at roughly
//! 4 ms). Everything the actor holds is rebuildable; everything
//! durable went through `Journal::commit` first.
//!
//! The brain is COMPOSED, not configured into a cloud: [`Brain::with_parts`] takes a journal
//! store, key custody, typed Hand services and (optionally) a provider factory -- all trait
//! objects (see [`crate::adapter`]). [`Brain::local`] is the explicitly unsafe development
//! composition; durable standalone and cloud implementations live behind the same public ports.

use crate::adapter::{CallOutcome, DisabledToolExecutor, TerminalOutcome, ToolExecutor};
use crate::compact::{
    DEFAULT_CONTEXT_HARD_TOKENS, DEFAULT_CONTEXT_SOFT_TOKENS, DEFAULT_CONTEXT_TAIL_TOKENS,
};
use crate::config::{AgentDef, Dialect, GenOpts, OutputTokenParameter, ProviderKey, SessionConfig};
use crate::events::EventHub;
use crate::journal::{
    ContextForkDoc, DELETION_TOMBSTONE_TTL_MS, DeletionStatusDoc, Entry, FailureDoc, Head, HeadDoc,
    Journal, Lease, PrefixDoc, Record, StorageDeleteReservationDoc, StorageUploadReservationDoc,
};
use crate::keys::{KeyCustody, blob_from_b64, blob_to_b64, validate_custody_plaintext};
use crate::message::{ContentBlock, Message, Role};
use crate::provider::Provider;
use crate::turn::{TurnRun, TurnState};
use crate::{BrainError, Result};
use base64::Engine;
use brain_protocol::session::{
    self, CreateSessionRequest, MessageRequestContent, Provider as ApiProvider,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Notify, OnceCell, Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub const PROVIDER_HEADER_TIMEOUT_ENV: &str = "BRAIN_PROVIDER_HEADER_TIMEOUT_MS";
pub const PROVIDER_IDLE_TIMEOUT_ENV: &str = "BRAIN_PROVIDER_IDLE_TIMEOUT_MS";
pub const PROVIDER_TOTAL_TIMEOUT_ENV: &str = "BRAIN_PROVIDER_TOTAL_TIMEOUT_MS";
pub const EXTERNAL_TOOL_TIMEOUT_ENV: &str = "BRAIN_EXTERNAL_TOOL_TIMEOUT_MS";
pub const MAX_MODEL_ROUNDS_ENV: &str = "BRAIN_MAX_MODEL_ROUNDS";
pub const DEFAULT_MAX_ROUNDS_PER_TURN_ENV: &str = "BRAIN_DEFAULT_MAX_ROUNDS_PER_TURN";
pub const DRAIN_TIMEOUT_ENV: &str = "BRAIN_DRAIN_TIMEOUT_MS";
pub const DEFAULT_DRAIN_TIMEOUT_MS: u64 = 110_000;
pub const MAX_TURNS_ENV: &str = "BRAIN_MAX_TURNS";
pub const MAX_CONCURRENT_CREATES_ENV: &str = "BRAIN_MAX_CONCURRENT_CREATES";
pub const MAX_EVENT_FOLLOWERS_ENV: &str = "BRAIN_MAX_EVENT_FOLLOWERS";
pub const MAX_RESIDENT_SESSIONS_ENV: &str = "BRAIN_MAX_RESIDENT_SESSIONS";
pub const IDLE_DISCARD_SECONDS_ENV: &str = "BRAIN_IDLE_DISCARD_SECONDS";
pub const OUTBOUND_ALLOW_PRIVATE_ENV: &str = "BRAIN_OUTBOUND_ALLOW_PRIVATE";
pub const STORAGE_MAX_OBJECT_BYTES_ENV: &str = "BRAIN_STORAGE_MAX_OBJECT_BYTES";
pub const STORAGE_MAX_SESSION_BYTES_ENV: &str = "BRAIN_STORAGE_MAX_SESSION_BYTES";
pub const STORAGE_MAX_TENANT_BYTES_ENV: &str = "BRAIN_STORAGE_MAX_TENANT_BYTES";
pub const STORAGE_TRANSFER_TTL_ENV: &str = "BRAIN_STORAGE_TRANSFER_TTL_MS";
pub const MAX_ADDITIONAL_SANDBOXES_ENV: &str = "BRAIN_MAX_ADDITIONAL_SANDBOXES_PER_ROOT";
pub const EXTERNAL_EXECUTOR_URL_ENV: &str = "BRAIN_EXTERNAL_TOOL_EXECUTOR_URL";
pub const EXTERNAL_EXECUTOR_TOKEN_ENV: &str = "BRAIN_EXTERNAL_TOOL_EXECUTOR_TOKEN";
pub const EXTERNAL_EXECUTOR_CAPABILITIES_ENV: &str = "BRAIN_EXTERNAL_TOOL_CAPABILITIES";
pub const RECOVERY_POLL_ENV: &str = "BRAIN_RECOVERY_POLL_MS";
pub const RECOVERY_SHARDS_PER_POLL_ENV: &str = "BRAIN_RECOVERY_SHARDS_PER_POLL";
pub const RECOVERY_PAGE_SIZE_ENV: &str = "BRAIN_RECOVERY_PAGE_SIZE";
pub const MAX_CONCURRENT_RECOVERIES_ENV: &str = "BRAIN_MAX_CONCURRENT_RECOVERIES";

pub const DEFAULT_PROVIDER_HEADER_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_PROVIDER_IDLE_TIMEOUT_MS: u64 = 60_000;
pub const DEFAULT_PROVIDER_TOTAL_TIMEOUT_MS: u64 = 15 * 60_000;
pub const DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS: usize = 64;
pub const DEFAULT_MAX_CONCURRENT_TURNS: usize = 64;
pub const DEFAULT_MAX_CONCURRENT_CREATES: usize = 4;
pub const DEFAULT_MAX_EVENT_FOLLOWERS: usize = 64;
pub const DEFAULT_MAX_RESIDENT_SESSIONS: usize = 128;
pub const DEFAULT_IDLE_DISCARD_SECONDS: u64 = 900;
pub const DEFAULT_STORAGE_MAX_TENANT_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_ADDITIONAL_SANDBOXES: usize = 2;
pub const DEFAULT_RECOVERY_POLL_MS: u64 = 1_000;
pub const DEFAULT_RECOVERY_SHARDS_PER_POLL: usize = 4;
pub const DEFAULT_RECOVERY_PAGE_SIZE: usize = 32;
pub const DEFAULT_MAX_CONCURRENT_RECOVERIES: usize = 16;

pub const MIN_PROVIDER_HEADER_TIMEOUT_MS: u64 = 1_000;
pub const MAX_PROVIDER_HEADER_TIMEOUT_MS: u64 = 5 * 60_000;
pub const MIN_PROVIDER_IDLE_TIMEOUT_MS: u64 = 1_000;
pub const MAX_PROVIDER_IDLE_TIMEOUT_MS: u64 = 5 * 60_000;
pub const MIN_PROVIDER_TOTAL_TIMEOUT_MS: u64 = 1_000;
pub const MAX_PROVIDER_TOTAL_TIMEOUT_MS: u64 = 60 * 60_000;
pub const MIN_EXTERNAL_TOOL_TIMEOUT_MS: u64 = 1_000;
pub const MAX_EXTERNAL_TOOL_TIMEOUT_MS: u64 = 5 * 60_000;
pub const MAX_CONCURRENT_MODEL_ROUNDS: usize = 1_024;
pub const MAX_CONCURRENT_TURNS: usize = 1_024;
pub const MAX_CONCURRENT_CREATES: usize = 64;
pub const MAX_EVENT_FOLLOWERS: usize = 1_024;
pub const MAX_RESIDENT_SESSIONS: usize = 4_096;
pub const MAX_IDLE_DISCARD_SECONDS: u64 = 24 * 60 * 60;
pub const MAX_STORAGE_POLICY_BYTES: u64 = i64::MAX as u64;
pub const MIN_STORAGE_TRANSFER_TTL_MS: u64 = 60_000;
pub const MAX_STORAGE_TRANSFER_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub const MAX_ADDITIONAL_SANDBOXES: usize = 64;
pub const MIN_RECOVERY_POLL_MS: u64 = 1;
pub const MAX_RECOVERY_POLL_MS: u64 = 60_000;
pub const MAX_RECOVERY_PAGE_SIZE: usize = 100;
pub const MAX_CONCURRENT_RECOVERIES: usize = 1_024;
pub const MAX_EXTERNAL_EXECUTOR_URL_BYTES: usize = 2_048;
pub const MAX_EXTERNAL_EXECUTOR_TOKEN_BYTES: usize = 8 * 1_024;
pub const MAX_EXTERNAL_EXECUTOR_CAPABILITIES: usize = 128;
pub const MAX_EXTERNAL_EXECUTOR_CAPABILITY_BYTES: usize = 128;

/// Process configuration: the knobs that are NOT adapters.
#[derive(Debug, Clone)]
pub struct BrainConfig {
    /// Admission: concurrent model rounds across the process.
    pub max_concurrent_model_rounds: usize,
    /// Admission: concurrent active turns across the process.
    pub max_concurrent_turns: usize,
    /// Expensive create resolution/bundle validation admission. HTTP compositions should also
    /// acquire before buffering; this core guard protects direct/local callers.
    pub max_concurrent_creates: usize,
    /// Process-wide SSE follower admission. Each admitted distinct session may retain one small
    /// fixed live-event ring; slow consumers disconnect and replay durable records.
    pub max_event_followers: usize,
    /// Hard process-wide bound on resident actors/folds. Admission is non-blocking; a saturated
    /// process returns backpressure and durable sessions transparently hydrate after an idle
    /// resident releases its slot.
    pub max_resident_sessions: usize,
    /// Idle residency before the actor discards its fold and exits.
    pub idle_discard: Duration,
    pub context_soft_tokens: usize,
    pub context_hard_tokens: usize,
    pub context_tail_tokens: usize,
    /// Provider response header/first-byte, inter-event idle, and total-call budgets.
    pub provider_header_timeout: Duration,
    pub provider_idle_timeout: Duration,
    pub provider_total_timeout: Duration,
    /// Sealed per-turn model-round ceiling for new sessions. Kernel runaway authorization, not
    /// working policy: the graceful closing round wraps the turn up in text at the cap.
    pub default_max_rounds: u32,
    /// How long a draining process waits for admitted turns before exiting anyway. Size it
    /// under the orchestrator's stop timeout (Fargate caps stopTimeout at 120 s).
    pub drain_timeout: Duration,
    /// Whether user-controlled provider base URLs may reach loopback/private/link-local addresses.
    /// Production keeps this false; explicit local development may opt into private endpoints.
    pub outbound_allow_private: bool,
    /// Maximum bytes buffered by one public file upload or download.
    /// Sealed durable-storage limits. These are host policy, copied into each session's immutable
    /// configuration so a later deployment cannot silently widen an existing capability.
    pub storage_max_object_bytes: u64,
    pub storage_max_session_bytes: u64,
    /// Authoritative aggregate ceiling across published and reserved bytes for every session
    /// owned by one tenant. Journal adapters enforce this in the same atomic decision as HEAD.
    pub storage_max_tenant_bytes: u64,
    /// Append-only journal retention is independently bounded from model-context compaction.
    /// These limits include roots, ordinary descendants, ended sessions, and charged recovery
    /// headroom until explicit physical deletion releases the identity and bytes.
    pub journal_max_session_bytes: u64,
    pub journal_max_tenant_bytes: u64,
    pub journal_max_tenant_sessions: u64,
    pub storage_transfer_ttl: Duration,
    /// Host-sealed root-tree quota for simultaneously live additional sandboxes. The shared
    /// default target is excluded; terminated IDs remain tombstoned but release a slot.
    pub max_additional_sandboxes_per_root: u32,
    /// Optional host executor shared by every external tool declaration. The URL and service
    /// credential are process configuration and never enter the sealed model prefix.
    pub external_executor_url: Option<String>,
    pub external_executor_token: Option<ProviderKey>,
    /// Stable capabilities registered by the HTTP executor. An empty set advertises none.
    pub external_executor_capabilities: HashSet<String>,
    /// Trusted host registry for fixed official capability IDs. Public Tool JSON may reference a
    /// capability but cannot choose scope, completion, replay policy, or input ceilings.
    pub official_capabilities: HashMap<String, crate::config::ServerToolPolicy>,
    pub external_call_timeout: Duration,
    /// Background recovery discovery. The index is eventually consistent; correctness comes
    /// from the base-journal claim/fence before an actor is started.
    pub recovery_poll_interval: Duration,
    pub recovery_shards_per_poll: usize,
    pub recovery_page_size: usize,
    /// Independent cap for background effect recovery so a due-time wave cannot create an
    /// unbounded resident/adapter fan-out.
    pub max_concurrent_recoveries: usize,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            max_concurrent_model_rounds: DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS,
            max_concurrent_turns: DEFAULT_MAX_CONCURRENT_TURNS,
            max_concurrent_creates: DEFAULT_MAX_CONCURRENT_CREATES,
            max_event_followers: DEFAULT_MAX_EVENT_FOLLOWERS,
            max_resident_sessions: DEFAULT_MAX_RESIDENT_SESSIONS,
            idle_discard: Duration::from_secs(DEFAULT_IDLE_DISCARD_SECONDS),
            context_soft_tokens: DEFAULT_CONTEXT_SOFT_TOKENS,
            context_hard_tokens: DEFAULT_CONTEXT_HARD_TOKENS,
            context_tail_tokens: DEFAULT_CONTEXT_TAIL_TOKENS,
            provider_header_timeout: Duration::from_millis(DEFAULT_PROVIDER_HEADER_TIMEOUT_MS),
            provider_idle_timeout: Duration::from_millis(DEFAULT_PROVIDER_IDLE_TIMEOUT_MS),
            provider_total_timeout: Duration::from_millis(DEFAULT_PROVIDER_TOTAL_TIMEOUT_MS),
            default_max_rounds: crate::config::Limits::default().max_rounds,
            drain_timeout: Duration::from_millis(DEFAULT_DRAIN_TIMEOUT_MS),
            outbound_allow_private: false,
            storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
            storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
            storage_max_tenant_bytes: DEFAULT_STORAGE_MAX_TENANT_BYTES,
            journal_max_session_bytes: crate::journal::DEFAULT_MAX_SESSION_JOURNAL_BYTES,
            journal_max_tenant_bytes: crate::journal::DEFAULT_MAX_TENANT_JOURNAL_BYTES,
            journal_max_tenant_sessions: crate::journal::DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
            storage_transfer_ttl: Duration::from_millis(
                crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
            ),
            max_additional_sandboxes_per_root: DEFAULT_MAX_ADDITIONAL_SANDBOXES as u32,
            external_executor_url: None,
            external_executor_token: None,
            external_executor_capabilities: HashSet::new(),
            official_capabilities: HashMap::new(),
            external_call_timeout: Duration::from_millis(DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS),
            recovery_poll_interval: Duration::from_millis(DEFAULT_RECOVERY_POLL_MS),
            recovery_shards_per_poll: DEFAULT_RECOVERY_SHARDS_PER_POLL,
            recovery_page_size: DEFAULT_RECOVERY_PAGE_SIZE,
            max_concurrent_recoveries: DEFAULT_MAX_CONCURRENT_RECOVERIES,
        }
    }
}

impl BrainConfig {
    /// Load every Brain-owned process policy without silently replacing or clamping malformed
    /// operator input. `Default` is deliberately environment-independent for tests and explicit
    /// local composition; hosted processes must call this constructor.
    pub fn from_env() -> Result<Self> {
        Self::from_env_values(|name| {
            let Some(raw) = std::env::var_os(name) else {
                return Ok(None);
            };
            raw.into_string()
                .map(Some)
                .map_err(|_| BrainError::Invalid(format!("{name} must contain valid UTF-8 text")))
        })
    }

    #[allow(clippy::field_reassign_with_default)]
    fn from_env_values(mut read: impl FnMut(&str) -> Result<Option<String>>) -> Result<Self> {
        let mut cfg = Self::default();
        cfg.max_concurrent_model_rounds = parse_env_usize(
            MAX_MODEL_ROUNDS_ENV,
            read(MAX_MODEL_ROUNDS_ENV)?.as_deref(),
            DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS,
            1,
            MAX_CONCURRENT_MODEL_ROUNDS,
        )?;
        cfg.max_concurrent_turns = parse_env_usize(
            MAX_TURNS_ENV,
            read(MAX_TURNS_ENV)?.as_deref(),
            DEFAULT_MAX_CONCURRENT_TURNS,
            1,
            MAX_CONCURRENT_TURNS,
        )?;
        cfg.max_concurrent_creates = parse_env_usize(
            MAX_CONCURRENT_CREATES_ENV,
            read(MAX_CONCURRENT_CREATES_ENV)?.as_deref(),
            DEFAULT_MAX_CONCURRENT_CREATES,
            1,
            MAX_CONCURRENT_CREATES,
        )?;
        cfg.max_event_followers = parse_env_usize(
            MAX_EVENT_FOLLOWERS_ENV,
            read(MAX_EVENT_FOLLOWERS_ENV)?.as_deref(),
            DEFAULT_MAX_EVENT_FOLLOWERS,
            1,
            MAX_EVENT_FOLLOWERS,
        )?;
        cfg.max_resident_sessions = parse_env_usize(
            MAX_RESIDENT_SESSIONS_ENV,
            read(MAX_RESIDENT_SESSIONS_ENV)?.as_deref(),
            DEFAULT_MAX_RESIDENT_SESSIONS,
            1,
            MAX_RESIDENT_SESSIONS,
        )?;
        cfg.idle_discard = Duration::from_secs(parse_env_u64(
            IDLE_DISCARD_SECONDS_ENV,
            read(IDLE_DISCARD_SECONDS_ENV)?.as_deref(),
            DEFAULT_IDLE_DISCARD_SECONDS,
            1,
            MAX_IDLE_DISCARD_SECONDS,
        )?);
        cfg.provider_header_timeout = Duration::from_millis(parse_env_u64(
            PROVIDER_HEADER_TIMEOUT_ENV,
            read(PROVIDER_HEADER_TIMEOUT_ENV)?.as_deref(),
            DEFAULT_PROVIDER_HEADER_TIMEOUT_MS,
            MIN_PROVIDER_HEADER_TIMEOUT_MS,
            MAX_PROVIDER_HEADER_TIMEOUT_MS,
        )?);
        cfg.provider_idle_timeout = Duration::from_millis(parse_env_u64(
            PROVIDER_IDLE_TIMEOUT_ENV,
            read(PROVIDER_IDLE_TIMEOUT_ENV)?.as_deref(),
            DEFAULT_PROVIDER_IDLE_TIMEOUT_MS,
            MIN_PROVIDER_IDLE_TIMEOUT_MS,
            MAX_PROVIDER_IDLE_TIMEOUT_MS,
        )?);
        cfg.provider_total_timeout = Duration::from_millis(parse_env_u64(
            PROVIDER_TOTAL_TIMEOUT_ENV,
            read(PROVIDER_TOTAL_TIMEOUT_ENV)?.as_deref(),
            DEFAULT_PROVIDER_TOTAL_TIMEOUT_MS,
            MIN_PROVIDER_TOTAL_TIMEOUT_MS,
            MAX_PROVIDER_TOTAL_TIMEOUT_MS,
        )?);
        cfg.external_call_timeout = Duration::from_millis(parse_env_u64(
            EXTERNAL_TOOL_TIMEOUT_ENV,
            read(EXTERNAL_TOOL_TIMEOUT_ENV)?.as_deref(),
            DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS,
            MIN_EXTERNAL_TOOL_TIMEOUT_MS,
            MAX_EXTERNAL_TOOL_TIMEOUT_MS,
        )?);
        cfg.default_max_rounds = u32::try_from(parse_env_u64(
            DEFAULT_MAX_ROUNDS_PER_TURN_ENV,
            read(DEFAULT_MAX_ROUNDS_PER_TURN_ENV)?.as_deref(),
            crate::config::Limits::default().max_rounds as u64,
            1,
            100_000,
        )?)
        .expect("bounded above by 100000");
        cfg.drain_timeout = Duration::from_millis(parse_env_u64(
            DRAIN_TIMEOUT_ENV,
            read(DRAIN_TIMEOUT_ENV)?.as_deref(),
            DEFAULT_DRAIN_TIMEOUT_MS,
            1_000,
            600_000,
        )?);
        cfg.outbound_allow_private = parse_env_bool(
            OUTBOUND_ALLOW_PRIVATE_ENV,
            read(OUTBOUND_ALLOW_PRIVATE_ENV)?.as_deref(),
            false,
        )?;
        cfg.storage_max_object_bytes = parse_env_u64(
            STORAGE_MAX_OBJECT_BYTES_ENV,
            read(STORAGE_MAX_OBJECT_BYTES_ENV)?.as_deref(),
            crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
            1,
            MAX_STORAGE_POLICY_BYTES,
        )?;
        cfg.storage_max_session_bytes = parse_env_u64(
            STORAGE_MAX_SESSION_BYTES_ENV,
            read(STORAGE_MAX_SESSION_BYTES_ENV)?.as_deref(),
            crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
            1,
            MAX_STORAGE_POLICY_BYTES,
        )?;
        cfg.storage_max_tenant_bytes = parse_env_u64(
            STORAGE_MAX_TENANT_BYTES_ENV,
            read(STORAGE_MAX_TENANT_BYTES_ENV)?.as_deref(),
            DEFAULT_STORAGE_MAX_TENANT_BYTES,
            1,
            MAX_STORAGE_POLICY_BYTES,
        )?;
        cfg.journal_max_session_bytes = parse_env_u64(
            crate::journal::JOURNAL_MAX_SESSION_BYTES_ENV,
            read(crate::journal::JOURNAL_MAX_SESSION_BYTES_ENV)?.as_deref(),
            crate::journal::DEFAULT_MAX_SESSION_JOURNAL_BYTES,
            crate::journal::MIN_SESSION_JOURNAL_BYTES,
            crate::journal::MAX_JOURNAL_BYTES,
        )?;
        cfg.journal_max_tenant_bytes = parse_env_u64(
            crate::journal::JOURNAL_MAX_TENANT_BYTES_ENV,
            read(crate::journal::JOURNAL_MAX_TENANT_BYTES_ENV)?.as_deref(),
            crate::journal::DEFAULT_MAX_TENANT_JOURNAL_BYTES,
            crate::journal::MIN_SESSION_JOURNAL_BYTES,
            crate::journal::MAX_JOURNAL_BYTES,
        )?;
        cfg.journal_max_tenant_sessions = parse_env_u64(
            crate::journal::JOURNAL_MAX_TENANT_SESSIONS_ENV,
            read(crate::journal::JOURNAL_MAX_TENANT_SESSIONS_ENV)?.as_deref(),
            crate::journal::DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
            crate::journal::MIN_TENANT_RETAINED_SESSIONS,
            crate::journal::MAX_TENANT_RETAINED_SESSIONS,
        )?;
        cfg.storage_transfer_ttl = Duration::from_millis(parse_env_u64(
            STORAGE_TRANSFER_TTL_ENV,
            read(STORAGE_TRANSFER_TTL_ENV)?.as_deref(),
            crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
            MIN_STORAGE_TRANSFER_TTL_MS,
            MAX_STORAGE_TRANSFER_TTL_MS,
        )?);
        cfg.max_additional_sandboxes_per_root = u32::try_from(parse_env_usize(
            MAX_ADDITIONAL_SANDBOXES_ENV,
            read(MAX_ADDITIONAL_SANDBOXES_ENV)?.as_deref(),
            DEFAULT_MAX_ADDITIONAL_SANDBOXES,
            1,
            MAX_ADDITIONAL_SANDBOXES,
        )?)
        .expect("the validated additional-sandbox maximum fits u32");
        cfg.external_executor_url = parse_optional_env_string(
            EXTERNAL_EXECUTOR_URL_ENV,
            read(EXTERNAL_EXECUTOR_URL_ENV)?,
            MAX_EXTERNAL_EXECUTOR_URL_BYTES,
        )?;
        cfg.external_executor_token = parse_optional_env_string(
            EXTERNAL_EXECUTOR_TOKEN_ENV,
            read(EXTERNAL_EXECUTOR_TOKEN_ENV)?,
            MAX_EXTERNAL_EXECUTOR_TOKEN_BYTES,
        )?
        .map(ProviderKey::new);
        cfg.external_executor_capabilities =
            parse_capabilities(read(EXTERNAL_EXECUTOR_CAPABILITIES_ENV)?)?;
        cfg.recovery_poll_interval = Duration::from_millis(parse_env_u64(
            RECOVERY_POLL_ENV,
            read(RECOVERY_POLL_ENV)?.as_deref(),
            DEFAULT_RECOVERY_POLL_MS,
            MIN_RECOVERY_POLL_MS,
            MAX_RECOVERY_POLL_MS,
        )?);
        cfg.recovery_shards_per_poll = parse_env_usize(
            RECOVERY_SHARDS_PER_POLL_ENV,
            read(RECOVERY_SHARDS_PER_POLL_ENV)?.as_deref(),
            DEFAULT_RECOVERY_SHARDS_PER_POLL,
            1,
            crate::journal::RECOVERY_SHARDS,
        )?;
        cfg.recovery_page_size = parse_env_usize(
            RECOVERY_PAGE_SIZE_ENV,
            read(RECOVERY_PAGE_SIZE_ENV)?.as_deref(),
            DEFAULT_RECOVERY_PAGE_SIZE,
            1,
            MAX_RECOVERY_PAGE_SIZE,
        )?;
        cfg.max_concurrent_recoveries = parse_env_usize(
            MAX_CONCURRENT_RECOVERIES_ENV,
            read(MAX_CONCURRENT_RECOVERIES_ENV)?.as_deref(),
            DEFAULT_MAX_CONCURRENT_RECOVERIES,
            1,
            MAX_CONCURRENT_RECOVERIES,
        )?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        validate_usize_range(
            MAX_MODEL_ROUNDS_ENV,
            self.max_concurrent_model_rounds,
            1,
            MAX_CONCURRENT_MODEL_ROUNDS,
        )?;
        validate_usize_range(
            MAX_TURNS_ENV,
            self.max_concurrent_turns,
            1,
            MAX_CONCURRENT_TURNS,
        )?;
        validate_usize_range(
            MAX_CONCURRENT_CREATES_ENV,
            self.max_concurrent_creates,
            1,
            MAX_CONCURRENT_CREATES,
        )?;
        validate_usize_range(
            MAX_EVENT_FOLLOWERS_ENV,
            self.max_event_followers,
            1,
            MAX_EVENT_FOLLOWERS,
        )?;
        validate_usize_range(
            MAX_RESIDENT_SESSIONS_ENV,
            self.max_resident_sessions,
            1,
            MAX_RESIDENT_SESSIONS,
        )?;
        validate_timeout(
            IDLE_DISCARD_SECONDS_ENV,
            self.idle_discard,
            1,
            MAX_IDLE_DISCARD_SECONDS.saturating_mul(1_000),
        )?;
        if self.context_tail_tokens == 0
            || self.context_tail_tokens >= self.context_soft_tokens
            || self.context_soft_tokens >= self.context_hard_tokens
        {
            return Err(BrainError::Invalid(
                "context tail must be positive and ordered tail < soft < hard".into(),
            ));
        }
        crate::journal::JournalRetentionLimits {
            session_bytes: self.journal_max_session_bytes,
            tenant_bytes: self.journal_max_tenant_bytes,
            tenant_sessions: self.journal_max_tenant_sessions,
        }
        .validate()?;
        validate_timeout(
            PROVIDER_HEADER_TIMEOUT_ENV,
            self.provider_header_timeout,
            MIN_PROVIDER_HEADER_TIMEOUT_MS,
            MAX_PROVIDER_HEADER_TIMEOUT_MS,
        )?;
        validate_timeout(
            PROVIDER_IDLE_TIMEOUT_ENV,
            self.provider_idle_timeout,
            MIN_PROVIDER_IDLE_TIMEOUT_MS,
            MAX_PROVIDER_IDLE_TIMEOUT_MS,
        )?;
        validate_timeout(
            PROVIDER_TOTAL_TIMEOUT_ENV,
            self.provider_total_timeout,
            MIN_PROVIDER_TOTAL_TIMEOUT_MS,
            MAX_PROVIDER_TOTAL_TIMEOUT_MS,
        )?;
        validate_timeout(
            EXTERNAL_TOOL_TIMEOUT_ENV,
            self.external_call_timeout,
            MIN_EXTERNAL_TOOL_TIMEOUT_MS,
            MAX_EXTERNAL_TOOL_TIMEOUT_MS,
        )?;
        if self.provider_total_timeout < self.provider_header_timeout
            || self.provider_total_timeout < self.provider_idle_timeout
        {
            return Err(BrainError::Invalid(format!(
                "{PROVIDER_TOTAL_TIMEOUT_ENV} must be greater than or equal to both {PROVIDER_HEADER_TIMEOUT_ENV} and {PROVIDER_IDLE_TIMEOUT_ENV}"
            )));
        }
        if self.storage_max_object_bytes == 0
            || self.storage_max_object_bytes > MAX_STORAGE_POLICY_BYTES
            || self.storage_max_session_bytes > MAX_STORAGE_POLICY_BYTES
            || self.storage_max_tenant_bytes > MAX_STORAGE_POLICY_BYTES
            || self.storage_max_object_bytes > self.storage_max_session_bytes
            || self.storage_max_session_bytes > self.storage_max_tenant_bytes
        {
            return Err(BrainError::Invalid(format!(
                "storage byte policies must be positive, representable, and ordered {STORAGE_MAX_OBJECT_BYTES_ENV} <= {STORAGE_MAX_SESSION_BYTES_ENV} <= {STORAGE_MAX_TENANT_BYTES_ENV}"
            )));
        }
        validate_timeout(
            STORAGE_TRANSFER_TTL_ENV,
            self.storage_transfer_ttl,
            MIN_STORAGE_TRANSFER_TTL_MS,
            MAX_STORAGE_TRANSFER_TTL_MS,
        )?;
        validate_usize_range(
            MAX_ADDITIONAL_SANDBOXES_ENV,
            self.max_additional_sandboxes_per_root as usize,
            1,
            MAX_ADDITIONAL_SANDBOXES,
        )?;
        validate_timeout(
            RECOVERY_POLL_ENV,
            self.recovery_poll_interval,
            MIN_RECOVERY_POLL_MS,
            MAX_RECOVERY_POLL_MS,
        )?;
        validate_usize_range(
            RECOVERY_SHARDS_PER_POLL_ENV,
            self.recovery_shards_per_poll,
            1,
            crate::journal::RECOVERY_SHARDS,
        )?;
        validate_usize_range(
            RECOVERY_PAGE_SIZE_ENV,
            self.recovery_page_size,
            1,
            MAX_RECOVERY_PAGE_SIZE,
        )?;
        validate_usize_range(
            MAX_CONCURRENT_RECOVERIES_ENV,
            self.max_concurrent_recoveries,
            1,
            MAX_CONCURRENT_RECOVERIES,
        )?;
        validate_external_executor_config(self)?;
        Ok(())
    }
}

fn validate_external_executor_config(cfg: &BrainConfig) -> Result<()> {
    if cfg.external_executor_url.is_none()
        && (cfg.external_executor_token.is_some() || !cfg.external_executor_capabilities.is_empty())
    {
        return Err(BrainError::Invalid(format!(
            "{EXTERNAL_EXECUTOR_TOKEN_ENV} and {EXTERNAL_EXECUTOR_CAPABILITIES_ENV} require {EXTERNAL_EXECUTOR_URL_ENV}"
        )));
    }
    if cfg.external_executor_capabilities.len() > MAX_EXTERNAL_EXECUTOR_CAPABILITIES
        || cfg.external_executor_capabilities.iter().any(|capability| {
            capability.is_empty()
                || capability.len() > MAX_EXTERNAL_EXECUTOR_CAPABILITY_BYTES
                || !capability
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(BrainError::Invalid(format!(
            "{EXTERNAL_EXECUTOR_CAPABILITIES_ENV} exceeds its count, byte, or identifier bound"
        )));
    }
    if let Some(endpoint) = &cfg.external_executor_url {
        crate::external::HttpExternalToolExecutor::new(
            endpoint.clone(),
            cfg.external_executor_token
                .as_ref()
                .map(|token| token.expose().to_owned()),
            cfg.external_call_timeout,
            cfg.external_executor_capabilities.iter().cloned(),
        )?;
    }
    Ok(())
}

fn validate_timeout(name: &str, value: Duration, minimum_ms: u64, maximum_ms: u64) -> Result<()> {
    let millis = u64::try_from(value.as_millis()).map_err(|_| {
        BrainError::Invalid(format!(
            "{name} must be between {minimum_ms} and {maximum_ms} milliseconds"
        ))
    })?;
    if !(minimum_ms..=maximum_ms).contains(&millis) {
        return Err(BrainError::Invalid(format!(
            "{name} must be between {minimum_ms} and {maximum_ms} milliseconds"
        )));
    }
    Ok(())
}

#[cfg(test)]
fn parse_strict_env_u64(k: &str, raw: Option<&str>, default: u64) -> Result<u64> {
    parse_env_u64(k, raw, default, 0, u64::MAX)
}

fn parse_env_u64(
    name: &str,
    raw: Option<&str>,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let value = raw.parse::<u64>().map_err(|_| {
        BrainError::Invalid(format!("{name} must contain an unsigned decimal integer"))
    })?;
    if !(minimum..=maximum).contains(&value) {
        return Err(BrainError::Invalid(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn parse_env_usize(
    name: &str,
    raw: Option<&str>,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize> {
    let value = parse_env_u64(name, raw, default as u64, minimum as u64, maximum as u64)?;
    usize::try_from(value)
        .map_err(|_| BrainError::Invalid(format!("{name} must be between {minimum} and {maximum}")))
}

fn parse_env_bool(name: &str, raw: Option<&str>, default: bool) -> Result<bool> {
    match raw {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(BrainError::Invalid(format!(
            "{name} must be exactly true or false"
        ))),
    }
}

fn parse_optional_env_string(
    name: &str,
    raw: Option<String>,
    maximum_bytes: usize,
) -> Result<Option<String>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.is_empty() || raw.len() > maximum_bytes {
        return Err(BrainError::Invalid(format!(
            "{name} must contain between 1 and {maximum_bytes} UTF-8 bytes"
        )));
    }
    Ok(Some(raw))
}

fn parse_capabilities(raw: Option<String>) -> Result<HashSet<String>> {
    let Some(raw) = raw else {
        return Ok(HashSet::new());
    };
    if raw.is_empty() {
        return Err(BrainError::Invalid(format!(
            "{EXTERNAL_EXECUTOR_CAPABILITIES_ENV} must not be empty when set"
        )));
    }
    let mut capabilities = HashSet::new();
    for capability in raw.split(',').map(str::trim) {
        if capability.is_empty()
            || capability.len() > MAX_EXTERNAL_EXECUTOR_CAPABILITY_BYTES
            || !capability
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || !capabilities.insert(capability.to_owned())
        {
            return Err(BrainError::Invalid(format!(
                "{EXTERNAL_EXECUTOR_CAPABILITIES_ENV} contains an invalid or duplicate capability"
            )));
        }
    }
    if capabilities.len() > MAX_EXTERNAL_EXECUTOR_CAPABILITIES {
        return Err(BrainError::Invalid(format!(
            "{EXTERNAL_EXECUTOR_CAPABILITIES_ENV} contains more than {MAX_EXTERNAL_EXECUTOR_CAPABILITIES} capabilities"
        )));
    }
    Ok(capabilities)
}

fn validate_usize_range(name: &str, value: usize, minimum: usize, maximum: usize) -> Result<()> {
    if !(minimum..=maximum).contains(&value) {
        return Err(BrainError::Invalid(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(())
}

/// Process-wide reclaim policy. Measurements showed that allocators retain dropped session memory
/// unless reclamation is requested explicitly.
fn reclaim_policy() -> &'static crate::reclaim::ReclaimPolicy {
    static POLICY: std::sync::OnceLock<crate::reclaim::ReclaimPolicy> = std::sync::OnceLock::new();
    POLICY
        .get_or_init(|| crate::reclaim::ReclaimPolicy::new(crate::reclaim::DEFAULT_THRESHOLD_BYTES))
}

/// How turns obtain a provider. Overridable so tests can inject the scripted fake.
pub type ProviderFactory = Arc<dyn Fn(Dialect) -> Arc<dyn Provider> + Send + Sync>;

fn default_provider_factory() -> ProviderFactory {
    Arc::new(|d| Arc::from(crate::provider::for_dialect(d)))
}

/// The supervisor: the composed parts and the resident-session map.
pub struct Brain {
    pub cfg: BrainConfig,
    pub journal: Journal,
    pub custody: Arc<dyn KeyCustody>,
    pub hub: Arc<EventHub>,
    pub model_permits: Arc<Semaphore>,
    /// Guarded transport used for user-controlled provider base URLs.
    pub outbound: crate::outbound::Outbound,
    pub external_executor: Arc<dyn ToolExecutor>,
    /// Resolves each session's sealed selector to the loop implementation driving its turns.
    /// The default registry serves only the official `aex` policy.
    pub agentloop_registry: Arc<dyn crate::agentloop::AgentloopRegistry>,
    /// Durable per-session object storage. Hosted composition supplies this adapter; `None` means
    /// the storage resource is unavailable, never an in-memory production fallback.
    pub session_storage: Option<Arc<dyn crate::storage::SessionStoragePort>>,
    pub bundle_storage: Option<Arc<dyn crate::storage::BundleStoragePort>>,
    pub hand: Option<Arc<dyn crate::hand::HandPort>>,
    pub session_preparation: Option<Arc<dyn crate::hand::SessionPreparationPort>>,
    pub sandbox_files: Option<Arc<dyn crate::hand::SandboxFilesPort>>,
    pub sandbox_control: Option<Arc<dyn crate::hand::SandboxControlPort>>,
    /// Hosted customer-app delivery (for example API Gateway Management API). Absence means
    /// customer-app Tools are unavailable; Brain never silently routes them elsewhere.
    pub customer_delivery: Option<Arc<dyn crate::customer::CustomerHandDeliveryPort>>,
    /// Customer-app connection/receipt coordinator. Present only when the composition supplied
    /// absolute socket and observation callback URLs.
    pub customer: Option<Arc<crate::customer::CustomerCoordinator>>,
    pub compactor: Arc<dyn crate::compact::CompactionPort>,
    provider_factory: ProviderFactory,
    turn_permits: Arc<Semaphore>,
    create_permits: Arc<Semaphore>,
    sessions: Mutex<HashMap<String, mpsc::Sender<Command>>>,
    /// Deploy drain: once set, new creates/messages and recovery starts are refused while
    /// already-admitted turns run to completion. Never cleared — a draining process exits.
    draining: AtomicBool,
    recovery_started: AtomicBool,
    recovery_next_shard: AtomicUsize,
    recovery_cursors: Mutex<Vec<Option<String>>>,
    recovery_permits: Arc<Semaphore>,
    resident_permits: Arc<Semaphore>,
    resident_pressure: Arc<Notify>,
    /// Process-local decryption cache keyed by immutable root configuration. Entries are weak:
    /// secrets disappear as soon as the last resident descendant of that root is discarded.
    root_secret_cells: Mutex<HashMap<String, Weak<RootSecretCell>>>,
    /// Short-lived, one-redemption managed-secret capabilities. Values remain in custody; this
    /// table retains only exact scope, names and expiry and is pruned on every mint/redeem.
    managed_secret_grants: Mutex<HashMap<String, ManagedSecretGrant>>,
    /// Process-local happy-path sandbox transfer tickets. Durable storage owns the staged bytes
    /// and quota; this map retains only bounded routing metadata and exact completed responses.
    /// Losing it on restart intentionally yields `sandbox_transfer_unknown` rather than guessing
    /// whether an ambiguous external effect happened.
    direct_sandbox_transfers: Mutex<HashMap<String, DirectSandboxTransfer>>,
}

struct RootExecutionSecrets {
    key: ProviderKey,
    hand_env: HashMap<String, String>,
}

struct RootSecretCell {
    value: OnceCell<Arc<RootExecutionSecrets>>,
}

const MANAGED_SECRET_GRANT_TTL_MS: u64 = 15 * 60 * 1_000;
const MAX_MANAGED_SECRET_GRANTS: usize = 4_096;

pub const MAX_PENDING_SANDBOX_TRANSFERS: usize = 256;
pub const MAX_PENDING_SANDBOX_TRANSFERS_PER_SESSION: usize = 16;
pub const MAX_PENDING_SANDBOX_TRANSFER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_PENDING_SANDBOX_TRANSFER_BYTES_PER_SESSION: u64 = 1024 * 1024 * 1024;
const SANDBOX_TRANSFER_CLEANUP_SKEW_MS: u64 = 60_000;

struct ManagedSecretGrant {
    root_id: String,
    session_id: String,
    hand_id: String,
    binding_refs: HashSet<String>,
    env_names: Vec<String>,
    expires_at_ms: u64,
}

#[derive(Clone)]
struct DirectSandboxTransfer {
    session_id: String,
    storage_key: String,
    declared_bytes: u64,
    expires_at_ms: u64,
    cleanup_at_ms: u64,
    storage_transfer_id: Option<String>,
    destination: Option<DirectSandboxDestination>,
    state: DirectSandboxTransferState,
}

#[derive(Clone)]
struct DirectSandboxDestination {
    path: String,
    generation: String,
    overwrite: bool,
}

#[derive(Clone)]
enum DirectSandboxTransferState {
    Preparing,
    DownloadReady,
    UploadReady,
    Completing,
    Completed(brain_protocol::hand::FileEntry),
    Ambiguous,
}

#[derive(Default)]
pub struct BrainServices {
    pub session_storage: Option<Arc<dyn crate::storage::SessionStoragePort>>,
    pub bundle_storage: Option<Arc<dyn crate::storage::BundleStoragePort>>,
    pub hand: Option<Arc<dyn crate::hand::HandPort>>,
    pub session_preparation: Option<Arc<dyn crate::hand::SessionPreparationPort>>,
    pub sandbox_files: Option<Arc<dyn crate::hand::SandboxFilesPort>>,
    pub sandbox_control: Option<Arc<dyn crate::hand::SandboxControlPort>>,
    pub customer_delivery: Option<Arc<dyn crate::customer::CustomerHandDeliveryPort>>,
    pub customer_transport: Option<crate::customer::CustomerTransportConfig>,
    pub compactor: Option<Arc<dyn crate::compact::CompactionPort>>,
    /// The implementation of the official `aex` loop. `None` installs the in-process builtin;
    /// loop-host compositions install the wasm guest or the remote adapter here.
    pub agentloop: Option<Arc<dyn crate::agentloop::Agentloop>>,
    /// Selector→loop resolution. `None` installs [`crate::agentloop::OfficialAexRegistry`]
    /// over the `agentloop` slot: official `aex` only, everything else refused at create.
    pub agentloop_registry: Option<Arc<dyn crate::agentloop::AgentloopRegistry>>,
}

fn hash_create_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

static CREATE_SESSION_VALIDATOR: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let mut schema: serde_json::Value = serde_json::from_str(brain_protocol::SESSION_SCHEMA_JSON)
        .expect("embedded session schema is valid JSON");
    let object = schema
        .as_object_mut()
        .expect("embedded session schema is an object");
    object.insert(
        "$ref".into(),
        serde_json::Value::String("#/$defs/CreateSessionRequest".into()),
    );
    // Keep all local `$defs`, but do not let the canonical documentation URI trigger a remote
    // resolution path in a request validator.
    object.remove("$id");
    jsonschema::draft202012::new(&schema).expect("embedded CreateSessionRequest schema compiles")
});

/// Generated structs intentionally stay ergonomic Rust DTOs; serde does not enforce JSON Schema
/// numeric and container bounds. Validate the complete typed projection before hashing, custody,
/// bundle staging or any other external effect.
fn validate_create_request(req: &CreateSessionRequest) -> Result<()> {
    let value = serde_json::to_value(req)?;
    if let Err(error) = CREATE_SESSION_VALIDATOR.validate(&value) {
        let path = error.instance_path().as_str();
        let path = if path.is_empty() { "/" } else { path };
        // Never format the validation error itself: it may retain a secret-bearing instance.
        return Err(BrainError::Invalid(format!(
            "create request does not satisfy the public contract at {path}"
        )));
    }
    if req.secrets.len() > brain_protocol::MAX_SESSION_SECRET_NAMES
        || req
            .secrets
            .keys()
            .any(|name| name.len() > brain_protocol::MAX_SESSION_SECRET_NAME_BYTES)
        || req
            .secrets
            .values()
            .any(|secret| secret.len() > brain_protocol::MAX_SESSION_SECRET_VALUE_UTF8_BYTES)
    {
        return Err(BrainError::Invalid(
            "session secrets exceed the canonical name or UTF-8 value bounds".into(),
        ));
    }
    if !req.secrets.is_empty()
        && serde_jcs::to_vec(&req.secrets)?.len()
            > brain_protocol::MAX_SESSION_SECRET_DOCUMENT_BYTES
    {
        return Err(BrainError::Invalid(format!(
            "session secret document exceeds {} UTF-8 bytes",
            brain_protocol::MAX_SESSION_SECRET_DOCUMENT_BYTES
        )));
    }
    Ok(())
}

fn validate_model_tool_projection<'a>(
    definitions: impl IntoIterator<Item = (&'a str, &'a str, &'a serde_json::Value)>,
) -> Result<()> {
    let definitions: Vec<_> = definitions
        .into_iter()
        .map(|(name, description, input_schema)| {
            serde_json::json!({
                "name": name,
                "description": description,
                "input_schema": input_schema,
            })
        })
        .collect();
    if definitions.len() > brain_protocol::MAX_MODEL_TOOLS {
        return Err(BrainError::Invalid(format!(
            "tools expose {} model-visible definitions; maximum is {}",
            definitions.len(),
            brain_protocol::MAX_MODEL_TOOLS
        )));
    }
    let bytes = serde_jcs::to_vec(&definitions)?.len();
    if bytes > brain_protocol::MAX_MODEL_TOOL_DEFINITION_BYTES {
        return Err(BrainError::Invalid(format!(
            "model-visible Tool definitions are {bytes} bytes; maximum is {}",
            brain_protocol::MAX_MODEL_TOOL_DEFINITION_BYTES
        )));
    }
    Ok(())
}

fn managed_bundle_descriptors(
    hand_tools: &[(&crate::config::ToolDecl, &crate::config::HandToolSeal)],
    decoded_bundles: &[(String, Vec<u8>, String)],
) -> Result<Vec<brain_protocol::hand::BundleDescriptor>> {
    hand_tools
        .iter()
        .map(|(decl, seal)| {
            let (_, bytes, media_type) = decoded_bundles
                .iter()
                .find(|(digest, _, _)| digest == &seal.checksum)
                .ok_or_else(|| {
                    BrainError::Invalid(format!(
                        "missing verified bundle for managed Tool {}",
                        decl.name
                    ))
                })?;
            serde_json::from_value(serde_json::json!({
                "bundle_digest": seal.checksum,
                "bytes": bytes.len(),
                "contract_digest": decl.contract_digest,
                "description": (!decl.description.is_empty()).then_some(decl.description.as_str()),
                "object": {
                    "bytes": bytes.len(),
                    "media_type": media_type,
                    "object_id": format!("bundle_{}", seal.checksum),
                    "sha256": seal.checksum,
                },
                "required_env": seal.required_env,
                "runtime": "node22",
                "tool_name": decl.name,
            }))
            .map_err(BrainError::from)
        })
        .collect()
}

fn bundle_object_matches_descriptor(
    object: &brain_protocol::hand::ObjectReference,
    descriptor: &brain_protocol::hand::BundleDescriptor,
) -> Result<bool> {
    // An uncomputable comparison is an error, never a match: `.ok() == .ok()` would report
    // two canonicalization failures as equality.
    let object = serde_jcs::to_vec(object)
        .map_err(|error| BrainError::Journal(format!("bundle object canonicalization: {error}")))?;
    let descriptor = serde_jcs::to_vec(&descriptor.object).map_err(|error| {
        BrainError::Journal(format!("bundle descriptor canonicalization: {error}"))
    })?;
    Ok(object == descriptor)
}

pub(crate) fn managed_hand_resources() -> Result<brain_protocol::hand::ResourceCeiling> {
    serde_json::from_value(serde_json::json!({
        "timeout_ms": 600_000,
        "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
    }))
    .map_err(BrainError::from)
}

fn sealed_managed_binding(
    session_id: &str,
    doc: &HeadDoc,
    descriptor: &brain_protocol::hand::BundleDescriptor,
) -> Result<brain_protocol::hand::SealedBinding> {
    let network = sealed_sandbox_network(doc)?;
    let resources = managed_hand_resources()?;
    let policy_digest = brain_protocol::contract::canonical_digest(&serde_json::json!({
        "network": network,
        "required_env": descriptor.required_env,
        "resources": resources,
    }))?;
    let binding_identity = brain_protocol::contract::canonical_digest(&serde_json::json!({
        "bundle": descriptor,
        "root_id": doc.root_id,
        "session_id": session_id,
    }))?;
    serde_json::from_value(serde_json::json!({
        "binding_id": format!("bnd_{}", &binding_identity.as_str()[..24]),
        "bundle": descriptor,
        "capability": descriptor.tool_name,
        "contract_digest": descriptor.contract_digest,
        "implementation_identity": descriptor.bundle_digest,
        "policy_digest": policy_digest,
        "realm": "aex_managed",
        "realm_id": "node22",
        "required_capabilities": ["execution", "session_preparation"],
        "root_id": doc.root_id,
        "session_id": session_id,
    }))
    .map_err(BrainError::from)
}

pub(crate) fn default_sandbox_target(root_id: &str) -> Result<brain_protocol::hand::SandboxTarget> {
    let digest = hex::encode(Sha256::digest(
        format!("aex.default-target\0{root_id}").as_bytes(),
    ));
    serde_json::from_value(serde_json::json!({
        "kind": "default",
        "session_id": root_id,
        "root_id": root_id,
        "binding_ref": format!("bnd_{}", &digest[..24]),
    }))
    .map_err(BrainError::from)
}

fn initial_default_sandbox(root_id: &str) -> Result<brain_protocol::hand::SandboxStatus> {
    serde_json::from_value(serde_json::json!({
        "state": "never_materialized",
        "target": default_sandbox_target(root_id)?,
        "expires_at_ms": null,
    }))
    .map_err(BrainError::from)
}

fn default_sandbox_request(
    doc: &HeadDoc,
    generation_intent: &str,
) -> Result<brain_protocol::hand::CreateSandboxRequest> {
    sandbox_create_request(
        doc,
        default_sandbox_target(&doc.root_id)?,
        generation_intent,
    )
}

pub(crate) fn sealed_sandbox_network(
    doc: &HeadDoc,
) -> Result<brain_protocol::hand::NetworkCeiling> {
    let network = match doc
        .prefix
        .network
        .get("outbound")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("none")
    {
        "none" => serde_json::json!({"kind":"none"}),
        "public" => serde_json::json!({"kind":"public"}),
        "allowlist" => serde_json::json!({
            "kind":"allowlist",
            "destinations": doc.prefix.network.get("destinations").cloned().unwrap_or_else(|| serde_json::json!([])),
        }),
        other => {
            return Err(BrainError::Invalid(format!(
                "sealed network policy has unknown outbound mode {other}"
            )));
        }
    };
    serde_json::from_value(network).map_err(BrainError::from)
}

fn sandbox_create_request(
    doc: &HeadDoc,
    target: brain_protocol::hand::SandboxTarget,
    generation_intent: &str,
) -> Result<brain_protocol::hand::CreateSandboxRequest> {
    serde_json::from_value(serde_json::json!({
        "target": target,
        "generation_intent": generation_intent,
        "network": sealed_sandbox_network(doc)?,
        "resource_class": "microvm-1gb",
        "resources": {
            "timeout_ms": 600_000,
            "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
        },
    }))
    .map_err(BrainError::from)
}

pub(crate) fn map_hand_port_error(error: brain_protocol::hand::HandError) -> BrainError {
    use brain_protocol::hand::HandErrorCode;
    match error.code {
        HandErrorCode::SandboxNotMaterialized => BrainError::SandboxNotMaterialized,
        HandErrorCode::SandboxGone => BrainError::SandboxGone,
        HandErrorCode::GenerationConflict => BrainError::SandboxGenerationConflict,
        HandErrorCode::ResourceExhausted => BrainError::SandboxResourceExhausted,
        _ => BrainError::Hand(error.message.to_string()),
    }
}

pub(crate) fn idempotent_session_id(tenant_id: &str, key: &str) -> String {
    let hash = hash_create_key(&format!("{tenant_id}\0{key}"));
    format!("ses_{}", &hash[..24])
}

/// A tenant identity asserted by a trusted hosting composition after authenticating the public
/// caller. It is deliberately separate from `CreateSessionRequest`, so untrusted JSON can never
/// choose journal ownership.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrustedPrincipal(String);

impl TrustedPrincipal {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(BrainError::Invalid(
                "trusted tenant id must contain 1 to 128 safe ASCII bytes".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn local() -> Self {
        Self("local".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageIdentity {
    key_hash: String,
    request_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageReplay {
    request_hash: String,
    turn_id: String,
    user_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ForkTurns {
    All,
    None,
    Last(u32),
}

impl ForkTurns {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("all") {
            "all" => Ok(Self::All),
            "none" => Ok(Self::None),
            value => {
                let turns = value.parse::<u32>().map_err(|_| {
                    BrainError::Invalid(
                        "fork_turns must be all, none, or a positive integer".into(),
                    )
                })?;
                if turns == 0 {
                    return Err(BrainError::Invalid(
                        "fork_turns integer must be positive; use none for no inherited turns"
                            .into(),
                    ));
                }
                Ok(Self::Last(turns))
            }
        }
    }

    fn request_value(&self) -> serde_json::Value {
        match self {
            Self::All => serde_json::Value::String("all".into()),
            Self::None => serde_json::Value::String("none".into()),
            Self::Last(turns) => serde_json::Value::String(turns.to_string()),
        }
    }
}

enum Command {
    Message {
        content: Vec<ContentBlock>,
        metadata: HashMap<String, String>,
        idempotency: Option<MessageIdentity>,
        reply: oneshot::Sender<Result<(String, u64)>>, // (turn_id, seq of the user message)
    },
    Cancel {
        reply: oneshot::Sender<Result<HeadDoc>>,
    },
    End {
        reply: oneshot::Sender<Result<HeadDoc>>,
    },
    MaterializeDefaultSandbox {
        reply: oneshot::Sender<Result<brain_protocol::hand::SandboxStatus>>,
    },
    WriteDefaultSandboxFile {
        operation_id: String,
        generation: String,
        path: String,
        content_base64: String,
        overwrite: bool,
        reply: oneshot::Sender<Result<brain_protocol::hand::FileEntry>>,
    },
    CopyStorageToDefaultSandbox {
        operation_id: String,
        generation: String,
        key: String,
        path: String,
        overwrite: bool,
        reply: oneshot::Sender<Result<brain_protocol::hand::FileEntry>>,
    },
    CopyDefaultSandboxToStorage {
        operation_id: String,
        generation: String,
        key: String,
        path: String,
        overwrite: bool,
        reply: oneshot::Sender<Result<crate::storage::StorageObject>>,
    },
    CreateChild {
        prompt: String,
        name: Option<String>,
        fork_turns: ForkTurns,
        idempotency_key: Option<String>,
        reply: oneshot::Sender<Result<session::Session>>,
    },
    Delete {
        queued: bool,
        reply: oneshot::Sender<Result<()>>,
    },
    PrepareStorageUpload {
        request: crate::storage::StorageUploadIntent,
        reply: oneshot::Sender<Result<crate::storage::StorageTransferTicket>>,
    },
    CompleteStorageUpload {
        transfer_id: String,
        reply: oneshot::Sender<Result<crate::storage::StorageObject>>,
    },
    ReconcileStorage {
        reply: oneshot::Sender<Result<()>>,
    },
    WriteStorageInline {
        key: String,
        content_base64: String,
        content_type: Option<String>,
        overwrite: bool,
        reply: oneshot::Sender<Result<crate::storage::StorageObject>>,
    },
    DeleteStorageObject {
        key: String,
        reply: oneshot::Sender<Result<()>>,
    },
}

impl Brain {
    /// The general constructor: bring your own backends. This is the whole composition
    /// surface -- a custom substrate needs no core change.
    pub fn with_parts(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        let external_executor: Arc<dyn ToolExecutor> = match &cfg.external_executor_url {
            Some(endpoint) => Arc::new(
                crate::external::HttpExternalToolExecutor::new(
                    endpoint.clone(),
                    cfg.external_executor_token
                        .as_ref()
                        .map(|token| token.expose().to_string()),
                    cfg.external_call_timeout,
                    cfg.external_executor_capabilities.iter().cloned(),
                )
                .expect("BRAIN_EXTERNAL_TOOL_EXECUTOR_URL must be a literal loopback HTTP URL"),
            ),
            None => Arc::new(DisabledToolExecutor),
        };
        Self::with_parts_and_external(cfg, journal, custody, external_executor, provider_factory)
    }

    /// General composition including a host-owned executor for sealed external tools.
    pub fn with_parts_and_external(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        external_executor: Arc<dyn ToolExecutor>,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        Self::with_parts_and_external_and_storage(
            cfg,
            journal,
            custody,
            external_executor,
            None,
            provider_factory,
        )
    }

    /// General composition including durable session storage. Keeping storage as an injected
    /// neutral port lets the hosted product use S3 while local development can choose a local
    /// implementation without teaching the state machine about either backend.
    pub fn with_parts_and_external_and_storage(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        external_executor: Arc<dyn ToolExecutor>,
        session_storage: Option<Arc<dyn crate::storage::SessionStoragePort>>,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        Self::with_parts_and_services(
            cfg,
            journal,
            custody,
            external_executor,
            BrainServices {
                session_storage,
                customer_delivery: None,
                customer_transport: None,
                compactor: None,
                ..BrainServices::default()
            },
            provider_factory,
        )
    }

    /// Full neutral composition seam used by hosted adapters.
    pub fn with_parts_and_services(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        external_executor: Arc<dyn ToolExecutor>,
        services: BrainServices,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        let outbound = crate::outbound::Outbound::new(cfg.outbound_allow_private);
        let journal = journal
            .with_tenant_storage_limit(cfg.storage_max_tenant_bytes)
            .with_retention_limits(crate::journal::JournalRetentionLimits {
                session_bytes: cfg.journal_max_session_bytes,
                tenant_bytes: cfg.journal_max_tenant_bytes,
                tenant_sessions: cfg.journal_max_tenant_sessions,
            });
        let customer = services.customer_transport.map(|config| {
            crate::customer::CustomerCoordinator::new(config, services.customer_delivery.clone())
        });
        let aex_loop = services
            .agentloop
            .unwrap_or_else(|| Arc::new(crate::agentloop::BuiltinAexLoop));
        Arc::new(Self {
            agentloop_registry: services.agentloop_registry.unwrap_or_else(|| {
                Arc::new(crate::agentloop::OfficialAexRegistry { aex: aex_loop })
            }),
            model_permits: Arc::new(Semaphore::new(cfg.max_concurrent_model_rounds)),
            turn_permits: Arc::new(Semaphore::new(cfg.max_concurrent_turns)),
            create_permits: Arc::new(Semaphore::new(cfg.max_concurrent_creates)),
            journal,
            custody,
            provider_factory: provider_factory.unwrap_or_else(default_provider_factory),
            hub: Arc::new(EventHub::with_max_followers(cfg.max_event_followers)),
            sessions: Mutex::new(HashMap::new()),
            draining: AtomicBool::new(false),
            recovery_started: AtomicBool::new(false),
            recovery_next_shard: AtomicUsize::new(0),
            recovery_cursors: Mutex::new(vec![None; crate::journal::RECOVERY_SHARDS]),
            recovery_permits: Arc::new(Semaphore::new(cfg.max_concurrent_recoveries)),
            resident_permits: Arc::new(Semaphore::new(cfg.max_resident_sessions)),
            resident_pressure: Arc::new(Notify::new()),
            root_secret_cells: Mutex::new(HashMap::new()),
            managed_secret_grants: Mutex::new(HashMap::new()),
            direct_sandbox_transfers: Mutex::new(HashMap::new()),
            outbound,
            external_executor,
            session_storage: services.session_storage,
            bundle_storage: services.bundle_storage,
            hand: services.hand,
            session_preparation: services.session_preparation,
            sandbox_files: services.sandbox_files,
            sandbox_control: services.sandbox_control,
            customer_delivery: services.customer_delivery,
            customer,
            compactor: services
                .compactor
                .unwrap_or_else(|| Arc::new(crate::compact::SameProviderCompactor)),
            cfg,
        })
    }

    /// Unit/integration-test composition. Product and server code must use explicit durable
    /// adapters; this is deliberately not a runtime mode.
    #[doc(hidden)]
    pub fn in_memory_test(data_dir: impl Into<PathBuf>, cfg: BrainConfig) -> Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| BrainError::Invalid(format!("data dir: {e}")))?;
        // Local mode may use a loopback provider gateway. Setting
        // BRAIN_OUTBOUND_ALLOW_PRIVATE explicitly wins; this is a composition choice of this
        // constructor, never of the guard itself.
        let mut cfg = cfg;
        if std::env::var("BRAIN_OUTBOUND_ALLOW_PRIVATE").is_err() {
            cfg.outbound_allow_private = true;
        }
        let owner = format!("brain-{}", crate::mint_id("i", 12));
        Ok(Self::with_parts(
            cfg,
            Journal::new_memory(owner),
            Arc::new(crate::keys::PlainCustody),
            None,
        ))
    }

    async fn root_execution_secrets(
        &self,
        doc: &HeadDoc,
    ) -> Result<(Arc<RootSecretCell>, Arc<RootExecutionSecrets>)> {
        let cell = {
            let mut cells = self.root_secret_cells.lock().expect("root secret cells");
            cells.retain(|_, cell| cell.strong_count() > 0);
            if let Some(cell) = cells.get(&doc.root_id).and_then(Weak::upgrade) {
                cell
            } else {
                let cell = Arc::new(RootSecretCell {
                    value: OnceCell::new(),
                });
                cells.insert(doc.root_id.clone(), Arc::downgrade(&cell));
                cell
            }
        };
        let value = cell
            .value
            .get_or_try_init(|| async {
                let key = self
                    .custody
                    .decrypt(&doc.root_id, &blob_from_b64(&doc.key_b64)?)
                    .await?;
                let hand_env = if doc.hand_secrets_b64.is_empty() {
                    HashMap::new()
                } else {
                    let plain = self
                        .custody
                        .decrypt(&doc.root_id, &blob_from_b64(&doc.hand_secrets_b64)?)
                        .await?;
                    serde_json::from_str(plain.expose()).map_err(|error| {
                        BrainError::Custody(format!("managed Tool secret document: {error}"))
                    })?
                };
                Ok::<_, BrainError>(Arc::new(RootExecutionSecrets { key, hand_env }))
            })
            .await?
            .clone();
        Ok((cell, value))
    }

    fn mint_managed_secret_capability(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        hand_id: &str,
        binding_refs: HashSet<String>,
        mut env_names: Vec<String>,
    ) -> Result<Option<brain_protocol::hand::SecretCapability>> {
        env_names.sort_unstable();
        env_names.dedup();
        if env_names.is_empty() {
            return Ok(None);
        }
        if env_names.len() > brain_protocol::MAX_SESSION_SECRET_NAMES
            || env_names
                .iter()
                .any(|name| !doc.prefix.hand_env_keys.contains(name))
        {
            return Err(BrainError::Invalid(
                "managed Tool secret names are outside the immutable session seal".into(),
            ));
        }
        let now = crate::wall_ms();
        let expires_at_ms = now.saturating_add(MANAGED_SECRET_GRANT_TTL_MS);
        let capability_ref = crate::mint_id("secret", 32);
        {
            let mut grants = self
                .managed_secret_grants
                .lock()
                .expect("managed secret grants");
            grants.retain(|_, grant| grant.expires_at_ms > now);
            if grants.len() >= MAX_MANAGED_SECRET_GRANTS {
                return Err(BrainError::Overloaded);
            }
            grants.insert(
                capability_ref.clone(),
                ManagedSecretGrant {
                    root_id: doc.root_id.clone(),
                    session_id: session_id.to_owned(),
                    hand_id: hand_id.to_owned(),
                    binding_refs,
                    env_names: env_names.clone(),
                    expires_at_ms,
                },
            );
        }
        serde_json::from_value(serde_json::json!({
            "capability_ref": capability_ref,
            "env_names": env_names,
            "expires_at_ms": expires_at_ms,
        }))
        .map(Some)
        .map_err(BrainError::from)
    }

    pub(crate) async fn prepare_managed_session(
        &self,
        session_id: &str,
        doc: &HeadDoc,
    ) -> Result<Arc<HashMap<String, brain_protocol::hand::ResolvedBinding>>> {
        if doc.prefix.managed_bundles.is_empty() {
            return Ok(Arc::new(HashMap::new()));
        }
        let hand = self.hand.as_ref().ok_or_else(|| {
            BrainError::Invalid("managed Tools require the canonical Hand execution port".into())
        })?;
        let preparation = self.session_preparation.as_ref().ok_or_else(|| {
            BrainError::Invalid("managed Tools require the canonical Hand preparation port".into())
        })?;
        let bundle_storage = self.bundle_storage.as_ref().ok_or_else(|| {
            BrainError::Invalid("managed Tools require durable Tool-bundle custody".into())
        })?;

        let mut prepared_bindings = Vec::with_capacity(doc.prefix.managed_bundles.len());
        let mut resolved_by_tool = HashMap::with_capacity(doc.prefix.managed_bundles.len());
        let mut hand_id = None::<String>;
        let mut binding_refs = HashSet::new();
        let mut env_names = Vec::new();
        for descriptor in &doc.prefix.managed_bundles {
            let binding = sealed_managed_binding(session_id, doc, descriptor)?;
            let resolved = hand
                .resolve_binding(binding)
                .await
                .map_err(map_hand_port_error)?;
            if resolved.realm != brain_protocol::hand::ExecutionRealm::AexManaged
                || resolved.recovery != brain_protocol::hand::RecoveryClass::Retained
                || !resolved
                    .capabilities
                    .contains(&brain_protocol::hand::HandCapability::Execution)
                || !resolved
                    .capabilities
                    .contains(&brain_protocol::hand::HandCapability::SessionPreparation)
                || resolved.limits.max_inline_input_bytes.get()
                    < brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES as u64
                || resolved.limits.max_inline_result_bytes.get()
                    < brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES as u64
            {
                return Err(BrainError::HandUnavailable(
                    "resolved managed binding cannot enforce the immutable execution seal".into(),
                ));
            }
            match &hand_id {
                Some(expected) if expected != resolved.hand_id.as_str() => {
                    return Err(BrainError::HandUnavailable(
                        "one session's managed bindings resolved to different Hands".into(),
                    ));
                }
                None => hand_id = Some(resolved.hand_id.to_string()),
                _ => {}
            }
            env_names.extend(
                descriptor
                    .required_env
                    .iter()
                    .map(|name| name.as_str().to_owned()),
            );
            binding_refs.insert(resolved.binding_ref.to_string());
            if resolved_by_tool
                .insert(descriptor.tool_name.to_string(), resolved.clone())
                .is_some()
            {
                return Err(BrainError::Invalid(format!(
                    "managed Tool {} has more than one immutable binding",
                    descriptor.tool_name.as_str()
                )));
            }
            prepared_bindings.push(serde_json::json!({
                "binding_ref": resolved.binding_ref.clone(),
                "bundle_digests": [descriptor.bundle_digest],
            }));
        }

        let mut seen = HashSet::new();
        let mut bundles = Vec::new();
        for descriptor in &doc.prefix.managed_bundles {
            if seen.insert(descriptor.bundle_digest.to_string()) {
                bundles.push(
                    bundle_storage
                        .prepare_bundle_fetch(&doc.root_id, descriptor.bundle_digest.as_str())
                        .await?,
                );
            }
        }
        let secret_capability = self.mint_managed_secret_capability(
            session_id,
            doc,
            hand_id.as_deref().expect("managed bindings are nonempty"),
            binding_refs,
            env_names,
        )?;
        let request: brain_protocol::hand::PrepareSessionRequest =
            serde_json::from_value(serde_json::json!({
                "bindings": prepared_bindings,
                "bundles": bundles,
                "network": sealed_sandbox_network(doc)?,
                "resources": managed_hand_resources()?,
                "root_id": doc.root_id,
                "secret_capability": secret_capability,
                "session_id": session_id,
            }))?;
        preparation
            .prepare(request)
            .await
            .map_err(map_hand_port_error)?;
        Ok(Arc::new(resolved_by_tool))
    }

    // -- create ------------------------------------------------------------------------------

    pub async fn create_session(
        self: &Arc<Self>,
        req: CreateSessionRequest,
        idempotency_key: Option<&str>,
    ) -> Result<session::Session> {
        self.create_session_for(&TrustedPrincipal::local(), req, idempotency_key)
            .await
    }

    pub async fn create_session_for(
        self: &Arc<Self>,
        principal: &TrustedPrincipal,
        req: CreateSessionRequest,
        idempotency_key: Option<&str>,
    ) -> Result<session::Session> {
        let _create_permit = self.try_admit_create()?;
        self.create_session_for_admitted(principal, req, idempotency_key)
            .await
    }

    /// Acquire create admission without reading or allocating the request body. HTTP
    /// compositions hold this permit across extraction and call `create_session_for_admitted`;
    /// direct callers retain the guard in `create_session_for` above.
    pub(crate) fn try_admit_create(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.refuse_while_draining()?;
        self.create_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| BrainError::Overloaded)
    }

    pub(crate) async fn create_session_for_admitted(
        self: &Arc<Self>,
        principal: &TrustedPrincipal,
        req: CreateSessionRequest,
        idempotency_key: Option<&str>,
    ) -> Result<session::Session> {
        if let Some(key) = idempotency_key
            && (key.is_empty() || key.len() > 128)
        {
            return Err(BrainError::Invalid(
                "Idempotency-Key must contain 1 to 128 bytes".into(),
            ));
        }
        validate_create_request(&req)?;
        let create_key_hash =
            idempotency_key.map(|key| hash_create_key(&format!("{}\0{key}", principal.as_str())));
        let create_request_hash = create_key_hash
            .as_ref()
            .map(|_| {
                let canonical = serde_jcs::to_vec(&req)?;
                Ok::<_, BrainError>(hex::encode(Sha256::digest(canonical)))
            })
            .transpose()?;
        let session_id = idempotency_key
            .map(|key| idempotent_session_id(principal.as_str(), key))
            .unwrap_or_else(|| crate::mint_id("ses", 24));

        if create_key_hash.is_some()
            && let Some(doc) = self
                .replay_create(&session_id, &create_key_hash, &create_request_hash)
                .await?
        {
            return Ok(doc);
        }

        // Validate and resolve the sealed configuration.
        let provider = provider_name(&req.model.provider);
        let base_url = resolve_base_url(
            &req.model.provider,
            req.model.base_url.as_ref().map(|value| value.as_str()),
        )?;
        let checked_base = self.outbound.check_url(&base_url)?;
        if checked_base.query().is_some() {
            return Err(BrainError::Invalid(
                "model.base_url must not contain a query".into(),
            ));
        }
        if req.model.api_key.is_empty() {
            return Err(BrainError::Invalid(
                "model.api_key must not be empty".into(),
            ));
        }
        validate_custody_plaintext("model.api_key", req.model.api_key.as_ref())?;
        reqwest::header::HeaderValue::from_str(req.model.api_key.as_ref()).map_err(|_| {
            BrainError::Invalid("model.api_key is not a valid HTTP header value".into())
        })?;
        if req.metadata.len() > 16 {
            return Err(BrainError::Invalid("metadata: at most 16 pairs".into()));
        }
        let tools_cfg = req.tools.clone().unwrap_or_default();
        if !req.secrets.is_empty() {
            validate_custody_plaintext("secrets", &serde_json::to_string(&req.secrets)?)?;
        }
        let tool_items = tools_cfg.items.clone();
        let mut decls = crate::tools::resolve(&tool_items)?;
        let has_customer_tools = decls
            .iter()
            .any(|decl| matches!(decl.route, crate::config::ToolRoute::Customer { .. }));
        if has_customer_tools && req.client.is_none() {
            return Err(BrainError::Invalid(
                "customer-app Tools require client.id on session creation".into(),
            ));
        }
        if has_customer_tools && self.customer.is_none() {
            return Err(BrainError::Invalid(
                "customer-app Tools are unavailable in this Brain composition".into(),
            ));
        }
        let mut official_capabilities = HashMap::new();
        for decl in &mut decls {
            let crate::config::ToolRoute::Intrinsic(capability) = &decl.route else {
                continue;
            };
            if crate::tools::is_direct_engine_capability(capability) {
                continue;
            }
            let policy = self
                .cfg
                .official_capabilities
                .get(capability)
                .cloned()
                .ok_or_else(|| {
                    BrainError::Invalid(format!(
                        "tool {} requires unavailable official capability {capability}",
                        decl.name
                    ))
                })?;
            official_capabilities.insert(capability.clone(), policy.clone());
            decl.route = crate::config::ToolRoute::Server(policy);
        }

        for decl in &decls {
            match &decl.route {
                crate::config::ToolRoute::Server(policy)
                    if policy.max_input_bytes == 0
                        || policy.max_input_bytes
                            > brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} server input ceiling must be between 1 and {} bytes",
                        decl.name,
                        brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES
                    )));
                }
                crate::config::ToolRoute::Server(policy)
                    if !self.external_executor.supports(&policy.capability) =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} requires unavailable server capability {}",
                        decl.name, policy.capability
                    )));
                }
                crate::config::ToolRoute::Intrinsic(capability)
                    if !crate::tools::is_direct_engine_capability(capability) =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} requires unavailable intrinsic capability {}",
                        decl.name, capability
                    )));
                }
                crate::config::ToolRoute::Intrinsic(capability)
                    if capability == "brain.storage"
                        && (self.session_storage.is_none() || self.sandbox_files.is_none()) =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} requires session storage and sandbox-files ports",
                        decl.name
                    )));
                }
                crate::config::ToolRoute::Intrinsic(capability)
                    if capability == "brain.sandbox"
                        && (self.session_storage.is_none()
                            || self.sandbox_files.is_none()
                            || self.sandbox_control.is_none()) =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} requires session storage, sandbox-files and sandbox-control ports",
                        decl.name
                    )));
                }
                _ => {}
            }
        }
        validate_model_tool_projection(decls.iter().map(|decl| {
            (
                decl.name.as_str(),
                decl.description.as_str(),
                &decl.input_schema,
            )
        }))?;
        let mut native_tool_names = HashSet::new();
        for name in crate::tools::names(&decls) {
            if !native_tool_names.insert(name.clone()) {
                return Err(BrainError::Invalid(format!(
                    "tools: duplicate model-visible name {name:?}"
                )));
            }
        }

        let hand_env: HashMap<String, String> = req
            .secrets
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_str().to_owned()))
            .collect();
        let shape = "1gb".to_string();
        let hand_tools: Vec<_> = decls
            .iter()
            .filter_map(|decl| match &decl.route {
                crate::config::ToolRoute::Hand(seal) => Some((decl, seal)),
                _ => None,
            })
            .collect();
        for (decl, seal) in &hand_tools {
            let missing: Vec<_> = seal
                .required_env
                .iter()
                .filter(|key| !hand_env.contains_key(*key))
                .collect();
            if !missing.is_empty() {
                return Err(BrainError::Invalid(format!(
                    "tool {} is missing required Hand environment keys: {}",
                    decl.name,
                    missing
                        .into_iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }

        // Bundle bytes and seals are entirely local validation. Finish this phase before any
        // external create-time effect.
        let mut decoded_bundles = Vec::with_capacity(req.tool_bundles.len());
        let mut total_bundle_bytes = 0usize;
        let mut bundle_checksums = HashSet::new();
        for (index, bundle) in req.tool_bundles.iter().enumerate() {
            let checksum = bundle.checksum.to_string();
            if !bundle_checksums.insert(checksum.clone()) {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}]: duplicate checksum"
                )));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(bundle.content_base64.as_bytes())
                .map_err(|error| {
                    BrainError::Invalid(format!("tool_bundles[{index}].content_base64: {error}"))
                })?;
            if bytes.len() != bundle.bytes.get() as usize {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}].bytes does not match decoded content"
                )));
            }
            if bytes.len() > brain_protocol::MAX_TOOL_BUNDLE_BYTES {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}] exceeds {} bytes",
                    brain_protocol::MAX_TOOL_BUNDLE_BYTES,
                )));
            }
            total_bundle_bytes = total_bundle_bytes.saturating_add(bytes.len());
            if total_bundle_bytes > brain_protocol::MAX_SESSION_BUNDLE_BYTES {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles exceed the {}-byte session limit",
                    brain_protocol::MAX_SESSION_BUNDLE_BYTES,
                )));
            }
            let actual = hex::encode(Sha256::digest(&bytes));
            if actual != checksum {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}] checksum mismatch"
                )));
            }
            decoded_bundles.push((checksum, bytes, bundle.media_type.clone()));
        }
        let referenced_bundle_checksums: HashSet<_> = hand_tools
            .iter()
            .map(|(_, seal)| seal.checksum.as_str())
            .collect();
        for (_, seal) in &hand_tools {
            if !bundle_checksums.contains(&seal.checksum) {
                return Err(BrainError::Invalid(format!(
                    "Hand bundle {} was not supplied",
                    seal.checksum
                )));
            }
        }
        if let Some(unused) = bundle_checksums
            .iter()
            .find(|checksum| !referenced_bundle_checksums.contains(checksum.as_str()))
        {
            return Err(BrainError::Invalid(format!(
                "unreferenced tool bundle {unused}"
            )));
        }
        let managed_bundles = managed_bundle_descriptors(&hand_tools, &decoded_bundles)?;

        let mut hand_env_keys: Vec<_> = hand_env.keys().cloned().collect();
        hand_env_keys.sort();

        // Seal the loop identity before anything else commits, rejecting a loop this
        // composition cannot run while the request is still refusable.
        let agentloop_selector = match &req.agentloop {
            None => self.agentloop_registry.pin_official("aex")?,
            Some(brain_protocol::session::AgentloopConfig::String(name)) => {
                self.agentloop_registry.pin_official(name.as_str())?
            }
            Some(brain_protocol::session::AgentloopConfig::Object {
                source_bundle_sha256,
                toolchain,
                bundle_base64,
            }) => {
                use base64::Engine as _;
                let bundle = base64::engine::general_purpose::STANDARD
                    .decode(bundle_base64.as_str())
                    .map_err(|_| {
                        BrainError::Invalid("agentloop.bundle_base64 is not valid base64".into())
                    })?;
                if bundle.is_empty() || bundle.len() > brain_protocol::MAX_LOOP_BUNDLE_BYTES {
                    return Err(BrainError::Invalid(
                        "agentloop bundle must be between 1 byte and 8 MiB".into(),
                    ));
                }
                let digest = hex::encode(Sha256::digest(&bundle));
                if digest != source_bundle_sha256.as_str() {
                    return Err(BrainError::Invalid(format!(
                        "agentloop bundle digest is {digest}, not the declared {}",
                        source_bundle_sha256.as_str()
                    )));
                }
                self.agentloop_registry
                    .admit_custom(&digest, toolchain.as_str(), &bundle)?
            }
        };
        self.agentloop_registry.resolve(&agentloop_selector)?;

        let now = crate::wall_ms();
        let mut prefix = PrefixDoc {
            agentloop: Some(agentloop_selector),
            system_prompt: req.system_prompt.clone().map(String::from),
            provider: provider.to_string(),
            model: req.model.name.to_string(),
            base_url: Some(base_url),
            max_output_tokens: req.model.max_output_tokens.map(|n| n.get()),
            context_window_tokens: u32::try_from(req.model.context_window_tokens.unwrap_or(
                i64::from(brain_protocol::DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS),
            ))
            .expect("CreateSessionRequest schema validated model.context_window_tokens"),
            context_soft_tokens: 0,
            context_hard_tokens: 0,
            context_tail_tokens: 0,
            context_summary_tokens: 0,
            temperature: req.model.temperature,
            reasoning_effort: req
                .model
                .reasoning_effort
                .as_ref()
                .map(|r| format!("{r:?}").to_lowercase()),
            provider_recovery_retries: u32::try_from(req.provider_recovery_retries)
                .expect("CreateSessionRequest schema validated provider_recovery_retries"),
            storage_max_object_bytes: self.cfg.storage_max_object_bytes,
            storage_max_session_bytes: self.cfg.storage_max_session_bytes,
            storage_transfer_ttl_ms: self.cfg.storage_transfer_ttl.as_millis() as u64,
            max_additional_sandboxes_per_root: self.cfg.max_additional_sandboxes_per_root,
            network: req
                .network
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?
                .unwrap_or_else(|| serde_json::json!({"outbound": "none"})),
            max_child_depth: req.children.as_ref().map_or(4, |limits| {
                u32::try_from(limits.max_depth)
                    .expect("CreateSessionRequest schema validated children.max_depth")
            }),
            max_direct_children: req.children.as_ref().map_or(32, |limits| {
                u32::try_from(limits.max_direct_children)
                    .expect("CreateSessionRequest schema validated children.max_direct_children")
            }),
            max_descendants: req.children.as_ref().map_or(256, |limits| {
                u32::try_from(limits.max_descendants)
                    .expect("CreateSessionRequest schema validated children.max_descendants")
            }),
            customer_client_id: req
                .client
                .as_ref()
                .map(|client| String::from(client.id.clone())),
            customer_submit_retries: req.client.as_ref().map_or(1, |client| {
                u32::try_from(client.submit_retries)
                    .expect("CreateSessionRequest schema validated client.submit_retries")
            }),
            rendered_base: serde_json::Value::Null,
            rendered_base_digest: String::new(),
            prompt_cache_key: format!("aex:{session_id}"),
            tools: tool_items,
            managed_bundles,
            official_capabilities,
            hand_enabled: true,
            shape: shape.clone(),
            sync_interval_seconds: 0,
            hand_env_keys,
            metadata: req
                .metadata
                .iter()
                .map(|(key, value)| (key.as_str().to_owned(), value.as_str().to_owned()))
                .collect(),
        };
        let (render_prefix, _) = build_prefix(&prefix, self.cfg.default_max_rounds)?;
        prefix.rendered_base = crate::provider::render_base(&render_prefix);
        prefix.rendered_base_digest =
            hex::encode(Sha256::digest(serde_jcs::to_vec(&prefix.rendered_base)?));
        let context_budget = crate::compact::derive_context_budget(
            &prefix.rendered_base,
            prefix.context_window_tokens as usize,
            usize::try_from(prefix.max_output_tokens.unwrap_or(4096)).map_err(|_| {
                BrainError::Invalid("model.max_output_tokens exceeds this host's usize".into())
            })?,
            self.cfg.context_soft_tokens,
            self.cfg.context_hard_tokens,
            self.cfg.context_tail_tokens,
        )?;
        prefix.context_soft_tokens = u32::try_from(context_budget.soft_tokens)
            .map_err(|_| BrainError::Invalid("derived context soft limit exceeds u32".into()))?;
        prefix.context_hard_tokens = u32::try_from(context_budget.hard_tokens)
            .map_err(|_| BrainError::Invalid("derived context hard limit exceeds u32".into()))?;
        prefix.context_tail_tokens = u32::try_from(context_budget.tail_tokens)
            .map_err(|_| BrainError::Invalid("derived context tail limit exceeds u32".into()))?;
        prefix.context_summary_tokens = u32::try_from(context_budget.summary_tokens)
            .map_err(|_| BrainError::Invalid("derived summary limit exceeds u32".into()))?;

        if !decoded_bundles.is_empty() {
            let bundle_storage = self.bundle_storage.as_ref().ok_or_else(|| {
                BrainError::Invalid(
                    "managed Tools require durable internal Tool-bundle custody".into(),
                )
            })?;
            for (digest, bytes, _) in &decoded_bundles {
                let object = bundle_storage
                    .store_bundle(&session_id, digest, bytes)
                    .await?;
                let descriptor = prefix
                    .managed_bundles
                    .iter()
                    .find(|descriptor| descriptor.bundle_digest.as_str() == digest)
                    .ok_or_else(|| {
                        BrainError::Journal("stored Tool bundle has no immutable descriptor".into())
                    })?;
                if !bundle_object_matches_descriptor(&object, descriptor)? {
                    return Err(BrainError::Journal(
                        "Tool-bundle storage returned an object outside the immutable descriptor"
                            .into(),
                    ));
                }
            }
        }

        // Only after every pure request/prefix/budget validation succeeds may custody perform an
        // external effect. The plaintext key and session secrets never reach the journal.
        let key = ProviderKey::new(req.model.api_key.to_string());
        let blob = self.custody.encrypt(&session_id, &key).await?;
        let hand_secrets_b64 = if hand_env.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(&hand_env)?;
            let encrypted = self
                .custody
                .encrypt(&session_id, &ProviderKey::new(json))
                .await?;
            blob_to_b64(&encrypted)
        };

        let doc = HeadDoc {
            loop_state: None,
            tenant_id: principal.as_str().into(),
            root_id: session_id.clone(),
            parent_id: None,
            ancestor_ids: Vec::new(),
            child_name: None,
            context_fork: None,
            depth: 0,
            last_seq: 1,
            state: "open".into(),
            failure: None,
            turn: None,
            active_phase: None,
            provider_attempt: None,
            active_context: HashMap::new(),
            active_rounds: 0,
            active_tool_calls: 0,
            message_replays: Vec::new(),
            context: None,
            turns: 0,
            created_ms: now,
            updated_ms: now,
            recovery_due_ms: None,
            recovery_attempt: 0,
            create_key_hash: create_key_hash.clone(),
            create_request_hash: create_request_hash.clone(),
            last_message_ms: None,
            ended: false,
            prefix,
            key_b64: blob_to_b64(&blob),
            hand_secrets_b64,
            session_storage_bytes: 0,
            storage_reserved_bytes: 0,
            // Root-hidden managed bundles are not part of the public storage gauges, but they
            // consume real durable bytes and therefore reserve the authoritative tenant meter.
            // Descendants share these root objects and contribute zero additional bytes.
            tenant_metered_storage_bytes: total_bundle_bytes as u64,
            storage_upload: None,
            storage_delete: None,
            pending_customer_acks: Vec::new(),
            pending_managed_acks: Vec::new(),
            default_sandbox: Some(initial_default_sandbox(&session_id)?),
        };
        if let Err(error) = self
            .journal
            .create(
                &session_id,
                &doc,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
        {
            if matches!(&error, BrainError::TenantStorageQuotaExceeded { .. })
                && !decoded_bundles.is_empty()
                && let Some(bundle_storage) = &self.bundle_storage
                && let Err(cleanup) = bundle_storage.purge_root_bundles(&session_id).await
            {
                return Err(BrainError::Journal(format!(
                    "tenant bundle reservation was rejected and bundle cleanup failed: {cleanup}"
                )));
            }
            if create_key_hash.is_some()
                && let Some(doc) = self
                    .replay_create(&session_id, &create_key_hash, &create_request_hash)
                    .await?
            {
                return Ok(doc);
            }
            return Err(error);
        }

        session_doc(&session_id, &doc)
    }

    async fn replay_create(
        &self,
        session_id: &str,
        key_hash: &Option<String>,
        request_hash: &Option<String>,
    ) -> Result<Option<session::Session>> {
        let head = match self.journal.get_head(session_id).await {
            Ok(head) => head,
            Err(BrainError::NoSuchSession(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if &head.doc.create_key_hash != key_hash || &head.doc.create_request_hash != request_hash {
            return Err(BrainError::IdempotencyConflict);
        }
        Ok(Some(session_doc(session_id, &head.doc)?))
    }

    // -- routing to actors -------------------------------------------------------------------

    async fn sender(&self, session_id: &str) -> Result<mpsc::Sender<Command>> {
        if let Some(tx) = self
            .sessions
            .lock()
            .expect("sessions lock")
            .get(session_id)
            .cloned()
            && !tx.is_closed()
        {
            return Ok(tx);
        }
        Err(BrainError::NoSuchSession(session_id.into()))
    }

    async fn sender_or_spawn(self: &Arc<Self>, session_id: &str) -> Result<mpsc::Sender<Command>> {
        if let Ok(tx) = self.sender(session_id).await {
            return Ok(tx);
        }
        // Not resident: prove the session exists before spawning.
        let head = self.journal.get_head(session_id).await?;
        if head.doc.state == "deleted" {
            return Err(BrainError::SessionDeleted(session_id.into()));
        }
        self.spawn_actor(session_id, ActorStartup::Lazy).await
    }

    fn spawn_actor<'a>(
        self: &'a Arc<Self>,
        session_id: &'a str,
        startup: ActorStartup,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<mpsc::Sender<Command>>> + Send + 'a>,
    > {
        // Type erase this edge: an ordinary child can be spawned from an engine Tool running
        // inside an actor, while the spawned actor itself can later run that same Tool. Boxing
        // prevents the compiler from recursively expanding the two async state machines.
        Box::pin(async move {
            self.spawn_actor_with_permit(session_id, startup, None)
                .await
        })
    }

    async fn spawn_actor_with_permit(
        self: &Arc<Self>,
        session_id: &str,
        startup: ActorStartup,
        recovery_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<mpsc::Sender<Command>> {
        if let Some(existing) = self
            .sessions
            .lock()
            .expect("sessions lock")
            .get(session_id)
            .cloned()
            && !existing.is_closed()
        {
            return Ok(existing);
        }
        let resident_permit = match self.resident_permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                // Ask one safe idle actor to release its own lease/state. Active actors and
                // actors retaining pending acknowledgements/storage deadlines do not register
                // for this notification, so pressure can never steal effectful work.
                self.resident_pressure.notify_one();
                tokio::time::timeout(
                    Duration::from_millis(250),
                    self.resident_permits.clone().acquire_owned(),
                )
                .await
                .map_err(|_| BrainError::Overloaded)?
                .map_err(|_| BrainError::Overloaded)?
            }
        };
        let (tx, rx) = mpsc::channel(16);
        {
            let mut map = self.sessions.lock().expect("sessions lock");
            if let Some(existing) = map.get(session_id)
                && !existing.is_closed()
            {
                return Ok(existing.clone());
            }
            map.insert(session_id.to_string(), tx.clone());
        }
        let brain = self.clone();
        let sid = session_id.to_string();
        tokio::spawn(async move {
            let _recovery_permit = recovery_permit;
            let _resident_permit = resident_permit;
            actor(brain.clone(), sid.clone(), rx, startup).await;
            let mut map = brain.sessions.lock().expect("sessions lock");
            if let Some(cur) = map.get(&sid)
                && cur.is_closed()
            {
                map.remove(&sid);
            }
            // Root secret cells are weak and contain no material once their last resident is
            // gone. Prune dead keys on every actor exit so root churn cannot grow the registry.
            brain
                .root_secret_cells
                .lock()
                .expect("root secret cells")
                .retain(|_, cell| cell.strong_count() > 0);
        });
        Ok(tx)
    }

    /// Start bounded, sharded recovery discovery for nonterminal durable work. This is explicit
    /// so library users can construct a Brain outside a Tokio runtime; the HTTP router calls it
    /// automatically. Calling it more than once is harmless.
    /// Begin the deploy drain: refuse new creates, messages and recovery starts while every
    /// already-admitted turn runs to completion. Event followers keep streaming until the
    /// process exits; on reconnect they replay durable records from the replacement.
    pub fn begin_drain(&self) {
        if !self.draining.swap(true, Ordering::SeqCst) {
            tracing::info!(
                active_turns = self.active_turns(),
                "drain started; refusing new work while admitted turns finish"
            );
        }
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Turns currently holding an admission permit (message-driven and recovery-driven alike).
    pub fn active_turns(&self) -> usize {
        self.cfg
            .max_concurrent_turns
            .saturating_sub(self.turn_permits.available_permits())
    }

    fn refuse_while_draining(&self) -> Result<()> {
        if self.is_draining() {
            return Err(BrainError::Draining);
        }
        Ok(())
    }

    pub fn start_recovery_worker(self: &Arc<Self>) {
        if self
            .recovery_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let brain = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = brain.recover_due_pass().await {
                    tracing::warn!(error = %error, "recovery discovery pass failed");
                }
                // A small process-local jitter prevents a fleet from polling all shards at the
                // same instant. It is deliberately not persisted: a claim never advances the
                // durable due key, so a crashed claimant remains discoverable after lease expiry.
                let jitter_ms = crate::wall_ms() % 251;
                tokio::time::sleep(
                    brain.cfg.recovery_poll_interval + Duration::from_millis(jitter_ms),
                )
                .await;
            }
        });
    }

    async fn recover_due_pass(self: &Arc<Self>) -> Result<usize> {
        if self.is_draining() {
            // Due anchors stay durable; the replacement process discovers them.
            return Ok(0);
        }
        let start = self
            .recovery_next_shard
            .fetch_add(self.cfg.recovery_shards_per_poll, Ordering::Relaxed);
        let now_ms = crate::wall_ms();
        let mut started = 0usize;
        for offset in 0..self.cfg.recovery_shards_per_poll {
            let shard_index = (start + offset) % crate::journal::RECOVERY_SHARDS;
            let shard = format!("r{shard_index:02x}");
            let cursor =
                self.recovery_cursors.lock().expect("recovery cursors")[shard_index].clone();
            let page = self
                .journal
                .list_recovery_page(&crate::journal::RecoveryQuery {
                    shard: &shard,
                    due_before_ms: now_ms,
                    limit: self.cfg.recovery_page_size,
                    cursor: cursor.as_deref(),
                })
                .await?;
            self.recovery_cursors.lock().expect("recovery cursors")[shard_index] =
                page.next_cursor.clone();
            for item in page.items {
                if self.sender(&item.session_id).await.is_ok() {
                    continue;
                }
                let Ok(permit) = self.recovery_permits.clone().try_acquire_owned() else {
                    // Leave every remaining due anchor untouched. The rotating shard/page cursor
                    // will revisit it once a bounded recovery slot is free.
                    return Ok(started);
                };
                match self
                    .spawn_actor_with_permit(&item.session_id, ActorStartup::Recovery, Some(permit))
                    .await
                {
                    Ok(_) => started += 1,
                    Err(BrainError::Overloaded) => return Ok(started),
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(started)
    }

    /// Deliver a command to the session's actor, retrying ONCE against a fresh actor when the
    /// send itself fails. A failed send means the actor closed its inbox while exiting on the
    /// idle timer — the command never entered the channel, so retrying cannot double-apply.
    /// A dropped REPLY stays an error: the command may have partially run.
    async fn deliver<R>(
        self: &Arc<Self>,
        session_id: &str,
        build: impl Fn(oneshot::Sender<R>) -> Command,
    ) -> Result<R> {
        for _ in 0..2 {
            let tx = self.sender_or_spawn(session_id).await?;
            let (reply, rx) = oneshot::channel();
            if tx.send(build(reply)).await.is_err() {
                continue;
            }
            return rx
                .await
                .map_err(|_| BrainError::Journal("actor dropped the reply".into()));
        }
        Err(BrainError::NoSuchSession(session_id.into()))
    }

    pub async fn message(
        self: &Arc<Self>,
        session_id: &str,
        content: MessageRequestContent,
    ) -> Result<(String, u64)> {
        self.message_with_metadata(session_id, content, HashMap::new())
            .await
    }

    pub async fn message_with_metadata(
        self: &Arc<Self>,
        session_id: &str,
        content: MessageRequestContent,
        metadata: HashMap<String, String>,
    ) -> Result<(String, u64)> {
        self.message_with_metadata_idempotent(session_id, content, metadata, None)
            .await
    }

    pub async fn message_with_metadata_idempotent(
        self: &Arc<Self>,
        session_id: &str,
        content: MessageRequestContent,
        metadata: HashMap<String, String>,
        idempotency_key: Option<&str>,
    ) -> Result<(String, u64)> {
        self.refuse_while_draining()?;
        if metadata.len() > 32
            || metadata
                .iter()
                .any(|(key, value)| key.len() > 128 || value.len() > 4096)
        {
            return Err(BrainError::Invalid(
                "message metadata exceeds the 32-pair, 128-byte key, or 4096-byte value limit"
                    .into(),
            ));
        }
        if let Some(key) = idempotency_key
            && (key.is_empty() || key.len() > 128)
        {
            return Err(BrainError::Invalid(
                "Idempotency-Key must contain 1 to 128 bytes".into(),
            ));
        }
        let blocks = content_blocks(content)?;
        let idempotency = idempotency_key
            .map(|key| {
                let canonical = serde_jcs::to_vec(&serde_json::json!({
                    "content": &blocks,
                    "metadata": &metadata,
                }))?;
                Ok::<_, BrainError>(MessageIdentity {
                    key_hash: hash_create_key(key),
                    request_hash: hex::encode(Sha256::digest(canonical)),
                })
            })
            .transpose()?;
        self.deliver(session_id, |reply| Command::Message {
            content: blocks.clone(),
            metadata: metadata.clone(),
            idempotency: idempotency.clone(),
            reply,
        })
        .await?
    }

    pub async fn cancel(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        let doc = self
            .deliver(session_id, |reply| Command::Cancel { reply })
            .await??;
        session_doc(session_id, &doc)
    }

    pub async fn end(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        let doc = self
            .deliver(session_id, |reply| Command::End { reply })
            .await??;
        session_doc(session_id, &doc)
    }

    /// Current logical status of the root tree's shared default sandbox. Reading status never
    /// materializes compute. Descendants resolve through their immutable `root_id`.
    pub async fn default_sandbox_status(
        &self,
        session_id: &str,
    ) -> Result<brain_protocol::hand::SandboxStatus> {
        let head = self.journal.get_head(session_id).await?;
        let root = if head.doc.root_id == session_id {
            head
        } else {
            self.journal.get_head(&head.doc.root_id).await?
        };
        root.doc
            .default_sandbox
            .clone()
            .map(Ok)
            .unwrap_or_else(|| initial_default_sandbox(&root.doc.root_id))
    }

    /// Idempotently materialize the shared default target. This is separate from the official
    /// model sandbox Tool, which creates additional isolated targets under a root quota.
    pub async fn materialize_default_sandbox(
        self: &Arc<Self>,
        session_id: &str,
    ) -> Result<brain_protocol::hand::SandboxStatus> {
        let head = self.journal.get_head(session_id).await?;
        let root_id = head.doc.root_id.clone();
        self.deliver(&root_id, |reply| Command::MaterializeDefaultSandbox {
            reply,
        })
        .await?
    }

    async fn default_sandbox_file_target(
        &self,
        session_id: &str,
        expected_generation: &str,
    ) -> Result<brain_protocol::hand::SandboxTarget> {
        let session = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&session.doc, session_id)?;
        let root = if session.doc.root_id == session_id {
            session
        } else {
            self.journal.get_head(&session.doc.root_id).await?
        };
        if root.doc.state != "open" || root.doc.ended {
            return Err(BrainError::SessionDeleted(root.doc.root_id.clone()));
        }
        let status = root
            .doc
            .default_sandbox
            .as_ref()
            .ok_or(BrainError::SandboxNotMaterialized)?;
        if status.generation.as_ref().map(|value| value.as_str()) != Some(expected_generation) {
            return Err(
                if matches!(
                    status.state,
                    brain_protocol::hand::SandboxState::Gone
                        | brain_protocol::hand::SandboxState::Terminated
                ) {
                    BrainError::SandboxGone
                } else {
                    BrainError::SandboxGenerationConflict
                },
            );
        }
        match status.state {
            brain_protocol::hand::SandboxState::Running
            | brain_protocol::hand::SandboxState::Suspended => Ok(status.target.clone()),
            brain_protocol::hand::SandboxState::Gone
            | brain_protocol::hand::SandboxState::Terminated => Err(BrainError::SandboxGone),
            brain_protocol::hand::SandboxState::NeverMaterialized
            | brain_protocol::hand::SandboxState::Creating => {
                Err(BrainError::SandboxNotMaterialized)
            }
        }
    }

    fn sandbox_files_port(&self) -> Result<&Arc<dyn crate::hand::SandboxFilesPort>> {
        self.sandbox_files.as_ref().ok_or_else(|| {
            BrainError::Invalid("sandbox files are unavailable in this composition".into())
        })
    }

    pub async fn sandbox_file_list(
        &self,
        session_id: &str,
        generation: &str,
        path: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::hand::SandboxFileList> {
        let path = normalize_workspace_path(path)?;
        let target = self
            .default_sandbox_file_target(session_id, generation)
            .await?;
        self.sandbox_files_port()?
            .list(crate::hand::SandboxFileListRequest {
                target,
                expected_generation: generation.to_owned(),
                path,
                cursor: cursor.map(str::to_owned),
                limit: limit.clamp(1, 100),
            })
            .await
            .map_err(map_hand_port_error)
    }

    pub async fn sandbox_file_stat(
        &self,
        session_id: &str,
        generation: &str,
        path: &str,
    ) -> Result<brain_protocol::hand::FileEntry> {
        let path = normalize_workspace_path(path)?;
        let target = self
            .default_sandbox_file_target(session_id, generation)
            .await?;
        self.sandbox_files_port()?
            .stat(sandbox_file_request(&target, generation, &path)?)
            .await
            .map_err(map_hand_port_error)
    }

    pub async fn sandbox_file_read_inline(
        &self,
        session_id: &str,
        generation: &str,
        path: &str,
        max_bytes: u64,
    ) -> Result<crate::hand::SandboxFileContent> {
        let path = normalize_workspace_path(path)?;
        let target = self
            .default_sandbox_file_target(session_id, generation)
            .await?;
        let content = self
            .sandbox_files_port()?
            .read(sandbox_file_request(&target, generation, &path)?)
            .await
            .map_err(map_hand_port_error)?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&content.content_base64)
            .map_err(|_| BrainError::Hand("sandbox returned invalid base64".into()))?;
        let limit = max_bytes.min(1024 * 1024) as usize;
        if decoded.len() > limit || decoded.len() as u64 != content.entry.bytes {
            return Err(BrainError::FileTooLarge { limit });
        }
        Ok(content)
    }

    pub async fn sandbox_file_write_inline(
        self: &Arc<Self>,
        session_id: &str,
        generation: String,
        path: String,
        content_base64: String,
        overwrite: bool,
        idempotency_key: &str,
    ) -> Result<brain_protocol::hand::FileEntry> {
        if idempotency_key.is_empty() || idempotency_key.len() > 128 {
            return Err(BrainError::Invalid(
                "Idempotency-Key must contain 1 to 128 bytes".into(),
            ));
        }
        let path = normalize_workspace_path(&path)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&content_base64)
            .map_err(|_| {
                BrainError::Invalid("sandbox inline content is not valid base64".into())
            })?;
        if bytes.len() > 1024 * 1024 {
            return Err(BrainError::FileTooLarge { limit: 1024 * 1024 });
        }
        if base64::engine::general_purpose::STANDARD.encode(&bytes) != content_base64 {
            return Err(BrainError::Invalid(
                "sandbox inline content must use canonical padded base64".into(),
            ));
        }
        let identity = hash_create_key(&format!(
            "aex.sandbox-file-write.v1\0{session_id}\0{idempotency_key}"
        ));
        let operation_id = format!("file_{}", &identity[..24]);
        self.deliver(session_id, |reply| Command::WriteDefaultSandboxFile {
            operation_id: operation_id.clone(),
            generation: generation.clone(),
            path: path.clone(),
            content_base64: content_base64.clone(),
            overwrite,
            reply,
        })
        .await?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn sandbox_file_search(
        &self,
        session_id: &str,
        generation: &str,
        path: &str,
        expression: &str,
        cursor: Option<&str>,
        limit: u32,
        grep: bool,
    ) -> Result<crate::hand::SandboxFileList> {
        let path = normalize_workspace_path(path)?;
        if expression.is_empty() || expression.len() > 4096 {
            return Err(BrainError::Invalid(
                "sandbox search expression must contain 1 to 4096 UTF-8 bytes".into(),
            ));
        }
        let target = self
            .default_sandbox_file_target(session_id, generation)
            .await?;
        let request = sandbox_search_request(
            &target,
            generation,
            &path,
            expression,
            cursor,
            limit.clamp(1, 100),
        )?;
        let files = self.sandbox_files_port()?;
        if grep {
            files.grep(request).await
        } else {
            files.find(request).await
        }
        .map_err(map_hand_port_error)
    }

    /// Prepare a short-lived direct download of one generation-fenced sandbox file. Bytes are
    /// exported through the existing exact-pair Hand copy operation into quota-metered hidden
    /// session storage; the public ticket never exposes that storage key.
    pub async fn sandbox_file_prepare_download(
        self: &Arc<Self>,
        session_id: &str,
        generation: String,
        path: String,
    ) -> Result<crate::storage::StorageTransferTicket> {
        self.storage_port()?;
        self.sandbox_files_port()?;
        let path = normalize_workspace_path(&path)?;
        let entry = self
            .sandbox_file_stat(session_id, &generation, &path)
            .await?;
        if entry.kind != brain_protocol::hand::FileEntryKind::File {
            return Err(BrainError::Invalid(
                "sandbox download source must be a regular file".into(),
            ));
        }
        let head = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&head.doc, session_id)?;
        if entry.bytes > head.doc.prefix.storage_max_object_bytes {
            return Err(BrainError::StorageObjectTooLarge {
                limit: head.doc.prefix.storage_max_object_bytes,
            });
        }

        let transfer_id = crate::mint_id("sbxfer", 24);
        let storage_key = direct_sandbox_transfer_key(&transfer_id);
        let expires_at_ms = crate::wall_ms()
            .checked_add(head.doc.prefix.storage_transfer_ttl_ms)
            .ok_or_else(|| BrainError::Invalid("sandbox transfer expiry overflow".into()))?;
        self.reserve_direct_sandbox_transfer(
            &transfer_id,
            DirectSandboxTransfer {
                session_id: session_id.to_owned(),
                storage_key: storage_key.clone(),
                declared_bytes: entry.bytes,
                expires_at_ms,
                cleanup_at_ms: expires_at_ms.saturating_add(SANDBOX_TRANSFER_CLEANUP_SKEW_MS),
                storage_transfer_id: None,
                destination: None,
                state: DirectSandboxTransferState::Preparing,
            },
        )?;
        self.schedule_direct_sandbox_transfer_cleanup(transfer_id.clone());

        let object = self
            .storage_copy_from_sandbox_internal(
                session_id,
                storage_key.clone(),
                path,
                generation,
                &transfer_id,
            )
            .await;
        let object = match object {
            Ok(object) => object,
            Err(error) => {
                self.mark_direct_sandbox_transfer_ambiguous(session_id, &transfer_id);
                return Err(if direct_sandbox_prepare_failure_is_definitive(&error) {
                    error
                } else {
                    BrainError::SandboxTransferAmbiguous
                });
            }
        };
        let result = async {
            if object.bytes != entry.bytes {
                return Err(BrainError::Hand(
                    "sandbox export changed size while preparing the download".into(),
                ));
            }
            let mut ticket = self
                .storage_prepare_download_internal(session_id, &storage_key)
                .await?;
            if ticket.max_bytes != object.bytes {
                return Err(BrainError::Journal(
                    "sandbox download authority does not match the staged object size".into(),
                ));
            }
            let mut transfers = self
                .direct_sandbox_transfers
                .lock()
                .expect("direct sandbox transfers");
            let transfer = transfers
                .get_mut(&transfer_id)
                .filter(|transfer| transfer.session_id == session_id)
                .ok_or_else(|| BrainError::SandboxTransferExpired(transfer_id.clone()))?;
            transfer.expires_at_ms = ticket.expires_at_ms;
            transfer.cleanup_at_ms = ticket
                .expires_at_ms
                .saturating_add(SANDBOX_TRANSFER_CLEANUP_SKEW_MS);
            transfer.state = DirectSandboxTransferState::DownloadReady;
            ticket.transfer_id.clone_from(&transfer_id);
            Ok(ticket)
        }
        .await;

        if result.is_err() {
            self.mark_direct_sandbox_transfer_ambiguous(session_id, &transfer_id);
            return Err(BrainError::SandboxTransferAmbiguous);
        }
        result
    }

    /// Prepare a direct upload into hidden quota-metered storage. Completion performs one
    /// generation-fenced exact-pair import and never guesses after process loss or ambiguity.
    pub async fn sandbox_file_prepare_upload(
        self: &Arc<Self>,
        session_id: &str,
        generation: String,
        path: String,
        bytes: u64,
        sha256: String,
        overwrite: bool,
    ) -> Result<crate::storage::StorageTransferTicket> {
        self.storage_port()?;
        self.sandbox_files_port()?;
        let path = normalize_workspace_path(&path)?;
        // This read-only lookup both authenticates the session's root and rejects a stale/gone
        // generation before reserving shared process or storage quota.
        self.default_sandbox_file_target(session_id, &generation)
            .await?;
        let head = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&head.doc, session_id)?;
        let transfer_id = crate::mint_id("sbxfer", 24);
        let storage_key = direct_sandbox_transfer_key(&transfer_id);
        let intent = crate::storage::StorageUploadIntent {
            key: storage_key.clone(),
            bytes,
            sha256: Some(sha256),
            content_type: None,
            overwrite: false,
        };
        validate_storage_upload_intent(&intent, head.doc.prefix.storage_max_object_bytes)?;
        let expires_at_ms = crate::wall_ms()
            .checked_add(head.doc.prefix.storage_transfer_ttl_ms)
            .ok_or_else(|| BrainError::Invalid("sandbox transfer expiry overflow".into()))?;
        self.reserve_direct_sandbox_transfer(
            &transfer_id,
            DirectSandboxTransfer {
                session_id: session_id.to_owned(),
                storage_key,
                declared_bytes: bytes,
                expires_at_ms,
                cleanup_at_ms: expires_at_ms.saturating_add(SANDBOX_TRANSFER_CLEANUP_SKEW_MS),
                storage_transfer_id: None,
                destination: Some(DirectSandboxDestination {
                    path,
                    generation,
                    overwrite,
                }),
                state: DirectSandboxTransferState::Preparing,
            },
        )?;
        self.schedule_direct_sandbox_transfer_cleanup(transfer_id.clone());

        let result = async {
            let mut ticket = self
                .storage_prepare_upload_internal(session_id, intent)
                .await?;
            let mut transfers = self
                .direct_sandbox_transfers
                .lock()
                .expect("direct sandbox transfers");
            let transfer = transfers
                .get_mut(&transfer_id)
                .filter(|transfer| transfer.session_id == session_id)
                .ok_or_else(|| BrainError::SandboxTransferExpired(transfer_id.clone()))?;
            transfer.storage_transfer_id = Some(ticket.transfer_id.clone());
            transfer.expires_at_ms = ticket.expires_at_ms;
            transfer.cleanup_at_ms = ticket
                .expires_at_ms
                .saturating_add(SANDBOX_TRANSFER_CLEANUP_SKEW_MS);
            transfer.state = DirectSandboxTransferState::UploadReady;
            ticket.transfer_id.clone_from(&transfer_id);
            Ok(ticket)
        }
        .await;

        if result.is_err() {
            self.mark_direct_sandbox_transfer_ambiguous(session_id, &transfer_id);
        }
        result
    }

    pub async fn sandbox_file_complete_upload(
        self: &Arc<Self>,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<brain_protocol::hand::FileEntry> {
        if transfer_id.is_empty() || transfer_id.len() > 128 {
            return Err(BrainError::Invalid(
                "sandbox transfer id must contain 1 to 128 bytes".into(),
            ));
        }
        let (storage_transfer_id, storage_key, destination) = {
            let mut transfers = self
                .direct_sandbox_transfers
                .lock()
                .expect("direct sandbox transfers");
            let Some(transfer) = transfers.get_mut(transfer_id) else {
                return Err(BrainError::SandboxTransferUnknown(transfer_id.into()));
            };
            if transfer.session_id != session_id {
                return Err(BrainError::SandboxTransferUnknown(transfer_id.into()));
            }
            if crate::wall_ms() > transfer.expires_at_ms {
                let expired = transfers.remove(transfer_id).expect("known transfer");
                drop(transfers);
                self.spawn_direct_sandbox_transfer_cleanup(expired);
                return Err(BrainError::SandboxTransferExpired(transfer_id.into()));
            }
            match &transfer.state {
                DirectSandboxTransferState::Completed(file) => return Ok(file.clone()),
                DirectSandboxTransferState::UploadReady => {}
                DirectSandboxTransferState::Preparing
                | DirectSandboxTransferState::DownloadReady
                | DirectSandboxTransferState::Completing
                | DirectSandboxTransferState::Ambiguous => {
                    return Err(BrainError::SandboxTransferAmbiguous);
                }
            }
            let underlying = transfer.storage_transfer_id.clone().ok_or_else(|| {
                BrainError::Journal("ready sandbox upload lacks storage transfer identity".into())
            })?;
            let destination = transfer.destination.clone().ok_or_else(|| {
                BrainError::Journal("ready sandbox upload lacks destination".into())
            })?;
            transfer.state = DirectSandboxTransferState::Completing;
            (underlying, transfer.storage_key.clone(), destination)
        };

        let result = async {
            self.storage_complete_upload_internal(session_id, &storage_transfer_id)
                .await?;
            self.storage_copy_to_sandbox_internal(
                session_id,
                storage_key.clone(),
                destination.path,
                destination.generation,
                destination.overwrite,
                transfer_id,
            )
            .await
        }
        .await;

        match result {
            Ok(file) => {
                {
                    let mut transfers = self
                        .direct_sandbox_transfers
                        .lock()
                        .expect("direct sandbox transfers");
                    let transfer = transfers
                        .get_mut(transfer_id)
                        .filter(|transfer| transfer.session_id == session_id)
                        .ok_or_else(|| BrainError::SandboxTransferAmbiguous)?;
                    transfer.declared_bytes = 0;
                    transfer.state = DirectSandboxTransferState::Completed(file.clone());
                }
                // The sandbox import is already durably complete. Staging cleanup must never turn
                // that success into a failure; the retained quota and root purge bound a miss.
                let _ = self.storage_delete_internal(session_id, storage_key).await;
                Ok(file)
            }
            Err(error)
                if matches!(
                    error,
                    BrainError::SandboxGenerationConflict
                        | BrainError::SandboxGone
                        | BrainError::SandboxNotMaterialized
                ) =>
            {
                let transfer = self.remove_direct_sandbox_transfer(session_id, transfer_id);
                if let Some(transfer) = transfer {
                    self.spawn_direct_sandbox_transfer_cleanup(transfer);
                }
                Err(error)
            }
            Err(BrainError::StorageUploadExpired(_)) => {
                let transfer = self.remove_direct_sandbox_transfer(session_id, transfer_id);
                if let Some(transfer) = transfer {
                    self.spawn_direct_sandbox_transfer_cleanup(transfer);
                }
                Err(BrainError::SandboxTransferExpired(transfer_id.into()))
            }
            Err(BrainError::FileNotFound(_)) => {
                let transfer = self.remove_direct_sandbox_transfer(session_id, transfer_id);
                if let Some(transfer) = transfer {
                    self.spawn_direct_sandbox_transfer_cleanup(transfer);
                }
                Err(BrainError::SandboxTransferUnknown(transfer_id.into()))
            }
            Err(_) => {
                self.mark_direct_sandbox_transfer_ambiguous(session_id, transfer_id);
                Err(BrainError::SandboxTransferAmbiguous)
            }
        }
    }

    pub async fn storage_copy_to_sandbox(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        idempotency_key: &str,
    ) -> Result<brain_protocol::hand::FileEntry> {
        crate::storage::validate_storage_key(&key)?;
        self.storage_copy_to_sandbox_admitted(
            session_id,
            key,
            path,
            generation,
            overwrite,
            idempotency_key,
        )
        .await
    }

    async fn storage_copy_to_sandbox_internal(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        operation_key: &str,
    ) -> Result<brain_protocol::hand::FileEntry> {
        crate::storage::validate_internal_storage_key(&key)?;
        self.storage_copy_to_sandbox_admitted(
            session_id,
            key,
            path,
            generation,
            overwrite,
            operation_key,
        )
        .await
    }

    async fn storage_copy_to_sandbox_admitted(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        operation_key: &str,
    ) -> Result<brain_protocol::hand::FileEntry> {
        let operation_id = sandbox_file_effect_id(session_id, operation_key, "storage-to-sandbox")?;
        let path = normalize_workspace_path(&path)?;
        self.deliver(session_id, |reply| Command::CopyStorageToDefaultSandbox {
            operation_id: operation_id.clone(),
            generation: generation.clone(),
            key: key.clone(),
            path: path.clone(),
            overwrite,
            reply,
        })
        .await?
    }

    pub async fn storage_copy_from_sandbox(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        idempotency_key: &str,
    ) -> Result<crate::storage::StorageObject> {
        crate::storage::validate_storage_key(&key)?;
        self.storage_copy_from_sandbox_admitted(
            session_id,
            key,
            path,
            generation,
            overwrite,
            idempotency_key,
        )
        .await
    }

    async fn storage_copy_from_sandbox_internal(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        path: String,
        generation: String,
        operation_key: &str,
    ) -> Result<crate::storage::StorageObject> {
        crate::storage::validate_internal_storage_key(&key)?;
        self.storage_copy_from_sandbox_admitted(
            session_id,
            key,
            path,
            generation,
            false,
            operation_key,
        )
        .await
    }

    async fn storage_copy_from_sandbox_admitted(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        operation_key: &str,
    ) -> Result<crate::storage::StorageObject> {
        let operation_id = sandbox_file_effect_id(session_id, operation_key, "sandbox-to-storage")?;
        let path = normalize_workspace_path(&path)?;
        self.deliver(session_id, |reply| Command::CopyDefaultSandboxToStorage {
            operation_id: operation_id.clone(),
            generation: generation.clone(),
            key: key.clone(),
            path: path.clone(),
            overwrite,
            reply,
        })
        .await?
    }

    pub async fn create_child(
        self: &Arc<Self>,
        parent_id: &str,
        prompt: String,
        name: Option<String>,
        fork_turns: Option<String>,
        idempotency_key: Option<&str>,
    ) -> Result<session::Session> {
        if prompt.is_empty() {
            return Err(BrainError::Invalid("child prompt must not be empty".into()));
        }
        if idempotency_key.is_some_and(|key| key.is_empty() || key.len() > 128) {
            return Err(BrainError::Invalid(
                "Idempotency-Key must contain 1 to 128 bytes".into(),
            ));
        }
        if name
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.len() > 128)
        {
            return Err(BrainError::Invalid(
                "child name must contain 1 to 128 bytes".into(),
            ));
        }
        let fork_turns = ForkTurns::parse(fork_turns.as_deref())?;
        self.deliver(parent_id, |reply| Command::CreateChild {
            prompt: prompt.clone(),
            name: name.clone(),
            fork_turns: fork_turns.clone(),
            idempotency_key: idempotency_key.map(str::to_owned),
            reply,
        })
        .await?
    }

    pub async fn list_children(
        &self,
        parent_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<session::Session>, Option<String>)> {
        let page = self
            .journal
            .list_child_page(&crate::journal::ChildListQuery {
                parent_id,
                limit: limit.clamp(1, 100),
                cursor,
            })
            .await?;
        Ok((
            page.sessions.iter().map(session_doc_summary).collect(),
            page.next_cursor,
        ))
    }

    pub async fn get_child(&self, parent_id: &str, child_id: &str) -> Result<session::Session> {
        let head = self.journal.get_head(child_id).await?;
        if head.doc.parent_id.as_deref() != Some(parent_id) {
            return Err(BrainError::NoSuchSession(child_id.to_owned()));
        }
        session_doc(child_id, &head.doc)
    }

    pub async fn wait_child(
        &self,
        parent_id: &str,
        child_id: &str,
        timeout: Duration,
    ) -> Result<session::Session> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let child = self.get_child(parent_id, child_id).await?;
            if child.current_turn.is_none()
                || matches!(
                    child.state,
                    session::SessionState::Ended
                        | session::SessionState::Deleting
                        | session::SessionState::Deleted
                        | session::SessionState::Failed
                )
                || tokio::time::Instant::now() >= deadline
            {
                return Ok(child);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Engine-owned model facade over the same ordinary child-session resources used by the
    /// public API. Model-visible names never select an implementation; the sealed capability ID
    /// reaches this method only after the ToolCall intent is durable.
    pub(crate) async fn execute_child_capability(
        self: &Arc<Self>,
        parent_id: &str,
        operation_id: &str,
        input: serde_json::Value,
        cancel: CancellationToken,
    ) -> CallOutcome {
        let started = std::time::Instant::now();
        let action = input.get("action").and_then(serde_json::Value::as_str);
        let result: Result<serde_json::Value> = async {
            if cancel.is_cancelled() {
                return Err(BrainError::Cancelled);
            }
            match action {
                Some("spawn_agent") => {
                    let task_name = required_child_string(&input, "task_name")?;
                    let message = required_child_string(&input, "message")?;
                    let fork_turns = input
                        .get("fork_turns")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    let child = self
                        .create_child(
                            parent_id,
                            message,
                            Some(task_name),
                            fork_turns,
                            Some(operation_id),
                        )
                        .await?;
                    Ok(serde_json::to_value(child)?)
                }
                Some("send_message" | "follow_up") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    let message = required_child_string(&input, "message")?;
                    self.get_child(parent_id, &child_id).await?;
                    let content =
                        MessageRequestContent::String(message.parse().map_err(|error| {
                            BrainError::Invalid(format!("child message: {error}"))
                        })?);
                    let (turn_id, seq) = self
                        .message_with_metadata_idempotent(
                            &child_id,
                            content,
                            HashMap::new(),
                            Some(operation_id),
                        )
                        .await?;
                    Ok(serde_json::json!({
                        "child_id": child_id,
                        "turn_id": turn_id,
                        "seq": seq,
                    }))
                }
                Some("peek") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    Ok(serde_json::to_value(
                        self.get_child(parent_id, &child_id).await?,
                    )?)
                }
                Some("wait") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    let timeout = input
                        .get("timeout_ms")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(30_000)
                        .min(300_000);
                    let wait =
                        self.wait_child(parent_id, &child_id, Duration::from_millis(timeout));
                    let child = tokio::select! {
                        result = wait => result?,
                        _ = cancel.cancelled() => return Err(BrainError::Cancelled),
                    };
                    Ok(serde_json::to_value(child)?)
                }
                Some("list_children") => {
                    let cursor = input.get("cursor").and_then(serde_json::Value::as_str);
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(20)
                        .min(100) as usize;
                    let (data, next_cursor) = self.list_children(parent_id, cursor, limit).await?;
                    Ok(serde_json::json!({
                        "has_more": next_cursor.is_some(),
                        "next_cursor": next_cursor,
                        "data": data,
                    }))
                }
                Some("interrupt_agent") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    self.get_child(parent_id, &child_id).await?;
                    Ok(serde_json::to_value(self.cancel(&child_id).await?)?)
                }
                Some("end_agent") => {
                    let child_id = required_child_string(&input, "child_id")?;
                    self.get_child(parent_id, &child_id).await?;
                    Ok(serde_json::to_value(self.end(&child_id).await?)?)
                }
                Some(other) => Err(BrainError::Invalid(format!(
                    "unknown subagents action {other:?}"
                ))),
                None => Err(BrainError::Invalid("subagents action is required".into())),
            }
        }
        .await;
        match result {
            Ok(value) => CallOutcome {
                outcome: "completed".into(),
                content: serde_json::to_string(&value)
                    .unwrap_or_else(|_| "child operation completed".into()),
                value: Some(value),
                is_error: false,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                truncated: false,
                terminal: None,
            },
            Err(BrainError::Cancelled) => CallOutcome {
                outcome: "cancelled".into(),
                content: "child operation cancelled".into(),
                value: None,
                is_error: true,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                truncated: false,
                terminal: None,
            },
            Err(error) => {
                let mut outcome = CallOutcome::failed(error.to_string());
                outcome.duration_ms = started.elapsed().as_millis() as u64;
                outcome
            }
        }
    }

    /// Engine-owned durable-storage facade. The caller passes the turn's already-claimed mutable
    /// state so storage reservations and their ToolCall live under one fence; this must never
    /// recurse through the session actor while that same turn is running.
    pub(crate) async fn execute_storage_capability(
        self: &Arc<Self>,
        session_id: &str,
        operation_id: &str,
        input: serde_json::Value,
        cancel: CancellationToken,
        st: &mut TurnState,
    ) -> Result<CallOutcome> {
        let started = std::time::Instant::now();
        let operation = async {
            let action = input
                .get("action")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| BrainError::Invalid("storage action is required".into()))?;
            match action {
                "list" => {
                    let prefix = optional_bounded_string(&input, "prefix", 1024)?;
                    if let Some(prefix) = &prefix {
                        validate_storage_prefix(prefix)?;
                    }
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(20);
                    if !(1..=100).contains(&limit) {
                        return Err(BrainError::Invalid(
                            "storage list limit must be between 1 and 100".into(),
                        ));
                    }
                    ensure_storage_readable(&st.head, session_id)?;
                    let page = self
                        .storage_port()?
                        .list(
                            session_id,
                            prefix.as_deref(),
                            cursor.as_deref(),
                            limit as u32,
                        )
                        .await?;
                    Ok(serde_json::to_value(page)?)
                }
                "save" => {
                    let key = required_bounded_string(&input, "key", 1024, "storage")?;
                    crate::storage::validate_storage_key(&key)?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let source = input.get("source").ok_or_else(|| {
                        BrainError::Invalid("storage save source is required".into())
                    })?;
                    match source.get("kind").and_then(serde_json::Value::as_str) {
                        Some("inline_text") => {
                            let text = required_bounded_string(
                                source,
                                "text",
                                brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                                "storage save source",
                            )?;
                            if text.len() > brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES {
                                return Err(BrainError::FileTooLarge {
                                    limit: brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                                });
                            }
                            let object = write_storage_inline_state(
                                self,
                                session_id,
                                st,
                                key,
                                base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
                                Some("text/plain; charset=utf-8".into()),
                                overwrite,
                            )
                            .await?;
                            Ok(serde_json::to_value(object)?)
                        }
                        Some("sandbox_path") => {
                            let path = required_bounded_string(
                                source,
                                "path",
                                4096,
                                "storage save source",
                            )?;
                            let generation =
                                required_identifier(source, "generation", "storage save source")?;
                            let target = default_sandbox_target(&st.head.root_id)?;
                            let file_request = sandbox_file_request(&target, &generation, &path)?;
                            let files = self.sandbox_files.as_ref().ok_or_else(|| {
                                BrainError::Invalid("sandbox files are unavailable".into())
                            })?;
                            let entry = files
                                .stat(file_request)
                                .await
                                .map_err(map_hand_port_error)?;
                            if entry.kind != brain_protocol::hand::FileEntryKind::File {
                                return Err(BrainError::Invalid(
                                    "storage save source must be a regular file".into(),
                                ));
                            }
                            let ticket = prepare_storage_upload_state(
                                self,
                                session_id,
                                st,
                                crate::storage::StorageUploadIntent {
                                    key,
                                    bytes: entry.bytes,
                                    sha256: None,
                                    content_type: None,
                                    overwrite,
                                },
                            )
                            .await?;
                            let copy = sandbox_copy_request(
                                operation_id,
                                &target,
                                &generation,
                                &path,
                                None,
                                &ticket,
                                "export",
                                false,
                            )?;
                            let expected_digest = copy.request_digest.clone();
                            let result = files.transfer(copy).await.map_err(map_hand_port_error)?;
                            validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                            let exported = result.object.as_ref().ok_or_else(|| {
                                BrainError::Hand(
                                    "sandbox export omitted its uploaded object identity".into(),
                                )
                            })?;
                            if exported.object_id.as_str() != ticket.object_id
                                || exported.bytes != entry.bytes
                            {
                                return Err(BrainError::Hand(
                                    "sandbox export returned a different object identity".into(),
                                ));
                            }
                            let object = complete_storage_upload_state(
                                self,
                                session_id,
                                st,
                                ticket.transfer_id,
                            )
                            .await?;
                            if object.bytes != exported.bytes
                                || object.sha256 != exported.sha256.as_str()
                            {
                                return Err(BrainError::Journal(
                                    "published storage object differs from the sandbox export"
                                        .into(),
                                ));
                            }
                            Ok(serde_json::to_value(object)?)
                        }
                        Some(other) => Err(BrainError::Invalid(format!(
                            "unknown storage save source kind {other:?}"
                        ))),
                        None => Err(BrainError::Invalid(
                            "storage save source kind is required".into(),
                        )),
                    }
                }
                "load" => {
                    let key = required_bounded_string(&input, "key", 1024, "storage")?;
                    crate::storage::validate_storage_key(&key)?;
                    let path = required_bounded_string(&input, "path", 4096, "storage")?;
                    let generation = required_identifier(&input, "generation", "storage")?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    ensure_storage_readable(&st.head, session_id)?;
                    let storage = self.storage_port()?;
                    let object = storage.stat(session_id, &key).await?;
                    let ticket = storage.prepare_download(session_id, &key).await?;
                    if ticket.max_bytes != object.bytes {
                        return Err(BrainError::Journal(
                            "download authority does not match the stored object size".into(),
                        ));
                    }
                    let reference = storage_object_reference(
                        &ticket.object_id,
                        object.bytes,
                        &object.sha256,
                        object.content_type.as_deref(),
                    )?;
                    let target = default_sandbox_target(&st.head.root_id)?;
                    let copy = sandbox_copy_request(
                        operation_id,
                        &target,
                        &generation,
                        &path,
                        Some(reference),
                        &ticket,
                        "import",
                        overwrite,
                    )?;
                    let expected_digest = copy.request_digest.clone();
                    let result = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .transfer(copy)
                        .await
                        .map_err(map_hand_port_error)?;
                    validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                    if result.object.is_some() {
                        return Err(BrainError::Hand(
                            "sandbox import returned an unexpected object identity".into(),
                        ));
                    }
                    Ok(serde_json::to_value(result.file)?)
                }
                other => Err(BrainError::Invalid(format!(
                    "unknown storage action {other:?}"
                ))),
            }
        };
        let result = tokio::select! {
            result = operation => result,
            _ = cancel.cancelled() => Err(BrainError::Cancelled),
        };
        engine_outcome(result, started)
    }

    async fn persist_additional_sandbox_status(
        &self,
        current: &crate::journal::SandboxInventoryDoc,
        status: brain_protocol::hand::SandboxStatus,
    ) -> Result<crate::journal::SandboxInventoryDoc> {
        use brain_protocol::hand::SandboxState;

        if !sandbox_status_matches_target(&status, &current.status.target)? {
            return Err(BrainError::Hand(
                "Hand returned a lifecycle receipt for a different sandbox target".into(),
            ));
        }
        if matches!(
            status.state,
            SandboxState::Creating | SandboxState::Running | SandboxState::Suspended
        ) && status
            .generation
            .as_ref()
            .map(|generation| generation.as_str())
            != Some(current.generation_intent.as_str())
        {
            return Err(BrainError::Hand(
                "Hand returned a different additional-sandbox generation".into(),
            ));
        }
        if matches!(
            status.state,
            SandboxState::Running | SandboxState::Suspended
        ) && (status.target_ref.is_none() || status.expires_at_ms.is_none())
        {
            return Err(BrainError::Hand(
                "live sandbox receipt lacks target_ref or hard expiry".into(),
            ));
        }
        self.journal
            .update_sandbox(&crate::journal::SandboxUpdateRequest {
                root_id: current.root_id.clone(),
                sandbox_id: current.sandbox_id.clone(),
                expected_version: current.version,
                release_slot: sandbox_status_releases_slot(&status),
                status,
                now_ms: crate::wall_ms(),
            })
            .await
    }

    async fn inspect_additional_sandbox(
        &self,
        current: crate::journal::SandboxInventoryDoc,
    ) -> Result<crate::journal::SandboxInventoryDoc> {
        if sandbox_status_releases_slot(&current.status) {
            return Ok(current);
        }
        let control = self.sandbox_control.as_ref().ok_or_else(|| {
            BrainError::Invalid("additional sandbox control is unavailable".into())
        })?;
        let status = match control.inspect(current.status.target.clone()).await {
            Ok(status) => status,
            Err(error) if error.code == brain_protocol::hand::HandErrorCode::SandboxGone => {
                sandbox_gone_status(&current.status, "hand_reported_gone")?
            }
            Err(error) => return Err(map_hand_port_error(error)),
        };
        self.persist_additional_sandbox_status(&current, status)
            .await
    }

    async fn additional_sandbox_for_action(
        &self,
        root_id: &str,
        input: &serde_json::Value,
        require_generation: bool,
    ) -> Result<(crate::journal::SandboxInventoryDoc, Option<String>)> {
        let sandbox_id = required_identifier(input, "sandbox_id", "sandbox")?;
        let mut item = self.journal.get_sandbox(root_id, &sandbox_id).await?;
        if item.root_id != root_id {
            return Err(BrainError::FileNotFound(format!("sandbox {sandbox_id}")));
        }
        let generation = if require_generation {
            Some(required_identifier(input, "generation", "sandbox")?)
        } else {
            None
        };
        if let Some(expected) = &generation {
            let observed = item
                .status
                .generation
                .as_ref()
                .map(|generation| generation.as_str());
            if observed != Some(expected.as_str()) {
                return Err(if sandbox_status_releases_slot(&item.status) {
                    BrainError::SandboxGone
                } else {
                    BrainError::SandboxGenerationConflict
                });
            }
        }
        if sandbox_status_releases_slot(&item.status) {
            return Err(BrainError::SandboxGone);
        }
        if item
            .status
            .expires_at_ms
            .is_some_and(|expiry| expiry.get() <= crate::wall_ms())
        {
            item = self.inspect_additional_sandbox(item).await?;
            if sandbox_status_releases_slot(&item.status) {
                return Err(BrainError::SandboxGone);
            }
        }
        if !matches!(
            item.status.state,
            brain_protocol::hand::SandboxState::Running
                | brain_protocol::hand::SandboxState::Suspended
        ) {
            return Err(BrainError::HandUnavailable(
                "additional sandbox has not reached a live state".into(),
            ));
        }
        Ok((item, generation))
    }

    /// The closed official `brain.sandbox` capability. Logical inventory, authorization and
    /// quota live in Brain; Hand receives only the exact typed target selected from that durable
    /// inventory. No action can switch on a model-visible Tool name or fabricate a physical id.
    pub(crate) async fn execute_sandbox_capability(
        self: &Arc<Self>,
        session_id: &str,
        operation_id: &str,
        input: serde_json::Value,
        cancel: CancellationToken,
        st: &mut TurnState,
    ) -> Result<CallOutcome> {
        let started = std::time::Instant::now();
        let operation = async {
            let action = input
                .get("action")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| BrainError::Invalid("sandbox action is required".into()))?;
            let root_id = st.head.root_id.clone();
            match action {
                "create" => {
                    let (sandbox_id, generation, target) =
                        additional_sandbox_identity(&root_id, session_id, operation_id)?;
                    let request_digest =
                        sandbox_request_digest(&root_id, session_id, operation_id, &input)?;
                    let now = crate::wall_ms();
                    let creating: brain_protocol::hand::SandboxStatus =
                        serde_json::from_value(serde_json::json!({
                            "state": "creating",
                            "target": target,
                            "generation": generation,
                            "changed_at_ms": now,
                            "expires_at_ms": null,
                        }))?;
                    let reserved = self
                        .journal
                        .reserve_sandbox(&crate::journal::SandboxReserveRequest {
                            root_id: root_id.clone(),
                            owner_session_id: session_id.to_owned(),
                            sandbox_id: sandbox_id.clone(),
                            operation_id: operation_id.to_owned(),
                            request_digest,
                            generation_intent: generation.clone(),
                            initial_status: creating,
                            now_ms: now,
                        })
                        .await?;
                    if sandbox_status_releases_slot(&reserved.status)
                        || matches!(
                            reserved.status.state,
                            brain_protocol::hand::SandboxState::Running
                                | brain_protocol::hand::SandboxState::Suspended
                        )
                    {
                        return Ok(serde_json::json!({
                            "sandbox_id": sandbox_id,
                            "status": reserved.status,
                        }));
                    }
                    let control = self.sandbox_control.as_ref().ok_or_else(|| {
                        BrainError::Invalid("additional sandbox control is unavailable".into())
                    })?;
                    let request = sandbox_create_request(
                        &st.head,
                        reserved.status.target.clone(),
                        &reserved.generation_intent,
                    )?;
                    let status = match control.create(request).await {
                        Ok(status) => status,
                        Err(error)
                            if error.code == brain_protocol::hand::HandErrorCode::SandboxGone =>
                        {
                            sandbox_gone_status(&reserved.status, "hand_reported_gone")?
                        }
                        Err(error) => return Err(map_hand_port_error(error)),
                    };
                    let persisted = self
                        .persist_additional_sandbox_status(&reserved, status)
                        .await?;
                    Ok(serde_json::json!({
                        "sandbox_id": sandbox_id,
                        "status": persisted.status,
                    }))
                }
                "list" => {
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(20);
                    if !(1..=100).contains(&limit) {
                        return Err(BrainError::Invalid(
                            "sandbox list limit must be between 1 and 100".into(),
                        ));
                    }
                    let page = self
                        .journal
                        .list_sandbox_page(&crate::journal::SandboxListQuery {
                            root_id: &root_id,
                            limit: limit as usize,
                            cursor: cursor.as_deref(),
                        })
                        .await?;
                    let data = page
                        .sandboxes
                        .into_iter()
                        .map(|item| {
                            serde_json::json!({
                                "sandbox_id": item.sandbox_id,
                                "owner_session_id": item.owner_session_id,
                                "status": item.status,
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(serde_json::json!({
                        "has_more": page.next_cursor.is_some(),
                        "next_cursor": page.next_cursor,
                        "data": data,
                    }))
                }
                "status" => {
                    let sandbox_id = required_identifier(&input, "sandbox_id", "sandbox")?;
                    let item = self.journal.get_sandbox(&root_id, &sandbox_id).await?;
                    let item = self.inspect_additional_sandbox(item).await?;
                    Ok(serde_json::json!({
                        "sandbox_id": item.sandbox_id,
                        "owner_session_id": item.owner_session_id,
                        "status": item.status,
                    }))
                }
                "exec" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let generation = generation.expect("required above");
                    let expected_target_ref = item
                        .status
                        .target_ref
                        .as_ref()
                        .map(|value| value.as_str().to_owned())
                        .ok_or_else(|| {
                            BrainError::Hand(
                                "live sandbox status is missing its target reference".into(),
                            )
                        })?;
                    let command = required_bounded_string(&input, "command", 131_072, "sandbox")?;
                    let cwd = optional_bounded_string(&input, "cwd", 4096)?;
                    let interactive = input
                        .get("interactive")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let execution_digest = hex::encode(Sha256::digest(
                        format!("aex.sandbox-execution\0{root_id}\0{operation_id}").as_bytes(),
                    ));
                    let execution_id = format!("exe_{}", &execution_digest[..24]);
                    let mut request: brain_protocol::hand::SandboxExecutionRequest =
                        serde_json::from_value(serde_json::json!({
                            "target": item.status.target,
                            "expected_generation": generation,
                            "execution_id": execution_id,
                            "request_digest": "0".repeat(64),
                            "input": {
                                "command": command,
                                "cwd": cwd,
                                "interactive": interactive,
                            },
                            "resources": {
                                "timeout_ms": 600_000,
                                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                            },
                            "network": sealed_sandbox_network(&st.head)?,
                        }))?;
                    request.request_digest =
                        brain_protocol::contract::sandbox_execution_request_digest(&request);
                    let expected_digest = String::from(request.request_digest.clone());
                    let receipt = self
                        .sandbox_control
                        .as_ref()
                        .ok_or_else(|| {
                            BrainError::Invalid("additional sandbox control is unavailable".into())
                        })?
                        .execute(request)
                        .await
                        .map_err(map_hand_port_error)?;
                    if String::from(receipt.operation.operation_id.clone()) != execution_id
                        || String::from(receipt.operation.request_digest.clone()) != expected_digest
                        || serde_json::to_value(&receipt.operation.target)?
                            != serde_json::to_value(&item.status.target)?
                        || String::from(receipt.operation.generation.clone()) != generation
                        || String::from(receipt.operation.target_ref.clone()) != expected_target_ref
                        || serde_json::to_value(&receipt.observation.operation)?
                            != serde_json::to_value(&receipt.operation)?
                    {
                        return Err(BrainError::Hand(
                            "sandbox execution receipt identity mismatch".into(),
                        ));
                    }
                    if let Some(terminal) = &receipt.observation.terminal
                        && (brain_protocol::contract::terminal_result_digest(terminal)
                            != terminal.terminal_digest
                            || terminal.inline.as_ref().is_some_and(|value| {
                                !brain_protocol::contract::terminal_inline_fits(value)
                            }))
                    {
                        return Err(BrainError::Hand(
                            "sandbox execution terminal receipt is invalid or oversized".into(),
                        ));
                    }
                    Ok(serde_json::json!({
                        "execution_id": execution_id,
                        "state": receipt.observation.state,
                        "output": receipt.observation.output,
                        "terminal": receipt.observation.terminal,
                    }))
                }
                "write_stdin" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let generation = generation.expect("required above");
                    let expected_target_ref = item
                        .status
                        .target_ref
                        .as_ref()
                        .map(|value| value.as_str().to_owned())
                        .ok_or_else(|| {
                            BrainError::Hand(
                                "live sandbox status is missing its target reference".into(),
                            )
                        })?;
                    let eof = input
                        .get("eof")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let execution_id = required_identifier(&input, "execution_id", "sandbox")?;
                    let text = optional_bounded_string(&input, "text", 4096)?.unwrap_or_default();
                    if text.len() > 4096 {
                        return Err(BrainError::Invalid(
                            "sandbox write_stdin text exceeds 4096 UTF-8 bytes".into(),
                        ));
                    }
                    let mut request: brain_protocol::hand::WriteStdinRequest =
                        serde_json::from_value(serde_json::json!({
                            "operation_id": operation_id,
                            "request_digest": "0".repeat(64),
                            "target": item.status.target,
                            "expected_generation": generation,
                            "execution_id": execution_id,
                            "text": text,
                            "eof": eof,
                        }))?;
                    request.request_digest =
                        brain_protocol::contract::write_stdin_request_digest(&request);
                    let expected_digest = request.request_digest.clone();
                    let receipt = self
                        .sandbox_control
                        .as_ref()
                        .ok_or_else(|| {
                            BrainError::Invalid("additional sandbox control is unavailable".into())
                        })?
                        .write_stdin(request)
                        .await
                        .map_err(map_hand_port_error)?;
                    if String::from(receipt.operation_id.clone()) != operation_id
                        || receipt.request_digest != expected_digest
                        || String::from(receipt.observation.operation.operation_id.clone())
                            != execution_id
                        || serde_json::to_value(&receipt.observation.operation.target)?
                            != serde_json::to_value(&item.status.target)?
                        || String::from(receipt.observation.operation.generation.clone())
                            != generation
                        || String::from(receipt.observation.operation.target_ref.clone())
                            != expected_target_ref
                    {
                        return Err(BrainError::Hand(
                            "sandbox stdin receipt identity mismatch".into(),
                        ));
                    }
                    if let Some(terminal) = &receipt.observation.terminal
                        && (brain_protocol::contract::terminal_result_digest(terminal)
                            != terminal.terminal_digest
                            || terminal.inline.as_ref().is_some_and(|value| {
                                !brain_protocol::contract::terminal_inline_fits(value)
                            }))
                    {
                        return Err(BrainError::Hand(
                            "sandbox stdin terminal receipt is invalid or oversized".into(),
                        ));
                    }
                    Ok(serde_json::json!({
                        "accepted": receipt.accepted,
                        "replayed": receipt.replayed,
                        "state": receipt.observation.state,
                        "output": receipt.observation.output,
                        "terminal": receipt.observation.terminal,
                    }))
                }
                "list_files" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(50);
                    if !(1..=100).contains(&limit) {
                        return Err(BrainError::Invalid(
                            "sandbox file list limit must be between 1 and 100".into(),
                        ));
                    }
                    let page = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .list(crate::hand::SandboxFileListRequest {
                            target: item.status.target,
                            expected_generation: generation.expect("required above"),
                            path,
                            cursor,
                            limit: limit as u32,
                        })
                        .await
                        .map_err(map_hand_port_error)?;
                    Ok(serde_json::to_value(page)?)
                }
                "stat_file" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let entry = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .stat(sandbox_file_request(
                            &item.status.target,
                            &generation.expect("required above"),
                            &path,
                        )?)
                        .await
                        .map_err(map_hand_port_error)?;
                    Ok(serde_json::to_value(entry)?)
                }
                "read_file" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let content = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .read(sandbox_file_request(
                            &item.status.target,
                            &generation.expect("required above"),
                            &path,
                        )?)
                        .await
                        .map_err(map_hand_port_error)?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(&content.content_base64)
                        .map_err(|_| BrainError::Hand("sandbox returned invalid base64".into()))?;
                    if bytes.len() > brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES {
                        return Err(BrainError::FileTooLarge {
                            limit: brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                        });
                    }
                    let text = String::from_utf8(bytes).map_err(|_| {
                        BrainError::Invalid(
                            "sandbox read_file is model-inline UTF-8 only; use save for binary data"
                                .into(),
                        )
                    })?;
                    Ok(serde_json::json!({"entry": content.entry, "text": text}))
                }
                "write_file" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let text = required_bounded_string(
                        &input,
                        "text",
                        brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                        "sandbox",
                    )?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let request = sandbox_file_write_request(
                        operation_id,
                        &item.status.target,
                        &generation.expect("required above"),
                        &path,
                        text.as_bytes(),
                        overwrite,
                    )?;
                    let expected_digest = request.request_digest.clone();
                    let result = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .write(request)
                        .await
                        .map_err(map_hand_port_error)?;
                    validate_sandbox_file_write_result(&result, operation_id, &expected_digest)?;
                    Ok(serde_json::to_value(result.file)?)
                }
                "find_files" | "grep_files" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let field = if action == "find_files" {
                        "glob"
                    } else {
                        "query"
                    };
                    let expression = required_bounded_string(&input, field, 4096, "sandbox")?;
                    let cursor = optional_bounded_string(&input, "cursor", 4096)?;
                    let limit = input
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(50);
                    if !(1..=100).contains(&limit) {
                        return Err(BrainError::Invalid(
                            "sandbox search limit must be between 1 and 100".into(),
                        ));
                    }
                    let request = sandbox_search_request(
                        &item.status.target,
                        &generation.expect("required above"),
                        &path,
                        &expression,
                        cursor.as_deref(),
                        limit as u32,
                    )?;
                    let files = self.sandbox_files.as_ref().ok_or_else(|| {
                        BrainError::Invalid("sandbox files are unavailable".into())
                    })?;
                    let page = if action == "find_files" {
                        files.find(request).await
                    } else {
                        files.grep(request).await
                    }
                    .map_err(map_hand_port_error)?;
                    Ok(serde_json::to_value(page)?)
                }
                "load" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let key = required_bounded_string(&input, "key", 1024, "sandbox")?;
                    crate::storage::validate_storage_key(&key)?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    ensure_storage_readable(&st.head, session_id)?;
                    let storage = self.storage_port()?;
                    let object = storage.stat(session_id, &key).await?;
                    let ticket = storage.prepare_download(session_id, &key).await?;
                    if ticket.max_bytes != object.bytes {
                        return Err(BrainError::Journal(
                            "download authority does not match the stored object size".into(),
                        ));
                    }
                    let reference = storage_object_reference(
                        &ticket.object_id,
                        object.bytes,
                        &object.sha256,
                        object.content_type.as_deref(),
                    )?;
                    let copy = sandbox_copy_request(
                        operation_id,
                        &item.status.target,
                        &generation.expect("required above"),
                        &path,
                        Some(reference),
                        &ticket,
                        "import",
                        overwrite,
                    )?;
                    let expected_digest = copy.request_digest.clone();
                    let result = self
                        .sandbox_files
                        .as_ref()
                        .ok_or_else(|| BrainError::Invalid("sandbox files are unavailable".into()))?
                        .transfer(copy)
                        .await
                        .map_err(map_hand_port_error)?;
                    validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                    if result.object.is_some() {
                        return Err(BrainError::Hand(
                            "sandbox import returned an unexpected object identity".into(),
                        ));
                    }
                    Ok(serde_json::to_value(result.file)?)
                }
                "save" => {
                    let (item, generation) = self
                        .additional_sandbox_for_action(&root_id, &input, true)
                        .await?;
                    let key = required_bounded_string(&input, "key", 1024, "sandbox")?;
                    crate::storage::validate_storage_key(&key)?;
                    let path = required_bounded_string(&input, "path", 4096, "sandbox")?;
                    let overwrite = input
                        .get("overwrite")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let generation = generation.expect("required above");
                    let files = self.sandbox_files.as_ref().ok_or_else(|| {
                        BrainError::Invalid("sandbox files are unavailable".into())
                    })?;
                    let entry = files
                        .stat(sandbox_file_request(
                            &item.status.target,
                            &generation,
                            &path,
                        )?)
                        .await
                        .map_err(map_hand_port_error)?;
                    if entry.kind != brain_protocol::hand::FileEntryKind::File {
                        return Err(BrainError::Invalid(
                            "sandbox save source must be a regular file".into(),
                        ));
                    }
                    let ticket = prepare_storage_upload_state(
                        self,
                        session_id,
                        st,
                        crate::storage::StorageUploadIntent {
                            key,
                            bytes: entry.bytes,
                            sha256: None,
                            content_type: None,
                            overwrite,
                        },
                    )
                    .await?;
                    let copy = sandbox_copy_request(
                        operation_id,
                        &item.status.target,
                        &generation,
                        &path,
                        None,
                        &ticket,
                        "export",
                        false,
                    )?;
                    let expected_digest = copy.request_digest.clone();
                    let result = files.transfer(copy).await.map_err(map_hand_port_error)?;
                    validate_sandbox_copy_result(&result, operation_id, &expected_digest)?;
                    let exported = result.object.as_ref().ok_or_else(|| {
                        BrainError::Hand("sandbox export omitted its object identity".into())
                    })?;
                    if exported.object_id.as_str() != ticket.object_id
                        || exported.bytes != entry.bytes
                    {
                        return Err(BrainError::Hand(
                            "sandbox export returned a different object identity".into(),
                        ));
                    }
                    let object =
                        complete_storage_upload_state(self, session_id, st, ticket.transfer_id)
                            .await?;
                    if object.bytes != exported.bytes || object.sha256 != exported.sha256.as_str() {
                        return Err(BrainError::Journal(
                            "published storage object differs from the sandbox export".into(),
                        ));
                    }
                    Ok(serde_json::to_value(object)?)
                }
                "terminate" => {
                    let sandbox_id = required_identifier(&input, "sandbox_id", "sandbox")?;
                    let current = self.journal.get_sandbox(&root_id, &sandbox_id).await?;
                    if sandbox_status_releases_slot(&current.status) {
                        return Ok(serde_json::json!({
                            "sandbox_id": sandbox_id,
                            "status": current.status,
                        }));
                    }
                    let status = match self
                        .sandbox_control
                        .as_ref()
                        .ok_or_else(|| {
                            BrainError::Invalid("additional sandbox control is unavailable".into())
                        })?
                        .terminate(current.status.target.clone())
                        .await
                    {
                        Ok(status) => status,
                        Err(error)
                            if error.code == brain_protocol::hand::HandErrorCode::SandboxGone =>
                        {
                            sandbox_gone_status(&current.status, "hand_reported_gone")?
                        }
                        Err(error) => return Err(map_hand_port_error(error)),
                    };
                    if !sandbox_status_releases_slot(&status) {
                        return Err(BrainError::Hand(
                            "sandbox termination did not return a confirmed terminal state".into(),
                        ));
                    }
                    let persisted = self
                        .persist_additional_sandbox_status(&current, status)
                        .await?;
                    Ok(serde_json::json!({
                        "sandbox_id": sandbox_id,
                        "status": persisted.status,
                    }))
                }
                other => Err(BrainError::Invalid(format!(
                    "unknown sandbox action {other:?}"
                ))),
            }
        };
        let result = tokio::select! {
            result = operation => result,
            _ = cancel.cancelled() => Err(BrainError::Cancelled),
        };
        engine_outcome(result, started)
    }

    pub async fn delete(self: &Arc<Self>, session_id: &str) -> Result<()> {
        self.deliver(session_id, |reply| Command::Delete {
            queued: false,
            reply,
        })
        .await?
    }

    /// Durably fence the session into `deleting`, then return while the actor performs the same
    /// idempotent cleanup as strict deletion. Recovery discovery resumes it after a crash.
    pub async fn queue_delete(self: &Arc<Self>, session_id: &str) -> Result<()> {
        if self
            .journal
            .get_deletion_status(session_id)
            .await?
            .is_some_and(|status| status.state == "succeeded")
        {
            return Ok(());
        }
        let result = self
            .deliver(session_id, |reply| Command::Delete {
                queued: true,
                reply,
            })
            .await?;
        match result {
            Ok(()) => Ok(()),
            Err(BrainError::NoSuchSession(_))
                if self
                    .journal
                    .get_deletion_status(session_id)
                    .await?
                    .is_some() =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub async fn deletion_status(&self, session_id: &str) -> Result<DeletionStatusDoc> {
        self.journal
            .get_deletion_status(session_id)
            .await?
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))
    }

    fn reserve_direct_sandbox_transfer(
        &self,
        transfer_id: &str,
        transfer: DirectSandboxTransfer,
    ) -> Result<()> {
        let mut transfers = self
            .direct_sandbox_transfers
            .lock()
            .expect("direct sandbox transfers");
        if transfers.contains_key(transfer_id) || transfers.len() >= MAX_PENDING_SANDBOX_TRANSFERS {
            return Err(BrainError::Overloaded);
        }
        let mut process_bytes = 0_u64;
        let mut session_bytes = 0_u64;
        let mut session_count = 0_usize;
        for existing in transfers.values() {
            process_bytes = process_bytes
                .checked_add(existing.declared_bytes)
                .ok_or(BrainError::Overloaded)?;
            if existing.session_id == transfer.session_id {
                session_count += 1;
                session_bytes = session_bytes
                    .checked_add(existing.declared_bytes)
                    .ok_or(BrainError::Overloaded)?;
            }
        }
        if session_count >= MAX_PENDING_SANDBOX_TRANSFERS_PER_SESSION
            || process_bytes
                .checked_add(transfer.declared_bytes)
                .is_none_or(|bytes| bytes > MAX_PENDING_SANDBOX_TRANSFER_BYTES)
            || session_bytes
                .checked_add(transfer.declared_bytes)
                .is_none_or(|bytes| bytes > MAX_PENDING_SANDBOX_TRANSFER_BYTES_PER_SESSION)
        {
            return Err(BrainError::Overloaded);
        }
        transfers.insert(transfer_id.to_owned(), transfer);
        Ok(())
    }

    fn mark_direct_sandbox_transfer_ambiguous(&self, session_id: &str, transfer_id: &str) {
        if let Some(transfer) = self
            .direct_sandbox_transfers
            .lock()
            .expect("direct sandbox transfers")
            .get_mut(transfer_id)
            .filter(|transfer| transfer.session_id == session_id)
        {
            transfer.state = DirectSandboxTransferState::Ambiguous;
        }
    }

    fn remove_direct_sandbox_transfer(
        &self,
        session_id: &str,
        transfer_id: &str,
    ) -> Option<DirectSandboxTransfer> {
        let mut transfers = self
            .direct_sandbox_transfers
            .lock()
            .expect("direct sandbox transfers");
        if transfers
            .get(transfer_id)
            .is_some_and(|transfer| transfer.session_id == session_id)
        {
            transfers.remove(transfer_id)
        } else {
            None
        }
    }

    fn schedule_direct_sandbox_transfer_cleanup(self: &Arc<Self>, transfer_id: String) {
        let brain = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                let Some(brain) = brain.upgrade() else {
                    return;
                };
                let cleanup_at_ms = brain
                    .direct_sandbox_transfers
                    .lock()
                    .expect("direct sandbox transfers")
                    .get(&transfer_id)
                    .map(|transfer| transfer.cleanup_at_ms);
                let Some(cleanup_at_ms) = cleanup_at_ms else {
                    return;
                };
                let remaining = cleanup_at_ms.saturating_sub(crate::wall_ms());
                if remaining > 0 {
                    drop(brain);
                    tokio::time::sleep(Duration::from_millis(remaining)).await;
                    continue;
                }
                let transfer = brain
                    .direct_sandbox_transfers
                    .lock()
                    .expect("direct sandbox transfers")
                    .remove(&transfer_id);
                if let Some(transfer) = transfer {
                    brain.cleanup_direct_sandbox_transfer(transfer).await;
                }
                return;
            }
        });
    }

    fn spawn_direct_sandbox_transfer_cleanup(self: &Arc<Self>, transfer: DirectSandboxTransfer) {
        let brain = Arc::clone(self);
        tokio::spawn(async move {
            brain.cleanup_direct_sandbox_transfer(transfer).await;
        });
    }

    async fn cleanup_direct_sandbox_transfer(self: &Arc<Self>, transfer: DirectSandboxTransfer) {
        // Reconciliation aborts an expired uncompleted upload and releases its durable byte
        // reservation. A completed hidden object is then removed through the normal journaled
        // delete path. Either failure remains quota-bounded and root deletion is exhaustive.
        let _ = self.storage_reconcile(&transfer.session_id).await;
        let _ = self
            .storage_delete_internal(&transfer.session_id, transfer.storage_key)
            .await;
    }

    fn storage_port(&self) -> Result<&Arc<dyn crate::storage::SessionStoragePort>> {
        self.session_storage.as_ref().ok_or_else(|| {
            BrainError::Invalid("session storage is unavailable in this composition".into())
        })
    }

    pub async fn storage_list(
        &self,
        session_id: &str,
        prefix: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::storage::StoragePage> {
        if let Some(prefix) = prefix
            && !prefix.is_empty()
        {
            validate_storage_prefix(prefix)?;
            let normalized = prefix.trim_end_matches('/');
            if !normalized.is_empty() {
                crate::storage::validate_storage_key(normalized)?;
            }
        }
        let head = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&head.doc, session_id)?;
        let mut page = self
            .storage_port()?
            .list(session_id, prefix, cursor, limit.clamp(1, 100))
            .await?;
        // Adapters also filter this namespace so opaque/local cursors cannot reveal it. Keep the
        // core fence as defense in depth for custom adapters.
        page.objects
            .retain(|object| !crate::storage::is_internal_storage_key(&object.key));
        Ok(page)
    }

    pub async fn storage_stat(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<crate::storage::StorageObject> {
        crate::storage::validate_storage_key(key)?;
        let head = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&head.doc, session_id)?;
        self.storage_port()?.stat(session_id, key).await
    }

    pub async fn storage_read_inline(
        &self,
        session_id: &str,
        key: &str,
        max_bytes: u64,
    ) -> Result<(crate::storage::StorageObject, Vec<u8>)> {
        let max_bytes = max_bytes.min(1024 * 1024);
        let object = self.storage_stat(session_id, key).await?;
        if object.bytes > max_bytes {
            return Err(BrainError::FileTooLarge {
                limit: max_bytes as usize,
            });
        }
        let bytes = self
            .storage_port()?
            .read(session_id, key, max_bytes)
            .await?;
        Ok((object, bytes))
    }

    pub async fn storage_prepare_download(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<crate::storage::StorageTransferTicket> {
        crate::storage::validate_storage_key(key)?;
        let head = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&head.doc, session_id)?;
        self.storage_port()?.prepare_download(session_id, key).await
    }

    async fn storage_prepare_download_internal(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<crate::storage::StorageTransferTicket> {
        crate::storage::validate_internal_storage_key(key)?;
        let head = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&head.doc, session_id)?;
        self.storage_port()?.prepare_download(session_id, key).await
    }

    pub async fn storage_prepare_upload(
        self: &Arc<Self>,
        session_id: &str,
        request: crate::storage::StorageUploadIntent,
    ) -> Result<crate::storage::StorageTransferTicket> {
        crate::storage::validate_storage_key(&request.key)?;
        self.deliver(session_id, |reply| Command::PrepareStorageUpload {
            request: request.clone(),
            reply,
        })
        .await?
    }

    async fn storage_prepare_upload_internal(
        self: &Arc<Self>,
        session_id: &str,
        request: crate::storage::StorageUploadIntent,
    ) -> Result<crate::storage::StorageTransferTicket> {
        crate::storage::validate_internal_storage_key(&request.key)?;
        self.deliver(session_id, |reply| Command::PrepareStorageUpload {
            request: request.clone(),
            reply,
        })
        .await?
    }

    pub async fn storage_complete_upload(
        self: &Arc<Self>,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<crate::storage::StorageObject> {
        let head = self.journal.get_head(session_id).await?;
        if head.doc.storage_upload.as_ref().is_some_and(|upload| {
            upload.transfer_id == transfer_id
                && crate::storage::is_internal_storage_key(&upload.key)
        }) {
            return Err(BrainError::Invalid(
                "sandbox transfer staging is not a public storage upload".into(),
            ));
        }
        self.deliver(session_id, |reply| Command::CompleteStorageUpload {
            transfer_id: transfer_id.to_owned(),
            reply,
        })
        .await?
    }

    async fn storage_complete_upload_internal(
        self: &Arc<Self>,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<crate::storage::StorageObject> {
        self.deliver(session_id, |reply| Command::CompleteStorageUpload {
            transfer_id: transfer_id.to_owned(),
            reply,
        })
        .await?
    }

    /// Operator/sweeper hook. Hosted composition calls this for discovered sessions; the actor
    /// also schedules the persisted deadline while resident. This makes expiry cleanup retryable
    /// after a process restart without putting storage policy in the product facade.
    pub async fn storage_reconcile(self: &Arc<Self>, session_id: &str) -> Result<()> {
        self.deliver(session_id, |reply| Command::ReconcileStorage { reply })
            .await?
    }

    pub async fn storage_write_inline(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
        content_base64: String,
        content_type: Option<String>,
        overwrite: bool,
    ) -> Result<crate::storage::StorageObject> {
        self.deliver(session_id, |reply| Command::WriteStorageInline {
            key: key.clone(),
            content_base64: content_base64.clone(),
            content_type: content_type.clone(),
            overwrite,
            reply,
        })
        .await?
    }

    pub async fn storage_delete(self: &Arc<Self>, session_id: &str, key: String) -> Result<()> {
        crate::storage::validate_storage_key(&key)?;
        self.deliver(session_id, |reply| Command::DeleteStorageObject {
            key: key.clone(),
            reply,
        })
        .await?
    }

    async fn storage_delete_internal(
        self: &Arc<Self>,
        session_id: &str,
        key: String,
    ) -> Result<()> {
        crate::storage::validate_internal_storage_key(&key)?;
        self.deliver(session_id, |reply| Command::DeleteStorageObject {
            key: key.clone(),
            reply,
        })
        .await?
    }

    /// GET never hydrates and never queues behind a potentially slow resident cleanup. HEAD is
    /// the authoritative strongly-consistent projection; actors commit every visible transition
    /// before acknowledging it, so an in-memory snapshot cannot be more authoritative.
    pub async fn get(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        let head = self.journal.get_head(session_id).await?;
        if head.doc.state == "deleted" {
            return Err(BrainError::NoSuchSession(session_id.into()));
        }
        session_doc(session_id, &head.doc)
    }

    pub async fn get_for(
        self: &Arc<Self>,
        principal: &TrustedPrincipal,
        session_id: &str,
    ) -> Result<session::Session> {
        self.authorize(principal, session_id).await?;
        self.get(session_id).await
    }

    pub async fn authorize(&self, principal: &TrustedPrincipal, session_id: &str) -> Result<()> {
        let head = self.journal.get_head(session_id).await?;
        if head.doc.tenant_id != principal.as_str() {
            // Do not disclose whether another tenant owns this identifier.
            return Err(BrainError::NoSuchSession(session_id.into()));
        }
        Ok(())
    }

    pub async fn list(self: &Arc<Self>, limit: usize) -> Result<Vec<session::Session>> {
        let heads = self.journal.list_sessions(limit).await?;
        heads
            .iter()
            .filter(|h| h.doc.state != "deleted")
            .map(|h| session_doc(&h.session_id, &h.doc))
            .collect()
    }

    pub async fn list_for(
        self: &Arc<Self>,
        principal: &TrustedPrincipal,
        state: Option<&str>,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<(Vec<session::Session>, Option<String>)> {
        let page = self
            .journal
            .list_session_page(&crate::journal::SessionListQuery {
                tenant_id: principal.as_str(),
                state,
                limit,
                cursor,
            })
            .await?;
        Ok((
            page.sessions.iter().map(session_doc_summary).collect(),
            page.next_cursor,
        ))
    }

    pub async fn head(&self, session_id: &str) -> Result<Head> {
        self.journal.get_head(session_id).await
    }
}

fn secret_delivery_error(
    code: brain_protocol::hand::HandErrorCode,
    retryable: bool,
    message: &str,
) -> brain_protocol::hand::HandError {
    serde_json::from_value(serde_json::json!({
        "code": code,
        "details": {},
        "message": message,
        "retryable": retryable,
    }))
    .expect("static secret-delivery Hand errors satisfy the contract")
}

#[async_trait::async_trait]
impl crate::hand::SecretDeliveryPort for Brain {
    async fn redeem(
        &self,
        request: brain_protocol::hand::SecretDeliveryRequest,
    ) -> crate::hand::HandResult<crate::hand::SecretMaterial> {
        use brain_protocol::hand::HandErrorCode;

        let now = crate::wall_ms();
        let grant = {
            let mut grants = self
                .managed_secret_grants
                .lock()
                .expect("managed secret grants");
            grants.retain(|_, grant| grant.expires_at_ms > now);
            grants.remove(request.capability_ref.as_str())
        }
        .ok_or_else(|| {
            secret_delivery_error(
                HandErrorCode::CapabilityUnavailable,
                false,
                "secret capability is absent, expired, or already redeemed",
            )
        })?;

        if grant.root_id != request.root_id.as_str()
            || grant.session_id != request.session_id.as_str()
            || grant.hand_id != request.hand_id.as_str()
            || request.target.root_id.as_str() != grant.root_id
            || request.target.session_id.as_str() != grant.session_id
            || !grant
                .binding_refs
                .contains(request.target.binding_ref.as_str())
            || request.target.kind != brain_protocol::hand::TargetKind::Default
        {
            return Err(secret_delivery_error(
                HandErrorCode::BindingConflict,
                false,
                "secret capability does not match the exact Hand/session/target scope",
            ));
        }

        let head = self
            .journal
            .get_head(&grant.session_id)
            .await
            .map_err(|_| {
                secret_delivery_error(
                    HandErrorCode::TemporarilyUnavailable,
                    true,
                    "secret custody is temporarily unavailable",
                )
            })?;
        if head.doc.root_id != grant.root_id || head.doc.state != "open" {
            return Err(secret_delivery_error(
                HandErrorCode::CapabilityUnavailable,
                false,
                "session no longer permits managed secret delivery",
            ));
        }
        let (_, secrets) = self.root_execution_secrets(&head.doc).await.map_err(|_| {
            secret_delivery_error(
                HandErrorCode::TemporarilyUnavailable,
                true,
                "secret custody is temporarily unavailable",
            )
        })?;
        let mut values = HashMap::with_capacity(grant.env_names.len());
        for name in grant.env_names {
            let value = secrets.hand_env.get(&name).ok_or_else(|| {
                secret_delivery_error(
                    HandErrorCode::CapabilityUnavailable,
                    false,
                    "immutable managed secret material is incomplete",
                )
            })?;
            values.insert(name, value.clone());
        }
        Ok(crate::hand::SecretMaterial::new(values))
    }
}

fn required_child_string(input: &serde_json::Value, field: &str) -> Result<String> {
    input
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BrainError::Invalid(format!("subagents.{field} is required")))
}

fn sandbox_file_effect_id(session_id: &str, key: &str, action: &str) -> Result<String> {
    if key.is_empty() || key.len() > 128 {
        return Err(BrainError::Invalid(
            "Idempotency-Key must contain 1 to 128 bytes".into(),
        ));
    }
    let identity = hash_create_key(&format!(
        "aex.sandbox-file-effect.v1\0{action}\0{session_id}\0{key}"
    ));
    Ok(format!("file_{}", &identity[..24]))
}

fn direct_sandbox_transfer_key(transfer_id: &str) -> String {
    format!(
        "{}{transfer_id}",
        crate::storage::INTERNAL_SANDBOX_TRANSFER_PREFIX
    )
}

fn direct_sandbox_prepare_failure_is_definitive(error: &BrainError) -> bool {
    matches!(
        error,
        BrainError::FileTooLarge { .. }
            | BrainError::StorageObjectTooLarge { .. }
            | BrainError::StorageQuotaExceeded { .. }
            | BrainError::TenantStorageQuotaExceeded { .. }
            | BrainError::StorageUploadInProgress { .. }
            | BrainError::SandboxNotMaterialized
            | BrainError::SandboxGone
            | BrainError::SandboxGenerationConflict
            | BrainError::Overloaded
    )
}

fn required_bounded_string(
    input: &serde_json::Value,
    field: &str,
    max_bytes: usize,
    scope: &str,
) -> Result<String> {
    input
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= max_bytes)
        .map(str::to_owned)
        .ok_or_else(|| {
            BrainError::Invalid(format!(
                "{scope}.{field} must contain 1 to {max_bytes} UTF-8 bytes"
            ))
        })
}

fn optional_bounded_string(
    input: &serde_json::Value,
    field: &str,
    max_bytes: usize,
) -> Result<Option<String>> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| BrainError::Invalid(format!("{field} must be a string when supplied")))?;
    if value.len() > max_bytes {
        return Err(BrainError::Invalid(format!(
            "{field} exceeds {max_bytes} UTF-8 bytes"
        )));
    }
    Ok(Some(value.to_owned()))
}

fn required_identifier(input: &serde_json::Value, field: &str, scope: &str) -> Result<String> {
    let value = required_bounded_string(input, field, 128, scope)?;
    value
        .parse::<brain_protocol::hand::Identifier>()
        .map_err(|error| BrainError::Invalid(format!("{scope}.{field}: {error}")))?;
    Ok(value)
}

fn validate_storage_prefix(prefix: &str) -> Result<()> {
    if prefix.starts_with('/')
        || prefix.contains('\0')
        || prefix
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(BrainError::Invalid(
            "storage prefix must be relative and contain no . or .. components".into(),
        ));
    }
    Ok(())
}

fn sandbox_file_request(
    target: &brain_protocol::hand::SandboxTarget,
    generation: &str,
    path: &str,
) -> Result<brain_protocol::hand::SandboxFileRequest> {
    serde_json::from_value(serde_json::json!({
        "target": target,
        "expected_generation": generation,
        "path": path,
    }))
    .map_err(BrainError::from)
}

fn storage_object_reference(
    object_id: &str,
    bytes: u64,
    sha256: &str,
    media_type: Option<&str>,
) -> Result<brain_protocol::hand::ObjectReference> {
    serde_json::from_value(serde_json::json!({
        "object_id": object_id,
        "bytes": bytes,
        "sha256": sha256,
        "media_type": media_type,
    }))
    .map_err(BrainError::from)
}

#[allow(clippy::too_many_arguments)]
fn sandbox_copy_request(
    operation_id: &str,
    target: &brain_protocol::hand::SandboxTarget,
    generation: &str,
    path: &str,
    object: Option<brain_protocol::hand::ObjectReference>,
    ticket: &crate::storage::StorageTransferTicket,
    direction: &str,
    overwrite: bool,
) -> Result<brain_protocol::hand::SandboxCopyRequest> {
    let mut request: brain_protocol::hand::SandboxCopyRequest =
        serde_json::from_value(serde_json::json!({
            "operation_id": operation_id,
            "request_digest": "0".repeat(64),
            "target": target,
            "expected_generation": generation,
            "path": path,
            "object": object,
            "transfer": {
                "transfer_id": ticket.transfer_id,
                "object_id": ticket.object_id,
                "method": ticket.method,
                "url": ticket.url,
                "headers": ticket.headers,
                "expires_at_ms": ticket.expires_at_ms,
                "max_bytes": ticket.max_bytes,
            },
            "direction": direction,
            "overwrite": overwrite,
        }))?;
    request.request_digest = brain_protocol::contract::sandbox_copy_request_digest(&request);
    Ok(request)
}

fn validate_sandbox_copy_result(
    result: &brain_protocol::hand::SandboxCopyResult,
    operation_id: &str,
    request_digest: &brain_protocol::hand::Digest,
) -> Result<()> {
    if result.operation_id.as_str() != operation_id || &result.request_digest != request_digest {
        return Err(BrainError::Hand(
            "sandbox copy receipt identity mismatch".into(),
        ));
    }
    Ok(())
}

fn additional_sandbox_identity(
    root_id: &str,
    owner_session_id: &str,
    operation_id: &str,
) -> Result<(String, String, brain_protocol::hand::SandboxTarget)> {
    let identity = |domain: &str| {
        hex::encode(Sha256::digest(
            format!("aex.{domain}\0{root_id}\0{owner_session_id}\0{operation_id}").as_bytes(),
        ))
    };
    let sandbox_digest = identity("additional-sandbox");
    let generation_digest = identity("additional-generation");
    let binding_digest = identity("additional-binding");
    let sandbox_id = format!("sbx_{}", &sandbox_digest[..24]);
    let generation = format!("gen_{}", &generation_digest[..24]);
    let target = serde_json::from_value(serde_json::json!({
        "kind": "additional",
        "session_id": owner_session_id,
        "root_id": root_id,
        "binding_ref": format!("bnd_{}", &binding_digest[..24]),
        "sandbox_id": sandbox_id,
    }))?;
    Ok((sandbox_id, generation, target))
}

fn sandbox_request_digest(
    root_id: &str,
    owner_session_id: &str,
    operation_id: &str,
    input: &serde_json::Value,
) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_jcs::to_vec(
        &serde_json::json!({
            "domain": "aex.brain.sandbox.v1",
            "root_id": root_id,
            "owner_session_id": owner_session_id,
            "operation_id": operation_id,
            "input": input,
        }),
    )?)))
}

fn sandbox_status_matches_target(
    status: &brain_protocol::hand::SandboxStatus,
    target: &brain_protocol::hand::SandboxTarget,
) -> Result<bool> {
    Ok(serde_json::to_value(&status.target)? == serde_json::to_value(target)?)
}

fn sandbox_status_releases_slot(status: &brain_protocol::hand::SandboxStatus) -> bool {
    matches!(
        status.state,
        brain_protocol::hand::SandboxState::Gone | brain_protocol::hand::SandboxState::Terminated
    )
}

fn sandbox_gone_status(
    current: &brain_protocol::hand::SandboxStatus,
    reason: &str,
) -> Result<brain_protocol::hand::SandboxStatus> {
    serde_json::from_value(serde_json::json!({
        "state": "gone",
        "target": current.target,
        "generation": current.generation,
        "target_ref": current.target_ref,
        "changed_at_ms": crate::wall_ms(),
        "expires_at_ms": current.expires_at_ms,
        "reason": reason,
    }))
    .map_err(BrainError::from)
}

fn sandbox_file_write_request(
    operation_id: &str,
    target: &brain_protocol::hand::SandboxTarget,
    generation: &str,
    path: &str,
    content: &[u8],
    overwrite: bool,
) -> Result<brain_protocol::hand::SandboxFileWriteRequest> {
    let mut request: brain_protocol::hand::SandboxFileWriteRequest =
        serde_json::from_value(serde_json::json!({
            "operation_id": operation_id,
            "request_digest": "0".repeat(64),
            "target": target,
            "expected_generation": generation,
            "path": path,
            "source": {
                "kind": "inline",
                "content_base64": base64::engine::general_purpose::STANDARD.encode(content),
            },
            "overwrite": overwrite,
        }))?;
    request.request_digest = brain_protocol::contract::sandbox_file_write_request_digest(&request);
    Ok(request)
}

fn validate_sandbox_file_write_result(
    result: &brain_protocol::hand::SandboxFileWriteResult,
    operation_id: &str,
    request_digest: &brain_protocol::hand::Digest,
) -> Result<()> {
    if result.operation_id.as_str() != operation_id || &result.request_digest != request_digest {
        return Err(BrainError::Hand(
            "sandbox file write receipt identity mismatch".into(),
        ));
    }
    Ok(())
}

fn sandbox_search_request(
    target: &brain_protocol::hand::SandboxTarget,
    generation: &str,
    path: &str,
    expression: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<crate::hand::SandboxSearchRequest> {
    Ok(crate::hand::SandboxSearchRequest {
        target: target.clone(),
        expected_generation: generation.to_owned(),
        path: path.to_owned(),
        expression: expression.to_owned(),
        cursor: cursor.map(str::to_owned),
        limit,
    })
}

fn engine_outcome(
    result: Result<serde_json::Value>,
    started: std::time::Instant,
) -> Result<CallOutcome> {
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(value) => Ok(CallOutcome {
            outcome: "completed".into(),
            content: serde_json::to_string(&value)?,
            value: Some(value),
            is_error: false,
            exit_code: None,
            duration_ms,
            truncated: false,
            terminal: None,
        }),
        Err(BrainError::Cancelled) => Ok(CallOutcome {
            outcome: "cancelled".into(),
            content: "engine operation cancelled".into(),
            value: None,
            is_error: true,
            exit_code: None,
            duration_ms,
            truncated: false,
            terminal: None,
        }),
        Err(error @ (BrainError::Journal(_) | BrainError::Fenced | BrainError::Custody(_))) => {
            Err(error)
        }
        Err(error) => {
            let mut outcome = CallOutcome::failed(error.to_string());
            outcome.duration_ms = duration_ms;
            Ok(outcome)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------------------------

struct Resident {
    st: TurnState,
    key: ProviderKey,
    managed_bindings: Arc<HashMap<String, brain_protocol::hand::ResolvedBinding>>,
    /// Keeps the root-scoped OnceCell alive while any root/descendant actor is resident.
    _root_secrets: Arc<RootSecretCell>,
    message_replays: HashMap<String, MessageReplay>,
}

struct Running {
    handle: tokio::task::JoinHandle<(TurnState, RunningOutcome)>,
    cancel: CancellationToken,
    key: ProviderKey,
    managed_bindings: Arc<HashMap<String, brain_protocol::hand::ResolvedBinding>>,
    root_secrets: Arc<RootSecretCell>,
    message_replays: HashMap<String, MessageReplay>,
    _heartbeat: LeaseHeartbeatGuard,
}

/// A lease renewal is deliberately independent from recovery scheduling. Long provider, Hand,
/// storage, and deletion effects keep ownership alive even while the actor is awaiting them. The
/// due key is advanced only for immediate recoverable work; scheduled reservations keep their
/// fixed expiry and quiescent sessions keep no due key at all.
struct LeaseHeartbeatGuard {
    stop: CancellationToken,
}

impl Drop for LeaseHeartbeatGuard {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn start_lease_heartbeat(
    brain: &Arc<Brain>,
    session_id: &str,
    lease: &Lease,
    advance_active_due: bool,
    cancel_effect: Option<CancellationToken>,
) -> LeaseHeartbeatGuard {
    let stop = CancellationToken::new();
    let task_stop = stop.clone();
    let journal = brain.journal.clone();
    let session_id = session_id.to_owned();
    let lease = lease.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = task_stop.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(crate::journal::LEASE_MS / 3)) => {}
            }
            if let Err(error) = journal.renew(&session_id, &lease, advance_active_due).await {
                tracing::warn!(session = %session_id, error = %error, "session lease heartbeat failed");
                if matches!(error, BrainError::Fenced) {
                    if let Some(cancel) = &cancel_effect {
                        cancel.cancel();
                    }
                    break;
                }
            }
        }
    });
    LeaseHeartbeatGuard { stop }
}

enum RunningOutcome {
    Turn {
        turn_id: String,
        outcome: Result<crate::turn::TurnReport>,
    },
}

fn collect_message_replays(
    head: &HeadDoc,
    entries: &[Entry],
) -> Result<HashMap<String, MessageReplay>> {
    let mut replays = head
        .message_replays
        .iter()
        .map(|replay| {
            (
                replay.key_hash.clone(),
                MessageReplay {
                    request_hash: replay.request_hash.clone(),
                    turn_id: replay.turn_id.clone(),
                    user_seq: replay.user_seq,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    for entry in entries {
        let Record::UserMessage {
            turn,
            idempotency_key_hash,
            request_hash,
            ..
        } = &entry.record
        else {
            continue;
        };
        let (key_hash, request_hash) = match (idempotency_key_hash, request_hash) {
            (None, None) => continue,
            (Some(key_hash), Some(request_hash)) => (key_hash, request_hash),
            _ => {
                return Err(BrainError::Journal(
                    "message idempotency record is incomplete".into(),
                ));
            }
        };
        let replay = MessageReplay {
            request_hash: request_hash.clone(),
            turn_id: turn.clone(),
            user_seq: entry.seq,
        };
        if let Some(previous) = replays.insert(key_hash.clone(), replay.clone())
            && previous != replay
        {
            return Err(BrainError::Journal(
                "message idempotency key maps to conflicting journal records".into(),
            ));
        }
    }
    Ok(replays)
}

fn replay_message(
    replays: &HashMap<String, MessageReplay>,
    identity: Option<&MessageIdentity>,
) -> Result<Option<(String, u64)>> {
    let Some(identity) = identity else {
        return Ok(None);
    };
    let Some(replay) = replays.get(&identity.key_hash) else {
        return Ok(None);
    };
    if replay.request_hash != identity.request_hash {
        return Err(BrainError::IdempotencyConflict);
    }
    Ok(Some((replay.turn_id.clone(), replay.user_seq)))
}

#[derive(Debug)]
struct PendingVolatile {
    seq: u64,
    turn: String,
    agent: String,
    call: String,
    name: String,
}

#[derive(Debug, Clone)]
struct PendingExternal {
    seq: u64,
    turn: String,
    call: String,
    name: String,
    input: serde_json::Value,
    context: HashMap<String, String>,
    policy: crate::config::ServerToolPolicy,
    parallel_batch: bool,
}

#[derive(Debug, Clone)]
struct PendingCustomer {
    seq: u64,
    turn: String,
    call: String,
    name: String,
    intent: crate::customer::CustomerOperationIntent,
}

#[derive(Debug, Clone)]
struct PendingManaged {
    seq: u64,
    turn: String,
    call: String,
    name: String,
    envelope: brain_protocol::hand::OperationEnvelope,
    operation: Option<brain_protocol::hand::OperationRef>,
    submit_unknown: bool,
}

fn resolve_sealed_tools(prefix: &PrefixDoc) -> Vec<crate::config::ToolDecl> {
    let mut tools = crate::tools::resolve(&prefix.tools).unwrap_or_default();
    for tool in &mut tools {
        let crate::config::ToolRoute::Intrinsic(capability) = &tool.route else {
            continue;
        };
        if let Some(policy) = prefix.official_capabilities.get(capability) {
            tool.route = crate::config::ToolRoute::Server(policy.clone());
        }
    }
    tools
}

/// Host-tool calls whose intent committed but whose result did not. Only the stable sealed policy
/// determines whether replay is permitted; model arguments cannot opt a call into replay.
fn pending_external(entries: &[Entry], prefix: &PrefixDoc) -> Vec<PendingExternal> {
    let answered: HashSet<&str> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ToolResult { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect();
    let terminal_turns: HashSet<&str> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::TurnCompleted { turn, .. } | Record::TurnFailed { turn, .. } => {
                Some(turn.as_str())
            }
            _ => None,
        })
        .collect();
    let contexts: HashMap<&str, &HashMap<String, String>> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::UserMessage { turn, metadata, .. } => Some((turn.as_str(), metadata)),
            _ => None,
        })
        .collect();
    let policies: HashMap<String, crate::config::ServerToolPolicy> = resolve_sealed_tools(prefix)
        .into_iter()
        .filter_map(|tool| match tool.route {
            crate::config::ToolRoute::Server(policy) => Some((tool.name, policy)),
            _ => None,
        })
        .collect();

    let mut pending = Vec::new();
    for entry in entries {
        let Record::ToolCall {
            turn,
            agent,
            call,
            name,
            input,
            ..
        } = &entry.record
        else {
            continue;
        };
        let Some(policy) = policies.get(name.as_str()).cloned() else {
            continue;
        };
        if agent != "root"
            || answered.contains(call.as_str())
            || terminal_turns.contains(turn.as_str())
        {
            continue;
        }
        let assistant_seq = entries
            .iter()
            .rev()
            .find_map(|candidate| match &candidate.record {
                Record::Assistant {
                    turn: assistant_turn,
                    agent,
                    ..
                } if candidate.seq < entry.seq && assistant_turn == turn && agent == "root" => {
                    Some(candidate.seq)
                }
                _ => None,
            })
            .unwrap_or(0);
        let next_assistant_seq = entries
            .iter()
            .filter_map(|candidate| match &candidate.record {
                Record::Assistant {
                    turn: assistant_turn,
                    agent,
                    ..
                } if candidate.seq > assistant_seq && assistant_turn == turn && agent == "root" => {
                    Some(candidate.seq)
                }
                _ => None,
            })
            .min()
            .unwrap_or(u64::MAX);
        let batch_size = entries
            .iter()
            .filter(|candidate| {
                candidate.seq > assistant_seq
                    && candidate.seq < next_assistant_seq
                    && matches!(
                        &candidate.record,
                        Record::ToolCall { turn: other_turn, agent, .. }
                            if other_turn == turn && agent == "root"
                    )
            })
            .count();
        pending.push(PendingExternal {
            seq: entry.seq,
            turn: turn.clone(),
            call: call.clone(),
            name: name.clone(),
            input: input.clone(),
            context: contexts
                .get(turn.as_str())
                .map_or_else(HashMap::new, |v| (*v).clone()),
            policy,
            parallel_batch: batch_size > 1,
        });
    }
    pending.sort_by_key(|call| call.seq);
    pending
}

fn pending_customer(
    entries: &[Entry],
    prefix: &PrefixDoc,
    tenant_id: &str,
    session_id: &str,
) -> Vec<PendingCustomer> {
    let answered = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ToolResult { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let intents = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::CustomerCallIntent {
                call,
                client_id,
                process_id,
                request_digest,
                deadline_at_ms,
                ..
            } => Some((
                call.as_str(),
                (
                    client_id.clone(),
                    process_id.clone(),
                    request_digest.clone(),
                    *deadline_at_ms,
                ),
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let tools = resolve_sealed_tools(prefix)
        .into_iter()
        .map(|tool| (tool.name.clone(), tool))
        .collect::<HashMap<_, _>>();
    let mut pending = Vec::new();
    for entry in entries {
        let Record::ToolCall {
            turn,
            agent,
            call,
            name,
            input,
            ..
        } = &entry.record
        else {
            continue;
        };
        if agent != "root" || answered.contains(call.as_str()) {
            continue;
        }
        let Some((client_id, process_id, request_digest, deadline_at_ms)) =
            intents.get(call.as_str())
        else {
            continue;
        };
        let Some(tool) = tools.get(name) else {
            continue;
        };
        let crate::config::ToolRoute::Customer { registration } = &tool.route else {
            continue;
        };
        pending.push(PendingCustomer {
            seq: entry.seq,
            turn: turn.clone(),
            call: call.clone(),
            name: name.clone(),
            intent: crate::customer::CustomerOperationIntent {
                tenant_id: tenant_id.to_owned(),
                client_id: client_id.clone(),
                process_id: process_id.clone(),
                session_id: session_id.to_owned(),
                operation_id: call.clone(),
                registration: registration.clone(),
                name: name.clone(),
                contract_digest: tool.contract_digest.clone(),
                input: input.clone(),
                deadline_at_ms: *deadline_at_ms,
                request_digest: request_digest.clone(),
            },
        });
    }
    pending.sort_by_key(|call| call.seq);
    pending
}

fn pending_managed(entries: &[Entry]) -> Result<Vec<PendingManaged>> {
    let mut pending = HashMap::<String, PendingManaged>::new();
    for entry in entries {
        match &entry.record {
            Record::ManagedCallIntent {
                turn,
                call,
                name,
                envelope,
            } => {
                let next = PendingManaged {
                    seq: entry.seq,
                    turn: turn.clone(),
                    call: call.clone(),
                    name: name.clone(),
                    envelope: envelope.clone(),
                    operation: None,
                    submit_unknown: false,
                };
                if let Some(previous) = pending.insert(call.clone(), next)
                    && (previous.turn != *turn
                        || previous.name != *name
                        || serde_jcs::to_vec(&previous.envelope)? != serde_jcs::to_vec(envelope)?)
                {
                    return Err(BrainError::Journal(
                        "managed operation id maps to conflicting durable intents".into(),
                    ));
                }
            }
            Record::ManagedCallAccepted {
                turn,
                call,
                operation,
            } => {
                let item = pending.get_mut(call).ok_or_else(|| {
                    BrainError::Journal(
                        "managed accepted receipt has no preceding durable intent".into(),
                    )
                })?;
                if item.turn != *turn {
                    return Err(BrainError::Journal(
                        "managed accepted receipt references a different turn".into(),
                    ));
                }
                if let Some(previous) = &item.operation
                    && serde_jcs::to_vec(previous)? != serde_jcs::to_vec(operation)?
                {
                    return Err(BrainError::Journal(
                        "managed operation id maps to conflicting accepted receipts".into(),
                    ));
                }
                item.operation = Some(operation.clone());
            }
            Record::ManagedCallUnknown {
                turn,
                call,
                request_digest,
            } => {
                let item = pending.get_mut(call).ok_or_else(|| {
                    BrainError::Journal(
                        "managed unknown marker has no preceding durable intent".into(),
                    )
                })?;
                if item.turn != *turn
                    || item.envelope.request_digest.as_str() != request_digest.as_str()
                {
                    return Err(BrainError::Journal(
                        "managed unknown marker conflicts with its durable intent".into(),
                    ));
                }
                item.submit_unknown = true;
            }
            Record::ToolResult { call, .. } => {
                pending.remove(call);
            }
            _ => {}
        }
    }
    let mut pending = pending.into_values().collect::<Vec<_>>();
    pending.sort_by_key(|call| call.seq);
    Ok(pending)
}

/// Finds unanswered calls whose executor cannot be recovered unambiguously. A newly claimed
/// session never reassigns these calls: Hand, customer-app and in-process intrinsic effects may
/// already have happened. Replay-safe server capabilities are handled separately.
fn pending_volatile(entries: &[Entry], prefix: &PrefixDoc) -> Vec<PendingVolatile> {
    let customer_intents = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::CustomerCallIntent { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let managed_intents = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ManagedCallIntent { call, .. } => Some(call.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let volatile_names: HashSet<String> = resolve_sealed_tools(prefix)
        .into_iter()
        .filter_map(|tool| match tool.route {
            crate::config::ToolRoute::Server(_) => None,
            _ => Some(tool.name),
        })
        .collect();
    let mut pending = HashMap::<String, PendingVolatile>::new();
    for entry in entries {
        match &entry.record {
            Record::ToolCall {
                turn,
                agent,
                call,
                name,
                detach,
                ..
            } if volatile_names.contains(name)
                && !customer_intents.contains(call.as_str())
                && !managed_intents.contains(call.as_str())
                && !detach =>
            {
                pending.insert(
                    call.clone(),
                    PendingVolatile {
                        seq: entry.seq,
                        turn: turn.clone(),
                        agent: agent.clone(),
                        call: call.clone(),
                        name: name.clone(),
                    },
                );
            }
            Record::ToolResult { call, .. } => {
                pending.remove(call);
            }
            _ => {}
        }
    }
    let mut pending: Vec<_> = pending.into_values().collect();
    pending.sort_by_key(|call| call.seq);
    pending
}

struct RecoveredTurn {
    turn: String,
    context: HashMap<String, String>,
    rounds: u64,
    tool_calls: u64,
}

async fn recover_customer_calls(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    entries: &[Entry],
) -> Result<bool> {
    let Some(customer) = &brain.customer else {
        if pending_customer(
            entries,
            &resident.st.head.prefix,
            &resident.st.head.tenant_id,
            session_id,
        )
        .is_empty()
        {
            return Ok(false);
        }
        return Err(BrainError::HandUnavailable(
            "customer application coordinator is unavailable during recovery".into(),
        ));
    };
    let pending = pending_customer(
        entries,
        &resident.st.head.prefix,
        &resident.st.head.tenant_id,
        session_id,
    );
    let mut changed = false;
    for call in pending {
        let execution = customer
            .execute_prepared(
                call.intent.clone(),
                resident.st.head.prefix.customer_submit_retries,
                CancellationToken::new(),
            )
            .await;
        if execution.retryable_without_effect && crate::wall_ms() < call.intent.deadline_at_ms {
            return Err(BrainError::HandUnavailable(
                "sealed customer application process has not reconnected".into(),
            ));
        }
        let tool = crate::tools::resolve(&resident.st.head.prefix.tools)?
            .into_iter()
            .find(|tool| tool.name == call.name);
        let outcome = crate::tools::enforce_outcome(tool.as_ref(), &call.name, execution.outcome);
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content.clone()
        };
        let mut records = vec![(
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn.clone(),
                agent: "root".into(),
                call: call.call.clone(),
                name: call.name.clone(),
                outcome: outcome.outcome,
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        )];
        if let Some(receipt) = &execution.terminal_receipt {
            let ack = crate::journal::CustomerTerminalAckDoc {
                turn: call.turn.clone(),
                call: call.call.clone(),
                client_id: call.intent.client_id.clone(),
                process_id: receipt.process_id.clone(),
                request_digest: receipt.request_digest.clone(),
                terminal_digest: receipt.terminal_digest.clone(),
            };
            if !resident
                .st
                .head
                .pending_customer_acks
                .iter()
                .any(|current| current.call == ack.call)
            {
                resident.st.head.pending_customer_acks.push(ack);
            }
            records.push((
                resident.st.take_seq(),
                Record::CustomerTerminalReceived {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    client_id: call.intent.client_id.clone(),
                    process_id: receipt.process_id.clone(),
                    request_digest: receipt.request_digest.clone(),
                    terminal_digest: receipt.terminal_digest.clone(),
                },
            ));
        }
        commit(brain, session_id, &mut resident.st, records).await?;
        changed = true;

        if let Some(receipt) = execution.terminal_receipt
            && customer.acknowledge_terminal(&receipt).await.is_ok()
        {
            let previous = resident.st.head.pending_customer_acks.clone();
            resident.st.head.pending_customer_acks.retain(|pending| {
                pending.call != receipt.operation_id
                    || pending.request_digest != receipt.request_digest
                    || pending.terminal_digest != receipt.terminal_digest
            });
            let acked = vec![(
                resident.st.take_seq(),
                Record::CustomerTerminalAcknowledged {
                    turn: call.turn,
                    call: call.call,
                    request_digest: receipt.request_digest,
                    terminal_digest: receipt.terminal_digest,
                },
            )];
            if let Err(error) = commit(brain, session_id, &mut resident.st, acked).await {
                resident.st.head.pending_customer_acks = previous;
                return Err(error);
            }
        }
    }
    Ok(changed)
}

async fn recover_managed_calls(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    entries: &[Entry],
) -> Result<bool> {
    use brain_protocol::hand::{HandErrorCode, OperationState};

    let pending = pending_managed(entries)?;
    if pending.is_empty() {
        return Ok(false);
    }
    let tools = crate::tools::resolve(&resident.st.head.prefix.tools)?;
    let active_turn = resident.st.head.turn.clone();
    let rounds = resident.st.head.active_rounds;
    let tool_calls = resident.st.head.active_tool_calls;
    let mut recovered = Vec::new();
    let mut sandbox_gone = None;

    let (stale, pending): (Vec<_>, Vec<_>) = pending
        .into_iter()
        .partition(|call| active_turn.as_deref() != Some(call.turn.as_str()));
    for call in stale {
        if !call.submit_unknown {
            let unknown = vec![(
                resident.st.take_seq(),
                Record::ManagedCallUnknown {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    request_digest: call.envelope.request_digest.to_string(),
                },
            )];
            commit(brain, session_id, &mut resident.st, unknown).await?;
        }
        reconcile_managed_unknown_default_sandbox(brain, session_id, &mut resident.st).await?;
        let outcome = crate::tools::enforce_outcome(
            tools.iter().find(|tool| tool.name == call.name),
            &call.name,
            crate::turn::managed_unknown_call_outcome(&call.name),
        );
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content
        };
        let result = vec![(
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn,
                agent: "root".into(),
                call: call.call,
                name: call.name,
                outcome: outcome.outcome,
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        )];
        commit(brain, session_id, &mut resident.st, result).await?;
    }
    if pending.is_empty() {
        return Ok(true);
    }

    let hand = brain.hand.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "managed Hand is unavailable for durable operation recovery".into(),
        )
    })?;

    'managed_calls: for call in pending {
        if call.submit_unknown {
            reconcile_managed_unknown_default_sandbox(brain, session_id, &mut resident.st).await?;
            recovered.push((
                call.clone(),
                call.operation.clone(),
                crate::turn::managed_unknown_call_outcome(&call.name),
                None,
            ));
            continue;
        }
        let binding = resident.managed_bindings.get(&call.name).ok_or_else(|| {
            BrainError::HandUnavailable(format!(
                "managed Tool {} has no recovered immutable binding",
                call.name
            ))
        })?;
        if binding.binding_ref != call.envelope.binding_ref {
            return Err(BrainError::Protocol(
                "managed Tool binding changed across durable recovery".into(),
            ));
        }

        let mut accepted_now = false;
        let (operation, mut observation) = if let Some(operation) = call.operation.clone() {
            crate::turn::verify_managed_operation(
                &operation,
                &call.call,
                call.envelope.request_digest.as_str(),
                session_id,
                &resident.st.head,
            )?;
            let request: brain_protocol::hand::ObserveRequest =
                serde_json::from_value(serde_json::json!({
                    "operation": operation,
                    "cursor": "",
                    "wait_ms": binding.limits.max_wait_ms.min(30_000),
                }))?;
            let observed = tokio::time::timeout(
                Duration::from_millis(request.wait_ms.saturating_add(1_000).max(1)),
                hand.observe(request),
            )
            .await
            .map_err(|_| {
                BrainError::HandUnavailable("managed Tool recovery observation timed out".into())
            })?;
            match observed {
                Ok(observation) => (operation, observation),
                Err(error) if error.code == HandErrorCode::SandboxGone => {
                    sandbox_gone = Some(managed_operation_gone_status(&operation)?);
                    recovered.push((
                        call,
                        Some(operation),
                        CallOutcome {
                            outcome: "interrupted".into(),
                            value: None,
                            content: "managed Tool target disappeared before its durable terminal could be recovered".into(),
                            is_error: true,
                            exit_code: None,
                            duration_ms: 0,
                            truncated: false,
                            terminal: None,
                        },
                        None,
                    ));
                    continue;
                }
                Err(error) => return Err(map_hand_port_error(error)),
            }
        } else {
            let request = brain_protocol::hand::SubmitRequest {
                envelope: call.envelope.clone(),
                wait_up_to_ms: binding.limits.max_wait_ms.min(30_000),
            };
            let mut reprepared = false;
            let receipt = loop {
                let submitted = tokio::time::timeout(
                    Duration::from_millis(request.wait_up_to_ms.saturating_add(1_000).max(1)),
                    hand.submit(request.clone()),
                )
                .await
                .map_err(|_| {
                    BrainError::HandUnavailable("managed Tool recovery submit timed out".into())
                })?;
                match submitted {
                    Ok(receipt) => break receipt,
                    Err(error)
                        if error.code == HandErrorCode::CapabilityUnavailable && !reprepared =>
                    {
                        let refreshed = brain
                            .prepare_managed_session(session_id, &resident.st.head)
                            .await?;
                        let refreshed = refreshed.get(&call.name).ok_or_else(|| {
                            BrainError::HandUnavailable(
                                "managed Tool disappeared during exact re-preparation".into(),
                            )
                        })?;
                        if refreshed.binding_ref != binding.binding_ref {
                            return Err(BrainError::Protocol(
                                "managed Tool binding changed during exact re-preparation".into(),
                            ));
                        }
                        reprepared = true;
                    }
                    Err(error) if error.code == HandErrorCode::OperationUnknown => {
                        let unknown = vec![(
                            resident.st.take_seq(),
                            Record::ManagedCallUnknown {
                                turn: call.turn.clone(),
                                call: call.call.clone(),
                                request_digest: call.envelope.request_digest.to_string(),
                            },
                        )];
                        commit(brain, session_id, &mut resident.st, unknown).await?;
                        reconcile_managed_unknown_default_sandbox(
                            brain,
                            session_id,
                            &mut resident.st,
                        )
                        .await?;
                        recovered.push((
                            call.clone(),
                            None,
                            crate::turn::managed_unknown_call_outcome(&call.name),
                            None,
                        ));
                        continue 'managed_calls;
                    }
                    Err(error) => return Err(map_hand_port_error(error)),
                }
            };
            crate::turn::verify_managed_operation(
                &receipt.operation,
                &call.call,
                call.envelope.request_digest.as_str(),
                session_id,
                &resident.st.head,
            )?;
            crate::turn::verify_managed_observation(&receipt.observation, &receipt.operation)?;
            accepted_now = true;
            (receipt.operation, receipt.observation)
        };

        if accepted_now {
            if let Some(target) = &observation.target {
                resident.st.head.default_sandbox = Some(managed_operation_running_status(
                    &operation,
                    target.expires_at_ms,
                )?);
            }
            let accepted = vec![(
                resident.st.take_seq(),
                Record::ManagedCallAccepted {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    operation: operation.clone(),
                },
            )];
            commit(brain, session_id, &mut resident.st, accepted).await?;
        }

        crate::turn::verify_managed_observation(&observation, &operation)?;
        if observation.state != OperationState::Terminal {
            if crate::wall_ms() >= call.envelope.deadline_at_ms.get() {
                let cancel: brain_protocol::hand::CancelRequest =
                    serde_json::from_value(serde_json::json!({
                        "operation": operation,
                        "reason": "recovery_deadline_elapsed",
                    }))?;
                match hand.cancel(cancel).await {
                    Ok(_) => {}
                    Err(error) if error.code == HandErrorCode::SandboxGone => {
                        sandbox_gone = Some(managed_operation_gone_status(&operation)?);
                    }
                    Err(error) => return Err(map_hand_port_error(error)),
                }
            }
            let request: brain_protocol::hand::ObserveRequest =
                serde_json::from_value(serde_json::json!({
                    "operation": operation,
                    "cursor": observation.next_cursor,
                    "wait_ms": binding.limits.max_wait_ms.min(30_000),
                }))?;
            match tokio::time::timeout(
                Duration::from_millis(request.wait_ms.saturating_add(1_000).max(1)),
                hand.observe(request),
            )
            .await
            .map_err(|_| {
                BrainError::HandUnavailable("managed Tool recovery observation timed out".into())
            })? {
                Ok(next) => observation = next,
                Err(error) if error.code == HandErrorCode::SandboxGone => {
                    sandbox_gone = Some(managed_operation_gone_status(&operation)?);
                    recovered.push((
                        call,
                        Some(operation),
                        CallOutcome {
                            outcome: "interrupted".into(),
                            value: None,
                            content: "managed Tool target disappeared before its durable terminal could be recovered".into(),
                            is_error: true,
                            exit_code: None,
                            duration_ms: 0,
                            truncated: false,
                            terminal: None,
                        },
                        None,
                    ));
                    continue;
                }
                Err(error) => return Err(map_hand_port_error(error)),
            }
            crate::turn::verify_managed_observation(&observation, &operation)?;
        }
        if observation.state != OperationState::Terminal {
            return Err(BrainError::HandUnavailable(
                "managed Tool remains in progress during durable recovery".into(),
            ));
        }
        let terminal = observation.terminal.ok_or_else(|| {
            BrainError::Protocol(
                "managed Hand reported terminal state without a terminal receipt".into(),
            )
        })?;
        let (outcome, terminal_digest) = crate::turn::managed_terminal_call_outcome(terminal)?;
        recovered.push((call, Some(operation), outcome, Some(terminal_digest)));
    }

    let mut records = Vec::new();
    let mut terminal = None;
    for (call, operation, outcome, terminal_digest) in recovered {
        let tool = tools.iter().find(|tool| tool.name == call.name);
        let outcome = crate::tools::enforce_outcome(tool, &call.name, outcome);
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content.clone()
        };
        records.push((
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn.clone(),
                agent: "root".into(),
                call: call.call.clone(),
                name: call.name.clone(),
                outcome: outcome.outcome,
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        ));
        if let Some(terminal_digest) = terminal_digest {
            let operation = operation.ok_or_else(|| {
                BrainError::Journal(
                    "managed terminal receipt has no accepted operation reference".into(),
                )
            })?;
            let pending = crate::journal::ManagedTerminalAckDoc {
                turn: call.turn.clone(),
                call: call.call.clone(),
                operation: operation.clone(),
                terminal_digest: terminal_digest.clone(),
            };
            if !resident
                .st
                .head
                .pending_managed_acks
                .iter()
                .any(|current| current.call == pending.call)
            {
                resident.st.head.pending_managed_acks.push(pending);
            }
            records.push((
                resident.st.take_seq(),
                Record::ManagedTerminalReceived {
                    turn: call.turn.clone(),
                    call: call.call.clone(),
                    operation,
                    terminal_digest,
                },
            ));
        }
        if terminal.is_none() {
            terminal = outcome.terminal.map(|value| (call.call, call.name, value));
        }
    }
    if let Some(status) = sandbox_gone {
        resident.st.head.default_sandbox = Some(status.clone());
        records.push((
            resident.st.take_seq(),
            Record::DefaultSandboxChanged { status },
        ));
    }
    if let Some((call, name, terminal)) = terminal {
        let turn = active_turn.expect("pending managed call required an active turn");
        resident.st.head.state = resident.st.head.lifecycle_after_turn();
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.provider_attempt = None;
        resident.st.head.active_context.clear();
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        match terminal {
            TerminalOutcome::Complete { value, metadata } => records.push((
                resident.st.take_seq(),
                Record::TurnCompleted {
                    turn: turn.clone(),
                    stop_reason: "end_turn".into(),
                    rounds,
                    tool_calls,
                    result: Some(brain_protocol::session::TurnResult {
                        call_id: call.parse().map_err(|error| {
                            BrainError::Protocol(format!("managed call id: {error}"))
                        })?,
                        metadata,
                        name,
                        value,
                    }),
                },
            )),
            TerminalOutcome::Fail { error } => records.push((
                resident.st.take_seq(),
                Record::TurnFailed {
                    turn: turn.clone(),
                    code: error.code.to_string(),
                    message: error.message,
                    details: error.details,
                },
            )),
        }
        records.push((
            resident.st.take_seq(),
            Record::State {
                state: resident.st.head.state.clone(),
                turn: None,
            },
        ));
    } else {
        resident.st.head.active_phase = Some("ready_to_continue_model".into());
    }
    commit(brain, session_id, &mut resident.st, records).await?;
    let _ = recover_managed_terminal_acks(brain, session_id, resident).await;
    Ok(true)
}

const MANAGED_UNKNOWN_SANDBOX_REASON: &str = "managed_operation_unknown_cleanup";

/// Reconcile the rooted default target after Hand reports that Submit may have reached the guest
/// but no operation receipt can be recovered. The durable `ManagedCallUnknown` marker is always
/// committed by the caller first, so this routine may retry status/dematerialization freely but
/// must never authorize another Submit.
pub(crate) async fn reconcile_managed_unknown_default_sandbox(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    use brain_protocol::hand::{HandErrorCode, SandboxState};

    let target = st
        .head
        .default_sandbox
        .as_ref()
        .map(|status| status.target.clone())
        .unwrap_or(default_sandbox_target(&st.head.root_id)?);
    let files = brain.sandbox_files.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "managed Tool unknown-outcome status reconciliation is unavailable".into(),
        )
    })?;
    let mut status = match files.status(target.clone()).await {
        Ok(status) => status,
        Err(error)
            if matches!(
                error.code,
                HandErrorCode::SandboxGone | HandErrorCode::SandboxNotMaterialized
            ) =>
        {
            let current = st
                .head
                .default_sandbox
                .clone()
                .unwrap_or(initial_default_sandbox(&st.head.root_id)?);
            sandbox_gone_status(&current, "managed_operation_target_gone")?
        }
        Err(error) if error.retryable => {
            return Err(BrainError::HandUnavailable(error.message.to_string()));
        }
        Err(error) => return Err(map_hand_port_error(error)),
    };
    if !sandbox_status_matches_target(&status, &target)? {
        return Err(BrainError::Protocol(
            "managed Tool unknown-outcome status references a different default target".into(),
        ));
    }
    if matches!(status.state, SandboxState::NeverMaterialized) {
        status = sandbox_gone_status(&status, "managed_operation_target_not_materialized")?;
    }
    if matches!(
        status.state,
        SandboxState::Running | SandboxState::Suspended
    ) && (status.generation.is_none()
        || status.target_ref.is_none()
        || status.expires_at_ms.is_none())
    {
        return Err(BrainError::Protocol(
            "managed Tool unknown-outcome status lacks generation, target_ref, or expiry".into(),
        ));
    }
    if !sandbox_status_releases_slot(&status) {
        let mut value = serde_json::to_value(&status)?;
        value["reason"] = serde_json::Value::String(MANAGED_UNKNOWN_SANDBOX_REASON.into());
        value["changed_at_ms"] = serde_json::Value::from(crate::wall_ms());
        status = serde_json::from_value(value)?;
    }
    st.head.default_sandbox = Some(status.clone());
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::DefaultSandboxChanged {
                status: status.clone(),
            },
        )],
    )
    .await?;
    if sandbox_status_releases_slot(&status) {
        return Ok(());
    }

    let preparation = brain.session_preparation.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "managed Tool unknown-outcome dematerialization is unavailable".into(),
        )
    })?;
    let terminal = match preparation.dematerialize_default(target.clone()).await {
        Ok(status) => status,
        Err(error)
            if matches!(
                error.code,
                HandErrorCode::SandboxGone | HandErrorCode::SandboxNotMaterialized
            ) =>
        {
            sandbox_gone_status(&status, "managed_operation_target_gone")?
        }
        Err(error) if error.retryable => {
            return Err(BrainError::HandUnavailable(error.message.to_string()));
        }
        Err(error) => return Err(map_hand_port_error(error)),
    };
    if !sandbox_status_matches_target(&terminal, &target)? {
        return Err(BrainError::Protocol(
            "managed Tool unknown-outcome dematerialization references a different target".into(),
        ));
    }
    if !sandbox_status_releases_slot(&terminal) {
        return Err(BrainError::HandUnavailable(
            "managed Tool unknown-outcome target has not reached a terminal sandbox state".into(),
        ));
    }
    st.head.default_sandbox = Some(terminal.clone());
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(seq, Record::DefaultSandboxChanged { status: terminal })],
    )
    .await
}

fn managed_operation_running_status(
    operation: &brain_protocol::hand::OperationRef,
    expires_at_ms: std::num::NonZeroU64,
) -> Result<brain_protocol::hand::SandboxStatus> {
    serde_json::from_value(serde_json::json!({
        "state": "running",
        "target": operation.target,
        "generation": operation.generation,
        "target_ref": operation.target_ref,
        "changed_at_ms": crate::wall_ms(),
        "expires_at_ms": expires_at_ms,
    }))
    .map_err(BrainError::from)
}

fn managed_operation_gone_status(
    operation: &brain_protocol::hand::OperationRef,
) -> Result<brain_protocol::hand::SandboxStatus> {
    serde_json::from_value(serde_json::json!({
        "state": "gone",
        "target": operation.target,
        "generation": operation.generation,
        "target_ref": operation.target_ref,
        "changed_at_ms": crate::wall_ms(),
        "expires_at_ms": null,
        "reason": "managed_operation_target_gone",
    }))
    .map_err(BrainError::from)
}

async fn recover_external_calls(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    entries: &[Entry],
) -> Result<Option<RecoveredTurn>> {
    let Some(active_turn) = resident.st.head.turn.clone() else {
        return Ok(None);
    };
    let pending: Vec<_> = pending_external(entries, &resident.st.head.prefix)
        .into_iter()
        .filter(|call| call.turn == active_turn)
        .collect();
    let rounds = resident.st.head.active_rounds;
    let tool_calls = resident.st.head.active_tool_calls;
    let context = resident.st.head.active_context.clone();
    if pending.is_empty() {
        return Ok(Some(RecoveredTurn {
            turn: active_turn,
            context,
            rounds,
            tool_calls,
        }));
    }
    let sealed_tools = crate::tools::resolve(&resident.st.head.prefix.tools)?;
    let mut records = Vec::with_capacity(pending.len() + 2);
    let mut blocks = Vec::with_capacity(pending.len());
    let mut terminal = None;
    let mut unreplayable = false;

    for call in pending {
        let outcome =
            if call.policy.effect == brain_protocol::session::ExternalToolEffect::ReplaySafe {
                crate::turn::execute_external(
                    brain.external_executor.clone(),
                    call.policy,
                    call.parallel_batch,
                    session_id.to_string(),
                    call.turn.clone(),
                    "root".into(),
                    call.call.clone(),
                    call.name.clone(),
                    call.input,
                    call.context,
                    CancellationToken::new(),
                )
                .await
            } else {
                unreplayable = true;
                CallOutcome {
                    outcome: "interrupted".into(),
                    value: None,
                    content: format!(
                        "external tool {} was interrupted and its opaque effect was not replayed",
                        call.name
                    ),
                    is_error: true,
                    exit_code: None,
                    duration_ms: 0,
                    truncated: false,
                    terminal: None,
                }
            };
        let tool = sealed_tools.iter().find(|tool| tool.name == call.name);
        let outcome = match tool {
            Some(tool) => crate::tools::enforce_outcome(Some(tool), &call.name, outcome),
            None => crate::tools::enforce_outcome(
                None,
                &call.name,
                CallOutcome::failed(format!(
                    "tool {} is absent from the recovered execution seal",
                    call.name
                )),
            ),
        };
        let content = if outcome.content.is_empty() {
            format!("[{}: no output]", outcome.outcome)
        } else {
            outcome.content.clone()
        };
        blocks.push(ContentBlock::ToolResult {
            tool_use_id: call.call.clone(),
            content: content.clone(),
            is_error: outcome.is_error,
        });
        records.push((
            resident.st.take_seq(),
            Record::ToolResult {
                turn: call.turn,
                agent: "root".into(),
                call: call.call.clone(),
                name: call.name.clone(),
                outcome: outcome.outcome,
                content,
                is_error: outcome.is_error,
                exit_code: outcome.exit_code,
                duration_ms: outcome.duration_ms,
                truncated: outcome.truncated,
            },
        ));
        if let Some(value) = outcome.terminal {
            terminal = Some((call.call, call.name, value));
        }
    }

    if let Some((call, name, terminal)) = terminal {
        resident.st.head.state = resident.st.head.lifecycle_after_turn();
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.provider_attempt = None;
        resident.st.head.active_context.clear();
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        match terminal {
            TerminalOutcome::Complete { value, metadata } => {
                records.push((
                    resident.st.take_seq(),
                    Record::TurnCompleted {
                        turn: active_turn.clone(),
                        stop_reason: "end_turn".into(),
                        rounds,
                        tool_calls,
                        result: Some(brain_protocol::session::TurnResult {
                            call_id: call.parse().map_err(|error| {
                                BrainError::Protocol(format!("external call id: {error}"))
                            })?,
                            metadata,
                            name,
                            value,
                        }),
                    },
                ));
            }
            TerminalOutcome::Fail { error } => records.push((
                resident.st.take_seq(),
                Record::TurnFailed {
                    turn: active_turn.clone(),
                    code: error.code.to_string(),
                    message: error.message,
                    details: error.details,
                },
            )),
        }
        records.push((
            resident.st.take_seq(),
            Record::State {
                state: resident.st.head.state.clone(),
                turn: None,
            },
        ));
    } else if unreplayable {
        resident.st.head.state = resident.st.head.lifecycle_after_turn();
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.provider_attempt = None;
        resident.st.head.active_context.clear();
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        records.push((
            resident.st.take_seq(),
            Record::TurnFailed {
                turn: active_turn.clone(),
                code: "cancelled".into(),
                message:
                    "turn interrupted with an opaque external-tool effect; it was not replayed"
                        .into(),
                details: None,
            },
        ));
        records.push((
            resident.st.take_seq(),
            Record::State {
                state: resident.st.head.state.clone(),
                turn: None,
            },
        ));
    }

    commit(brain, session_id, &mut resident.st, records).await?;
    resident.st.history.push(Message::tool_results(blocks));
    if resident.st.head.turn.is_none() {
        Ok(None)
    } else {
        Ok(Some(RecoveredTurn {
            turn: active_turn,
            context,
            rounds,
            tool_calls,
        }))
    }
}

/// Resolve the only ambiguous provider phase before a recovered turn is driven. Providers in the
/// MVP do not expose a durable retrieval handle, so an unfinished intent becomes UNKNOWN. A new
/// attempt is permitted only by the sealed crash-recovery budget; strict zero interrupts.
async fn recover_provider_attempt(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    let Some(turn) = resident.st.head.turn.clone() else {
        return Ok(());
    };
    let Some(mut attempt) = resident.st.head.provider_attempt.clone() else {
        resident.st.head.active_phase = Some("ready_to_build_model_request".into());
        commit(brain, session_id, &mut resident.st, vec![]).await?;
        return Ok(());
    };
    let is_compaction = attempt.logical_operation_id.starts_with("cmp_");

    let mut records = Vec::new();
    if matches!(attempt.state.as_str(), "intent" | "running") {
        records.push((
            resident.st.take_seq(),
            if is_compaction {
                Record::CompactionUnknown {
                    turn: turn.clone(),
                    logical_operation_id: attempt.logical_operation_id.clone(),
                    attempt_id: attempt.attempt_id.clone(),
                    request_digest: attempt.request_digest.clone(),
                    possibly_duplicated: true,
                }
            } else {
                Record::ModelCallUnknown {
                    turn: turn.clone(),
                    logical_operation_id: attempt.logical_operation_id.clone(),
                    attempt_id: attempt.attempt_id.clone(),
                    request_digest: attempt.request_digest.clone(),
                    possibly_duplicated: true,
                }
            },
        ));
        attempt.state = "unknown".into();
    }
    if attempt.state == "replacement_ready" {
        resident.st.head.provider_attempt = Some(attempt);
        resident.st.head.active_phase = Some(if is_compaction {
            "ready_to_compact".into()
        } else {
            "ready_to_build_model_request".into()
        });
        commit(brain, session_id, &mut resident.st, records).await?;
        return Ok(());
    }
    if attempt.state != "unknown" {
        return Err(BrainError::Journal(format!(
            "active provider attempt has invalid state {}",
            attempt.state
        )));
    }

    if attempt.replacements_used < resident.st.head.prefix.provider_recovery_retries {
        attempt.replacements_used += 1;
        attempt.state = "replacement_ready".into();
        resident.st.head.provider_attempt = Some(attempt);
        resident.st.head.active_phase = Some(if is_compaction {
            "ready_to_compact".into()
        } else {
            "ready_to_build_model_request".into()
        });
        commit(brain, session_id, &mut resident.st, records).await?;
        return Ok(());
    }

    resident.st.head.state = resident.st.head.lifecycle_after_turn();
    resident.st.head.turn = None;
    resident.st.head.active_phase = None;
    resident.st.head.provider_attempt = None;
    resident.st.head.active_context.clear();
    let rounds = resident.st.head.active_rounds;
    let tool_calls = resident.st.head.active_tool_calls;
    resident.st.head.active_rounds = 0;
    resident.st.head.active_tool_calls = 0;
    records.push((
        resident.st.take_seq(),
        Record::TurnCompleted {
            turn: turn.clone(),
            stop_reason: "interrupted".into(),
            rounds,
            tool_calls,
            result: None,
        },
    ));
    records.push((
        resident.st.take_seq(),
        Record::State {
            state: resident.st.head.state.clone(),
            turn: None,
        },
    ));
    commit(brain, session_id, &mut resident.st, records).await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActorStartup {
    Lazy,
    Recovery,
}

fn can_discard_under_pressure(resident: &Option<Resident>) -> bool {
    !has_pending_terminal_ack(resident)
        && !has_reserved_storage_upload(resident)
        && resident.as_ref().is_none_or(|resident| {
            resident.st.head.state != "deleting"
                && resident.st.head.active_phase.is_none()
                && resident.st.head.turn.is_none()
        })
}

fn discard_if_fenced<T>(result: &Result<T>, resident: &mut Option<Resident>) {
    if matches!(result, Err(BrainError::Fenced)) {
        *resident = None;
    }
}

async fn try_discard_resident(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> bool {
    let Some(current) = resident.take() else {
        return true;
    };
    let mut released = false;
    for delay_ms in [10, 25, 50] {
        match brain.journal.release(session_id, &current.st.lease).await {
            Ok(()) | Err(BrainError::Fenced) => {
                released = true;
                break;
            }
            Err(error) => {
                tracing::warn!(
                    session = %session_id,
                    error = %error,
                    "idle resident lease release will retry"
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
    if !released {
        *resident = Some(current);
        return false;
    }

    let freed: usize = current
        .st
        .history
        .iter()
        .map(|message| message.heap_bytes())
        .sum();
    drop(current);
    if reclaim_policy().freed(freed as u64).is_some() {
        tracing::debug!(freed, "malloc_trim after session drop");
    }
    true
}

async fn actor(
    brain: Arc<Brain>,
    session_id: String,
    mut rx: mpsc::Receiver<Command>,
    startup: ActorStartup,
) {
    let mut resident: Option<Resident> = None;
    let mut running: Option<Running> = None;
    if startup != ActorStartup::Lazy {
        match hydrate(&brain, &session_id).await {
            Ok(r) => {
                if r.st.head.state == "deleting" {
                    resident = Some(r);
                    if let Err(error) = delete_session(&brain, &session_id, &mut resident).await {
                        tracing::warn!(session = %session_id, error = %error, "background session deletion will retry");
                    }
                    return;
                } else if r.st.head.state == "ending" {
                    resident = Some(r);
                    match continue_end_session(&brain, &session_id, &mut resident).await {
                        Ok(true) => {
                            // End is complete and quiescent. Keep serving if the narrow lease
                            // release transiently fails; the ordinary idle path retries it.
                            if try_discard_resident(&brain, &session_id, &mut resident).await {
                                return;
                            }
                        }
                        Ok(false) => {
                            if brain.journal.defer_recovery(&session_id).await.is_ok() {
                                return;
                            }
                        }
                        Err(error) => {
                            tracing::warn!(session = %session_id, error = %error, "background session end will retry");
                            if brain.journal.defer_recovery(&session_id).await.is_ok() {
                                return;
                            }
                        }
                    }
                } else {
                    // Create may stage/prepare immutable code, but neither create nor background
                    // recovery materializes a target. Only the first managed operation or explicit
                    // default-sandbox materialization crosses that boundary.
                    resident = Some(r);
                }
            }
            Err(e) => {
                if !matches!(&e, BrainError::Fenced) {
                    tracing::warn!(session = %session_id, error = %e, "eager hydrate failed; durable recovery remains due");
                    if let Err(backoff_error) = brain.journal.defer_recovery(&session_id).await
                        && !matches!(&backoff_error, BrainError::Fenced)
                    {
                        tracing::warn!(session = %session_id, error = %backoff_error, "could not persist recovery backoff");
                    }
                }
                // Do not leave a dead actor resident after a failed background claim/hydrate.
                // Its inbox closes, the supervisor removes it, and the unchanged due key is
                // retried after the prior lease expires.
                return;
            }
        }
    }

    loop {
        tokio::select! {
            done = async { (&mut running.as_mut().expect("guarded").handle).await }, if running.is_some() => {
                let task = running.take().expect("running");
                resident = settle_running(
                    &brain,
                    &session_id,
                    task.key,
                    task.managed_bindings,
                    task.root_secrets,
                    task.message_replays,
                    done,
                )
                .await;
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    Command::Message { content, metadata, idempotency, reply } => {
                        if let Some(task) = &running {
                            let response = match replay_message(
                                &task.message_replays,
                                idempotency.as_ref(),
                            ) {
                                Ok(Some(replay)) => Ok(replay),
                                Ok(None) => Err(BrainError::TurnInFlight(session_id.clone())),
                                Err(error) => Err(error),
                            };
                            let _ = reply.send(response);
                            continue;
                        }
                        let r = match ensure_resident(&brain, &session_id, &mut resident).await {
                            Ok(r) => r,
                            Err(e) => { let _ = reply.send(Err(e)); continue; }
                        };
                        match replay_message(&r.message_replays, idempotency.as_ref()) {
                            Ok(Some(replay)) => {
                                let _ = reply.send(Ok(replay));
                                continue;
                            }
                            Ok(None) => {}
                            Err(error) => {
                                let _ = reply.send(Err(error));
                                continue;
                            }
                        }
                        let permit = match brain.turn_permits.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => { let _ = reply.send(Err(BrainError::Overloaded)); continue; }
                        };
                        let admitted_content = content.clone();
                        match admit(
                            &brain,
                            &session_id,
                            r,
                            content,
                            metadata.clone(),
                            idempotency.clone(),
                        )
                        .await
                        {
                            Ok((turn_id, seq, cancel)) => {
                                if let Some(identity) = &idempotency {
                                    r.message_replays.insert(
                                        identity.key_hash.clone(),
                                        MessageReplay {
                                            request_hash: identity.request_hash.clone(),
                                            turn_id: turn_id.clone(),
                                            user_seq: seq,
                                        },
                                    );
                                }
                                let _ = reply.send(Ok((turn_id.clone(), seq)));
                                let admitted = Some(crate::turn::AdmittedMessage {
                                    seq,
                                    at_ms: crate::wall_ms(),
                                    content: admitted_content,
                                });
                                // Park the resident state into the turn task; the key rides
                                // the running tuple until the task returns the state.
                                let mut parked = resident.take().expect("resident");
                                let run = match turn_run(&brain, &session_id, &turn_id, &parked, metadata, cancel.clone(), admitted) {
                                    Ok(run) => run,
                                    Err(e) => {
                                        let _ = fail_turn_now(&brain, &session_id, &turn_id, &mut parked.st, &e).await;
                                        resident = Some(parked);
                                        continue;
                                    }
                                };
                                let key = parked.key.clone();
                                let managed_bindings = parked.managed_bindings.clone();
                                let root_secrets = parked._root_secrets.clone();
                                let heartbeat_lease = parked.st.lease.clone();
                                let message_replays = std::mem::take(&mut parked.message_replays);
                                let handle = tokio::spawn(async move {
                                    let _permit = permit; // held for the whole turn (admission)
                                    let mut st = parked.st;
                                    let out = run.run(&mut st).await;
                                    (st, RunningOutcome::Turn { turn_id: turn_id.clone(), outcome: out })
                                });
                                running = Some(Running {
                                    handle,
                                    cancel: cancel.clone(),
                                    key,
                                    managed_bindings,
                                    root_secrets,
                                    message_replays,
                                    _heartbeat: start_lease_heartbeat(
                                        &brain,
                                        &session_id,
                                        &heartbeat_lease,
                                        true,
                                        Some(cancel),
                                    ),
                                });
                            }
                            Err(e) => {
                                if matches!(e, BrainError::Fenced) && resident.take().is_some() {
                                    // A conditional ancestor/end decision proved this resident
                                    // fold stale after local TurnStarted mutation. Stop admitting
                                    // new commands here; buffered commands drain through a cold
                                    // hydrate and the actor is then reclaimed.
                                    rx.close();
                                }
                                let _ = reply.send(Err(e));
                            }
                        }
                    }
                    Command::Cancel { reply } => {
                        if let Some(task) = &running {
                            task.cancel.cancel();
                        }
                        let doc = match &resident {
                            Some(r) => r.st.head.clone(),
                            None => match brain.journal.get_head(&session_id).await {
                                Ok(h) => h.doc,
                                Err(e) => { let _ = reply.send(Err(e)); continue; }
                            },
                        };
                        let _ = reply.send(Ok(doc));
                    }
                    Command::End { reply } => {
                        match begin_end_session(&brain, &session_id, &mut resident).await {
                            Ok(doc) => {
                                // The durable fence supersedes the parked turn's lease before we
                                // signal cancellation. A cancellation-resistant provider/Hand can
                                // finish its external wait, but every late journal decision loses
                                // the fence and descendants already observe admission closed.
                                if let Some(task) = running.take() {
                                    task.cancel.cancel();
                                    drop(task);
                                }
                                let pending = doc.state == "ending";
                                // The response proves the constant-size durable admission fence;
                                // descendant traversal and Hand release happen only afterwards.
                                let _ = reply.send(Ok(doc));
                                if pending {
                                    match continue_end_session(
                                        &brain,
                                        &session_id,
                                        &mut resident,
                                    )
                                    .await
                                    {
                                        Ok(true) => {}
                                        Ok(false) => {
                                            if brain.journal.defer_recovery(&session_id).await.is_ok() {
                                                break;
                                            }
                                        }
                                        Err(error) => {
                                            tracing::warn!(session = %session_id, error = %error, "session end will retry");
                                            if brain.journal.defer_recovery(&session_id).await.is_ok() {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => { let _ = reply.send(Err(e)); }
                        }
                    }
                    Command::MaterializeDefaultSandbox { reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_materialize_default_sandbox(
                            &brain,
                            &session_id,
                            &mut resident,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::WriteDefaultSandboxFile {
                        operation_id,
                        generation,
                        path,
                        content_base64,
                        overwrite,
                        reply,
                    } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_write_default_sandbox_file(
                            &brain,
                            &session_id,
                            &mut resident,
                            operation_id,
                            generation,
                            path,
                            content_base64,
                            overwrite,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::CopyStorageToDefaultSandbox {
                        operation_id,
                        generation,
                        key,
                        path,
                        overwrite,
                        reply,
                    } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_copy_storage_to_default_sandbox(
                            &brain,
                            &session_id,
                            &mut resident,
                            operation_id,
                            generation,
                            key,
                            path,
                            overwrite,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::CopyDefaultSandboxToStorage {
                        operation_id,
                        generation,
                        key,
                        path,
                        overwrite,
                        reply,
                    } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_copy_default_sandbox_to_storage(
                            &brain,
                            &session_id,
                            &mut resident,
                            operation_id,
                            generation,
                            key,
                            path,
                            overwrite,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::CreateChild {
                        prompt,
                        name,
                        fork_turns,
                        idempotency_key,
                        reply,
                    } => {
                        let out = create_child_session(
                            &brain,
                            &session_id,
                            prompt,
                            name,
                            fork_turns,
                            idempotency_key.as_deref(),
                        )
                        .await;
                        let _ = reply.send(out);
                    }
                    Command::Delete { queued, reply } => {
                        if let Some(task) = running.take() {
                            task.cancel.cancel();
                            let key = task.key;
                            let managed_bindings = task.managed_bindings;
                            let root_secrets = task.root_secrets;
                            let message_replays = task.message_replays;
                            let done = task.handle.await;
                            resident = settle_running(
                                &brain,
                                &session_id,
                                key,
                                managed_bindings,
                                root_secrets,
                                message_replays,
                                done,
                            )
                            .await;
                        }
                        if queued {
                            match begin_delete_session(&brain, &session_id, &mut resident).await {
                                Ok(()) => {
                                    let _ = reply.send(Ok(()));
                                    if let Err(error) = continue_delete_session(
                                        &brain,
                                        &session_id,
                                        &mut resident,
                                    )
                                    .await
                                    {
                                        tracing::warn!(session = %session_id, error = %error, "queued session deletion will retry");
                                        let _ = brain.journal.defer_recovery(&session_id).await;
                                    }
                                }
                                Err(error) => {
                                    let _ = reply.send(Err(error));
                                }
                            }
                        } else {
                            let out = delete_session(&brain, &session_id, &mut resident).await;
                            let _ = reply.send(out);
                        }
                        break; // complete or durable state=deleting; recovery owns any retry
                    }
                    Command::PrepareStorageUpload { request, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_prepare_storage_upload(
                            &brain,
                            &session_id,
                            &mut resident,
                            request,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::CompleteStorageUpload { transfer_id, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_complete_storage_upload(
                            &brain,
                            &session_id,
                            &mut resident,
                            transfer_id,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::ReconcileStorage { reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = match ensure_resident(&brain, &session_id, &mut resident).await {
                            Ok(r) => reconcile_storage_mutations(&brain, &session_id, &mut r.st).await,
                            Err(error) => Err(error),
                        };
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::WriteStorageInline { key, content_base64, content_type, overwrite, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_write_storage_inline(
                            &brain,
                            &session_id,
                            &mut resident,
                            key,
                            content_base64,
                            content_type,
                            overwrite,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::DeleteStorageObject { key, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_delete_storage_object(
                            &brain,
                            &session_id,
                            &mut resident,
                            key,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(5)), if running.is_none() && has_pending_terminal_ack(&resident) => {
                if let Some(r) = &mut resident {
                    let managed = recover_managed_terminal_acks(&brain, &session_id, r).await;
                    let customer = recover_customer_terminal_acks(&brain, &session_id, r).await;
                    if let Err(error) = &managed {
                        tracing::warn!(session = %session_id, error = %error, "managed terminal acknowledgement retry remains pending");
                    }
                    if let Err(error) = &customer {
                        tracing::warn!(session = %session_id, error = %error, "customer terminal acknowledgement retry remains pending");
                    }
                    if (managed.is_err() || customer.is_err())
                        && brain.journal.defer_recovery(&session_id).await.is_ok()
                    {
                        resident.take();
                        // Stop admitting new commands to this actor after preserving a durable
                        // recovery anchor. Already-buffered commands rehydrate; new sends retry
                        // against a fresh actor.
                        rx.close();
                    }
                }
            }
            _ = sleep_until_storage_expiry(&resident), if running.is_none() && has_reserved_storage_upload(&resident) => {
                if let Some(r) = &mut resident
                    && let Err(error) = expire_storage_upload(&brain, &session_id, &mut r.st).await
                {
                    tracing::warn!(session = %session_id, error = %error, "expired storage upload cleanup will retry");
                    // Avoid a tight loop after a transient adapter failure while preserving the
                    // durable reservation. The persisted expiry remains due and is retried after
                    // this bounded delay or on explicit reconcile.
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
            _ = brain.resident_pressure.notified(), if running.is_none() && can_discard_under_pressure(&resident) => {
                // Cooperative pressure eviction: only the actor itself decides it is safe to
                // release. Close after the lease release succeeds; commands already buffered
                // drain and rehydrate, while new sends retry against another resident slot.
                if try_discard_resident(&brain, &session_id, &mut resident).await {
                    rx.close();
                }
            }
            _ = tokio::time::sleep(brain.cfg.idle_discard), if running.is_none() && !has_reserved_storage_upload(&resident) && !has_pending_terminal_ack(&resident) => {
                // Idle: discard the fold, release the lease, drop the adapter. The substrate
                // does its own idling; the journal holds everything.
                //
                // The exit is a CLOSE-THEN-DRAIN, not a break: the idle timer and an incoming
                // command can be ready in the same select round, and a command already
                // buffered in the channel when we exit would have its reply dropped — the
                // caller sees "actor dropped the reply" (a 500) at exactly the discard
                // boundary. close() refuses NEW sends (the caller's send fails and it retries
                // against a fresh actor); everything already buffered still drains through
                // the normal arms above (they rehydrate on demand), and recv() then yields
                // None, which is the loop's exit.
                if try_discard_resident(&brain, &session_id, &mut resident).await {
                    rx.close();
                }
            }
        }
    }
    tracing::debug!(session = %session_id, "actor exited");
}

async fn settle_running(
    brain: &Arc<Brain>,
    session_id: &str,
    key: ProviderKey,
    managed_bindings: Arc<HashMap<String, brain_protocol::hand::ResolvedBinding>>,
    root_secrets: Arc<RootSecretCell>,
    message_replays: HashMap<String, MessageReplay>,
    done: std::result::Result<(TurnState, RunningOutcome), tokio::task::JoinError>,
) -> Option<Resident> {
    match done {
        Ok((
            _st,
            RunningOutcome::Turn {
                outcome: Err(BrainError::Fenced),
                ..
            },
        )) => {
            // A conditional ancestor/end race or newer owner made this fold stale. Never return
            // its locally mutated HEAD/history to the actor; the next command must cold-claim the
            // authoritative journal (or observe the still-live newer lease).
            None
        }
        Ok((
            st,
            RunningOutcome::Turn {
                outcome: Err(error),
                ..
            },
        )) if st.head.active_phase.as_deref() == Some("managed_running")
            && matches!(
                error,
                BrainError::HandUnavailable(_) | BrainError::Cancelled
            ) =>
        {
            // The managed intent is already durable and the effect may have crossed the Hand
            // boundary. Never manufacture a failed turn here. Release the lease with a due
            // anchor; exact submit/observe recovery reconstructs the terminal before the turn
            // can advance.
            if let Err(schedule_error) = brain.journal.defer_recovery(session_id).await
                && !matches!(schedule_error, BrainError::Fenced)
            {
                tracing::warn!(
                    session = session_id,
                    error = %schedule_error,
                    "managed operation recovery could not persist its immediate retry anchor"
                );
            }
            None
        }
        Ok((st, outcome)) => {
            let mut resident = Resident {
                st,
                key,
                managed_bindings,
                _root_secrets: root_secrets,
                message_replays,
            };
            match outcome {
                RunningOutcome::Turn { turn_id, outcome } => {
                    finish_turn(brain, session_id, &turn_id, &mut resident.st, outcome).await;
                }
            }
            Some(resident)
        }
        Err(join_error) => {
            tracing::error!(session = %session_id, error = %join_error, "session task panicked");
            // The fold moved into the task and is gone. Rehydrate immediately so the actor can
            // continue serving the durable session state.
            match hydrate(brain, session_id).await {
                Ok(resident) => Some(resident),
                Err(error) => {
                    tracing::error!(session = %session_id, error = %error, "session rehydrate after panic failed");
                    None
                }
            }
        }
    }
}

async fn ensure_resident<'a>(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &'a mut Option<Resident>,
) -> Result<&'a mut Resident> {
    if resident.is_none() {
        *resident = Some(hydrate(brain, session_id).await?);
    }
    Ok(resident.as_mut().expect("just set"))
}

/// Rebuilds a resident session from the journal: claim -> read -> fold -> decrypt -> open
/// the adapter from its persisted state.
async fn hydrate(brain: &Arc<Brain>, session_id: &str) -> Result<Resident> {
    let mut head = brain.journal.claim(session_id).await?;
    if head.doc.state == "deleted" {
        return Err(BrainError::SessionDeleted(session_id.into()));
    }
    let _heartbeat = start_lease_heartbeat(
        brain,
        session_id,
        &Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        },
        matches!(head.doc.state.as_str(), "ending" | "deleting")
            || head.doc.turn.is_some()
            || head.doc.active_phase.is_some()
            || head.doc.storage_delete.is_some()
            || !head.doc.pending_customer_acks.is_empty()
            || !head.doc.pending_managed_acks.is_empty()
            || head
                .doc
                .storage_upload
                .as_ref()
                .is_some_and(|upload| upload.state == "published"),
        None,
    );
    let context_after = head
        .doc
        .context
        .as_ref()
        .map_or(0, |context| context.chunk_start_sequence.saturating_sub(1));
    if let Some(context) = &head.doc.context
        && context.base_prefix_digest != head.doc.prefix.rendered_base_digest
    {
        return Err(BrainError::Journal(
            "installed context references a different provider base segment".into(),
        ));
    }
    let mut entries = brain
        .journal
        .read_records_through(session_id, context_after, head.last_seq)
        .await?;

    // A successfully claimed session has no live previous owner. Any unanswered volatile intent
    // is ambiguous and is answered as interrupted, never replayed. Nested results remain
    // audit-only because Fold excludes non-root agents.
    let interrupted = pending_volatile(&entries, &head.doc.prefix);
    if !interrupted.is_empty() {
        let active_turn = head.doc.turn.clone();
        let interrupted_root_turn = active_turn.as_deref().is_some_and(|turn| {
            interrupted
                .iter()
                .any(|task| task.agent == "root" && task.turn == turn)
        });
        let mut next_seq = head.last_seq + 1;
        let mut records = Vec::with_capacity(interrupted.len() + 2);
        for task in &interrupted {
            records.push((
                next_seq,
                Record::ToolResult {
                    turn: task.turn.clone(),
                    agent: task.agent.clone(),
                    call: task.call.clone(),
                    name: task.name.clone(),
                    outcome: "interrupted".into(),
                    content: "tool executor disconnected while the session was not resident; the call was not replayed".into(),
                    is_error: true,
                    exit_code: None,
                    duration_ms: 0,
                    truncated: false,
                },
            ));
            next_seq += 1;
        }
        if interrupted_root_turn {
            let turn = active_turn.expect("checked above");
            let rounds = entries
                .iter()
                .filter(|entry| {
                    matches!(
                        &entry.record,
                        Record::Assistant { turn: record_turn, agent, .. }
                            if record_turn == &turn && agent == "root"
                    )
                })
                .count() as u64;
            let tool_calls = entries
                .iter()
                .filter(|entry| {
                    matches!(
                        &entry.record,
                        Record::ToolCall { turn: record_turn, agent, .. }
                            if record_turn == &turn && agent == "root"
                    )
                })
                .count() as u64;
            records.push((
                next_seq,
                Record::TurnCompleted {
                    turn: turn.clone(),
                    stop_reason: "interrupted".into(),
                    rounds,
                    tool_calls,
                    result: None,
                },
            ));
            next_seq += 1;
            records.push((
                next_seq,
                Record::State {
                    state: head.doc.lifecycle_after_turn(),
                    turn: None,
                },
            ));
            next_seq += 1;
            head.doc.state = head.doc.lifecycle_after_turn();
            head.doc.turn = None;
            head.doc.active_phase = None;
            head.doc.provider_attempt = None;
        }
        head.doc.updated_ms = crate::wall_ms();

        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        let high_water = next_seq - 1;
        head.doc = brain
            .journal
            .commit(session_id, &mut lease, &records, &head.doc, high_water)
            .await?;
        let now = crate::wall_ms();
        for (seq, record) in &records {
            if let Some(event) = crate::events::derive(session_id, *seq, now, record) {
                brain.hub.publish(session_id, event);
            }
        }
        head.last_seq = lease.last_seq;
        entries = brain
            .journal
            .read_records_through(session_id, context_after, head.last_seq)
            .await?;
    }

    let message_replays = collect_message_replays(&head.doc, &entries)?;
    let history = materialize_session_history(brain, &head.doc, &entries).await?;
    // Loop kv/mark state folds from the full journal, independent of the model-context floor
    // (a kv write from a long-compacted turn must survive). Sessions that never committed a
    // loop record skip the read entirely — the HEAD marker gates it.
    let mut loop_kv = serde_json::Map::new();
    let mut latest_mark = None;
    if head.doc.loop_state.is_some() {
        for entry in brain.journal.read_records(session_id, 0).await? {
            crate::turn::apply_loop_record(
                &mut loop_kv,
                &mut latest_mark,
                entry.seq,
                &entry.record,
            );
        }
    }
    let (root_secret_cell, root_secrets) = brain.root_execution_secrets(&head.doc).await?;
    let key = root_secrets.key.clone();
    // A deleting session is past the subtree END barrier and can never execute another turn.
    // Re-preparing its managed bindings here recreates Hand definition rows immediately before
    // `purge_tree` removes them, so every cold deletion retry sees another nonempty purge page.
    let managed_bindings = if head.doc.state == "deleting" {
        Arc::new(HashMap::new())
    } else {
        brain.prepare_managed_session(session_id, &head.doc).await?
    };
    let persisted_head = head.doc.clone();
    let mut resident = Resident {
        st: TurnState {
            history,
            head: head.doc,
            persisted_head,
            lease: Lease {
                fence: head.fence,
                last_seq: head.last_seq,
                retention: head.retention,
            },
            seq: Arc::new(std::sync::atomic::AtomicU64::new(head.last_seq + 1)),
            loop_kv,
            latest_mark,
            pending_loop: Vec::new(),
        },
        key,
        managed_bindings,
        _root_secrets: root_secret_cell,
        message_replays,
    };
    if let Err(error) = recover_customer_terminal_acks(brain, session_id, &mut resident).await {
        tracing::warn!(session = session_id, error = %error, "customer terminal acknowledgement will retry while resident");
    }
    if let Err(error) = recover_managed_terminal_acks(brain, session_id, &mut resident).await {
        tracing::warn!(session = session_id, error = %error, "managed terminal acknowledgement will retry while resident");
    }
    if recover_customer_calls(brain, session_id, &mut resident, &entries).await? {
        entries = brain
            .journal
            .read_records_through(session_id, context_after, resident.st.head.last_seq)
            .await?;
        resident.st.history =
            materialize_session_history(brain, &resident.st.head, &entries).await?;
    }
    if recover_managed_calls(brain, session_id, &mut resident, &entries).await? {
        entries = brain
            .journal
            .read_records_through(session_id, context_after, resident.st.head.last_seq)
            .await?;
        resident.st.history =
            materialize_session_history(brain, &resident.st.head, &entries).await?;
    }
    if resident.st.head.root_id == session_id {
        let state = resident
            .st
            .head
            .default_sandbox
            .as_ref()
            .map(|status| status.state);
        if state == Some(brain_protocol::hand::SandboxState::Creating) {
            materialize_default_sandbox_resident(brain, session_id, &mut resident).await?;
        } else {
            reconcile_default_sandbox_expiry(brain, session_id, &mut resident).await?;
        }
    }
    recover_provider_attempt(brain, session_id, &mut resident).await?;
    let recovered_external =
        recover_external_calls(brain, session_id, &mut resident, &entries).await?;
    // Provider-only crashes have no external Tool receipt to recover. Replacement authorization
    // above is nevertheless a complete durable transition, so drive every still-active root turn
    // rather than accidentally waiting for a later customer request forever.
    let recovered = recovered_external.or_else(|| {
        resident.st.head.turn.clone().map(|turn| RecoveredTurn {
            turn,
            context: resident.st.head.active_context.clone(),
            rounds: resident.st.head.active_rounds,
            tool_calls: resident.st.head.active_tool_calls,
        })
    });
    if let Some(recovered) = recovered {
        let permit = brain
            .turn_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| BrainError::Overloaded)?;
        // A recovered turn rebuilds its admitted message from the journal when the record is
        // still ahead of the context floor; contract activations fail honestly otherwise.
        let recovered_message = entries.iter().find_map(|entry| match &entry.record {
            Record::UserMessage { turn, content, .. } if *turn == recovered.turn => {
                Some(crate::turn::AdmittedMessage {
                    seq: entry.seq,
                    at_ms: entry.ts_ms,
                    content: content.clone(),
                })
            }
            _ => None,
        });
        let run = turn_run(
            brain,
            session_id,
            &recovered.turn,
            &resident,
            recovered.context,
            CancellationToken::new(),
            recovered_message,
        )?;
        let outcome = run
            .resume(&mut resident.st, recovered.rounds, recovered.tool_calls)
            .await;
        drop(permit);
        finish_turn(
            brain,
            session_id,
            &recovered.turn,
            &mut resident.st,
            outcome,
        )
        .await;
    }
    Ok(resident)
}

async fn recover_customer_terminal_acks(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    let Some(customer) = &brain.customer else {
        return if resident.st.head.pending_customer_acks.is_empty() {
            Ok(())
        } else {
            Err(BrainError::HandUnavailable(
                "customer coordinator is unavailable for a durable terminal acknowledgement".into(),
            ))
        };
    };
    let pending = resident.st.head.pending_customer_acks.clone();
    let mut acknowledged = Vec::new();
    for item in pending {
        let receipt = crate::customer::CustomerTerminalReceipt {
            operation_id: item.call.clone(),
            request_digest: item.request_digest.clone(),
            terminal_digest: item.terminal_digest.clone(),
            process_id: item.process_id.clone(),
        };
        match customer
            .acknowledge_durable_terminal(&resident.st.head.tenant_id, &item.client_id, &receipt)
            .await
        {
            Ok(()) => acknowledged.push(item),
            Err(error) => tracing::warn!(
                session = session_id,
                operation = %receipt.operation_id,
                error = %error,
                "durable customer terminal acknowledgement remains pending"
            ),
        }
    }
    if !acknowledged.is_empty() {
        let records = acknowledged
            .iter()
            .map(|item| {
                (
                    resident.st.take_seq(),
                    Record::CustomerTerminalAcknowledged {
                        turn: item.turn.clone(),
                        call: item.call.clone(),
                        request_digest: item.request_digest.clone(),
                        terminal_digest: item.terminal_digest.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        resident.st.head.pending_customer_acks.retain(|current| {
            !acknowledged.iter().any(|item| {
                current.call == item.call
                    && current.request_digest == item.request_digest
                    && current.terminal_digest == item.terminal_digest
            })
        });
        commit(brain, session_id, &mut resident.st, records).await?;
    }
    if resident.st.head.pending_customer_acks.is_empty() {
        Ok(())
    } else {
        Err(BrainError::HandUnavailable(
            "customer terminal acknowledgement remains pending".into(),
        ))
    }
}

async fn recover_managed_terminal_acks(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    let Some(hand) = &brain.hand else {
        return if resident.st.head.pending_managed_acks.is_empty() {
            Ok(())
        } else {
            Err(BrainError::HandUnavailable(
                "managed Hand is unavailable for a durable terminal acknowledgement".into(),
            ))
        };
    };
    let pending = resident.st.head.pending_managed_acks.clone();
    let mut acknowledged = Vec::new();
    for item in pending {
        let terminal_digest = item.terminal_digest.parse().map_err(|error| {
            BrainError::Protocol(format!("persisted managed terminal digest: {error}"))
        })?;
        let request = brain_protocol::hand::AcknowledgeTerminalRequest {
            operation: item.operation.clone(),
            terminal_digest,
        };
        match hand.acknowledge_terminal(request).await {
            Ok(ack) if ack.acknowledged => acknowledged.push(item),
            Ok(_) => tracing::warn!(
                session = session_id,
                operation = item.operation.operation_id.as_str(),
                "durable managed terminal acknowledgement was not accepted"
            ),
            Err(error) => tracing::warn!(
                session = session_id,
                operation = item.operation.operation_id.as_str(),
                code = %error.code,
                "durable managed terminal acknowledgement remains pending"
            ),
        }
    }
    if !acknowledged.is_empty() {
        let records = acknowledged
            .iter()
            .map(|item| {
                (
                    resident.st.take_seq(),
                    Record::ManagedTerminalAcknowledged {
                        turn: item.turn.clone(),
                        call: item.call.clone(),
                        request_digest: item.operation.request_digest.to_string(),
                        terminal_digest: item.terminal_digest.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        resident.st.head.pending_managed_acks.retain(|current| {
            !acknowledged.iter().any(|item| {
                current.operation.operation_id == item.operation.operation_id
                    && current.operation.request_digest == item.operation.request_digest
                    && current.terminal_digest == item.terminal_digest
            })
        });
        commit(brain, session_id, &mut resident.st, records).await?;
    }
    if resident.st.head.pending_managed_acks.is_empty() {
        Ok(())
    } else {
        Err(BrainError::HandUnavailable(
            "managed terminal acknowledgement remains pending".into(),
        ))
    }
}

async fn commit(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    records: Vec<(u64, Record)>,
) -> Result<()> {
    st.head.updated_ms = crate::wall_ms();
    let high_water = st
        .seq
        .load(std::sync::atomic::Ordering::Relaxed)
        .saturating_sub(1);
    st.head.last_seq = high_water;
    let mut lease = st.lease.clone();
    let persisted = match brain
        .journal
        .commit(session_id, &mut lease, &records, &st.head, high_water)
        .await
    {
        Ok(persisted) => persisted,
        Err(error) => {
            // All mutations are staged against the last HEAD returned by a successful journal
            // decision. Restore locally rather than depending on a second, fallible strong read.
            // Sequence gaps are harmless; no uncommitted control state may survive in-resident.
            st.head = st.persisted_head.clone();
            return Err(error);
        }
    };
    st.lease = lease;
    st.persisted_head = persisted.clone();
    st.head = persisted;
    let now = crate::wall_ms();
    for (seq, record) in &records {
        if let Some(e) = crate::events::derive(session_id, *seq, now, record) {
            brain.hub.publish(session_id, e);
        }
    }
    Ok(())
}

async fn do_materialize_default_sandbox(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<brain_protocol::hand::SandboxStatus> {
    let r = ensure_resident(brain, session_id, resident).await?;
    materialize_default_sandbox_resident(brain, session_id, r).await
}

async fn default_sandbox_for_effect(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &TurnState,
    generation: &str,
) -> Result<brain_protocol::hand::SandboxTarget> {
    use brain_protocol::hand::SandboxState;

    if st.head.ended || st.head.state != "open" {
        return Err(BrainError::SessionDeleted(session_id.to_owned()));
    }
    let (root_state, root_ended, status) = if st.head.root_id == session_id {
        (
            st.head.state.clone(),
            st.head.ended,
            st.head.default_sandbox.clone(),
        )
    } else {
        let root = brain.journal.get_head(&st.head.root_id).await?;
        (root.doc.state, root.doc.ended, root.doc.default_sandbox)
    };
    if root_ended || root_state != "open" {
        return Err(BrainError::SessionDeleted(st.head.root_id.clone()));
    }
    let status = status.ok_or(BrainError::SandboxNotMaterialized)?;
    if status.generation.as_ref().map(|value| value.as_str()) != Some(generation) {
        return Err(
            if matches!(status.state, SandboxState::Gone | SandboxState::Terminated) {
                BrainError::SandboxGone
            } else {
                BrainError::SandboxGenerationConflict
            },
        );
    }
    if !matches!(
        status.state,
        SandboxState::Running | SandboxState::Suspended
    ) {
        return Err(
            if matches!(status.state, SandboxState::Gone | SandboxState::Terminated) {
                BrainError::SandboxGone
            } else {
                BrainError::SandboxNotMaterialized
            },
        );
    }
    Ok(status.target)
}

#[allow(clippy::too_many_arguments)]
async fn do_write_default_sandbox_file(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    operation_id: String,
    generation: String,
    path: String,
    content_base64: String,
    overwrite: bool,
) -> Result<brain_protocol::hand::FileEntry> {
    let r = ensure_resident(brain, session_id, resident).await?;
    let target = default_sandbox_for_effect(brain, session_id, &r.st, &generation).await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&content_base64)
        .map_err(|_| BrainError::Invalid("sandbox inline content is not valid base64".into()))?;
    let request = sandbox_file_write_request(
        &operation_id,
        &target,
        &generation,
        &path,
        &bytes,
        overwrite,
    )?;
    let request_digest = request.request_digest.clone();
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::SandboxFileEffectIntent {
                operation_id: operation_id.clone(),
                request_digest: request_digest.to_string(),
                action: "write_inline".into(),
                path: path.clone(),
            },
        )],
    )
    .await?;
    let result = brain
        .sandbox_files_port()?
        .write(request)
        .await
        .map_err(|error| {
            if error.code == brain_protocol::hand::HandErrorCode::BindingConflict {
                BrainError::IdempotencyConflict
            } else {
                map_hand_port_error(error)
            }
        })?;
    validate_sandbox_file_write_result(&result, &operation_id, &request_digest)?;
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::SandboxFileEffectCompleted {
                operation_id,
                request_digest: request_digest.to_string(),
                action: "write_inline".into(),
                path,
                replayed: result.replayed,
            },
        )],
    )
    .await?;
    Ok(result.file)
}

#[allow(clippy::too_many_arguments)]
async fn do_copy_storage_to_default_sandbox(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    operation_id: String,
    generation: String,
    key: String,
    path: String,
    overwrite: bool,
) -> Result<brain_protocol::hand::FileEntry> {
    let r = ensure_resident(brain, session_id, resident).await?;
    let target = default_sandbox_for_effect(brain, session_id, &r.st, &generation).await?;
    ensure_storage_readable(&r.st.head, session_id)?;
    let storage = brain.storage_port()?.clone();
    let object = storage.stat(session_id, &key).await?;
    let ticket = storage.prepare_download(session_id, &key).await?;
    if ticket.max_bytes != object.bytes {
        return Err(BrainError::Journal(
            "download authority does not match the stored object size".into(),
        ));
    }
    let reference = storage_object_reference(
        &ticket.object_id,
        object.bytes,
        &object.sha256,
        object.content_type.as_deref(),
    )?;
    let request = sandbox_copy_request(
        &operation_id,
        &target,
        &generation,
        &path,
        Some(reference),
        &ticket,
        "import",
        overwrite,
    )?;
    let request_digest = request.request_digest.clone();
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::SandboxFileEffectIntent {
                operation_id: operation_id.clone(),
                request_digest: request_digest.to_string(),
                action: "storage_to_sandbox".into(),
                path: path.clone(),
            },
        )],
    )
    .await?;
    let result = brain
        .sandbox_files_port()?
        .transfer(request)
        .await
        .map_err(|error| {
            if error.code == brain_protocol::hand::HandErrorCode::BindingConflict {
                BrainError::IdempotencyConflict
            } else {
                map_hand_port_error(error)
            }
        })?;
    validate_sandbox_copy_result(&result, &operation_id, &request_digest)?;
    if result.object.is_some() {
        return Err(BrainError::Hand(
            "sandbox import returned an unexpected object identity".into(),
        ));
    }
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::SandboxFileEffectCompleted {
                operation_id,
                request_digest: request_digest.to_string(),
                action: "storage_to_sandbox".into(),
                path,
                replayed: result.replayed,
            },
        )],
    )
    .await?;
    Ok(result.file)
}

#[allow(clippy::too_many_arguments)]
async fn do_copy_default_sandbox_to_storage(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    operation_id: String,
    generation: String,
    key: String,
    path: String,
    overwrite: bool,
) -> Result<crate::storage::StorageObject> {
    let r = ensure_resident(brain, session_id, resident).await?;
    let target = default_sandbox_for_effect(brain, session_id, &r.st, &generation).await?;
    let files = brain.sandbox_files_port()?.clone();
    let entry = files
        .stat(sandbox_file_request(&target, &generation, &path)?)
        .await
        .map_err(map_hand_port_error)?;
    if entry.kind != brain_protocol::hand::FileEntryKind::File {
        return Err(BrainError::Invalid(
            "sandbox copy source must be a regular file".into(),
        ));
    }
    let transfer_digest = hash_create_key(&format!(
        "aex.sandbox-storage-transfer.v1\0{session_id}\0{operation_id}"
    ));
    let transfer_id = format!("xfer_{}", &transfer_digest[..24]);
    let ticket = prepare_storage_upload_state_for_transfer(
        brain,
        session_id,
        &mut r.st,
        crate::storage::StorageUploadIntent {
            key,
            bytes: entry.bytes,
            sha256: None,
            content_type: None,
            overwrite,
        },
        Some(transfer_id),
    )
    .await?;
    let request = sandbox_copy_request(
        &operation_id,
        &target,
        &generation,
        &path,
        None,
        &ticket,
        "export",
        false,
    )?;
    let request_digest = request.request_digest.clone();
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::SandboxFileEffectIntent {
                operation_id: operation_id.clone(),
                request_digest: request_digest.to_string(),
                action: "sandbox_to_storage".into(),
                path: path.clone(),
            },
        )],
    )
    .await?;
    let result = files.transfer(request).await.map_err(|error| {
        if error.code == brain_protocol::hand::HandErrorCode::BindingConflict {
            BrainError::IdempotencyConflict
        } else {
            map_hand_port_error(error)
        }
    })?;
    validate_sandbox_copy_result(&result, &operation_id, &request_digest)?;
    let exported = result.object.as_ref().ok_or_else(|| {
        BrainError::Hand("sandbox export omitted its uploaded object identity".into())
    })?;
    if exported.object_id.as_str() != ticket.object_id || exported.bytes != entry.bytes {
        return Err(BrainError::Hand(
            "sandbox export returned a different object identity".into(),
        ));
    }
    let object =
        complete_storage_upload_state(brain, session_id, &mut r.st, ticket.transfer_id).await?;
    if object.bytes != exported.bytes || object.sha256 != exported.sha256.as_str() {
        return Err(BrainError::Journal(
            "published storage object differs from the sandbox export".into(),
        ));
    }
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::SandboxFileEffectCompleted {
                operation_id,
                request_digest: request_digest.to_string(),
                action: "sandbox_to_storage".into(),
                path,
                replayed: result.replayed,
            },
        )],
    )
    .await?;
    Ok(object)
}

async fn materialize_default_sandbox_resident(
    brain: &Arc<Brain>,
    session_id: &str,
    r: &mut Resident,
) -> Result<brain_protocol::hand::SandboxStatus> {
    use brain_protocol::hand::SandboxState;

    if r.st.head.root_id != session_id {
        return Err(BrainError::Invalid(
            "default sandbox materialization must be driven by the root actor".into(),
        ));
    }
    if r.st.head.ended || matches!(r.st.head.state.as_str(), "ending" | "ended" | "deleting") {
        return Err(BrainError::SessionDeleted(session_id.to_owned()));
    }
    let now = crate::wall_ms();
    let current =
        r.st.head
            .default_sandbox
            .clone()
            .unwrap_or(initial_default_sandbox(session_id)?);
    let unexpired = current
        .expires_at_ms
        .is_none_or(|expires| expires.get() > now);
    if matches!(
        current.state,
        SandboxState::Running | SandboxState::Suspended
    ) && unexpired
    {
        return Ok(current);
    }

    let generation_intent = if current.state == SandboxState::Creating {
        current
            .generation
            .as_ref()
            .map(|generation| String::from(generation.clone()))
            .ok_or_else(|| {
                BrainError::Journal("creating default sandbox lacks generation intent".into())
            })?
    } else {
        crate::mint_id("gen", 20)
    };
    let target = default_sandbox_target(session_id)?;
    let creating: brain_protocol::hand::SandboxStatus =
        serde_json::from_value(serde_json::json!({
            "state": "creating",
            "target": target,
            "generation": generation_intent,
            "changed_at_ms": now,
            "expires_at_ms": null,
        }))?;
    if current.state != SandboxState::Creating {
        r.st.head.default_sandbox = Some(creating.clone());
        let seq = r.st.take_seq();
        commit(
            brain,
            session_id,
            &mut r.st,
            vec![(seq, Record::DefaultSandboxChanged { status: creating })],
        )
        .await?;
    }

    let request = default_sandbox_request(&r.st.head, &generation_intent)?;
    let preparation = brain.session_preparation.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable("default sandbox preparation is unavailable".into())
    })?;
    let status = preparation
        .materialize_default(request)
        .await
        .map_err(map_hand_port_error)?;
    if serde_json::to_value(&status.target)?
        != serde_json::to_value(default_sandbox_target(session_id)?)?
    {
        return Err(BrainError::Hand(
            "Hand returned a default sandbox for a different logical target".into(),
        ));
    }
    if matches!(
        status.state,
        SandboxState::Running | SandboxState::Suspended
    ) && (status.generation.is_none()
        || status.target_ref.is_none()
        || status.expires_at_ms.is_none())
    {
        return Err(BrainError::Hand(
            "materialized sandbox receipt lacks generation, target_ref, or expiry".into(),
        ));
    }
    r.st.head.default_sandbox = Some(status.clone());
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::DefaultSandboxChanged {
                status: status.clone(),
            },
        )],
    )
    .await?;
    Ok(status)
}

async fn reconcile_default_sandbox_expiry(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    use brain_protocol::hand::SandboxState;

    let Some(current) = resident.st.head.default_sandbox.clone() else {
        return Ok(());
    };
    if !matches!(
        current.state,
        SandboxState::Running | SandboxState::Suspended
    ) || current
        .expires_at_ms
        .is_none_or(|expires| expires.get() > crate::wall_ms())
    {
        return Ok(());
    }
    let status = if let Some(files) = &brain.sandbox_files {
        match files.status(current.target.clone()).await {
            Ok(status) => status,
            Err(error) if error.code == brain_protocol::hand::HandErrorCode::SandboxGone => {
                serde_json::from_value(serde_json::json!({
                    "state": "gone",
                    "target": current.target,
                    "generation": current.generation,
                    "target_ref": current.target_ref,
                    "changed_at_ms": crate::wall_ms(),
                    "expires_at_ms": current.expires_at_ms,
                    "reason": "hard_expiry",
                }))?
            }
            Err(error) => return Err(map_hand_port_error(error)),
        }
    } else {
        // Local process-backed targets have no independent status port. Their sealed hard
        // deadline is authoritative, so expiration converges to gone without customer traffic.
        serde_json::from_value(serde_json::json!({
            "state": "gone",
            "target": current.target,
            "generation": current.generation,
            "target_ref": current.target_ref,
            "changed_at_ms": crate::wall_ms(),
            "expires_at_ms": current.expires_at_ms,
            "reason": "hard_expiry",
        }))?
    };
    resident.st.head.default_sandbox = Some(status.clone());
    let seq = resident.st.take_seq();
    commit(
        brain,
        session_id,
        &mut resident.st,
        vec![(seq, Record::DefaultSandboxChanged { status })],
    )
    .await
}

/// Admits one message: journals the decision, pokes the adapter, hands back the turn
/// identity. 202 semantics: the reply happens after this commit succeeds.
async fn admit(
    brain: &Arc<Brain>,
    session_id: &str,
    r: &mut Resident,
    content: Vec<ContentBlock>,
    metadata: HashMap<String, String>,
    idempotency: Option<MessageIdentity>,
) -> Result<(String, u64, CancellationToken)> {
    if r.st.head.state == "failed" {
        return Err(BrainError::SessionFailed(
            r.st.head
                .failure
                .as_ref()
                .map(|f| f.message.clone())
                .unwrap_or_default(),
        ));
    }
    let turn_id = crate::mint_id("trn", 24);
    let user_seq = r.st.take_seq();
    let started_seq = r.st.take_seq();
    // Turn activity is a separate axis. The lifecycle remains open while `turn` and
    // `active_phase` identify the admitted work.
    r.st.head.state = "open".into();
    r.st.head.turn = Some(turn_id.clone());
    r.st.head.active_phase = Some("ready_to_build_model_request".into());
    r.st.head.provider_attempt = None;
    r.st.head.active_context = metadata.clone();
    r.st.head.active_rounds = 0;
    r.st.head.active_tool_calls = 0;
    r.st.head.turns += 1;
    r.st.head.last_message_ms = Some(crate::wall_ms());
    let records = vec![
        (
            user_seq,
            Record::UserMessage {
                turn: turn_id.clone(),
                content: content.clone(),
                starts_turn: false,
                metadata,
                idempotency_key_hash: idempotency
                    .as_ref()
                    .map(|identity| identity.key_hash.clone()),
                request_hash: idempotency
                    .as_ref()
                    .map(|identity| identity.request_hash.clone()),
            },
        ),
        (
            started_seq,
            Record::TurnStarted {
                turn: turn_id.clone(),
            },
        ),
    ];
    if let Some(identity) = &idempotency {
        r.st.head
            .message_replays
            .retain(|replay| replay.key_hash != identity.key_hash);
        r.st.head
            .message_replays
            .push(crate::journal::MessageReplayDoc {
                key_hash: identity.key_hash.clone(),
                request_hash: identity.request_hash.clone(),
                turn_id: turn_id.clone(),
                user_seq,
            });
        if r.st.head.message_replays.len() > 64 {
            let excess = r.st.head.message_replays.len() - 64;
            r.st.head.message_replays.drain(..excess);
        }
    }
    commit(brain, session_id, &mut r.st, records).await?;
    if let Some(last) = r.st.history.last_mut()
        && last.role == crate::message::Role::User
        && !last.content.is_empty()
        && last
            .content
            .iter()
            .all(|block| matches!(block, ContentBlock::ToolResult { .. }))
    {
        // A recovered/cancelled turn can stop after tool results. Put the new
        // prompt in that same user message to retain provider role alternation.
        last.content.extend(content);
    } else {
        r.st.history.push(Message {
            role: crate::message::Role::User,
            content,
        });
    }

    Ok((turn_id, user_seq, CancellationToken::new()))
}

fn turn_run(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    r: &Resident,
    context: HashMap<String, String>,
    cancel: CancellationToken,
    message: Option<crate::turn::AdmittedMessage>,
) -> Result<TurnRun> {
    let (prefix, dialect) = build_prefix(&r.st.head.prefix, brain.cfg.default_max_rounds)?;
    let base_url = r.st.head.prefix.base_url.clone().unwrap_or_default();
    let session = SessionConfig::new(prefix.clone(), r.key.clone(), base_url);
    Ok(TurnRun {
        engine: Arc::downgrade(brain),
        agentloop: {
            let default = crate::journal::AgentloopSelectorDoc::official_aex();
            let selector = r.st.head.prefix.agentloop.as_ref().unwrap_or(&default);
            brain.agentloop_registry.resolve(selector)?
        },
        message,
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        prefix,
        session,
        provider: (brain.provider_factory)(dialect),
        provider_name: r.st.head.prefix.provider.clone(),
        outbound: brain.outbound.clone(),
        journal: brain.journal.clone(),
        hub: brain.hub.clone(),
        cancel,
        model_permits: brain.model_permits.clone(),
        context_soft_tokens: r.st.head.prefix.context_soft_tokens as usize,
        context_hard_tokens: r.st.head.prefix.context_hard_tokens as usize,
        context_tail_tokens: r.st.head.prefix.context_tail_tokens as usize,
        context_summary_tokens: r.st.head.prefix.context_summary_tokens as usize,
        context_window_tokens: r.st.head.prefix.context_window_tokens as usize,
        provider_header_timeout: brain.cfg.provider_header_timeout,
        provider_idle_timeout: brain.cfg.provider_idle_timeout,
        provider_total_timeout: brain.cfg.provider_total_timeout,
        compactor: brain.compactor.clone(),
        external_executor: brain.external_executor.clone(),
        hand: brain.hand.clone(),
        managed_bindings: r.managed_bindings.clone(),
        customer: brain.customer.clone(),
        tenant_id: r.st.head.tenant_id.clone(),
        customer_client_id: r.st.head.prefix.customer_client_id.clone(),
        customer_submit_retries: r.st.head.prefix.customer_submit_retries,
        customer_timeout: brain.cfg.external_call_timeout,
        context,
    })
}

/// Applies the turn outcome that `TurnRun::run` could not commit itself (failures). Workspace
/// persistence is always an explicit storage operation; ordinary turns never checkpoint files.
async fn finish_turn(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    st: &mut TurnState,
    outcome: Result<crate::turn::TurnReport>,
) {
    match outcome {
        Ok(report) => {
            tracing::info!(session = %session_id, turn = %turn_id, stop = %report.stop_reason, rounds = report.rounds, "turn done");
        }
        Err(e) => {
            let _ = fail_turn_now(brain, session_id, turn_id, st, &e).await;
        }
    }
}

async fn fail_turn_now(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    st: &mut TurnState,
    e: &BrainError,
) -> Result<()> {
    tracing::warn!(session = %session_id, turn = %turn_id, error = %e, "turn failed");
    let (code, session_fatal) = match e {
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => {
            ("provider_error", false)
        }
        BrainError::HandUnavailable(_) => ("hand_unavailable", false),
        BrainError::Agentloop(_) => ("agentloop_error", false),
        BrainError::SessionFailed(_) => ("session_failed", true),
        BrainError::Fenced => return Ok(()), // a newer owner exists; nothing to write
        _ => ("internal", false),
    };
    let failed_seq = st.take_seq();
    let state_seq = st.take_seq();
    if session_fatal && !st.head.ended {
        st.head.state = "failed".into();
        st.head.failure = Some(FailureDoc {
            code: "binding_conflict".into(),
            message: e.to_string(),
            at_ms: crate::wall_ms(),
        });
    } else {
        st.head.state = st.head.lifecycle_after_turn();
    }
    st.head.turn = None;
    st.head.active_phase = None;
    st.head.provider_attempt = None;
    let records = vec![
        (
            failed_seq,
            Record::TurnFailed {
                turn: turn_id.to_string(),
                code: code.into(),
                message: e.to_string(),
                details: None,
            },
        ),
        (
            state_seq,
            Record::State {
                state: st.head.state.clone(),
                turn: None,
            },
        ),
    ];
    commit(brain, session_id, st, records).await
}

async fn create_child_session(
    brain: &Arc<Brain>,
    parent_id: &str,
    prompt: String,
    name: Option<String>,
    fork_turns: ForkTurns,
    idempotency_key: Option<&str>,
) -> Result<session::Session> {
    let parent = brain.journal.get_head(parent_id).await?;
    let root = brain.journal.get_head(&parent.doc.root_id).await?;
    if parent.doc.ended
        || root.doc.ended
        || matches!(
            parent.doc.state.as_str(),
            "ending" | "ended" | "deleting" | "deleted"
        )
        || matches!(
            root.doc.state.as_str(),
            "ending" | "ended" | "deleting" | "deleted"
        )
    {
        return Err(BrainError::Invalid(
            "child admission is closed for this ended session tree".into(),
        ));
    }
    if parent.doc.depth >= root.doc.prefix.max_child_depth {
        return Err(BrainError::Invalid(format!(
            "child depth would exceed the sealed maximum {}",
            root.doc.prefix.max_child_depth
        )));
    }

    let child_id = idempotency_key
        .map(|key| idempotent_session_id(parent_id, key))
        .unwrap_or_else(|| crate::mint_id("ses", 24));
    let create_request_hash = idempotency_key
        .map(|_| {
            serde_jcs::to_vec(&serde_json::json!({
                "prompt": &prompt,
                "name": &name,
                "fork_turns": fork_turns.request_value(),
            }))
            .map(|canonical| hex::encode(Sha256::digest(canonical)))
        })
        .transpose()?;
    match brain.journal.get_head(&child_id).await {
        Ok(existing) => {
            if existing.doc.parent_id.as_deref() != Some(parent_id)
                || existing.doc.create_request_hash != create_request_hash
            {
                return Err(BrainError::IdempotencyConflict);
            }
            if existing.doc.turn.is_some() {
                let _ = brain.spawn_actor(&child_id, ActorStartup::Recovery).await;
            }
            return session_doc(&child_id, &existing.doc);
        }
        Err(BrainError::NoSuchSession(_)) => {}
        Err(error) => return Err(error),
    }

    let now = crate::wall_ms();
    let context_fork = capture_context_fork(brain, parent_id, &parent.doc, &fork_turns).await?;
    let turn_id = crate::mint_id("trn", 24);
    let content = vec![ContentBlock::text(prompt)];
    // Every descendant inherits one immutable root execution configuration. The ciphertext is
    // root-scoped and shared by reference: child creation never decrypts or re-encrypts provider
    // or managed-Tool secrets. Cold hydration uses `root_id` as the custody scope.
    let key_b64 = root.doc.key_b64.clone();
    let hand_secrets_b64 = root.doc.hand_secrets_b64.clone();
    let mut ancestor_ids = parent.doc.ancestor_ids.clone();
    ancestor_ids.push(parent_id.to_owned());
    let child = HeadDoc {
        loop_state: None,
        tenant_id: parent.doc.tenant_id.clone(),
        root_id: parent.doc.root_id.clone(),
        parent_id: Some(parent_id.to_owned()),
        ancestor_ids,
        child_name: name,
        context_fork: Some(context_fork),
        depth: parent.doc.depth + 1,
        last_seq: 1,
        state: "open".into(),
        failure: None,
        turn: Some(turn_id.clone()),
        active_phase: Some("ready_to_build_model_request".into()),
        provider_attempt: None,
        active_context: HashMap::new(),
        active_rounds: 0,
        active_tool_calls: 0,
        message_replays: Vec::new(),
        context: None,
        turns: 1,
        created_ms: now,
        updated_ms: now,
        recovery_due_ms: None,
        recovery_attempt: 0,
        create_key_hash: idempotency_key.map(hash_create_key),
        create_request_hash,
        last_message_ms: Some(now),
        ended: false,
        prefix: parent.doc.prefix.clone(),
        key_b64,
        hand_secrets_b64,
        session_storage_bytes: 0,
        storage_reserved_bytes: 0,
        tenant_metered_storage_bytes: 0,
        storage_upload: None,
        storage_delete: None,
        pending_customer_acks: Vec::new(),
        pending_managed_acks: Vec::new(),
        default_sandbox: None,
    };
    brain
        .journal
        .create(
            &child_id,
            &child,
            &Record::UserMessage {
                turn: turn_id,
                content,
                starts_turn: true,
                metadata: HashMap::new(),
                idempotency_key_hash: None,
                request_hash: None,
            },
        )
        .await?;
    // The active HEAD is already recovery-indexed by the create transaction. Start immediately
    // in this process, while a crash between here and scheduling remains recoverable with no
    // customer traffic.
    let _ = brain.spawn_actor(&child_id, ActorStartup::Recovery).await;
    let created = brain.journal.get_head(&child_id).await?;
    session_doc(&child_id, &created.doc)
}

fn fork_turn_opener(message: &Message) -> bool {
    message.role == Role::User
        && message
            .content
            .iter()
            .any(|block| !matches!(block, ContentBlock::ToolResult { .. }))
}

/// Return the exact provider-valid prefix that existed before the currently executing assistant
/// decision. In particular, an assistant ToolUse is forkable only together with a following user
/// ToolResult batch that answers every call. This excludes the spawning ToolUse and partial sibling
/// activity while retaining the user/request context that produced that decision.
fn complete_fork_projection(history: &[Message]) -> &[Message] {
    let mut index = 0usize;
    let mut safe_end = 0usize;
    while index < history.len() {
        let message = &history[index];
        match message.role {
            Role::User => {
                if message
                    .content
                    .iter()
                    .all(|block| matches!(block, ContentBlock::ToolResult { .. }))
                {
                    break;
                }
                safe_end = index + 1;
                index += 1;
            }
            Role::Assistant => {
                let calls = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                        _ => None,
                    })
                    .collect::<HashSet<_>>();
                if calls.is_empty() {
                    safe_end = index + 1;
                    index += 1;
                    continue;
                }
                let Some(results) = history.get(index + 1) else {
                    break;
                };
                if results.role != Role::User {
                    break;
                }
                let answered = results
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
                        _ => None,
                    })
                    .collect::<HashSet<_>>();
                if !calls.is_subset(&answered) {
                    break;
                }
                safe_end = index + 2;
                index += 2;
            }
        }
    }
    &history[..safe_end]
}

fn select_fork_history(history: &[Message], fork_turns: &ForkTurns) -> (Vec<Message>, u32) {
    let openers = history
        .iter()
        .enumerate()
        .filter_map(|(index, message)| fork_turn_opener(message).then_some(index))
        .collect::<Vec<_>>();
    match fork_turns {
        ForkTurns::None => (Vec::new(), 0),
        ForkTurns::All => (history.to_vec(), openers.len() as u32),
        ForkTurns::Last(requested) => {
            let count = (*requested as usize).min(openers.len());
            if count == 0 {
                (Vec::new(), 0)
            } else {
                let start = openers[openers.len() - count];
                (history[start..].to_vec(), count as u32)
            }
        }
    }
}

fn fork_mode_doc(fork_turns: &ForkTurns) -> (&'static str, Option<u32>) {
    match fork_turns {
        ForkTurns::All => ("all", None),
        ForkTurns::None => ("none", None),
        ForkTurns::Last(turns) => ("last_n", Some(*turns)),
    }
}

fn fork_mode_from_doc(doc: &ContextForkDoc) -> Result<ForkTurns> {
    match (doc.mode.as_str(), doc.last_n) {
        ("all", None) => Ok(ForkTurns::All),
        ("none", None) => Ok(ForkTurns::None),
        ("last_n", Some(turns)) if turns > 0 => Ok(ForkTurns::Last(turns)),
        _ => Err(BrainError::Journal(
            "child context fork carries an invalid immutable mode".into(),
        )),
    }
}

async fn parent_history_at(
    brain: &Arc<Brain>,
    source_session_id: &str,
    through_seq: u64,
    context: Option<&crate::journal::ContextPointerDoc>,
) -> Result<Vec<Message>> {
    if context.is_some_and(|pointer| pointer.covers_through_sequence > through_seq) {
        return Err(BrainError::Journal(
            "child context fork points before its source checkpoint".into(),
        ));
    }
    let after = context.map_or(0, |pointer| pointer.chunk_start_sequence.saturating_sub(1));
    let entries = brain
        .journal
        .read_records_through(source_session_id, after, through_seq)
        .await?;
    crate::compact::materialize_history(&entries, context)
}

async fn capture_context_fork(
    brain: &Arc<Brain>,
    parent_id: &str,
    parent: &HeadDoc,
    fork_turns: &ForkTurns,
) -> Result<ContextForkDoc> {
    let through_seq = parent.last_seq;
    let history = if matches!(fork_turns, ForkTurns::None) {
        Vec::new()
    } else {
        parent_history_at(brain, parent_id, through_seq, parent.context.as_ref()).await?
    };
    let (selected, resolved_turns) =
        select_fork_history(complete_fork_projection(&history), fork_turns);
    let (mode, last_n) = fork_mode_doc(fork_turns);
    Ok(ContextForkDoc {
        source_session_id: parent_id.to_owned(),
        source_context_generation: parent
            .context
            .as_ref()
            .map_or(0, |context| context.context_generation),
        source_through_sequence: through_seq,
        source_context: parent.context.clone(),
        mode: mode.into(),
        last_n,
        resolved_turns,
        source_projection_digest: hex::encode(Sha256::digest(serde_jcs::to_vec(&selected)?)),
    })
}

async fn materialize_context_fork(brain: &Arc<Brain>, child: &HeadDoc) -> Result<Vec<Message>> {
    let Some(fork) = &child.context_fork else {
        return Ok(Vec::new());
    };
    if fork
        .source_context
        .as_ref()
        .map_or(0, |context| context.context_generation)
        != fork.source_context_generation
    {
        return Err(BrainError::Journal(
            "child context fork generation does not match its checkpoint pointer".into(),
        ));
    }
    if fork.mode == "none" {
        let empty_digest = hex::encode(Sha256::digest(serde_jcs::to_vec(&Vec::<Message>::new())?));
        if fork.resolved_turns != 0 || fork.source_projection_digest != empty_digest {
            return Err(BrainError::Journal(
                "empty child context fork digest is invalid".into(),
            ));
        }
        return Ok(Vec::new());
    }
    let source = brain.journal.get_head(&fork.source_session_id).await?;
    if source.doc.root_id != child.root_id
        || source.doc.tenant_id != child.tenant_id
        || source.last_seq < fork.source_through_sequence
    {
        return Err(BrainError::Journal(
            "child context fork source scope or high-water is invalid".into(),
        ));
    }
    let history = parent_history_at(
        brain,
        &fork.source_session_id,
        fork.source_through_sequence,
        fork.source_context.as_ref(),
    )
    .await?;
    let mode = fork_mode_from_doc(fork)?;
    let (selected, resolved_turns) = select_fork_history(complete_fork_projection(&history), &mode);
    let digest = hex::encode(Sha256::digest(serde_jcs::to_vec(&selected)?));
    if resolved_turns != fork.resolved_turns || digest != fork.source_projection_digest {
        return Err(BrainError::Journal(
            "child context fork source digest mismatch".into(),
        ));
    }
    Ok(selected)
}

async fn materialize_session_history(
    brain: &Arc<Brain>,
    head: &HeadDoc,
    entries: &[Entry],
) -> Result<Vec<Message>> {
    let own_history = crate::compact::materialize_history(entries, head.context.as_ref())?;
    let mut history = if head.context.is_none() {
        materialize_context_fork(brain, head).await?
    } else {
        // A child checkpoint summarizes the already-materialized inherited prefix together with
        // the child's own turns. Prepending the immutable fork again would duplicate history.
        Vec::new()
    };
    for message in own_history {
        if let Some(last) = history.last_mut()
            && last.role == Role::User
            && message.role == Role::User
        {
            last.content.extend(message.content);
        } else {
            history.push(message);
        }
    }
    Ok(history)
}

/// Commit the constant-size subtree admission fence and return it to the caller. No child
/// traversal or Hand operation is allowed before this decision: a successful response therefore
/// means every later descendant admission observes an ending ancestor, even if this process dies
/// immediately after replying.
async fn begin_end_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<HeadDoc> {
    let fenced = brain.journal.fence_end(session_id).await?;
    *resident = None;
    if fenced.newly_fenced {
        let record = Record::State {
            state: "ending".into(),
            turn: fenced.head.doc.turn.clone(),
        };
        if let Some(event) = crate::events::derive(
            session_id,
            fenced.head.last_seq,
            fenced.head.doc.updated_ms,
            &record,
        ) {
            brain.hub.publish(session_id, event);
        }
    }
    Ok(fenced.head.doc)
}

/// Drive one recoverable subtree teardown pass. `Ok(false)` means at least one child has accepted
/// its own fence but has not yet converged to `ended`; the caller releases ownership with a
/// bounded due-time so no actor or HTTP request waits on the tree.
async fn continue_end_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<bool> {
    let heartbeat = {
        let resident = ensure_resident(brain, session_id, resident).await?;
        start_lease_heartbeat(brain, session_id, &resident.st.lease, true, None)
    };
    let r = ensure_resident(brain, session_id, resident).await?;
    if r.st.head.state != "ending" {
        drop(heartbeat);
        return Ok(r.st.head.state == "ended" || r.st.head.state == "deleting");
    }

    let mut children_settled = true;
    let mut cursor = None;
    loop {
        let page = brain
            .journal
            .list_child_page(&crate::journal::ChildListQuery {
                parent_id: session_id,
                limit: 100,
                cursor: cursor.as_deref(),
            })
            .await?;
        for child in &page.sessions {
            if matches!(child.state.as_str(), "ended" | "deleting" | "deleted") {
                continue;
            }
            match brain.end(&child.session_id).await {
                Ok(child) => {
                    if !matches!(
                        child.state,
                        session::SessionState::Ended
                            | session::SessionState::Deleting
                            | session::SessionState::Deleted
                    ) {
                        children_settled = false;
                    }
                }
                Err(BrainError::NoSuchSession(_) | BrainError::SessionDeleted(_)) => {}
                Err(error) => return Err(error),
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    if !children_settled {
        drop(heartbeat);
        return Ok(false);
    }

    terminate_additional_sandboxes_for_end(brain, session_id, &r.st.head.root_id).await?;

    // The default target belongs to the whole root tree. Ending a child never releases the
    // shared filesystem underneath its parent/siblings. This potentially slow external cleanup
    // happens only after the 202 response and under an independent lease heartbeat.
    if r.st.head.root_id == session_id {
        dematerialize_default_sandbox_for_end(brain, session_id, r).await?;
    }
    r.st.head.state = "ended".into();
    r.st.head.active_phase = None;
    r.st.head.provider_attempt = None;
    let seq = r.st.take_seq();
    let rec = Record::State {
        state: "ended".into(),
        turn: None,
    };
    commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;
    drop(heartbeat);
    Ok(true)
}

async fn dematerialize_default_sandbox_for_end(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    use brain_protocol::hand::SandboxState;

    let current = resident
        .st
        .head
        .default_sandbox
        .clone()
        .unwrap_or(initial_default_sandbox(session_id)?);
    if matches!(current.state, SandboxState::Gone | SandboxState::Terminated)
        || (current.state == SandboxState::NeverMaterialized
            && resident.st.head.prefix.managed_bundles.is_empty())
    {
        return Ok(());
    }
    let preparation = brain.session_preparation.as_ref().ok_or_else(|| {
        BrainError::HandUnavailable(
            "default sandbox dematerialization is unavailable during session end".into(),
        )
    })?;
    let status = match preparation
        .dematerialize_default(current.target.clone())
        .await
    {
        Ok(status) => status,
        Err(error)
            if matches!(
                error.code,
                brain_protocol::hand::HandErrorCode::SandboxGone
                    | brain_protocol::hand::HandErrorCode::SandboxNotMaterialized
            ) =>
        {
            sandbox_gone_status(&current, "hand_reported_gone")?
        }
        Err(error) => return Err(map_hand_port_error(error)),
    };
    if !matches!(status.state, SandboxState::Gone | SandboxState::Terminated) {
        return Err(BrainError::Hand(
            "default sandbox dematerialization did not return a terminal state".into(),
        ));
    }
    resident.st.head.default_sandbox = Some(status.clone());
    let seq = resident.st.take_seq();
    commit(
        brain,
        session_id,
        &mut resident.st,
        vec![(seq, Record::DefaultSandboxChanged { status })],
    )
    .await
}

/// Converge Brain's root-scoped additional-sandbox inventory before publishing ENDED.
///
/// A root END owns every inventory item. A child END owns only items whose immutable lifecycle
/// owner is that child; each descendant reaches ENDED only after cleaning its own items, so the
/// parent's already-settled child barrier covers the rest of the subtree without scanning Hand or
/// fabricating ownership. Terminal rows remain tombstoned until root deletion and release their
/// live slot exactly once through the version-fenced journal update.
async fn terminate_additional_sandboxes_for_end(
    brain: &Arc<Brain>,
    session_id: &str,
    root_id: &str,
) -> Result<()> {
    let terminate_all = session_id == root_id;
    let mut cursor = None;
    loop {
        let page = brain
            .journal
            .list_sandbox_page(&crate::journal::SandboxListQuery {
                root_id,
                limit: 100,
                cursor: cursor.as_deref(),
            })
            .await?;
        for current in page.sandboxes {
            if (!terminate_all && current.owner_session_id != session_id)
                || sandbox_status_releases_slot(&current.status)
            {
                continue;
            }
            let control = brain.sandbox_control.as_ref().ok_or_else(|| {
                BrainError::HandUnavailable(
                    "additional sandbox control is unavailable during session end".into(),
                )
            })?;
            let status = match control.terminate(current.status.target.clone()).await {
                Ok(status) => status,
                Err(error) if error.code == brain_protocol::hand::HandErrorCode::SandboxGone => {
                    sandbox_gone_status(&current.status, "hand_reported_gone")?
                }
                Err(error) => return Err(map_hand_port_error(error)),
            };
            if !sandbox_status_releases_slot(&status) {
                return Err(BrainError::Hand(
                    "sandbox termination did not return a confirmed terminal state".into(),
                ));
            }
            brain
                .persist_additional_sandbox_status(&current, status)
                .await?;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(())
}

async fn delete_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<()> {
    begin_delete_session(brain, session_id, resident).await?;
    match continue_delete_session(brain, session_id, resident).await {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Some(mut status) = brain.journal.get_deletion_status(session_id).await? {
                status.attempts = status.attempts.saturating_add(1);
                status.updated_at_ms = crate::wall_ms();
                status.state = "retrying".into();
                // Tombstones carry no adapter text, provider content, object locators or secrets.
                // Detailed diagnostics remain in the structured server log below.
                status.last_error = Some(deletion_error_code(&error).into());
                let _ = brain.journal.put_deletion_status(&status).await;
            }
            tracing::warn!(session_id, error = ?error, "session deletion cleanup will retry");
            let _ = brain.journal.defer_recovery(session_id).await;
            Err(error)
        }
    }
}

fn deletion_error_code(error: &BrainError) -> &'static str {
    match error {
        BrainError::Hand(_) | BrainError::HandUnavailable(_) => "sandbox_cleanup_failed",
        BrainError::FileNotFound(_)
        | BrainError::FileTooLarge { .. }
        | BrainError::StorageObjectTooLarge { .. }
        | BrainError::StorageQuotaExceeded { .. }
        | BrainError::StorageUploadExpired(_)
        | BrainError::StorageUploadInProgress { .. } => "storage_cleanup_failed",
        BrainError::Journal(_) | BrainError::Fenced => "journal_cleanup_failed",
        _ => "cleanup_failed",
    }
}

async fn begin_delete_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<()> {
    let needs_end = {
        let r = ensure_resident(brain, session_id, resident).await?;
        !r.st.head.ended
    };
    if needs_end {
        begin_end_session(brain, session_id, resident).await?;
    }
    let end_pending = {
        let r = ensure_resident(brain, session_id, resident).await?;
        r.st.head.state == "ending"
    };
    if end_pending && !continue_end_session(brain, session_id, resident).await? {
        return Err(BrainError::HandUnavailable(
            "session subtree end remains pending".into(),
        ));
    }
    let r = ensure_resident(brain, session_id, resident).await?;
    if r.st.head.state != "deleting" {
        // This decision is the admission fence for every later mutation. It lands before any
        // external cleanup, and remains indexed until all cleanup has succeeded.
        r.st.head.state = "deleting".into();
        r.st.head.ended = true;
        r.st.head.turn = None;
        r.st.head.active_phase = None;
        r.st.head.provider_attempt = None;
        r.st.head.active_context.clear();
        let seq = r.st.take_seq();
        commit(
            brain,
            session_id,
            &mut r.st,
            vec![(
                seq,
                Record::State {
                    state: "deleting".into(),
                    turn: None,
                },
            )],
        )
        .await?;
    }
    let now = crate::wall_ms();
    let existing = brain.journal.get_deletion_status(session_id).await?;
    let status = DeletionStatusDoc {
        session_id: session_id.to_owned(),
        tenant_id: r.st.head.tenant_id.clone(),
        root_id: r.st.head.root_id.clone(),
        parent_id: r.st.head.parent_id.clone(),
        metered_storage_bytes: r.st.head.tenant_metered_storage_bytes,
        metered_journal_bytes: r.st.lease.retention.metered_bytes,
        state: "deleting".into(),
        requested_at_ms: existing
            .as_ref()
            .map_or(now, |status| status.requested_at_ms),
        updated_at_ms: now,
        completed_at_ms: None,
        // Nonterminal jobs never expire. DynamoDB TTL ignores timestamps this far in the future;
        // the terminal transition replaces it with the bounded 24-hour tombstone expiry.
        expires_at_ms: u64::MAX,
        attempts: existing.as_ref().map_or(0, |status| status.attempts),
        last_error: None,
    };
    brain.journal.put_deletion_status(&status).await?;
    Ok(())
}

async fn continue_delete_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<()> {
    let _heartbeat = {
        let resident = ensure_resident(brain, session_id, resident).await?;
        start_lease_heartbeat(brain, session_id, &resident.st.lease, true, None)
    };
    {
        let r = ensure_resident(brain, session_id, resident).await?;
        if r.st.head.state != "deleting" {
            return Err(BrainError::Invalid(
                "session deletion cleanup requires the durable deleting fence".into(),
            ));
        }
    }

    // Child partitions are ordinary sessions and are deleted independently before their parent
    // adjacency partition disappears. The graph is stable because begin_delete ran the recursive
    // end fence first.
    let mut cursor = None;
    loop {
        let page = brain
            .journal
            .list_child_page(&crate::journal::ChildListQuery {
                parent_id: session_id,
                limit: 100,
                cursor: cursor.as_deref(),
            })
            .await?;
        for child in &page.sessions {
            match brain.delete(&child.session_id).await {
                Ok(()) | Err(BrainError::NoSuchSession(_)) => {}
                Err(error) => return Err(error),
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let r = ensure_resident(brain, session_id, resident).await?;
    // Every cleanup operation is idempotent. Any error leaves HEAD+CONFIG and its recovery-due
    // projection intact, so the background worker can retry without customer traffic.
    if r.st.head.root_id == session_id {
        if let Some(preparation) = &brain.session_preparation {
            preparation
                .purge_tree(session_id)
                .await
                .map_err(map_hand_port_error)?;
        } else if !r.st.head.prefix.managed_bundles.is_empty() {
            return Err(BrainError::HandUnavailable(
                "managed Tool purge is unavailable during root deletion".into(),
            ));
        }
        if let Some(bundle_storage) = &brain.bundle_storage {
            bundle_storage.purge_root_bundles(session_id).await?;
        } else if !r.st.head.prefix.managed_bundles.is_empty() {
            return Err(BrainError::HandUnavailable(
                "managed Tool bundle purge is unavailable during root deletion".into(),
            ));
        }
    }
    if let Some(storage) = &brain.session_storage {
        let mut cursor = None;
        loop {
            let page = storage
                .purge_session_page(session_id, cursor.as_deref())
                .await?;
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
    }
    brain.journal.purge_history(session_id).await?;
    // Atomically replace the final HEAD/CONFIG recovery anchor with a bounded non-content
    // tombstone. A lost final response is therefore distinguishable from an unknown deletion.
    let now = crate::wall_ms();
    let prior = brain.journal.get_deletion_status(session_id).await?;
    brain
        .journal
        .finalize_deletion(&DeletionStatusDoc {
            session_id: session_id.to_owned(),
            tenant_id: r.st.head.tenant_id.clone(),
            root_id: r.st.head.root_id.clone(),
            parent_id: r.st.head.parent_id.clone(),
            metered_storage_bytes: r.st.head.tenant_metered_storage_bytes,
            metered_journal_bytes: r.st.lease.retention.metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: prior.as_ref().map_or(now, |status| status.requested_at_ms),
            updated_at_ms: now,
            completed_at_ms: Some(now),
            expires_at_ms: now.saturating_add(DELETION_TOMBSTONE_TTL_MS),
            attempts: prior
                .as_ref()
                .map_or(1, |status| status.attempts.saturating_add(1)),
            last_error: None,
        })
        .await?;
    brain.hub.drop_session(session_id);
    *resident = None;
    Ok(())
}

fn ensure_storage_readable(doc: &HeadDoc, session_id: &str) -> Result<()> {
    match doc.state.as_str() {
        "deleted" | "deleting" => Err(BrainError::SessionDeleted(session_id.to_owned())),
        _ => Ok(()),
    }
}

fn has_reserved_storage_upload(resident: &Option<Resident>) -> bool {
    resident.as_ref().is_some_and(|resident| {
        resident.st.head.storage_delete.is_some()
            || resident
                .st
                .head
                .storage_upload
                .as_ref()
                .is_some_and(|upload| {
                    matches!(
                        upload.state.as_str(),
                        "reserved" | "inline_reserved" | "published"
                    )
                })
    })
}

fn has_pending_terminal_ack(resident: &Option<Resident>) -> bool {
    resident.as_ref().is_some_and(|resident| {
        !resident.st.head.pending_customer_acks.is_empty()
            || !resident.st.head.pending_managed_acks.is_empty()
    })
}

async fn sleep_until_storage_expiry(resident: &Option<Resident>) {
    let expires_at_ms = resident
        .as_ref()
        .and_then(|resident| resident.st.head.storage_upload.as_ref())
        .filter(|upload| {
            matches!(
                upload.state.as_str(),
                "reserved" | "inline_reserved" | "published"
            )
        })
        .map(|upload| {
            if upload.state == "published" {
                crate::wall_ms()
            } else {
                upload.expires_at_ms
            }
        })
        .or_else(|| {
            resident
                .as_ref()
                .and_then(|resident| resident.st.head.storage_delete.as_ref())
                .map(|_| crate::wall_ms())
        })
        .unwrap_or_else(|| crate::wall_ms().saturating_add(60_000));
    tokio::time::sleep(std::time::Duration::from_millis(
        expires_at_ms.saturating_sub(crate::wall_ms()),
    ))
    .await;
}

fn validate_storage_upload_intent(
    intent: &crate::storage::StorageUploadIntent,
    max_object_bytes: u64,
) -> Result<()> {
    crate::storage::validate_storage_adapter_key(&intent.key)?;
    if intent.bytes > max_object_bytes {
        return Err(BrainError::StorageObjectTooLarge {
            limit: max_object_bytes,
        });
    }
    if let Some(sha256) = &intent.sha256
        && (sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    {
        return Err(BrainError::Invalid(
            "sha256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    if intent.content_type.as_ref().is_some_and(|value| {
        value.is_empty() || value.len() > 255 || value.bytes().any(|byte| byte.is_ascii_control())
    }) {
        return Err(BrainError::Invalid(
            "content_type must contain 1 to 255 non-control bytes".into(),
        ));
    }
    Ok(())
}

fn matches_storage_publication(
    object: &crate::storage::StorageObject,
    upload: &StorageUploadReservationDoc,
) -> bool {
    object.bytes == upload.bytes
        && upload
            .sha256
            .as_ref()
            .is_none_or(|sha256| object.sha256 == *sha256)
        && object.content_type == upload.content_type
        && object.publication_id.as_deref() == Some(upload.transfer_id.as_str())
}

async fn expire_storage_upload(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    let _heartbeat = start_lease_heartbeat(brain, session_id, &st.lease, true, None);
    let Some(upload) = st.head.storage_upload.clone() else {
        return Ok(());
    };
    if upload.state == "completed" {
        return Ok(());
    }
    let storage = brain.storage_port()?.clone();
    if upload.state == "published" {
        let object = storage.stat(session_id, &upload.key).await?;
        if !matches_storage_publication(&object, &upload) {
            return Err(BrainError::Journal(
                "published storage object does not match its durable reservation".into(),
            ));
        }
        storage
            .abort_upload(session_id, &upload.transfer_id)
            .await?;
        let mut completed = upload.clone();
        completed.state = "completed".into();
        st.head.storage_reserved_bytes = 0;
        st.head.storage_upload = Some(completed);
        let seq = st.take_seq();
        return commit(
            brain,
            session_id,
            st,
            vec![(
                seq,
                Record::StorageUploadCompleted {
                    transfer_id: upload.transfer_id,
                    key: upload.key,
                    bytes: upload.bytes,
                    published_bytes: st.head.session_storage_bytes,
                    reserved_bytes: st.head.storage_reserved_bytes,
                },
            )],
        )
        .await;
    }
    if upload.state == "reserved" {
        match storage.stat(session_id, &upload.key).await {
            Ok(object) if matches_storage_publication(&object, &upload) => {
                // `complete_upload` may have copied destination bytes before its response or the
                // following journal decision was lost. Adopt that exact sealed result before
                // touching staging, even after ticket expiry; otherwise visible bytes would be
                // left unmetered while the reservation was released.
                st.head.session_storage_bytes = st
                    .head
                    .session_storage_bytes
                    .saturating_sub(upload.previous_bytes)
                    .checked_add(upload.bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("session storage meter overflowed".into())
                    })?;
                st.head.storage_reserved_bytes = 0;
                let mut published = upload.clone();
                published.sha256 = Some(object.sha256.clone());
                published.state = "published".into();
                st.head.storage_upload = Some(published);
                let seq = st.take_seq();
                commit(
                    brain,
                    session_id,
                    st,
                    vec![(
                        seq,
                        Record::StorageUploadPublished {
                            transfer_id: upload.transfer_id.clone(),
                            key: upload.key.clone(),
                            bytes: upload.bytes,
                            published_bytes: st.head.session_storage_bytes,
                            reserved_bytes: st.head.storage_reserved_bytes,
                        },
                    )],
                )
                .await?;
                // Publication is now durable. Staging cleanup may be retried independently and
                // cannot make the published object or its tenant charge disappear.
                storage
                    .abort_upload(session_id, &upload.transfer_id)
                    .await?;
                let mut completed = upload.clone();
                completed.sha256 = Some(object.sha256.clone());
                completed.state = "completed".into();
                st.head.storage_upload = Some(completed);
                let seq = st.take_seq();
                return commit(
                    brain,
                    session_id,
                    st,
                    vec![(
                        seq,
                        Record::StorageUploadCompleted {
                            transfer_id: upload.transfer_id,
                            key: upload.key,
                            bytes: upload.bytes,
                            published_bytes: st.head.session_storage_bytes,
                            reserved_bytes: st.head.storage_reserved_bytes,
                        },
                    )],
                )
                .await;
            }
            Err(BrainError::FileNotFound(_)) => {}
            // An overwrite may legitimately see the prior visible object before the staging
            // copy runs. Equality of bytes/hash is not publication proof; only the intent id is.
            Ok(_) if upload.overwrite => {}
            Ok(_) => {
                return Err(BrainError::Journal(
                    "storage destination conflicts with its durable upload reservation".into(),
                ));
            }
            Err(error) => return Err(error),
        }
    }
    if upload.expires_at_ms > crate::wall_ms() {
        return Ok(());
    }
    if upload.state == "inline_reserved" {
        match storage.stat(session_id, &upload.key).await {
            Ok(object) if matches_storage_publication(&object, &upload) => {
                st.head.session_storage_bytes = st
                    .head
                    .session_storage_bytes
                    .saturating_sub(upload.previous_bytes)
                    .checked_add(upload.bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("session storage meter overflowed".into())
                    })?;
                st.head.storage_reserved_bytes = 0;
                let mut completed = upload.clone();
                completed.state = "completed".into();
                st.head.storage_upload = Some(completed);
                let seq = st.take_seq();
                return commit(
                    brain,
                    session_id,
                    st,
                    vec![(
                        seq,
                        Record::StorageUploadCompleted {
                            transfer_id: upload.transfer_id,
                            key: upload.key,
                            bytes: upload.bytes,
                            published_bytes: st.head.session_storage_bytes,
                            reserved_bytes: st.head.storage_reserved_bytes,
                        },
                    )],
                )
                .await;
            }
            Err(BrainError::FileNotFound(_)) => {}
            // The pre-existing overwrite target is not evidence that this inline intent ran.
            // At expiry leave it untouched and release only the unpublished reservation.
            Ok(_) if upload.overwrite => {}
            Ok(_) => {
                return Err(BrainError::Journal(
                    "inline storage object conflicts with its durable intent".into(),
                ));
            }
            Err(error) => return Err(error),
        }
    } else if upload.state != "reserved" {
        return Err(BrainError::Journal(format!(
            "storage upload has invalid state {}",
            upload.state
        )));
    }
    // Deletion precedes reservation release. A transient S3 failure therefore leaves the hard
    // bound in place and the next operation retries cleanup instead of admitting more bytes.
    if upload.state == "reserved" {
        storage
            .abort_upload(session_id, &upload.transfer_id)
            .await?;
    }
    st.head.storage_reserved_bytes = 0;
    st.head.storage_upload = None;
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadExpired {
                transfer_id: upload.transfer_id,
                key: upload.key,
                bytes: upload.bytes,
                published_bytes: st.head.session_storage_bytes,
                reserved_bytes: st.head.storage_reserved_bytes,
            },
        )],
    )
    .await
}

async fn reconcile_storage_delete(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    let Some(deletion) = st.head.storage_delete.clone() else {
        return Ok(());
    };
    let storage = brain.storage_port()?.clone();
    match storage.stat(session_id, &deletion.key).await {
        Ok(object) if object.bytes == deletion.bytes && object.sha256 == deletion.sha256 => {
            storage.delete(session_id, &deletion.key).await?;
        }
        Err(BrainError::FileNotFound(_)) => {}
        Ok(_) => {
            return Err(BrainError::Journal(
                "storage delete target changed after its durable intent".into(),
            ));
        }
        Err(error) => return Err(error),
    }
    st.head.session_storage_bytes = st.head.session_storage_bytes.saturating_sub(deletion.bytes);
    st.head.storage_delete = None;
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageDeleteCompleted {
                operation_id: deletion.operation_id,
                key: deletion.key,
                bytes: deletion.bytes,
                published_bytes: st.head.session_storage_bytes,
                reserved_bytes: st.head.storage_reserved_bytes,
            },
        )],
    )
    .await
}

async fn reconcile_storage_mutations(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    expire_storage_upload(brain, session_id, st).await?;
    reconcile_storage_delete(brain, session_id, st).await
}

async fn do_prepare_storage_upload(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    intent: crate::storage::StorageUploadIntent,
) -> Result<crate::storage::StorageTransferTicket> {
    let r = ensure_resident(brain, session_id, resident).await?;
    prepare_storage_upload_state(brain, session_id, &mut r.st, intent).await
}

pub(crate) async fn prepare_storage_upload_state(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    intent: crate::storage::StorageUploadIntent,
) -> Result<crate::storage::StorageTransferTicket> {
    prepare_storage_upload_state_for_transfer(brain, session_id, st, intent, None).await
}

async fn prepare_storage_upload_state_for_transfer(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    intent: crate::storage::StorageUploadIntent,
    requested_transfer_id: Option<String>,
) -> Result<crate::storage::StorageTransferTicket> {
    ensure_storage_readable(&st.head, session_id)?;
    validate_storage_upload_intent(&intent, st.head.prefix.storage_max_object_bytes)?;
    expire_storage_upload(brain, session_id, st).await?;

    if let Some(upload) = &st.head.storage_upload {
        let same_intent = upload.key == intent.key
            && upload.bytes == intent.bytes
            && (upload.sha256 == intent.sha256
                || (requested_transfer_id.is_some() && intent.sha256.is_none()))
            && upload.content_type == intent.content_type
            && upload.overwrite == intent.overwrite;
        let same_requested_transfer = requested_transfer_id
            .as_deref()
            .is_some_and(|transfer_id| transfer_id == upload.transfer_id);
        if same_intent
            && ((upload.state == "reserved"
                && (requested_transfer_id.is_none() || same_requested_transfer))
                || (upload.state == "completed" && same_requested_transfer))
        {
            return brain
                .storage_port()?
                .prepare_upload(crate::storage::StorageUploadRequest {
                    session_id: session_id.to_owned(),
                    transfer_id: upload.transfer_id.clone(),
                    key: upload.key.clone(),
                    bytes: upload.bytes,
                    sha256: upload.sha256.clone(),
                    content_type: upload.content_type.clone(),
                    overwrite: upload.overwrite,
                    expires_at_ms: upload.expires_at_ms,
                })
                .await;
        }
        if upload.state != "completed" {
            return Err(BrainError::StorageUploadInProgress {
                transfer_id: upload.transfer_id.clone(),
            });
        }
    }

    let storage = brain.storage_port()?.clone();
    let previous_bytes = match storage.stat(session_id, &intent.key).await {
        Ok(object) if intent.overwrite => object.bytes,
        Ok(_) => {
            return Err(BrainError::Invalid(format!(
                "session storage object {} already exists",
                intent.key
            )));
        }
        Err(BrainError::FileNotFound(_)) => 0,
        Err(error) => return Err(error),
    };
    let visible_after_publish = st
        .head
        .session_storage_bytes
        .saturating_sub(previous_bytes)
        .checked_add(intent.bytes)
        .ok_or_else(|| BrainError::Invalid("session storage byte count overflowed".into()))?;
    // Completion briefly retains both the verified staging object and its published copy so a
    // lost response remains retryable. Keep that worst-case physical footprint within quota.
    let peak_bytes = visible_after_publish
        .checked_add(intent.bytes)
        .ok_or_else(|| BrainError::Invalid("session storage byte count overflowed".into()))?;
    if peak_bytes > st.head.prefix.storage_max_session_bytes {
        return Err(BrainError::StorageQuotaExceeded {
            published: st.head.session_storage_bytes,
            reserved: st.head.storage_reserved_bytes,
            requested: intent.bytes,
            limit: st.head.prefix.storage_max_session_bytes,
        });
    }

    let transfer_id = requested_transfer_id.unwrap_or_else(|| crate::mint_id("xfer", 24));
    let expires_at_ms = crate::wall_ms()
        .checked_add(st.head.prefix.storage_transfer_ttl_ms)
        .ok_or_else(|| BrainError::Invalid("storage transfer expiry overflowed".into()))?;
    let upload = StorageUploadReservationDoc {
        transfer_id: transfer_id.clone(),
        key: intent.key.clone(),
        bytes: intent.bytes,
        sha256: intent.sha256.clone(),
        content_type: intent.content_type.clone(),
        overwrite: intent.overwrite,
        previous_bytes,
        expires_at_ms,
        state: "reserved".into(),
    };
    st.head.storage_reserved_bytes = intent.bytes;
    st.head.storage_upload = Some(upload.clone());
    let storage_gauges = (
        st.head.session_storage_bytes,
        st.head.storage_reserved_bytes,
    );
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadReserved {
                transfer_id: transfer_id.clone(),
                key: intent.key.clone(),
                bytes: intent.bytes,
                sha256: intent.sha256.clone(),
                expires_at_ms,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;

    storage
        .prepare_upload(crate::storage::StorageUploadRequest {
            session_id: session_id.to_owned(),
            transfer_id,
            key: intent.key,
            bytes: intent.bytes,
            sha256: intent.sha256,
            content_type: intent.content_type,
            overwrite: intent.overwrite,
            expires_at_ms,
        })
        .await
}

async fn do_complete_storage_upload(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    transfer_id: String,
) -> Result<crate::storage::StorageObject> {
    let r = ensure_resident(brain, session_id, resident).await?;
    complete_storage_upload_state(brain, session_id, &mut r.st, transfer_id).await
}

pub(crate) async fn complete_storage_upload_state(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    transfer_id: String,
) -> Result<crate::storage::StorageObject> {
    ensure_storage_readable(&st.head, session_id)?;
    let requested_expired = st.head.storage_upload.as_ref().is_some_and(|upload| {
        upload.transfer_id == transfer_id
            && upload.state == "reserved"
            && upload.expires_at_ms <= crate::wall_ms()
    });
    expire_storage_upload(brain, session_id, st).await?;
    if requested_expired {
        return Err(BrainError::StorageUploadExpired(transfer_id));
    }
    let upload = st
        .head
        .storage_upload
        .clone()
        .filter(|upload| upload.transfer_id == transfer_id)
        .ok_or_else(|| BrainError::FileNotFound(format!("storage upload {transfer_id}")))?;
    let storage = brain.storage_port()?.clone();

    if upload.state == "completed" {
        return storage.stat(session_id, &upload.key).await;
    }

    let object = if upload.state == "published" {
        let object = storage.stat(session_id, &upload.key).await?;
        if !matches_storage_publication(&object, &upload) {
            return Err(BrainError::Journal(
                "published storage object does not match its durable reservation".into(),
            ));
        }
        object
    } else {
        if upload.expires_at_ms <= crate::wall_ms() {
            return Err(BrainError::StorageUploadExpired(transfer_id));
        }
        let object = storage.complete_upload(session_id, &transfer_id).await?;
        if object.key != upload.key || !matches_storage_publication(&object, &upload) {
            return Err(BrainError::Journal(
                "published storage object does not match its durable reservation".into(),
            ));
        }
        st.head.session_storage_bytes = st
            .head
            .session_storage_bytes
            .saturating_sub(upload.previous_bytes)
            .checked_add(upload.bytes)
            .ok_or_else(|| BrainError::Journal("session storage meter overflowed".into()))?;
        // Publication consumes the logical reservation. Staging cleanup stays pending through
        // `upload.state=published`, but tenant/session usage must not double-charge the bytes.
        st.head.storage_reserved_bytes = 0;
        let mut published = upload.clone();
        published.sha256 = Some(object.sha256.clone());
        published.state = "published".into();
        st.head.storage_upload = Some(published);
        let storage_gauges = (
            st.head.session_storage_bytes,
            st.head.storage_reserved_bytes,
        );
        let seq = st.take_seq();
        commit(
            brain,
            session_id,
            st,
            vec![(
                seq,
                Record::StorageUploadPublished {
                    transfer_id: transfer_id.clone(),
                    key: upload.key.clone(),
                    bytes: upload.bytes,
                    published_bytes: storage_gauges.0,
                    reserved_bytes: storage_gauges.1,
                },
            )],
        )
        .await?;
        object
    };

    // Cleanup is an idempotent external effect after the published decision. Until it succeeds,
    // the reservation remains charged and no second upload is admitted.
    storage.abort_upload(session_id, &transfer_id).await?;
    let mut completed = upload.clone();
    completed.sha256 = Some(object.sha256.clone());
    completed.state = "completed".into();
    st.head.storage_upload = Some(completed);
    st.head.storage_reserved_bytes = 0;
    let storage_gauges = (
        st.head.session_storage_bytes,
        st.head.storage_reserved_bytes,
    );
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadCompleted {
                transfer_id,
                key: upload.key,
                bytes: upload.bytes,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;
    Ok(object)
}

async fn do_write_storage_inline(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    key: String,
    content_base64: String,
    content_type: Option<String>,
    overwrite: bool,
) -> Result<crate::storage::StorageObject> {
    let r = ensure_resident(brain, session_id, resident).await?;
    write_storage_inline_state(
        brain,
        session_id,
        &mut r.st,
        key,
        content_base64,
        content_type,
        overwrite,
    )
    .await
}

/// The turn path already owns the claimed session state. Reuse the same durable reservation and
/// recovery state machine instead of trying to send a nested command to its own actor.
pub(crate) async fn write_storage_inline_state(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    key: String,
    content_base64: String,
    content_type: Option<String>,
    overwrite: bool,
) -> Result<crate::storage::StorageObject> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&content_base64)
        .map_err(|_| BrainError::Invalid("content_base64 is not valid base64".into()))?;
    const MAX_INLINE: usize = 1024 * 1024;
    if bytes.len() > MAX_INLINE {
        return Err(BrainError::FileTooLarge { limit: MAX_INLINE });
    }
    let intent = crate::storage::StorageUploadIntent {
        key: key.clone(),
        bytes: bytes.len() as u64,
        sha256: Some(hex::encode(Sha256::digest(&bytes))),
        content_type: content_type.clone(),
        overwrite,
    };
    ensure_storage_readable(&st.head, session_id)?;
    validate_storage_upload_intent(&intent, st.head.prefix.storage_max_object_bytes)?;
    reconcile_storage_mutations(brain, session_id, st).await?;
    let storage = brain.storage_port()?.clone();

    if let Some(upload) = st.head.storage_upload.clone() {
        let same = upload.key == intent.key
            && upload.bytes == intent.bytes
            && upload.sha256 == intent.sha256
            && upload.content_type == intent.content_type
            && upload.overwrite == intent.overwrite;
        if upload.state == "completed" && same {
            return storage.stat(session_id, &key).await;
        }
        if upload.state == "inline_reserved" && same {
            let object = match storage.stat(session_id, &key).await {
                Ok(object) if matches_storage_publication(&object, &upload) => object,
                Ok(_) if overwrite => {
                    storage
                        .write(crate::storage::StorageWriteRequest {
                            session_id: session_id.to_owned(),
                            publication_id: upload.transfer_id.clone(),
                            key: key.clone(),
                            content_base64: content_base64.clone(),
                            content_type: content_type.clone(),
                            overwrite,
                        })
                        .await?
                }
                Err(BrainError::FileNotFound(_)) => {
                    storage
                        .write(crate::storage::StorageWriteRequest {
                            session_id: session_id.to_owned(),
                            publication_id: upload.transfer_id.clone(),
                            key: key.clone(),
                            content_base64: content_base64.clone(),
                            content_type: content_type.clone(),
                            overwrite,
                        })
                        .await?
                }
                Ok(_) => {
                    return Err(BrainError::Journal(
                        "inline storage object conflicts with its durable intent".into(),
                    ));
                }
                Err(error) => return Err(error),
            };
            st.head.session_storage_bytes = st
                .head
                .session_storage_bytes
                .saturating_sub(upload.previous_bytes)
                .checked_add(upload.bytes)
                .ok_or_else(|| BrainError::Journal("session storage meter overflowed".into()))?;
            st.head.storage_reserved_bytes = 0;
            let mut completed = upload.clone();
            completed.state = "completed".into();
            st.head.storage_upload = Some(completed);
            let storage_gauges = (
                st.head.session_storage_bytes,
                st.head.storage_reserved_bytes,
            );
            let seq = st.take_seq();
            commit(
                brain,
                session_id,
                st,
                vec![(
                    seq,
                    Record::StorageUploadCompleted {
                        transfer_id: upload.transfer_id,
                        key: upload.key,
                        bytes: upload.bytes,
                        published_bytes: storage_gauges.0,
                        reserved_bytes: storage_gauges.1,
                    },
                )],
            )
            .await?;
            return Ok(object);
        }
        if upload.state != "completed" {
            return Err(BrainError::StorageUploadInProgress {
                transfer_id: upload.transfer_id,
            });
        }
    }

    let previous_bytes = match storage.stat(session_id, &key).await {
        Ok(object) if overwrite => object.bytes,
        Ok(_) => {
            return Err(BrainError::Invalid(format!(
                "session storage object {key} already exists"
            )));
        }
        Err(BrainError::FileNotFound(_)) => 0,
        Err(error) => return Err(error),
    };
    let peak_bytes = st
        .head
        .session_storage_bytes
        .checked_add(intent.bytes)
        .ok_or_else(|| BrainError::Invalid("session storage byte count overflowed".into()))?;
    let visible_after = st
        .head
        .session_storage_bytes
        .saturating_sub(previous_bytes)
        .checked_add(intent.bytes)
        .ok_or_else(|| BrainError::Invalid("session storage byte count overflowed".into()))?;
    if peak_bytes.max(visible_after) > st.head.prefix.storage_max_session_bytes {
        return Err(BrainError::StorageQuotaExceeded {
            published: st.head.session_storage_bytes,
            reserved: 0,
            requested: intent.bytes,
            limit: st.head.prefix.storage_max_session_bytes,
        });
    }
    let transfer_id = crate::mint_id("xfer", 24);
    let expires_at_ms = crate::wall_ms()
        .checked_add(st.head.prefix.storage_transfer_ttl_ms)
        .ok_or_else(|| BrainError::Invalid("storage transfer expiry overflowed".into()))?;
    let upload = StorageUploadReservationDoc {
        transfer_id: transfer_id.clone(),
        key: key.clone(),
        bytes: intent.bytes,
        sha256: intent.sha256.clone(),
        content_type: content_type.clone(),
        overwrite,
        previous_bytes,
        expires_at_ms,
        state: "inline_reserved".into(),
    };
    st.head.storage_reserved_bytes = upload.bytes;
    st.head.storage_upload = Some(upload.clone());
    let storage_gauges = (
        st.head.session_storage_bytes,
        st.head.storage_reserved_bytes,
    );
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadReserved {
                transfer_id: transfer_id.clone(),
                key: key.clone(),
                bytes: upload.bytes,
                sha256: upload.sha256.clone(),
                expires_at_ms,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;
    let object = storage
        .write(crate::storage::StorageWriteRequest {
            session_id: session_id.to_owned(),
            publication_id: transfer_id.clone(),
            key,
            content_base64,
            content_type,
            overwrite,
        })
        .await?;
    if !matches_storage_publication(&object, &upload) {
        return Err(BrainError::Journal(
            "inline storage result does not match its durable intent".into(),
        ));
    }
    st.head.session_storage_bytes = visible_after;
    st.head.storage_reserved_bytes = 0;
    let mut completed = upload.clone();
    completed.state = "completed".into();
    st.head.storage_upload = Some(completed);
    let storage_gauges = (
        st.head.session_storage_bytes,
        st.head.storage_reserved_bytes,
    );
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadCompleted {
                transfer_id,
                key: upload.key,
                bytes: upload.bytes,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;
    Ok(object)
}

async fn do_delete_storage_object(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    key: String,
) -> Result<()> {
    crate::storage::validate_storage_adapter_key(&key)?;
    let r = ensure_resident(brain, session_id, resident).await?;
    ensure_storage_readable(&r.st.head, session_id)?;
    reconcile_storage_mutations(brain, session_id, &mut r.st).await?;
    if let Some(upload) = &r.st.head.storage_upload
        && upload.state != "completed"
    {
        return Err(BrainError::StorageUploadInProgress {
            transfer_id: upload.transfer_id.clone(),
        });
    }
    if r.st.head.storage_delete.is_some() {
        reconcile_storage_delete(brain, session_id, &mut r.st).await?;
    }
    let storage = brain.storage_port()?.clone();
    let object = match storage.stat(session_id, &key).await {
        Ok(object) => object,
        Err(BrainError::FileNotFound(_)) => return Ok(()),
        Err(error) => return Err(error),
    };
    let operation_id = crate::mint_id("del", 24);
    r.st.head.storage_delete = Some(StorageDeleteReservationDoc {
        operation_id: operation_id.clone(),
        key: key.clone(),
        bytes: object.bytes,
        sha256: object.sha256.clone(),
    });
    let storage_gauges = (
        r.st.head.session_storage_bytes,
        r.st.head.storage_reserved_bytes,
    );
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::StorageDeleteIntent {
                operation_id,
                key,
                bytes: object.bytes,
                sha256: object.sha256,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;
    reconcile_storage_delete(brain, session_id, &mut r.st).await
}

// ---------------------------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------------------------

pub fn provider_name(p: &ApiProvider) -> &'static str {
    match p {
        ApiProvider::Openai => "openai",
        ApiProvider::Anthropic => "anthropic",
        ApiProvider::Deepseek => "deepseek",
        ApiProvider::Moonshot => "moonshot",
        ApiProvider::Xai => "xai",
        ApiProvider::OpenaiCompatible => "openai_compatible",
    }
}

/// Canonical public file path. The URL surface is deliberately narrower than hand tool paths:
/// only absolute POSIX paths beneath `/workspace` are accepted.
pub fn normalize_workspace_path(path: &str) -> Result<String> {
    if path.len() > 4096 {
        return Err(BrainError::Invalid(
            "file path exceeds 4096 UTF-8 bytes".into(),
        ));
    }
    if path.contains('\0') || path.contains('\\') {
        return Err(BrainError::Invalid(
            "file path contains a forbidden character".into(),
        ));
    }
    let mut parts = path.split('/');
    if parts.next() != Some("") || parts.next() != Some("workspace") {
        return Err(BrainError::Invalid(
            "file path must be absolute and beneath /workspace".into(),
        ));
    }
    let mut clean = Vec::new();
    for part in parts {
        match part {
            "" => continue,
            "." | ".." => {
                return Err(BrainError::Invalid(
                    "file path may not contain . or .. components".into(),
                ));
            }
            value => clean.push(value),
        }
    }
    Ok(if clean.is_empty() {
        "/workspace".into()
    } else {
        format!("/workspace/{}", clean.join("/"))
    })
}

/// Certified: openai, anthropic. Available uncertified: the rest (they speak one of the two
/// dialects). `openai_compatible` requires an explicit base_url.
pub fn resolve_base_url(p: &ApiProvider, base_url: Option<&str>) -> Result<String> {
    if let Some(u) = base_url {
        if !u.starts_with("https://") {
            return Err(BrainError::Invalid("model.base_url must be https".into()));
        }
        return Ok(u.trim_end_matches('/').to_string());
    }
    Ok(match p {
        ApiProvider::Openai => "https://api.openai.com".into(),
        ApiProvider::Anthropic => "https://api.anthropic.com".into(),
        ApiProvider::Deepseek => "https://api.deepseek.com".into(),
        ApiProvider::Moonshot => "https://api.moonshot.ai".into(),
        ApiProvider::Xai => "https://api.x.ai".into(),
        ApiProvider::OpenaiCompatible => {
            return Err(BrainError::Invalid(
                "model.base_url is required for provider openai_compatible".into(),
            ));
        }
    })
}

pub fn dialect_of(provider: &str) -> Dialect {
    match provider {
        "anthropic" => Dialect::AnthropicMessages,
        _ => Dialect::OpenAiChat,
    }
}

/// Explicit Chat-Completions compatibility profile. OpenAI and generic compatible endpoints
/// opt into the current OpenAI field; the named legacy-compatible providers retain the field
/// their published Chat APIs specify. This choice is sealed into the request digest.
fn output_token_parameter(provider: &str) -> OutputTokenParameter {
    match provider {
        "deepseek" | "moonshot" | "xai" | "anthropic" => OutputTokenParameter::MaxTokens,
        "openai" | "openai_compatible" => OutputTokenParameter::MaxCompletionTokens,
        _ => OutputTokenParameter::MaxCompletionTokens,
    }
}

/// Rebuilds the sealed prefix from the HEAD prefix doc. Deterministic: the same doc always
/// seals to the same digest.
pub fn build_prefix(
    p: &PrefixDoc,
    max_rounds: u32,
) -> Result<(crate::Shared<crate::config::SealedPrefix>, Dialect)> {
    let dialect = dialect_of(&p.provider);
    if dialect == Dialect::AnthropicMessages && p.reasoning_effort.is_some() {
        return Err(BrainError::Invalid(
            "model.reasoning_effort is not supported by the Anthropic MVP profile".into(),
        ));
    }
    let mut decls = crate::tools::resolve(&p.tools)?;
    for decl in &mut decls {
        if let crate::config::ToolRoute::Intrinsic(capability) = &decl.route
            && !crate::tools::is_direct_engine_capability(capability)
        {
            let policy = p
                .official_capabilities
                .get(capability)
                .cloned()
                .ok_or_else(|| {
                    BrainError::Journal(format!(
                        "sealed official capability {capability} has no trusted policy"
                    ))
                })?;
            decl.route = crate::config::ToolRoute::Server(policy);
        }
    }
    let mut def = AgentDef::new(
        p.system_prompt
            .clone()
            .unwrap_or_else(default_system_prompt),
        p.model.clone(),
        dialect,
    );
    for d in decls {
        def = def.tool(d);
    }
    def = def.sampling(GenOpts {
        max_tokens: u32::try_from(p.max_output_tokens.unwrap_or(4096)).map_err(|_| {
            BrainError::Journal("sealed max_output_tokens exceeds the canonical u32 bound".into())
        })?,
        output_token_parameter: output_token_parameter(&p.provider),
        temperature: p.temperature.map(|t| t as f32),
        reasoning_effort: p.reasoning_effort.clone(),
        stop_sequences: Vec::new(),
    });
    def = def.limits(crate::config::Limits {
        max_rounds,
        ..crate::config::Limits::default()
    });
    let rendered_base = if p.rendered_base.is_null() {
        None
    } else {
        let digest = hex::encode(Sha256::digest(serde_jcs::to_vec(&p.rendered_base)?));
        if digest != p.rendered_base_digest {
            return Err(BrainError::Journal(
                "stored provider base segment digest does not match".into(),
            ));
        }
        Some(p.rendered_base.clone())
    };
    let sealed = def.seal().with_provider_base(
        rendered_base,
        (!p.prompt_cache_key.is_empty()).then(|| p.prompt_cache_key.clone()),
    );
    Ok((sealed, dialect))
}

fn default_system_prompt() -> String {
    "You are an autonomous engineering agent running in an isolated Linux workspace \
     (/workspace, ARM64). Use the tools to inspect and change files and run commands. \
     Sandbox files are live compute state, not durable storage. Copy anything that must survive \
     target loss into session storage explicitly."
        .to_string()
}

/// Builds the contract Session document from the head. A sealed value that no longer parses
/// as its contract type is journal corruption and errors loudly, naming the field — the REST
/// read never substitutes placeholders or omits sealed identity.
pub fn session_doc(session_id: &str, doc: &HeadDoc) -> Result<session::Session> {
    let corrupt = |what: &str| {
        BrainError::Journal(format!(
            "session {session_id}: journaled {what} violates the public contract"
        ))
    };
    let agentloop = Some(
        match doc
            .prefix
            .agentloop
            .clone()
            .unwrap_or_else(crate::journal::AgentloopSelectorDoc::official_aex)
        {
            crate::journal::AgentloopSelectorDoc::Official { name, version } => {
                session::AgentloopInfo::Official {
                    name: name.parse().map_err(|_| corrupt("agentloop name"))?,
                    version: version.parse().map_err(|_| corrupt("agentloop version"))?,
                }
            }
            crate::journal::AgentloopSelectorDoc::Custom {
                source_bundle_sha256,
                toolchain,
                ..
            } => session::AgentloopInfo::Custom {
                source_bundle_sha256: source_bundle_sha256
                    .parse()
                    .map_err(|_| corrupt("agentloop bundle digest"))?,
                toolchain: toolchain
                    .parse()
                    .map_err(|_| corrupt("agentloop toolchain"))?,
            },
        },
    );
    Ok(session::Session {
        agentloop,
        context_fork: doc.context_fork.as_ref().map(public_context_fork),
        created_at: crate::events::ts(doc.created_ms),
        current_turn: doc
            .turn
            .as_deref()
            .map(|t| t.parse().map_err(|_| corrupt("turn id")))
            .transpose()?,
        failure: doc.failure.as_ref().map(|f| session::SessionFailure {
            at: crate::events::ts(f.at_ms),
            code: match f.code.as_str() {
                "binding_conflict" => session::SessionFailureCode::BindingConflict,
                "provider_unusable" => session::SessionFailureCode::ProviderUnusable,
                "hand_unavailable" => session::SessionFailureCode::HandUnavailable,
                _ => session::SessionFailureCode::Internal,
            },
            message: f.message.clone(),
        }),
        id: session_id.parse().map_err(|_| corrupt("session id"))?,
        parent_id: doc
            .parent_id
            .as_deref()
            .map(|id| id.parse().map_err(|_| corrupt("parent session id")))
            .transpose()?,
        root_id: doc
            .root_id
            .parse()
            .map_err(|_| corrupt("root session id"))?,
        depth: i64::from(doc.depth),
        last_seq: doc.last_seq,
        last_message_at: doc.last_message_ms.map(crate::events::ts),
        metadata: doc
            .prefix
            .metadata
            .iter()
            .map(|(key, value)| {
                (
                    key.as_str()
                        .parse()
                        .expect("sealed metadata key satisfies the public contract"),
                    value
                        .as_str()
                        .parse()
                        .expect("sealed metadata value satisfies the public contract"),
                )
            })
            .collect(),
        model: session::ModelInfo {
            base_url: doc.prefix.base_url.clone(),
            context_window_tokens: i64::from(doc.prefix.context_window_tokens),
            name: doc.prefix.model.clone(),
            provider: match doc.prefix.provider.as_str() {
                "openai" => ApiProvider::Openai,
                "anthropic" => ApiProvider::Anthropic,
                "deepseek" => ApiProvider::Deepseek,
                "moonshot" => ApiProvider::Moonshot,
                "xai" => ApiProvider::Xai,
                _ => ApiProvider::OpenaiCompatible,
            },
        },
        name: doc
            .child_name
            .as_deref()
            .map(|name| name.parse().map_err(|_| corrupt("child name")))
            .transpose()?,
        object: session::SessionObject::Session,
        state: crate::events::session_state(&doc.state)
            .expect("journal session lifecycle is a closed enum"),
        turn_phase: doc
            .active_phase
            .as_deref()
            .map(|phase| phase.parse().map_err(|_| corrupt("turn phase")))
            .transpose()?,
        turn_state: crate::events::session_turn_state(&doc.state, doc.turn.as_deref()),
        shape: doc.prefix.shape.clone(),
        storage: session::StorageInfo {
            session_storage_bytes: doc.session_storage_bytes,
            upload_reserved_bytes: doc.storage_reserved_bytes,
        },
        turns: doc.turns,
        updated_at: crate::events::ts(doc.updated_ms),
    })
}

fn session_doc_summary(summary: &crate::journal::SessionSummary) -> session::Session {
    session::Session {
        // The bounded listing summary does not carry the sealed prefix; the full session
        // resource does. Absent here, never a fabricated default.
        agentloop: None,
        context_fork: summary.context_fork.as_ref().map(public_context_fork),
        created_at: crate::events::ts(summary.created_ms),
        current_turn: summary.turn.as_deref().and_then(|turn| turn.parse().ok()),
        failure: summary
            .failure
            .as_ref()
            .map(|failure| session::SessionFailure {
                at: crate::events::ts(failure.at_ms),
                code: match failure.code.as_str() {
                    "binding_conflict" => session::SessionFailureCode::BindingConflict,
                    "provider_unusable" => session::SessionFailureCode::ProviderUnusable,
                    "hand_unavailable" => session::SessionFailureCode::HandUnavailable,
                    _ => session::SessionFailureCode::Internal,
                },
                message: failure.message.clone(),
            }),
        id: summary
            .session_id
            .parse()
            .unwrap_or_else(|_| "ses_00000000000000000000".parse().expect("fallback id")),
        parent_id: summary.parent_id.as_deref().and_then(|id| id.parse().ok()),
        root_id: summary
            .root_id
            .parse()
            .unwrap_or_else(|_| "ses_00000000000000000000".parse().expect("fallback id")),
        depth: i64::from(summary.depth),
        last_seq: summary.last_seq,
        last_message_at: summary.last_message_ms.map(crate::events::ts),
        metadata: summary
            .metadata
            .iter()
            .map(|(key, value)| {
                (
                    key.as_str()
                        .parse()
                        .expect("listed metadata key satisfies the public contract"),
                    value
                        .as_str()
                        .parse()
                        .expect("listed metadata value satisfies the public contract"),
                )
            })
            .collect(),
        model: session::ModelInfo {
            base_url: summary.base_url.clone(),
            context_window_tokens: i64::from(summary.context_window_tokens),
            name: summary.model.clone(),
            provider: match summary.provider.as_str() {
                "openai" => ApiProvider::Openai,
                "anthropic" => ApiProvider::Anthropic,
                "deepseek" => ApiProvider::Deepseek,
                "moonshot" => ApiProvider::Moonshot,
                "xai" => ApiProvider::Xai,
                _ => ApiProvider::OpenaiCompatible,
            },
        },
        name: summary
            .child_name
            .as_deref()
            .and_then(|name| name.parse().ok()),
        object: session::SessionObject::Session,
        state: crate::events::session_state(&summary.state)
            .expect("journal session lifecycle is a closed enum"),
        turn_phase: summary
            .active_phase
            .as_deref()
            .and_then(|phase| phase.parse().ok()),
        turn_state: crate::events::session_turn_state(&summary.state, summary.turn.as_deref()),
        shape: summary.shape.clone(),
        storage: session::StorageInfo {
            session_storage_bytes: summary.session_storage_bytes,
            upload_reserved_bytes: summary.storage_reserved_bytes,
        },
        turns: summary.turns,
        updated_at: crate::events::ts(summary.updated_ms),
    }
}

fn public_context_fork(fork: &ContextForkDoc) -> session::ContextFork {
    session::ContextFork {
        last_n: fork
            .last_n
            .and_then(|turns| std::num::NonZeroU64::new(u64::from(turns))),
        mode: match fork.mode.as_str() {
            "all" => session::ContextForkMode::All,
            "none" => session::ContextForkMode::None,
            "last_n" => session::ContextForkMode::LastN,
            _ => panic!("sealed child context fork mode is a closed enum"),
        },
        resolved_turns: u64::from(fork.resolved_turns),
        source_context_generation: fork.source_context_generation,
        source_projection_digest: fork
            .source_projection_digest
            .parse()
            .expect("sealed child context fork digest satisfies the public contract"),
        source_session_id: fork
            .source_session_id
            .parse()
            .expect("sealed child context fork source id satisfies the public contract"),
        source_through_sequence: fork.source_through_sequence,
    }
}

fn content_blocks(content: MessageRequestContent) -> Result<Vec<ContentBlock>> {
    match content {
        MessageRequestContent::String(s) => {
            let s: String = s.to_string();
            if s.is_empty() {
                return Err(BrainError::Invalid("content must not be empty".into()));
            }
            Ok(vec![ContentBlock::text(s)])
        }
        MessageRequestContent::Array(parts) => {
            if parts.is_empty() {
                return Err(BrainError::Invalid("content must not be empty".into()));
            }
            let mut blocks = Vec::with_capacity(parts.len());
            for p in parts {
                match p {
                    session::ContentPart::Text { text } => blocks.push(ContentBlock::text(text)),
                    session::ContentPart::WorkspaceFile { .. } => {
                        return Err(BrainError::Invalid(
                            "workspace_file content parts are not available yet (M1)".into(),
                        ));
                    }
                }
            }
            Ok(blocks)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::fake::{FakeProvider, Scripted};
    use crate::storage::SessionStoragePort as _;
    use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn typed_create(value: serde_json::Value) -> CreateSessionRequest {
        serde_json::from_value(value).expect("test CreateSessionRequest deserializes")
    }

    #[test]
    fn journal_retention_config_rejects_malformed_and_inconsistent_policy() {
        assert_eq!(parse_strict_env_u64("TEST_LIMIT", None, 17).unwrap(), 17);
        assert_eq!(
            parse_strict_env_u64("TEST_LIMIT", Some("42"), 17).unwrap(),
            42
        );
        for raw in ["", "-1", "1.5", "not-a-number"] {
            assert!(matches!(
                parse_strict_env_u64("TEST_LIMIT", Some(raw), 17),
                Err(BrainError::Invalid(_))
            ));
        }

        let mut cfg = BrainConfig::default();
        cfg.journal_max_session_bytes = crate::journal::MIN_SESSION_JOURNAL_BYTES;
        cfg.journal_max_tenant_bytes = cfg.journal_max_session_bytes;
        cfg.journal_max_tenant_sessions = 1;
        cfg.validate().unwrap();

        cfg.journal_max_tenant_bytes = cfg.journal_max_session_bytes - 1;
        assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
        cfg.journal_max_tenant_bytes = cfg.journal_max_session_bytes;
        cfg.journal_max_tenant_sessions = 0;
        assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
    }

    #[test]
    fn process_environment_policy_uses_exact_defaults_and_bounds() {
        let cfg = BrainConfig::from_env_values(|_| Ok(None)).unwrap();
        assert_eq!(
            cfg.max_concurrent_model_rounds,
            DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS
        );
        assert_eq!(cfg.max_concurrent_turns, DEFAULT_MAX_CONCURRENT_TURNS);
        assert_eq!(cfg.max_concurrent_creates, DEFAULT_MAX_CONCURRENT_CREATES);
        assert_eq!(cfg.max_event_followers, DEFAULT_MAX_EVENT_FOLLOWERS);
        assert_eq!(cfg.max_resident_sessions, DEFAULT_MAX_RESIDENT_SESSIONS);
        assert_eq!(
            cfg.storage_max_tenant_bytes,
            DEFAULT_STORAGE_MAX_TENANT_BYTES
        );
        assert_eq!(
            cfg.max_concurrent_recoveries,
            DEFAULT_MAX_CONCURRENT_RECOVERIES
        );

        for (name, default, minimum, maximum) in [
            (
                MAX_MODEL_ROUNDS_ENV,
                DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS,
                1,
                MAX_CONCURRENT_MODEL_ROUNDS,
            ),
            (
                MAX_TURNS_ENV,
                DEFAULT_MAX_CONCURRENT_TURNS,
                1,
                MAX_CONCURRENT_TURNS,
            ),
            (
                MAX_CONCURRENT_CREATES_ENV,
                DEFAULT_MAX_CONCURRENT_CREATES,
                1,
                MAX_CONCURRENT_CREATES,
            ),
            (
                MAX_EVENT_FOLLOWERS_ENV,
                DEFAULT_MAX_EVENT_FOLLOWERS,
                1,
                MAX_EVENT_FOLLOWERS,
            ),
            (
                MAX_RESIDENT_SESSIONS_ENV,
                DEFAULT_MAX_RESIDENT_SESSIONS,
                1,
                MAX_RESIDENT_SESSIONS,
            ),
            (
                MAX_ADDITIONAL_SANDBOXES_ENV,
                DEFAULT_MAX_ADDITIONAL_SANDBOXES,
                1,
                MAX_ADDITIONAL_SANDBOXES,
            ),
            (
                RECOVERY_SHARDS_PER_POLL_ENV,
                DEFAULT_RECOVERY_SHARDS_PER_POLL,
                1,
                crate::journal::RECOVERY_SHARDS,
            ),
            (
                RECOVERY_PAGE_SIZE_ENV,
                DEFAULT_RECOVERY_PAGE_SIZE,
                1,
                MAX_RECOVERY_PAGE_SIZE,
            ),
            (
                MAX_CONCURRENT_RECOVERIES_ENV,
                DEFAULT_MAX_CONCURRENT_RECOVERIES,
                1,
                MAX_CONCURRENT_RECOVERIES,
            ),
        ] {
            assert_eq!(
                parse_env_usize(name, None, default, minimum, maximum).unwrap(),
                default
            );
            assert_eq!(
                parse_env_usize(name, Some(&minimum.to_string()), default, minimum, maximum)
                    .unwrap(),
                minimum
            );
            assert_eq!(
                parse_env_usize(name, Some(&maximum.to_string()), default, minimum, maximum)
                    .unwrap(),
                maximum
            );
            assert!(
                parse_env_usize(
                    name,
                    Some(&maximum.saturating_add(1).to_string()),
                    default,
                    minimum,
                    maximum,
                )
                .is_err()
            );
            for invalid in ["", "-1", "1.5", "not-a-number"] {
                assert!(
                    parse_env_usize(name, Some(invalid), default, minimum, maximum).is_err(),
                    "{name} accepted {invalid:?}"
                );
            }
        }

        for (name, default, minimum, maximum) in [
            (
                PROVIDER_HEADER_TIMEOUT_ENV,
                DEFAULT_PROVIDER_HEADER_TIMEOUT_MS,
                MIN_PROVIDER_HEADER_TIMEOUT_MS,
                MAX_PROVIDER_HEADER_TIMEOUT_MS,
            ),
            (
                PROVIDER_IDLE_TIMEOUT_ENV,
                DEFAULT_PROVIDER_IDLE_TIMEOUT_MS,
                MIN_PROVIDER_IDLE_TIMEOUT_MS,
                MAX_PROVIDER_IDLE_TIMEOUT_MS,
            ),
            (
                PROVIDER_TOTAL_TIMEOUT_ENV,
                DEFAULT_PROVIDER_TOTAL_TIMEOUT_MS,
                MIN_PROVIDER_TOTAL_TIMEOUT_MS,
                MAX_PROVIDER_TOTAL_TIMEOUT_MS,
            ),
            (
                EXTERNAL_TOOL_TIMEOUT_ENV,
                DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS,
                MIN_EXTERNAL_TOOL_TIMEOUT_MS,
                MAX_EXTERNAL_TOOL_TIMEOUT_MS,
            ),
            (
                STORAGE_TRANSFER_TTL_ENV,
                crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
                MIN_STORAGE_TRANSFER_TTL_MS,
                MAX_STORAGE_TRANSFER_TTL_MS,
            ),
            (
                RECOVERY_POLL_ENV,
                DEFAULT_RECOVERY_POLL_MS,
                MIN_RECOVERY_POLL_MS,
                MAX_RECOVERY_POLL_MS,
            ),
        ] {
            assert_eq!(
                parse_env_u64(name, None, default, minimum, maximum).unwrap(),
                default
            );
            assert_eq!(
                parse_env_u64(name, Some(&minimum.to_string()), default, minimum, maximum).unwrap(),
                minimum
            );
            assert_eq!(
                parse_env_u64(name, Some(&maximum.to_string()), default, minimum, maximum).unwrap(),
                maximum
            );
            assert!(
                parse_env_u64(
                    name,
                    Some(&maximum.saturating_add(1).to_string()),
                    default,
                    minimum,
                    maximum,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn process_environment_policy_rejects_cross_field_and_string_drift() {
        let load = |values: &[(&str, &str)]| {
            BrainConfig::from_env_values(|name| {
                Ok(values
                    .iter()
                    .find_map(|(candidate, value)| (*candidate == name).then(|| (*value).into())))
            })
        };

        assert!(load(&[(MAX_TURNS_ENV, "0")]).is_err());
        assert!(load(&[(OUTBOUND_ALLOW_PRIVATE_ENV, "TRUE")]).is_err());
        assert!(
            load(&[
                (STORAGE_MAX_OBJECT_BYTES_ENV, "2"),
                (STORAGE_MAX_SESSION_BYTES_ENV, "1"),
            ])
            .is_err()
        );
        assert!(
            load(&[
                (PROVIDER_HEADER_TIMEOUT_ENV, "3000"),
                (PROVIDER_TOTAL_TIMEOUT_ENV, "2000"),
            ])
            .is_err()
        );
        assert!(load(&[(EXTERNAL_EXECUTOR_TOKEN_ENV, "secret-without-url")]).is_err());
        assert!(
            load(&[(
                EXTERNAL_EXECUTOR_URL_ENV,
                "http://127.0.0.1:1234/tools?credential=sentinel",
            )])
            .is_err()
        );
        assert!(
            load(&[
                (EXTERNAL_EXECUTOR_URL_ENV, "http://127.0.0.1:1234/tools"),
                (EXTERNAL_EXECUTOR_TOKEN_ENV, "invalid\nheader"),
            ])
            .is_err()
        );
        assert!(
            load(&[
                (EXTERNAL_EXECUTOR_URL_ENV, "http://127.0.0.1:1234/tools"),
                (EXTERNAL_EXECUTOR_CAPABILITIES_ENV, "aex.output,aex.output"),
            ])
            .is_err()
        );
        load(&[
            (EXTERNAL_EXECUTOR_URL_ENV, "http://127.0.0.1:1234/tools"),
            (EXTERNAL_EXECUTOR_TOKEN_ENV, "valid-token"),
            (EXTERNAL_EXECUTOR_CAPABILITIES_ENV, "aex.output,aex.web"),
        ])
        .unwrap();
    }

    #[test]
    fn transport_timeout_policy_accepts_exact_bounds_and_rejects_invalid_order() {
        let mut cfg = BrainConfig {
            provider_header_timeout: Duration::from_millis(MIN_PROVIDER_HEADER_TIMEOUT_MS),
            provider_idle_timeout: Duration::from_millis(MIN_PROVIDER_IDLE_TIMEOUT_MS),
            provider_total_timeout: Duration::from_millis(MIN_PROVIDER_TOTAL_TIMEOUT_MS),
            external_call_timeout: Duration::from_millis(MIN_EXTERNAL_TOOL_TIMEOUT_MS),
            ..BrainConfig::default()
        };
        cfg.validate().unwrap();

        cfg.provider_header_timeout = Duration::from_millis(MAX_PROVIDER_HEADER_TIMEOUT_MS);
        cfg.provider_idle_timeout = Duration::from_millis(MAX_PROVIDER_IDLE_TIMEOUT_MS);
        cfg.provider_total_timeout = Duration::from_millis(MAX_PROVIDER_TOTAL_TIMEOUT_MS);
        cfg.external_call_timeout = Duration::from_millis(MAX_EXTERNAL_TOOL_TIMEOUT_MS);
        cfg.validate().unwrap();

        cfg.provider_header_timeout =
            Duration::from_millis(MIN_PROVIDER_HEADER_TIMEOUT_MS.saturating_sub(1));
        assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
        cfg.provider_header_timeout = Duration::from_millis(MAX_PROVIDER_HEADER_TIMEOUT_MS);
        cfg.provider_idle_timeout =
            Duration::from_millis(MAX_PROVIDER_IDLE_TIMEOUT_MS.saturating_add(1));
        assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
        cfg.provider_idle_timeout = Duration::from_millis(MAX_PROVIDER_IDLE_TIMEOUT_MS);
        cfg.external_call_timeout =
            Duration::from_millis(MAX_EXTERNAL_TOOL_TIMEOUT_MS.saturating_add(1));
        assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));

        cfg.external_call_timeout = Duration::from_millis(DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS);
        cfg.provider_header_timeout = Duration::from_millis(DEFAULT_PROVIDER_HEADER_TIMEOUT_MS);
        cfg.provider_idle_timeout = Duration::from_millis(DEFAULT_PROVIDER_IDLE_TIMEOUT_MS);
        cfg.provider_total_timeout =
            Duration::from_millis(DEFAULT_PROVIDER_IDLE_TIMEOUT_MS.saturating_sub(1));
        assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
    }

    #[tokio::test]
    async fn resident_actor_admission_is_hard_bounded_and_prunes_dead_root_cells() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-resident-cap-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        let brain = Brain::with_parts(
            BrainConfig {
                max_resident_sessions: 2,
                idle_discard: Duration::from_millis(100),
                ..BrainConfig::default()
            },
            Journal::new_memory("brain-resident-cap"),
            Arc::new(crate::keys::PlainCustody),
            None,
        );
        let first = brain
            .spawn_actor("ses_resident_1", ActorStartup::Lazy)
            .await
            .unwrap();
        let second = brain
            .spawn_actor("ses_resident_2", ActorStartup::Lazy)
            .await
            .unwrap();
        let third = brain
            .spawn_actor("ses_resident_3", ActorStartup::Lazy)
            .await
            .expect("pressure evicts one safe idle resident");
        assert!(brain.sessions.lock().expect("sessions").len() <= 2);

        {
            let mut cells = brain.root_secret_cells.lock().expect("root secret cells");
            for index in 0..1_000 {
                cells.insert(format!("root-{index}"), Weak::new());
            }
        }

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if first.is_closed()
                    && second.is_closed()
                    && brain.sessions.lock().expect("sessions").is_empty()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("idle residents release their slots");
        assert!(
            brain
                .root_secret_cells
                .lock()
                .expect("root secret cells")
                .is_empty()
        );

        let fourth = brain
            .spawn_actor("ses_resident_4", ActorStartup::Lazy)
            .await
            .expect("released resident capacity is immediately reusable");
        assert!(!third.is_closed() || !fourth.is_closed());
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn resident_pressure_returns_overload_when_every_slot_is_active() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-resident-active-cap-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        let brain = Brain::with_parts(
            BrainConfig {
                max_resident_sessions: 1,
                ..BrainConfig::default()
            },
            Journal::new_memory("brain-resident-active-cap"),
            Arc::new(crate::keys::PlainCustody),
            None,
        );
        // The resident permit is the authoritative process bound and remains held across every
        // active turn/effect. With no idle actor registered for pressure, admission waits once
        // for the fixed short window and fails honestly.
        let _active = brain.resident_permits.clone().try_acquire_owned().unwrap();
        let started = std::time::Instant::now();
        assert!(matches!(
            brain
                .spawn_actor("ses_resident_busy", ActorStartup::Lazy)
                .await,
            Err(BrainError::Overloaded)
        ));
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(brain.sessions.lock().expect("sessions").is_empty());
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn end_returns_the_durable_fence_before_async_teardown_converges() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-async-end-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        let journal = Journal::new_memory("brain-async-end");
        let brain = Brain::with_parts(
            BrainConfig::default(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            None,
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
                })),
                Some("async-end"),
            )
            .await
            .unwrap();
        let session_id = created.id.to_string();
        assert_eq!(
            created.model.context_window_tokens,
            i64::from(brain_protocol::DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS)
        );

        let accepted = brain.end(&session_id).await.unwrap();
        assert_eq!(accepted.state, session::SessionState::Ending);
        assert_eq!(accepted.turn_state, session::SessionTurnState::Idle);
        assert!(
            journal.get_head(&session_id).await.unwrap().doc.ended,
            "the response must be backed by the durable admission fence"
        );

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if journal.get_head(&session_id).await.unwrap().doc.state == "ended" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("background end convergence");
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn end_fences_before_a_cancellation_resistant_effect_and_recovery_never_reopens() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-end-resistant-effect-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create resistant effect data dir");
        let journal = Journal::new_memory("brain-end-resistant-effect");
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::tool("submit", json!({"answer": 42}))]);
        let provider = fake.clone();
        let executor = Arc::new(CancellationResistantExecutor::default());
        let brain = Brain::with_parts_and_external(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                official_capabilities: HashMap::from([("aex.submit".into(), submit_policy())]),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            executor.clone(),
            Some(Arc::new(move |_| provider.clone())),
        );
        let root = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider":"anthropic", "name":"resistant-effect", "api_key":"key"
                    },
                    "tools": {"items": [{
                        "definition": {
                            "name":"submit", "description":"submit a final result",
                            "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "input_schema":{"type":"object"},
                            "output_schema":{"type":"object"}
                        },
                        "executor":{"kind":"engine", "capability":"aex.submit"}
                    }]}
                })),
                Some("resistant-end"),
            )
            .await
            .expect("create resistant-effect root");
        let root_id = root.id.to_string();

        // Persist a depth-two descendant so admission has to evaluate the immutable ancestor
        // chain while the root actor itself is parked inside the resistant effect.
        let root_head = journal.get_head(&root_id).await.expect("root head");
        let child_id = "ses_resistantendchild0000";
        let mut child = root_head.doc.clone();
        child.root_id = root_id.clone();
        child.parent_id = Some(root_id.clone());
        child.ancestor_ids = vec![root_id.clone()];
        child.depth = 1;
        child.last_seq = 1;
        child.create_key_hash = None;
        child.create_request_hash = None;
        child.context_fork = None;
        child.default_sandbox = None;
        journal
            .create(
                child_id,
                &child,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .expect("create resistant-effect child");
        let grandchild_id = "ses_resistantendgrand0000";
        let mut grandchild = child.clone();
        grandchild.parent_id = Some(child_id.into());
        grandchild.ancestor_ids = vec![root_id.clone(), child_id.into()];
        grandchild.depth = 2;
        journal
            .create(
                grandchild_id,
                &grandchild,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .expect("create resistant-effect grandchild");

        brain
            .message(
                &root_id,
                MessageRequestContent::String("run the resistant effect".parse().unwrap()),
            )
            .await
            .expect("admit root turn");
        tokio::time::timeout(Duration::from_secs(2), async {
            while executor.calls.load(Ordering::Acquire) == 0 {
                executor.entered.notified().await;
            }
        })
        .await
        .expect("external effect starts");

        let accepted = tokio::time::timeout(Duration::from_secs(1), brain.end(&root_id))
            .await
            .expect("END acceptance cannot wait for a cancellation-resistant effect")
            .expect("END fence commits");
        assert_eq!(accepted.state, session::SessionState::Ending);
        assert!(!executor.released.load(Ordering::Acquire));
        let fenced = journal.get_head(&root_id).await.expect("fenced root");
        assert_eq!(fenced.doc.state, "ending");
        assert!(fenced.doc.ended);
        assert!(
            fenced.doc.turn.is_some(),
            "pending work remains recoverable"
        );

        let descendant_error = brain
            .message(
                grandchild_id,
                MessageRequestContent::String("late follow-up".parse().unwrap()),
            )
            .await
            .expect_err("the durable ancestor fence closes deep admission immediately");
        assert!(matches!(descendant_error, BrainError::Fenced));

        executor.release();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let head = journal.get_head(&root_id).await.unwrap();
                if head.doc.turn.is_none() && executor.calls.load(Ordering::Acquire) >= 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("a new owner reconciles the resistant effect after the fence");
        assert!(matches!(
            journal.get_head(&root_id).await.unwrap().doc.state.as_str(),
            "ending" | "ended"
        ));
        let records = journal
            .read_records(&root_id, 0)
            .await
            .expect("root records");
        let ending_seq = records
            .iter()
            .find(|entry| matches!(&entry.record, Record::State { state, .. } if state == "ending"))
            .expect("durable ending record")
            .seq;
        assert!(
            records.iter().all(|entry| {
                entry.seq <= ending_seq
                    || !matches!(&entry.record, Record::State { state, .. } if state == "open")
            }),
            "turn reconciliation after the END fence must never reopen the lifecycle"
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn end_recovery_terminates_only_owned_child_sandboxes_then_all_root_inventory() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-end-additional-sandboxes-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create sandbox teardown data dir");
        let journal = Journal::new_memory("brain-end-additional-sandboxes");
        let control = Arc::new(EndSandboxControl::default());
        control.failures_remaining.store(1, Ordering::Release);
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                recovery_poll_interval: Duration::from_millis(10),
                recovery_shards_per_poll: crate::journal::RECOVERY_SHARDS,
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                sandbox_control: Some(control.clone()),
                ..BrainServices::default()
            },
            None,
        );
        brain.start_recovery_worker();
        let root = brain
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic", "name":"end-sandboxes", "api_key":"key"}
                })),
                Some("end-additional-sandboxes"),
            )
            .await
            .expect("create sandbox teardown root");
        let root_id = root.id.to_string();
        let root_head = journal.get_head(&root_id).await.expect("root head");
        let child_id = "ses_endsandboxchild00000";
        let mut child = root_head.doc.clone();
        child.root_id = root_id.clone();
        child.parent_id = Some(root_id.clone());
        child.ancestor_ids = vec![root_id.clone()];
        child.depth = 1;
        child.last_seq = 1;
        child.turn = None;
        child.turns = 0;
        child.create_key_hash = None;
        child.create_request_hash = None;
        child.context_fork = None;
        child.default_sandbox = None;
        journal
            .create(
                child_id,
                &child,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .expect("create sandbox teardown child");

        let root_sandbox =
            reserve_live_additional_sandbox(&journal, &root_id, &root_id, "op_root_sandbox").await;
        let child_sandbox =
            reserve_live_additional_sandbox(&journal, &root_id, child_id, "op_child_sandbox").await;

        let accepted = brain.end(child_id).await.expect("accept child END fence");
        assert_eq!(accepted.state, session::SessionState::Ending);
        assert_eq!(
            journal
                .get_sandbox(&root_id, &root_sandbox.sandbox_id)
                .await
                .unwrap()
                .status
                .state,
            brain_protocol::hand::SandboxState::Running,
            "a child END must not terminate an additional sandbox owned by its root"
        );

        // The first Hand termination fails after the END response. With no further API traffic,
        // the durable ending due-key must cause recovery to retry and reach ENDED.
        tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                if journal.get_head(child_id).await.unwrap().doc.state == "ended" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("no-traffic recovery terminates the child-owned sandbox");
        let child_terminal = journal
            .get_sandbox(&root_id, &child_sandbox.sandbox_id)
            .await
            .expect("child sandbox tombstone");
        assert!(sandbox_status_releases_slot(&child_terminal.status));
        assert!(child_terminal.slot_released);
        let root_live = journal
            .get_sandbox(&root_id, &root_sandbox.sandbox_id)
            .await
            .expect("root sandbox remains live");
        assert_eq!(
            root_live.status.state,
            brain_protocol::hand::SandboxState::Running
        );
        assert!(!root_live.slot_released);

        let root_accepted = brain.end(&root_id).await.expect("accept root END fence");
        assert_eq!(root_accepted.state, session::SessionState::Ending);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if journal.get_head(&root_id).await.unwrap().doc.state == "ended" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("root END terminates every remaining root inventory item");
        let root_terminal = journal
            .get_sandbox(&root_id, &root_sandbox.sandbox_id)
            .await
            .expect("root sandbox tombstone");
        assert!(sandbox_status_releases_slot(&root_terminal.status));
        assert!(root_terminal.slot_released);
        let terminated = control
            .terminated
            .lock()
            .expect("terminated sandboxes")
            .clone();
        assert_eq!(
            terminated
                .iter()
                .filter(|sandbox_id| *sandbox_id == &child_sandbox.sandbox_id)
                .count(),
            1,
            "a terminal child tombstone is not terminated again by root END"
        );
        assert!(terminated.contains(&root_sandbox.sandbox_id));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn complete_create_contract_bounds_are_enforced_before_resolution() {
        let omitted = typed_create(json!({
            "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
        }));
        validate_create_request(&omitted).expect("omitted values use schema defaults");

        let exact = typed_create(json!({
            "model": {
                "provider":"anthropic", "name":"model", "api_key":"key",
                "max_output_tokens": brain_protocol::MAX_MODEL_OUTPUT_TOKENS,
                "context_window_tokens": brain_protocol::MAX_MODEL_CONTEXT_WINDOW_TOKENS,
                "temperature": 2.0
            },
            "provider_recovery_retries": 8,
            "client": {"id":"app", "submit_retries":8},
            "children": {
                "max_depth":8, "max_direct_children":128,
                "max_descendants":1024
            }
        }));
        validate_create_request(&exact).expect("every exact public maximum is accepted");

        for (label, value) in [
            (
                "provider_recovery_retries",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"provider_recovery_retries":9}),
            ),
            (
                "client.submit_retries",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"client":{"id":"app","submit_retries":9}}),
            ),
            (
                "children.max_depth",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"children":{"max_depth":9}}),
            ),
            (
                "children.max_direct_children",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"children":{"max_direct_children":129}}),
            ),
            (
                "children.max_descendants",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"children":{"max_descendants":1025}}),
            ),
            (
                "model.max_output_tokens",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key","max_output_tokens":u64::from(brain_protocol::MAX_MODEL_OUTPUT_TOKENS)+1}}),
            ),
            (
                "model.context_window_tokens below minimum",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key","context_window_tokens":i64::from(brain_protocol::MIN_MODEL_CONTEXT_WINDOW_TOKENS)-1}}),
            ),
            (
                "model.context_window_tokens above maximum",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key","context_window_tokens":i64::from(brain_protocol::MAX_MODEL_CONTEXT_WINDOW_TOKENS)+1}}),
            ),
            (
                "model.temperature",
                json!({"model":{"provider":"anthropic","name":"model","api_key":"key","temperature":2.01}}),
            ),
        ] {
            let request = typed_create(value);
            let error = validate_create_request(&request)
                .expect_err("a value above the public maximum must be rejected");
            assert!(
                matches!(error, BrainError::Invalid(_)),
                "{label} produced {error:?}"
            );
        }

        let secrets = (0..129)
            .map(|index| (format!("SECRET_{index}"), json!("value")))
            .collect::<serde_json::Map<_, _>>();
        let request = typed_create(json!({
            "model":{"provider":"anthropic","name":"model","api_key":"key"},
            "secrets": secrets
        }));
        assert!(matches!(
            validate_create_request(&request),
            Err(BrainError::Invalid(_))
        ));

        let exact_secret_document = typed_create(json!({
            "model":{"provider":"anthropic","name":"model","api_key":"key"},
            "secrets":{"A":"é".repeat(2044)}
        }));
        assert_eq!(
            serde_jcs::to_vec(&exact_secret_document.secrets)
                .unwrap()
                .len(),
            brain_protocol::MAX_SESSION_SECRET_DOCUMENT_BYTES
        );
        validate_create_request(&exact_secret_document)
            .expect("an exact-size custody document is accepted");
        let oversized_secret_document = typed_create(json!({
            "model":{"provider":"anthropic","name":"model","api_key":"key"},
            "secrets":{"A":"é".repeat(2045)}
        }));
        assert!(matches!(
            validate_create_request(&oversized_secret_document),
            Err(BrainError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn context_capacity_rejects_before_custody_or_hand_effects() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-context-admission-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        let custody = Arc::new(CountingCustody::default());
        let brain = Brain::with_parts(
            BrainConfig::default(),
            Journal::new_memory("brain-context-admission"),
            custody.clone(),
            None,
        );
        let error = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider":"anthropic",
                        "name":"unknown-small-model",
                        "api_key":"key",
                        "max_output_tokens": brain_protocol::MAX_MODEL_OUTPUT_TOKENS
                    }
                })),
                Some("context-admission"),
            )
            .await
            .expect_err("the conservative default window cannot fit a 128K output reserve");
        assert!(matches!(error, BrainError::Invalid(_)));
        assert_eq!(custody.encrypts.load(Ordering::Relaxed), 0);
        assert!(
            !data_dir.exists(),
            "Hand staging must not run after a pure validation failure"
        );
    }

    fn submit_policy() -> crate::config::ServerToolPolicy {
        crate::config::ServerToolPolicy {
            capability: "aex.submit".into(),
            scope: brain_protocol::session::ExternalToolScope::Root,
            completion: brain_protocol::session::ExternalToolCompletion::ReturnDirect,
            effect: brain_protocol::session::ExternalToolEffect::ReplaySafe,
            max_input_bytes: 1024,
        }
    }

    #[derive(Default)]
    struct RecoveryExecutor {
        calls: AtomicUsize,
        call_ids: Mutex<Vec<String>>,
    }

    #[derive(Default)]
    struct CancellationResistantExecutor {
        calls: AtomicUsize,
        entered: Notify,
        released: AtomicBool,
        release_waiters: Notify,
    }

    #[derive(Default)]
    struct EndSandboxControl {
        failures_remaining: AtomicUsize,
        attempts: Mutex<Vec<String>>,
        terminated: Mutex<Vec<String>>,
    }

    impl EndSandboxControl {
        fn sandbox_id(target: &brain_protocol::hand::SandboxTarget) -> String {
            serde_json::to_value(target)
                .expect("serialize sandbox target")
                .get("sandbox_id")
                .and_then(serde_json::Value::as_str)
                .expect("additional target sandbox id")
                .to_owned()
        }
    }

    #[async_trait::async_trait]
    impl crate::hand::SandboxControlPort for EndSandboxControl {
        async fn create(
            &self,
            _request: brain_protocol::hand::CreateSandboxRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            panic!("unused")
        }

        async fn inspect(
            &self,
            _target: brain_protocol::hand::SandboxTarget,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            panic!("unused")
        }

        async fn execute(
            &self,
            _request: brain_protocol::hand::SandboxExecutionRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SubmitReceipt> {
            panic!("unused")
        }

        async fn write_stdin(
            &self,
            _request: brain_protocol::hand::WriteStdinRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::WriteStdinReceipt> {
            panic!("unused")
        }

        async fn terminate(
            &self,
            target: brain_protocol::hand::SandboxTarget,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            let sandbox_id = Self::sandbox_id(&target);
            self.attempts
                .lock()
                .expect("sandbox terminate attempts")
                .push(sandbox_id.clone());
            if self
                .failures_remaining
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(serde_json::from_value(json!({
                    "code":"temporarily_unavailable",
                    "message":"injected transient termination failure",
                    "retryable":true
                }))
                .expect("valid Hand error"));
            }
            self.terminated
                .lock()
                .expect("terminated sandboxes")
                .push(sandbox_id.clone());
            Ok(serde_json::from_value(json!({
                "state":"terminated",
                "target":target,
                "generation":format!("gen_{sandbox_id}"),
                "target_ref":format!("tgt_{sandbox_id}"),
                "changed_at_ms":crate::wall_ms(),
                "reason":"session_ended"
            }))
            .expect("valid terminal sandbox status"))
        }
    }

    async fn reserve_live_additional_sandbox(
        journal: &Journal,
        root_id: &str,
        owner_session_id: &str,
        operation_id: &str,
    ) -> crate::journal::SandboxInventoryDoc {
        let (sandbox_id, generation, target) =
            additional_sandbox_identity(root_id, owner_session_id, operation_id)
                .expect("additional sandbox identity");
        journal
            .reserve_sandbox(&crate::journal::SandboxReserveRequest {
                root_id: root_id.to_owned(),
                owner_session_id: owner_session_id.to_owned(),
                sandbox_id,
                operation_id: operation_id.to_owned(),
                request_digest: hex::encode(Sha256::digest(operation_id.as_bytes())),
                generation_intent: generation.clone(),
                initial_status: serde_json::from_value(json!({
                    "state":"running",
                    "target":target,
                    "generation":generation,
                    "target_ref":format!("tgt_{operation_id}"),
                    "changed_at_ms":crate::wall_ms(),
                    "expires_at_ms":crate::wall_ms() + 60_000
                }))
                .expect("valid live sandbox status"),
                now_ms: crate::wall_ms(),
            })
            .await
            .expect("reserve additional sandbox")
    }

    impl CancellationResistantExecutor {
        fn release(&self) {
            self.released.store(true, Ordering::Release);
            self.release_waiters.notify_waiters();
        }
    }

    #[async_trait::async_trait]
    impl ToolExecutor for CancellationResistantExecutor {
        fn supports(&self, capability: &str) -> bool {
            capability == "aex.submit"
        }

        async fn call(
            &self,
            capability: &str,
            request: ExternalToolCallRequest,
            _cancel: CancellationToken,
        ) -> Result<ExternalToolCallResponse> {
            assert_eq!(capability, "aex.submit");
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.entered.notify_waiters();
            loop {
                let notified = self.release_waiters.notified();
                if self.released.load(Ordering::Acquire) {
                    break;
                }
                notified.await;
            }
            Ok(serde_json::from_value(json!({
                "outcome": "completed",
                "content": "accepted after the END fence",
                "is_error": false,
                "disposition": "complete_turn",
                "result": request.input,
                "result_metadata": {"recovered": "true"}
            }))
            .expect("valid resistant-effect response"))
        }
    }

    struct ReservationStorage {
        journal: Journal,
        prepares: AtomicUsize,
        aborts: AtomicUsize,
        fail_next_abort: AtomicBool,
        fail_next_write_before_effect: AtomicBool,
        saw_durable_reservation: AtomicBool,
        writes: AtomicUsize,
        pending: Mutex<HashMap<String, crate::storage::StorageUploadRequest>>,
        staged: Mutex<HashSet<String>>,
        objects: Mutex<HashMap<String, crate::storage::StorageObject>>,
    }

    struct DirectTransferPreparation;

    struct DirectTransferFiles {
        storage: Arc<ReservationStorage>,
        imports: AtomicUsize,
        exports: AtomicUsize,
    }

    #[derive(Default)]
    struct UnknownManagedPorts {
        submits: AtomicUsize,
        status_calls: AtomicUsize,
        dematerialize_calls: AtomicUsize,
        fail_next_dematerialize: AtomicBool,
    }

    struct ScriptedCompactor {
        failures_remaining: AtomicUsize,
        calls: AtomicUsize,
    }

    #[derive(Default)]
    struct CountingCustody {
        encrypts: AtomicUsize,
        decrypts: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl KeyCustody for CountingCustody {
        async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>> {
            self.encrypts.fetch_add(1, Ordering::Relaxed);
            crate::keys::PlainCustody.encrypt(session_id, key).await
        }

        async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey> {
            self.decrypts.fetch_add(1, Ordering::Relaxed);
            crate::keys::PlainCustody.decrypt(session_id, blob).await
        }
    }

    async fn connect_customer_process(
        coordinator: &Arc<crate::customer::CustomerCoordinator>,
        process_id: &str,
    ) -> (
        crate::customer::CustomerGrant,
        tokio::sync::mpsc::Receiver<crate::customer::CustomerCommand>,
        u64,
    ) {
        let grant = coordinator.grant("local", "app").await.unwrap();
        let proof = crate::customer::frame_proof(&grant.protocol);
        let connection_id = crate::mint_id("conn", 20);
        crate::customer::CustomerHandIngressPort::receive(
            coordinator.as_ref(),
            crate::customer::CustomerGatewayInput {
                route: crate::customer::CustomerGatewayRoute::Connect,
                connection_id: connection_id.clone(),
                request_id: crate::mint_id("req", 16),
                route_key: "$connect".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: Some(grant.protocol.clone()),
                body: None,
            },
        )
        .await
        .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        coordinator
            .bind_local_sender(&connection_id, sender)
            .await
            .unwrap();
        crate::customer::CustomerHandIngressPort::receive(
            coordinator.as_ref(),
            crate::customer::CustomerGatewayInput {
                route: crate::customer::CustomerGatewayRoute::Message,
                connection_id: connection_id.clone(),
                request_id: crate::mint_id("req", 16),
                route_key: "$default".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: None,
                body: Some(
                    json!({
                        "type":"register", "client_id":"app",
                        "process_id":process_id, "proof":proof
                    })
                    .to_string(),
                ),
            },
        )
        .await
        .unwrap();
        let Some(crate::customer::CustomerCommand::Ready { epoch }) = receiver.recv().await else {
            panic!("customer ready")
        };
        crate::customer::CustomerHandIngressPort::receive(
            coordinator.as_ref(),
            crate::customer::CustomerGatewayInput {
                route: crate::customer::CustomerGatewayRoute::Message,
                connection_id,
                request_id: crate::mint_id("req", 16),
                route_key: "$default".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: None,
                body: Some(
                    json!({
                        "type":"register_tools", "epoch":epoch, "batch_id":"batch:test",
                        "proof":proof,
                        "registrations":[{
                            "registration":"lookup", "name":"lookup",
                            "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        }]
                    })
                    .to_string(),
                ),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(crate::customer::CustomerCommand::Registered { .. })
        ));
        (grant, receiver, epoch)
    }

    #[async_trait::async_trait]
    impl crate::compact::CompactionPort for ScriptedCompactor {
        async fn compact(
            &self,
            request: crate::compact::CompactionRequest,
            model: crate::compact::CompactionModel<'_>,
        ) -> Result<crate::compact::CompactionResult> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self
                .failures_remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(BrainError::Transport("compactor reset".into()));
            }
            Ok(crate::compact::CompactionResult {
                summary: format!(
                    "{}\nsemantic early fact preserved",
                    request.previous_summary.unwrap_or_default()
                ),
                provider: model.provider_name.into(),
                model: model.session.prefix.model.clone(),
                usage: crate::message::Usage {
                    input_tokens: Some(7),
                    output_tokens: Some(3),
                    cache_read_input_tokens: None,
                    cache_creation_input_tokens: None,
                    reasoning_tokens: None,
                },
            })
        }
    }

    impl ReservationStorage {
        fn new(journal: Journal) -> Self {
            Self {
                journal,
                prepares: AtomicUsize::new(0),
                aborts: AtomicUsize::new(0),
                fail_next_abort: AtomicBool::new(false),
                fail_next_write_before_effect: AtomicBool::new(false),
                saw_durable_reservation: AtomicBool::new(false),
                writes: AtomicUsize::new(0),
                pending: Mutex::new(HashMap::new()),
                staged: Mutex::new(HashSet::new()),
                objects: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::hand::SessionPreparationPort for DirectTransferPreparation {
        async fn prepare(
            &self,
            _request: brain_protocol::hand::PrepareSessionRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::PreparedSession> {
            Ok(
                serde_json::from_value(json!({"preparation_ref":"prep_direct_transfer"}))
                    .expect("prepared session"),
            )
        }

        async fn materialize_default(
            &self,
            request: brain_protocol::hand::CreateSandboxRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            Ok(serde_json::from_value(json!({
                "state":"running",
                "target":request.target,
                "generation":request.generation_intent,
                "target_ref":"target_direct_transfer",
                "changed_at_ms":crate::wall_ms(),
                "expires_at_ms":crate::wall_ms() + 60 * 60 * 1_000,
            }))
            .expect("running default sandbox"))
        }

        async fn dematerialize_default(
            &self,
            target: brain_protocol::hand::SandboxTarget,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            Ok(serde_json::from_value(json!({
                "state":"terminated",
                "target":target,
                "changed_at_ms":crate::wall_ms(),
                "expires_at_ms":null,
            }))
            .expect("terminated default sandbox"))
        }

        async fn purge_tree(&self, _root_id: &str) -> crate::hand::HandResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl crate::hand::HandPort for UnknownManagedPorts {
        async fn resolve_binding(
            &self,
            _binding: brain_protocol::hand::SealedBinding,
        ) -> crate::hand::HandResult<brain_protocol::hand::ResolvedBinding> {
            unreachable!("binding resolution is injected into the crash fold")
        }

        async fn submit(
            &self,
            _request: brain_protocol::hand::SubmitRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SubmitReceipt> {
            self.submits.fetch_add(1, Ordering::AcqRel);
            Err(serde_json::from_value(json!({
                "code":"operation_unknown",
                "message":"guest Submit may have run before the physical generation was lost",
                "retryable":false
            }))
            .expect("operation-unknown Hand error"))
        }

        async fn observe(
            &self,
            _request: brain_protocol::hand::ObserveRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::OperationObservation> {
            panic!("an unknown submit has no operation receipt to observe")
        }

        async fn cancel(
            &self,
            _request: brain_protocol::hand::CancelRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::CancellationReceipt> {
            panic!("an unknown submit has no operation receipt to cancel")
        }

        async fn acknowledge_terminal(
            &self,
            _request: brain_protocol::hand::AcknowledgeTerminalRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::Acknowledgement> {
            panic!("an unknown submit has no terminal receipt to acknowledge")
        }
    }

    #[async_trait::async_trait]
    impl crate::hand::SessionPreparationPort for UnknownManagedPorts {
        async fn prepare(
            &self,
            _request: brain_protocol::hand::PrepareSessionRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::PreparedSession> {
            unreachable!("the test session has no managed bundle preparation")
        }

        async fn materialize_default(
            &self,
            _request: brain_protocol::hand::CreateSandboxRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            panic!("OperationUnknown must not authorize replacement materialization")
        }

        async fn dematerialize_default(
            &self,
            target: brain_protocol::hand::SandboxTarget,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            self.dematerialize_calls.fetch_add(1, Ordering::AcqRel);
            if self.fail_next_dematerialize.swap(false, Ordering::AcqRel) {
                return Err(serde_json::from_value(json!({
                    "code":"temporarily_unavailable",
                    "message":"injected crash boundary before terminal sandbox cleanup",
                    "retryable":true
                }))
                .expect("transient Hand error"));
            }
            Ok(serde_json::from_value(json!({
                "state":"terminated",
                "target":target,
                "generation":"gen_unknown_submit",
                "target_ref":"tgt_unknown_submit",
                "changed_at_ms":crate::wall_ms(),
                "expires_at_ms":null,
                "reason":"operation_unknown_reconciled"
            }))
            .expect("terminal unknown target status"))
        }

        async fn purge_tree(&self, _root_id: &str) -> crate::hand::HandResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl crate::hand::SandboxFilesPort for UnknownManagedPorts {
        async fn status(
            &self,
            target: brain_protocol::hand::SandboxTarget,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            self.status_calls.fetch_add(1, Ordering::AcqRel);
            Ok(serde_json::from_value(json!({
                "state":"running",
                "target":target,
                "generation":"gen_unknown_submit",
                "target_ref":"tgt_unknown_submit",
                "changed_at_ms":crate::wall_ms(),
                "expires_at_ms":crate::wall_ms() + 60_000
            }))
            .expect("fenced unknown target status"))
        }

        async fn list(
            &self,
            _request: crate::hand::SandboxFileListRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileList> {
            unreachable!("unused")
        }

        async fn stat(
            &self,
            _request: brain_protocol::hand::SandboxFileRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::FileEntry> {
            unreachable!("unused")
        }

        async fn read(
            &self,
            _request: brain_protocol::hand::SandboxFileRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileContent> {
            unreachable!("unused")
        }

        async fn write(
            &self,
            _request: brain_protocol::hand::SandboxFileWriteRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxFileWriteResult> {
            unreachable!("unused")
        }

        async fn find(
            &self,
            _request: crate::hand::SandboxSearchRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileList> {
            unreachable!("unused")
        }

        async fn grep(
            &self,
            _request: crate::hand::SandboxSearchRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileList> {
            unreachable!("unused")
        }

        async fn transfer(
            &self,
            _request: brain_protocol::hand::SandboxCopyRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxCopyResult> {
            unreachable!("unused")
        }
    }

    fn direct_transfer_file(
        path: &str,
        bytes: u64,
        sha256: Option<&str>,
    ) -> brain_protocol::hand::FileEntry {
        serde_json::from_value(json!({
            "path":path,
            "kind":"file",
            "bytes":bytes,
            "sha256":sha256,
            "modified_at_ms":crate::wall_ms(),
        }))
        .expect("direct transfer file")
    }

    #[async_trait::async_trait]
    impl crate::hand::SandboxFilesPort for DirectTransferFiles {
        async fn status(
            &self,
            _target: brain_protocol::hand::SandboxTarget,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxStatus> {
            unreachable!("status is not used by the direct transfer test")
        }

        async fn list(
            &self,
            _request: crate::hand::SandboxFileListRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileList> {
            unreachable!("list is not used by the direct transfer test")
        }

        async fn stat(
            &self,
            request: brain_protocol::hand::SandboxFileRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::FileEntry> {
            Ok(direct_transfer_file(
                &String::from(request.path),
                2 * 1024 * 1024,
                None,
            ))
        }

        async fn read(
            &self,
            _request: brain_protocol::hand::SandboxFileRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileContent> {
            unreachable!("read is not used by the direct transfer test")
        }

        async fn write(
            &self,
            _request: brain_protocol::hand::SandboxFileWriteRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxFileWriteResult> {
            unreachable!("write is not used by the direct transfer test")
        }

        async fn find(
            &self,
            _request: crate::hand::SandboxSearchRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileList> {
            unreachable!("find is not used by the direct transfer test")
        }

        async fn grep(
            &self,
            _request: crate::hand::SandboxSearchRequest,
        ) -> crate::hand::HandResult<crate::hand::SandboxFileList> {
            unreachable!("grep is not used by the direct transfer test")
        }

        async fn transfer(
            &self,
            request: brain_protocol::hand::SandboxCopyRequest,
        ) -> crate::hand::HandResult<brain_protocol::hand::SandboxCopyResult> {
            let path = String::from(request.path.clone());
            let result = match request.direction {
                brain_protocol::hand::SandboxCopyRequestDirection::Export => {
                    self.exports.fetch_add(1, Ordering::Relaxed);
                    let transfer_id = String::from(request.transfer.transfer_id.clone());
                    self.storage
                        .staged
                        .lock()
                        .expect("staged uploads")
                        .insert(transfer_id);
                    json!({
                        "operation_id":request.operation_id,
                        "request_digest":request.request_digest,
                        "replayed":false,
                        "file":direct_transfer_file(&path, 2 * 1024 * 1024, None),
                        "object":{
                            "object_id":request.transfer.object_id,
                            "bytes":2 * 1024 * 1024,
                            "sha256":"d".repeat(64),
                        }
                    })
                }
                brain_protocol::hand::SandboxCopyRequestDirection::Import => {
                    self.imports.fetch_add(1, Ordering::Relaxed);
                    let object = request.object.expect("import object");
                    json!({
                        "operation_id":request.operation_id,
                        "request_digest":request.request_digest,
                        "replayed":false,
                        "file":direct_transfer_file(&path, object.bytes, Some(&object.sha256)),
                        "object":null,
                    })
                }
            };
            Ok(serde_json::from_value(result).expect("sandbox copy result"))
        }
    }

    #[async_trait::async_trait]
    impl crate::storage::SessionStoragePort for ReservationStorage {
        async fn list(
            &self,
            _session_id: &str,
            _prefix: Option<&str>,
            _cursor: Option<&str>,
            _limit: u32,
        ) -> Result<crate::storage::StoragePage> {
            Ok(crate::storage::StoragePage {
                objects: Vec::new(),
                next_cursor: None,
            })
        }

        async fn stat(&self, session_id: &str, key: &str) -> Result<crate::storage::StorageObject> {
            self.objects
                .lock()
                .expect("storage objects")
                .get(&format!("{session_id}\0{key}"))
                .cloned()
                .ok_or_else(|| BrainError::FileNotFound(key.into()))
        }

        async fn read(&self, _session_id: &str, key: &str, _max_bytes: u64) -> Result<Vec<u8>> {
            Err(BrainError::FileNotFound(key.into()))
        }

        async fn write(
            &self,
            request: crate::storage::StorageWriteRequest,
        ) -> Result<crate::storage::StorageObject> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            if self
                .fail_next_write_before_effect
                .swap(false, Ordering::Relaxed)
            {
                return Err(BrainError::Journal(
                    "simulated crash before inline publication".into(),
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&request.content_base64)
                .map_err(|_| BrainError::Invalid("test content is not base64".into()))?;
            let now = crate::wall_ms();
            let map_key = format!("{}\0{}", request.session_id, request.key);
            let created_at_ms = self
                .objects
                .lock()
                .expect("storage objects")
                .get(&map_key)
                .map_or(now, |object| object.created_at_ms);
            let object = crate::storage::StorageObject {
                key: request.key,
                bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                content_type: request.content_type,
                publication_id: Some(request.publication_id),
                created_at_ms,
                updated_at_ms: now,
            };
            self.objects
                .lock()
                .expect("storage objects")
                .insert(map_key, object.clone());
            Ok(object)
        }

        async fn prepare_download(
            &self,
            session_id: &str,
            key: &str,
        ) -> Result<crate::storage::StorageTransferTicket> {
            let object = self.stat(session_id, key).await?;
            Ok(crate::storage::StorageTransferTicket {
                object_id: crate::storage::stored_object_id(&object.sha256),
                transfer_id: crate::mint_id("xfer", 24),
                method: "GET".into(),
                url: "https://storage.invalid/download".into(),
                headers: HashMap::new(),
                expires_at_ms: crate::wall_ms() + 60 * 60 * 1_000,
                max_bytes: object.bytes,
            })
        }

        async fn prepare_upload(
            &self,
            request: crate::storage::StorageUploadRequest,
        ) -> Result<crate::storage::StorageTransferTicket> {
            self.prepares.fetch_add(1, Ordering::Relaxed);
            let head = self.journal.get_head(&request.session_id).await?;
            let durable = head.doc.storage_reserved_bytes == request.bytes
                && head.doc.storage_upload.as_ref().is_some_and(|upload| {
                    upload.transfer_id == request.transfer_id && upload.state == "reserved"
                });
            self.saw_durable_reservation
                .store(durable, Ordering::Relaxed);
            self.pending
                .lock()
                .expect("pending uploads")
                .insert(request.transfer_id.clone(), request.clone());
            Ok(crate::storage::StorageTransferTicket {
                object_id: crate::storage::pending_object_id(&request.transfer_id),
                transfer_id: request.transfer_id,
                method: "PUT".into(),
                url: "https://storage.invalid/upload".into(),
                headers: HashMap::new(),
                expires_at_ms: request.expires_at_ms,
                max_bytes: request.bytes,
            })
        }

        async fn complete_upload(
            &self,
            session_id: &str,
            transfer_id: &str,
        ) -> Result<crate::storage::StorageObject> {
            if !self
                .staged
                .lock()
                .expect("staged uploads")
                .contains(transfer_id)
            {
                return Err(BrainError::FileNotFound(transfer_id.into()));
            }
            let request = self
                .pending
                .lock()
                .expect("pending uploads")
                .get(transfer_id)
                .cloned()
                .ok_or_else(|| BrainError::FileNotFound(transfer_id.into()))?;
            if request.session_id != session_id {
                return Err(BrainError::FileNotFound(transfer_id.into()));
            }
            let now = crate::wall_ms();
            let object = crate::storage::StorageObject {
                key: request.key.clone(),
                bytes: request.bytes,
                sha256: request.sha256.unwrap_or_else(|| "d".repeat(64)),
                content_type: request.content_type,
                publication_id: Some(request.transfer_id),
                created_at_ms: now,
                updated_at_ms: now,
            };
            self.objects
                .lock()
                .expect("storage objects")
                .insert(format!("{session_id}\0{}", request.key), object.clone());
            Ok(object)
        }

        async fn abort_upload(&self, _session_id: &str, transfer_id: &str) -> Result<()> {
            self.aborts.fetch_add(1, Ordering::Relaxed);
            if self.fail_next_abort.swap(false, Ordering::Relaxed) {
                return Err(BrainError::Journal("transient staging deletion".into()));
            }
            self.pending
                .lock()
                .expect("pending uploads")
                .remove(transfer_id);
            self.staged
                .lock()
                .expect("staged uploads")
                .remove(transfer_id);
            Ok(())
        }

        async fn delete(&self, session_id: &str, key: &str) -> Result<()> {
            self.objects
                .lock()
                .expect("storage objects")
                .remove(&format!("{session_id}\0{key}"));
            Ok(())
        }

        async fn purge_session_page(
            &self,
            _session_id: &str,
            _cursor: Option<&str>,
        ) -> Result<crate::storage::StoragePurgePage> {
            Ok(crate::storage::StoragePurgePage {
                deleted_versions: 0,
                deleted_markers: 0,
                next_cursor: None,
            })
        }
    }

    #[test]
    fn direct_sandbox_transfer_admission_is_count_and_byte_bounded() {
        let brain = Brain::with_parts(
            BrainConfig::default(),
            Journal::new_memory("brain-direct-transfer-admission"),
            Arc::new(crate::keys::PlainCustody),
            None,
        );
        let entry = |session_id: &str, id: &str, bytes: u64| DirectSandboxTransfer {
            session_id: session_id.into(),
            storage_key: direct_sandbox_transfer_key(id),
            declared_bytes: bytes,
            expires_at_ms: crate::wall_ms() + 60_000,
            cleanup_at_ms: crate::wall_ms() + 120_000,
            storage_transfer_id: None,
            destination: None,
            state: DirectSandboxTransferState::Preparing,
        };
        for index in 0..MAX_PENDING_SANDBOX_TRANSFERS_PER_SESSION {
            let id = format!("sbxfer_count_{index}");
            brain
                .reserve_direct_sandbox_transfer(&id, entry("ses_count", &id, 1))
                .expect("exact per-session count is admitted");
        }
        let over = "sbxfer_count_over";
        assert!(matches!(
            brain.reserve_direct_sandbox_transfer(over, entry("ses_count", over, 1)),
            Err(BrainError::Overloaded)
        ));

        let bytes_brain = Brain::with_parts(
            BrainConfig::default(),
            Journal::new_memory("brain-direct-transfer-bytes"),
            Arc::new(crate::keys::PlainCustody),
            None,
        );
        bytes_brain
            .reserve_direct_sandbox_transfer(
                "sbxfer_bytes_exact",
                entry(
                    "ses_bytes",
                    "sbxfer_bytes_exact",
                    MAX_PENDING_SANDBOX_TRANSFER_BYTES_PER_SESSION,
                ),
            )
            .expect("exact per-session bytes are admitted");
        assert!(matches!(
            bytes_brain.reserve_direct_sandbox_transfer(
                "sbxfer_bytes_over",
                entry("ses_bytes", "sbxfer_bytes_over", 1),
            ),
            Err(BrainError::Overloaded)
        ));
    }

    #[tokio::test]
    async fn direct_sandbox_transfers_stage_hidden_bytes_and_replay_only_exact_success() {
        let journal = Journal::new_memory("brain-direct-sandbox-transfers");
        let storage = Arc::new(ReservationStorage::new(journal.clone()));
        let files = Arc::new(DirectTransferFiles {
            storage: storage.clone(),
            imports: AtomicUsize::new(0),
            exports: AtomicUsize::new(0),
        });
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                storage_transfer_ttl: Duration::from_secs(60 * 60),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage.clone()),
                session_preparation: Some(Arc::new(DirectTransferPreparation)),
                sandbox_files: Some(files.clone()),
                ..BrainServices::default()
            },
            None,
        );
        let session = brain
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic", "name":"direct-transfer", "api_key":"key"}
                })),
                Some("direct-sandbox-transfer"),
            )
            .await
            .expect("create direct-transfer session");
        let session_id = session.id.to_string();
        let status = brain
            .materialize_default_sandbox(&session_id)
            .await
            .expect("materialize default sandbox");
        let generation = status
            .generation
            .map(String::from)
            .expect("materialized generation");

        let download = brain
            .sandbox_file_prepare_download(
                &session_id,
                generation.clone(),
                "/workspace/source.bin".into(),
            )
            .await
            .expect("prepare sandbox download");
        assert!(download.transfer_id.starts_with("sbxfer_"));
        assert_eq!(download.method, "GET");
        assert_eq!(files.exports.load(Ordering::Relaxed), 1);
        let download_key = brain
            .direct_sandbox_transfers
            .lock()
            .expect("direct transfers")
            .get(&download.transfer_id)
            .expect("retained download")
            .storage_key
            .clone();
        assert!(crate::storage::is_internal_storage_key(&download_key));
        assert!(matches!(
            brain.storage_stat(&session_id, &download_key).await,
            Err(BrainError::Invalid(_))
        ));

        let upload = brain
            .sandbox_file_prepare_upload(
                &session_id,
                generation,
                "/workspace/upload.bin".into(),
                2 * 1024 * 1024,
                "e".repeat(64),
                true,
            )
            .await
            .expect("prepare sandbox upload");
        let (underlying, upload_key) = {
            let transfers = brain
                .direct_sandbox_transfers
                .lock()
                .expect("direct transfers");
            let transfer = transfers.get(&upload.transfer_id).expect("retained upload");
            (
                transfer
                    .storage_transfer_id
                    .clone()
                    .expect("underlying storage transfer"),
                transfer.storage_key.clone(),
            )
        };
        assert_ne!(upload.transfer_id, underlying);
        storage
            .staged
            .lock()
            .expect("staged uploads")
            .insert(underlying);
        let completed = brain
            .sandbox_file_complete_upload(&session_id, &upload.transfer_id)
            .await
            .expect("complete sandbox upload");
        assert_eq!(
            String::from(completed.path.clone()),
            "/workspace/upload.bin"
        );
        let replayed = brain
            .sandbox_file_complete_upload(&session_id, &upload.transfer_id)
            .await
            .expect("replay exact completed outcome");
        assert_eq!(String::from(replayed.path), "/workspace/upload.bin");
        assert_eq!(files.imports.load(Ordering::Relaxed), 1);
        assert!(
            storage
                .objects
                .lock()
                .expect("storage objects")
                .get(&format!("{session_id}\0{upload_key}"))
                .is_none(),
            "successful import best-effort purges hidden staging"
        );

        let restarted = Brain::with_parts_and_services(
            BrainConfig::default(),
            journal,
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage),
                sandbox_files: Some(files),
                ..BrainServices::default()
            },
            None,
        );
        assert!(matches!(
            restarted
                .sandbox_file_complete_upload(&session_id, &upload.transfer_id)
                .await,
            Err(BrainError::SandboxTransferUnknown(_))
        ));
    }

    #[async_trait::async_trait]
    impl ToolExecutor for RecoveryExecutor {
        fn supports(&self, capability: &str) -> bool {
            capability == "aex.submit"
        }

        async fn call(
            &self,
            capability: &str,
            request: ExternalToolCallRequest,
            _cancel: CancellationToken,
        ) -> Result<ExternalToolCallResponse> {
            assert_eq!(capability, "aex.submit");
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.call_ids
                .lock()
                .expect("recovery call ids")
                .push(request.call_id.to_string());
            Ok(serde_json::from_value(json!({
                "outcome": "completed",
                "content": "accepted after recovery",
                "is_error": false,
                "disposition": "complete_turn",
                "result": request.input,
                "result_metadata": {"recovered": "true"}
            }))
            .expect("valid recovery response"))
        }
    }

    #[test]
    fn base_urls_resolve_and_compatible_requires_one() {
        assert_eq!(
            resolve_base_url(&ApiProvider::Anthropic, None).unwrap(),
            "https://api.anthropic.com"
        );
        assert_eq!(
            resolve_base_url(&ApiProvider::Deepseek, None).unwrap(),
            "https://api.deepseek.com"
        );
        assert!(resolve_base_url(&ApiProvider::OpenaiCompatible, None).is_err());
        assert!(resolve_base_url(&ApiProvider::Openai, Some("http://insecure")).is_err());
        assert_eq!(
            resolve_base_url(&ApiProvider::Openai, Some("https://proxy.example/")).unwrap(),
            "https://proxy.example"
        );
    }

    #[test]
    fn child_fork_excludes_spawning_tool_use_and_partial_sibling_results() {
        let prompt = Message::user_text("delegate the focused task");
        let spawning = Message::assistant(vec![
            ContentBlock::ToolUse {
                id: "op_spawn".into(),
                name: "subagents".into(),
                input: json!({"action":"spawn","prompt":"work"}),
            },
            ContentBlock::ToolUse {
                id: "op_sibling".into(),
                name: "subagents".into(),
                input: json!({"action":"spawn","prompt":"other"}),
            },
        ]);
        let partial = Message::tool_results(vec![ContentBlock::ToolResult {
            tool_use_id: "op_sibling".into(),
            content: "created sibling".into(),
            is_error: false,
        }]);
        let history = vec![prompt.clone(), spawning.clone(), partial];
        assert_eq!(
            complete_fork_projection(&history),
            std::slice::from_ref(&prompt)
        );

        let complete = Message::tool_results(vec![
            ContentBlock::ToolResult {
                tool_use_id: "op_spawn".into(),
                content: "created".into(),
                is_error: false,
            },
            ContentBlock::ToolResult {
                tool_use_id: "op_sibling".into(),
                content: "created sibling".into(),
                is_error: false,
            },
        ]);
        let closed = vec![prompt, spawning, complete];
        assert_eq!(complete_fork_projection(&closed), closed.as_slice());
    }

    #[test]
    fn child_fork_modes_select_only_complete_recent_turns() {
        let history = vec![
            Message::user_text("one"),
            Message::assistant(vec![ContentBlock::text("answer one")]),
            Message::user_text("two"),
            Message::assistant(vec![ContentBlock::text("answer two")]),
            Message::user_text("three"),
        ];
        let (all, all_turns) = select_fork_history(&history, &ForkTurns::All);
        assert_eq!(all, history);
        assert_eq!(all_turns, 3);
        let (last_two, turns) = select_fork_history(&history, &ForkTurns::Last(2));
        assert_eq!(turns, 2);
        assert_eq!(last_two.first(), Some(&Message::user_text("two")));
        assert_eq!(ForkTurns::parse(None).unwrap(), ForkTurns::All);
        assert!(ForkTurns::parse(Some("0")).is_err());
    }

    #[tokio::test]
    async fn descendants_share_one_root_scoped_custody_decryption_cell() {
        let custody = Arc::new(CountingCustody::default());
        let brain = Brain::with_parts(
            BrainConfig::default(),
            Journal::new_memory("brain-root-secret-cache"),
            custody.clone(),
            None,
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider":"anthropic",
                        "name":"model",
                        "api_key":"root-provider-secret"
                    }
                })),
                Some("root-secret-cache"),
            )
            .await
            .unwrap();
        assert_eq!(custody.encrypts.load(Ordering::Relaxed), 1);
        assert_eq!(custody.decrypts.load(Ordering::Relaxed), 0);

        let root = brain
            .journal
            .get_head(&created.id.to_string())
            .await
            .unwrap();
        let mut child = root.doc.clone();
        child.parent_id = Some(root.session_id.clone());
        child.ancestor_ids = vec![root.session_id.clone()];
        child.depth = 1;
        let (root_cell, root_secrets) = brain.root_execution_secrets(&root.doc).await.unwrap();
        let (child_cell, child_secrets) = brain.root_execution_secrets(&child).await.unwrap();
        assert!(Arc::ptr_eq(&root_cell, &child_cell));
        assert_eq!(root_secrets.key.expose(), "root-provider-secret");
        assert_eq!(child_secrets.key.expose(), "root-provider-secret");
        assert_eq!(custody.decrypts.load(Ordering::Relaxed), 1);

        drop(root_secrets);
        drop(child_secrets);
        drop(root_cell);
        drop(child_cell);
        let (_new_cell, _new_secrets) = brain.root_execution_secrets(&root.doc).await.unwrap();
        assert_eq!(
            custody.decrypts.load(Ordering::Relaxed),
            2,
            "the weak cache must release secret material after the last residency"
        );
    }

    #[tokio::test]
    async fn child_create_atomically_admits_prompt_and_rebuilds_exact_parent_fork() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-child-fork-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        let journal = Journal::new_memory("brain-child-fork");
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([
            Scripted::Text("parent answer".into()),
            Scripted::Text("child answer".into()),
        ]);
        let provider = fake.clone();
        let provider_factory: ProviderFactory = Arc::new(move |_| provider.clone());
        let brain = Brain::with_parts(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Some(provider_factory),
        );
        let root = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider":"anthropic", "name":"child-fork-test", "api_key":"key"
                    }
                })),
                Some("child-fork-root"),
            )
            .await
            .unwrap();
        let root_id = root.id.to_string();
        brain
            .message(
                &root_id,
                MessageRequestContent::String("parent prompt".parse().unwrap()),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if journal.get_head(&root_id).await.unwrap().doc.turn.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("parent turn completes");

        let child = brain
            .create_child(
                &root_id,
                "child prompt".into(),
                Some("focused".into()),
                None,
                Some("spawn-1"),
            )
            .await
            .unwrap();
        let child_id = child.id.to_string();
        assert_eq!(child.name.as_deref().map(String::as_str), Some("focused"));
        assert_eq!(
            child.context_fork.as_ref().map(|fork| fork.mode),
            Some(session::ContextForkMode::All)
        );
        let initial = journal.read_records_through(&child_id, 0, 1).await.unwrap();
        assert!(matches!(
            &initial[..],
            [Entry {
                record: Record::UserMessage {
                    starts_turn: true,
                    content,
                    ..
                },
                ..
            }] if content == &vec![ContentBlock::text("child prompt")]
        ));
        let listed = journal
            .list_child_page(&crate::journal::ChildListQuery {
                parent_id: &root_id,
                limit: 10,
                cursor: None,
            })
            .await
            .unwrap();
        assert_eq!(listed.sessions.len(), 1);
        assert_eq!(listed.sessions[0].session_id, child_id);

        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if journal
                    .get_head(&child_id)
                    .await
                    .unwrap()
                    .doc
                    .turn
                    .is_none()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("child initial turn completes");
        let child_head = journal.get_head(&child_id).await.unwrap();
        let child_entries = journal.read_records(&child_id, 0).await.unwrap();
        let history = materialize_session_history(&brain, &child_head.doc, &child_entries)
            .await
            .unwrap();
        assert_eq!(history[0], Message::user_text("parent prompt"));
        assert_eq!(
            history[1],
            Message::assistant(vec![ContentBlock::text("parent answer")])
        );
        assert_eq!(history[2], Message::user_text("child prompt"));
        assert_eq!(
            history[3],
            Message::assistant(vec![ContentBlock::text("child answer")])
        );

        let replay = brain
            .create_child(
                &root_id,
                "child prompt".into(),
                Some("focused".into()),
                None,
                Some("spawn-1"),
            )
            .await
            .unwrap();
        assert_eq!(replay.id, child.id);
        assert!(matches!(
            brain
                .create_child(
                    &root_id,
                    "different prompt".into(),
                    Some("focused".into()),
                    None,
                    Some("spawn-1"),
                )
                .await,
            Err(BrainError::IdempotencyConflict)
        ));
        fake.assert_drained(2, "ordinary child fork").unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn prefix_rebuild_is_deterministic() {
        let p = PrefixDoc {
            agentloop: None,
            system_prompt: Some("sp".into()),
            provider: "anthropic".into(),
            model: "claude-x".into(),
            base_url: Some("https://api.anthropic.com".into()),
            max_output_tokens: Some(2048),
            context_window_tokens: 32 * 1024,
            context_soft_tokens: 18 * 1024,
            context_hard_tokens: 22 * 1024,
            context_tail_tokens: 4 * 1024,
            context_summary_tokens: 4 * 1024,
            temperature: Some(0.5),
            reasoning_effort: None,
            provider_recovery_retries: 1,
            storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
            storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
            storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
            max_child_depth: 4,
            max_direct_children: 32,
            max_descendants: 256,
            max_additional_sandboxes_per_root: 2,
            network: serde_json::json!({"outbound":"none"}),
            customer_client_id: None,
            customer_submit_retries: 1,
            rendered_base: serde_json::Value::Null,
            rendered_base_digest: String::new(),
            prompt_cache_key: String::new(),
            tools: serde_json::from_value(json!([
                {
                    "definition": {
                        "name":"run", "description":"run",
                        "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "input_schema":{"type":"object"},
                        "output_schema":{"type":"object"}
                    },
                    "executor": {
                        "kind":"aex_managed",
                        "bundle_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "required_env":[]
                    }
                },
                {
                    "definition": {
                        "name":"delegate", "description":"delegate",
                        "contract_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "input_schema":{"type":"object"},
                        "output_schema":{"type":"string"}
                    },
                    "executor":{"kind":"engine", "capability":"brain.subagents"}
                }
            ])).unwrap(),
            managed_bundles: vec![],
            official_capabilities: HashMap::new(),
            hand_enabled: true,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        };
        let (a, da) = build_prefix(&p, 512).unwrap();
        let (b, db) = build_prefix(&p, 512).unwrap();
        assert_eq!(a.digest(), b.digest());
        assert_eq!(da, db);
        assert_eq!(a.tools.len(), 2);

        let mut openai = p.clone();
        openai.provider = "openai".into();
        openai.model = "gpt-5.4".into();
        openai.reasoning_effort = Some("high".into());
        let (openai, _) = build_prefix(&openai, 512).unwrap();
        let rendered = crate::provider::openai::OpenAiChat::render_base(&openai);
        assert_eq!(rendered["max_completion_tokens"], 2_048);
        assert_eq!(rendered["reasoning_effort"], "high");
        assert!(!rendered.contains_key("max_tokens"));

        let mut unsupported = p;
        unsupported.reasoning_effort = Some("high".into());
        let error = build_prefix(&unsupported, 512).unwrap_err();
        assert!(matches!(error, BrainError::Invalid(_)));
        assert!(error.to_string().contains("reasoning_effort"));
    }

    #[test]
    fn dialects_route_by_provider() {
        assert_eq!(dialect_of("anthropic"), Dialect::AnthropicMessages);
        assert_eq!(dialect_of("deepseek"), Dialect::OpenAiChat);
        assert_eq!(dialect_of("openai"), Dialect::OpenAiChat);
    }

    #[test]
    fn pending_volatile_scan_routes_by_the_seal() {
        let task = |seq: u64, agent: &str, call: &str, detach: bool| Entry {
            seq,
            ts_ms: 0,
            record: Record::ToolCall {
                turn: "trn_test".into(),
                agent: agent.into(),
                call: call.into(),
                name: "delegate_under_any_name".into(),
                input: serde_json::json!({}),
                detach,
            },
        };
        let entries = vec![
            task(1, "root", "op_pending", false),
            task(2, "agt_child", "op_answered", false),
            Entry {
                seq: 3,
                ts_ms: 0,
                record: Record::ToolResult {
                    turn: "trn_test".into(),
                    agent: "agt_child".into(),
                    call: "op_answered".into(),
                    name: "delegate_under_any_name".into(),
                    outcome: "completed".into(),
                    content: "done".into(),
                    is_error: false,
                    exit_code: None,
                    duration_ms: 1,
                    truncated: false,
                },
            },
            task(4, "root", "op_detached", true),
            Entry {
                seq: 5,
                ts_ms: 0,
                record: Record::CustomerCallIntent {
                    turn: "trn_test".into(),
                    call: "op_customer".into(),
                    client_id: "app".into(),
                    process_id: "process:test".into(),
                    request_digest: "b".repeat(64),
                    deadline_at_ms: 9_999_999,
                },
            },
            Entry {
                seq: 6,
                ts_ms: 0,
                record: Record::ToolCall {
                    turn: "trn_test".into(),
                    agent: "root".into(),
                    call: "op_customer".into(),
                    name: "customer_lookup".into(),
                    input: serde_json::json!({"id":7}),
                    detach: false,
                },
            },
        ];
        let prefix = PrefixDoc {
            agentloop: None,
            system_prompt: None,
            provider: "anthropic".into(),
            model: "m".into(),
            base_url: None,
            max_output_tokens: None,
            context_window_tokens: 32 * 1024,
            context_soft_tokens: 18 * 1024,
            context_hard_tokens: 22 * 1024,
            context_tail_tokens: 4 * 1024,
            context_summary_tokens: 4 * 1024,
            temperature: None,
            reasoning_effort: None,
            provider_recovery_retries: 1,
            storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
            storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
            storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
            max_child_depth: 4,
            max_direct_children: 32,
            max_descendants: 256,
            max_additional_sandboxes_per_root: 2,
            network: serde_json::json!({"outbound":"none"}),
            customer_client_id: Some("app".into()),
            customer_submit_retries: 1,
            rendered_base: serde_json::json!({}),
            rendered_base_digest: String::new(),
            prompt_cache_key: String::new(),
            tools: serde_json::from_value(json!([
                {
                    "definition": {
                        "name":"delegate_under_any_name", "description":"delegate",
                        "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "input_schema":{"type":"object"}, "output_schema":{"type":"string"}
                    },
                    "executor":{"kind":"engine", "capability":"brain.subagents"}
                },
                {
                    "definition": {
                        "name":"customer_lookup", "description":"lookup",
                        "contract_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                        "input_schema":{"type":"object"}, "output_schema":{"type":"object"}
                    },
                    "executor":{"kind":"customer_app", "registration":"lookup"}
                }
            ]))
            .unwrap(),
            managed_bundles: vec![],
            official_capabilities: HashMap::new(),
            hand_enabled: false,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        };
        let pending = pending_volatile(&entries, &prefix);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].call, "op_pending");
        let customer = pending_customer(&entries, &prefix, "tenant", "ses_test");
        assert_eq!(customer.len(), 1);
        assert_eq!(customer[0].call, "op_customer");
        assert_eq!(customer[0].intent.process_id, "process:test");
    }

    #[test]
    fn pending_external_scan_recovers_only_unanswered_sealed_calls() {
        let prefix = PrefixDoc {
            agentloop: None,
            system_prompt: None,
            provider: "anthropic".into(),
            model: "m".into(),
            base_url: None,
            max_output_tokens: None,
            context_window_tokens: 32 * 1024,
            context_soft_tokens: 18 * 1024,
            context_hard_tokens: 22 * 1024,
            context_tail_tokens: 4 * 1024,
            context_summary_tokens: 4 * 1024,
            temperature: None,
            reasoning_effort: None,
            provider_recovery_retries: 1,
            storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
            storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
            storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
            max_child_depth: 4,
            max_direct_children: 32,
            max_descendants: 256,
            max_additional_sandboxes_per_root: 2,
            network: serde_json::json!({"outbound":"none"}),
            customer_client_id: None,
            customer_submit_retries: 1,
            rendered_base: serde_json::json!({}),
            rendered_base_digest: String::new(),
            prompt_cache_key: String::new(),
            tools: serde_json::from_value(json!([{
                "definition": {
                    "name":"submit", "description":"submit",
                    "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "input_schema":{"type":"object"},
                    "output_schema":{"type":"object"}
                },
                "executor": {"kind":"engine", "capability":"aex.submit"}
            }]))
            .unwrap(),
            managed_bundles: vec![],
            official_capabilities: HashMap::from([("aex.submit".into(), submit_policy())]),
            hand_enabled: false,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        };
        let mut context = HashMap::new();
        context.insert("request".into(), "out_1".into());
        let mut entries = vec![
            Entry {
                seq: 1,
                ts_ms: 0,
                record: Record::UserMessage {
                    turn: "trn_test".into(),
                    content: vec![ContentBlock::text("answer")],
                    starts_turn: false,
                    metadata: context,
                    idempotency_key_hash: None,
                    request_hash: None,
                },
            },
            Entry {
                seq: 2,
                ts_ms: 0,
                record: Record::Assistant {
                    turn: "trn_test".into(),
                    agent: "root".into(),
                    attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                    content: vec![],
                    stop: crate::message::StopReason::ToolUse,
                },
            },
            Entry {
                seq: 3,
                ts_ms: 0,
                record: Record::ToolCall {
                    turn: "trn_test".into(),
                    agent: "root".into(),
                    call: "op_submit".into(),
                    name: "submit".into(),
                    input: json!({"answer": 42}),
                    detach: false,
                },
            },
        ];
        let pending = pending_external(&entries, &prefix);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].call, "op_submit");
        assert_eq!(pending[0].context["request"], "out_1");
        assert!(!pending[0].parallel_batch);

        entries.push(Entry {
            seq: 4,
            ts_ms: 0,
            record: Record::ToolResult {
                turn: "trn_test".into(),
                agent: "root".into(),
                call: "op_submit".into(),
                name: "submit".into(),
                outcome: "completed".into(),
                content: "done".into(),
                is_error: false,
                exit_code: None,
                duration_ms: 1,
                truncated: false,
            },
        });
        assert!(pending_external(&entries, &prefix).is_empty());
    }

    #[tokio::test]
    async fn hydrate_replays_a_pending_replay_safe_external_call_with_the_same_id() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-external-recovery-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create recovery data dir");
        let journal = Journal::new_memory("brain-recovery-test");
        let executor = Arc::new(RecoveryExecutor::default());
        let brain = Brain::with_parts_and_external(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                official_capabilities: HashMap::from([("aex.submit".into(), submit_policy())]),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            executor.clone(),
            None,
        );
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "unused-during-terminal-recovery",
                        "api_key": "sk-test"
                    },
                    "tools": {
                        "items": [{
                            "definition": {
                                "name": "submit",
                                "description": "Submit the final value",
                                "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "input_schema": {"type": "object"},
                                "output_schema": {"type": "object"}
                            },
                            "executor": {"kind": "engine", "capability": "aex.submit"}
                        }]
                    }
                }))
                .expect("valid create request"),
                None,
            )
            .await
            .expect("create session");
        let session_id = created.id.to_string();

        // Let the eager actor finish its initial hand-state decision before fencing its fold.
        for _ in 0..100 {
            if journal
                .get_head(&session_id)
                .await
                .expect("head while waiting")
                .last_seq
                >= 2
            {
                break;
            }
            tokio::task::yield_now().await;
        }

        let mut head = journal
            .claim(&session_id)
            .await
            .expect("claim for crash setup");
        let turn = "trn_aaaaaaaaaaaaaaaaaaaa".to_string();
        let call = "op_aaaaaaaaaaaaaaaa".to_string();
        let input = json!({"answer": 42});
        head.doc.state = "open".into();
        head.doc.turn = Some(turn.clone());
        head.doc.turns += 1;
        head.doc.last_message_ms = Some(crate::wall_ms());
        let first_seq = head.last_seq + 1;
        let records = vec![
            (
                first_seq,
                Record::UserMessage {
                    turn: turn.clone(),
                    content: vec![ContentBlock::text("return a typed value")],
                    starts_turn: false,
                    metadata: HashMap::from([("request_id".into(), "out_test".into())]),
                    idempotency_key_hash: None,
                    request_hash: None,
                },
            ),
            (first_seq + 1, Record::TurnStarted { turn: turn.clone() }),
            (
                first_seq + 2,
                Record::Assistant {
                    turn: turn.clone(),
                    agent: "root".into(),
                    attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                    content: vec![ContentBlock::ToolUse {
                        id: call.clone(),
                        name: "submit".into(),
                        input: input.clone(),
                    }],
                    stop: crate::message::StopReason::ToolUse,
                },
            ),
            (
                first_seq + 3,
                Record::ToolCall {
                    turn: turn.clone(),
                    agent: "root".into(),
                    call: call.clone(),
                    name: "submit".into(),
                    input: input.clone(),
                    detach: false,
                },
            ),
        ];
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        journal
            .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 3)
            .await
            .expect("commit pending external intent");
        let crash_entries = journal
            .read_records(&session_id, 0)
            .await
            .expect("read simulated crash records");
        let resolved = resolve_sealed_tools(&head.doc.prefix);
        assert!(
            resolved.iter().any(|tool| {
                tool.name == "submit"
                    && matches!(
                        &tool.route,
                        crate::config::ToolRoute::Server(policy) if policy.capability == "aex.submit"
                    )
            }),
            "resolved tools: {resolved:?}; prefix tools: {:?}",
            head.doc.prefix.tools
        );
        assert!(crash_entries.iter().any(|entry| matches!(
            &entry.record,
            Record::ToolCall { call: recorded, .. } if recorded == &call
        )));
        let pending = pending_external(&crash_entries, &head.doc.prefix);
        assert_eq!(
            pending.len(),
            1,
            "the committed server-tool intent is pending"
        );
        assert_eq!(pending[0].policy.capability, "aex.submit");
        // Model the durable failure transition that follows an observed owner loss. It releases
        // the stale lease and installs the bounded retry due-time atomically, so the background
        // scheduler can resume without customer traffic and without waiting another lease term.
        journal
            .defer_recovery(&session_id)
            .await
            .expect("release crashed owner and schedule recovery");

        let resident = hydrate(&brain, &session_id)
            .await
            .expect("hydrate and replay pending call");
        assert_eq!(executor.calls.load(Ordering::Relaxed), 1);
        assert_eq!(resident.st.head.state, "open");
        assert!(resident.st.head.turn.is_none());
        assert_eq!(
            executor
                .call_ids
                .lock()
                .expect("recovery call ids")
                .as_slice(),
            [call.as_str()]
        );

        let recovered = journal
            .read_records(&session_id, first_seq + 3)
            .await
            .expect("recovery records");
        assert!(recovered.iter().any(|entry| matches!(
            &entry.record,
            Record::ToolResult { call: recovered_call, .. } if recovered_call == &call
        )));
        let result = recovered.iter().find_map(|entry| match &entry.record {
            Record::TurnCompleted {
                result: Some(result),
                ..
            } => Some(result),
            _ => None,
        });
        let result = result.expect("terminal result committed during hydrate");
        assert_eq!(result.call_id.to_string(), call);
        assert_eq!(result.value, input);
        assert_eq!(result.metadata.get("recovered"), Some(&"true".into()));

        journal
            .release(&session_id, &resident.st.lease)
            .await
            .expect("release recovered lease");
        drop(resident);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn customer_terminal_before_brain_crash_replays_without_reexecuting_the_effect() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-customer-recovery-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).unwrap();
        let journal = Journal::new_memory("brain-customer-crashed");
        let transport = crate::customer::CustomerTransportConfig::new(
            "ws://127.0.0.1:3210/v1/customer-hand/socket",
            "http://127.0.0.1:3210",
        )
        .unwrap();
        let crashed = Brain::with_parts_and_services(
            BrainConfig {
                external_call_timeout: Duration::from_secs(2),
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                customer_transport: Some(transport.clone()),
                ..BrainServices::default()
            },
            None,
        );
        let created = crashed
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic","name":"customer-recovery","api_key":"sk-test"},
                    "client":{"id":"app","submit_retries":1},
                    "tools":{"items":[{
                        "definition":{
                            "name":"lookup", "description":"lookup",
                            "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "input_schema":{"type":"object"}, "output_schema":{"type":"object"}
                        },
                        "executor":{"kind":"customer_app","registration":"lookup"}
                    }]}
                })),
                None,
            )
            .await
            .unwrap();
        let session_id = created.id.to_string();
        let crashed_customer = crashed.customer.as_ref().unwrap().clone();
        let (crashed_grant, mut crashed_socket, crashed_epoch) =
            connect_customer_process(&crashed_customer, "process:stable").await;

        let mut head = journal.claim(&session_id).await.unwrap();
        let turn = "trn_customercrash0000000".to_owned();
        let call = "op_customercrash".to_owned();
        let input = json!({"id":7});
        let intent = crashed_customer
            .prepare_operation(
                "local",
                "app",
                &session_id,
                &call,
                "lookup",
                "lookup",
                &"a".repeat(64),
                input.clone(),
                crate::wall_ms() + 5_000,
            )
            .await
            .unwrap();
        head.doc.state = "open".into();
        head.doc.turn = Some(turn.clone());
        head.doc.active_phase = Some("ready_to_dispatch_tools".into());
        head.doc.active_rounds = 1;
        head.doc.active_tool_calls = 1;
        head.doc.turns += 1;
        let first_seq = head.last_seq + 1;
        let records = vec![
            (
                first_seq,
                Record::UserMessage {
                    turn: turn.clone(),
                    content: vec![ContentBlock::text("look up id 7")],
                    starts_turn: false,
                    metadata: HashMap::new(),
                    idempotency_key_hash: None,
                    request_hash: None,
                },
            ),
            (first_seq + 1, Record::TurnStarted { turn: turn.clone() }),
            (
                first_seq + 2,
                Record::Assistant {
                    turn: turn.clone(),
                    agent: "root".into(),
                    attempt_id: "att_customercrash0000000".into(),
                    content: vec![ContentBlock::ToolUse {
                        id: call.clone(),
                        name: "lookup".into(),
                        input: input.clone(),
                    }],
                    stop: crate::message::StopReason::ToolUse,
                },
            ),
            (
                first_seq + 3,
                Record::CustomerCallIntent {
                    turn: turn.clone(),
                    call: call.clone(),
                    client_id: intent.client_id.clone(),
                    process_id: intent.process_id.clone(),
                    request_digest: intent.request_digest.clone(),
                    deadline_at_ms: intent.deadline_at_ms,
                },
            ),
            (
                first_seq + 4,
                Record::ToolCall {
                    turn: turn.clone(),
                    agent: "root".into(),
                    call: call.clone(),
                    name: "lookup".into(),
                    input,
                    detach: false,
                },
            ),
        ];
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        journal
            .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 4)
            .await
            .unwrap();

        // The application runs the effect once and publishes its terminal, but Brain crashes
        // before the ToolResult decision. The application process retains that exact fact.
        let first_execution = {
            let customer = crashed_customer.clone();
            let intent = intent.clone();
            tokio::spawn(async move {
                customer
                    .execute_prepared(intent, 0, CancellationToken::new())
                    .await
            })
        };
        let Some(crate::customer::CustomerCommand::Offer(first_offer)) =
            crashed_socket.recv().await
        else {
            panic!("first customer offer")
        };
        crashed_customer
            .observation(
                &crashed_grant.grant_id,
                &crashed_grant.observation_token,
                crate::customer::CustomerObservation::Receipt {
                    epoch: crashed_epoch,
                    operation_id: first_offer.operation_id.clone(),
                    request_digest: first_offer.request_digest.clone(),
                    replayed: false,
                },
            )
            .await
            .unwrap();
        crashed_customer
            .observation(
                &crashed_grant.grant_id,
                &crashed_grant.observation_token,
                crate::customer::CustomerObservation::Terminal {
                    epoch: crashed_epoch,
                    operation_id: first_offer.operation_id,
                    request_digest: first_offer.request_digest,
                    ok: true,
                    output: Some(json!({"value":7})),
                    error: None,
                },
            )
            .await
            .unwrap();
        let uncommitted = first_execution.await.unwrap();
        assert!(uncommitted.terminal_receipt.is_some());
        journal.release(&session_id, &lease).await.unwrap();
        drop(crashed);

        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::Text("done after replay".into())]);
        let provider = fake.clone();
        let recovering = Brain::with_parts_and_services(
            BrainConfig {
                external_call_timeout: Duration::from_secs(2),
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.cloned_as("brain-customer-recovering"),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                customer_transport: Some(transport),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| provider.clone())),
        );
        let recovering_customer = recovering.customer.as_ref().unwrap().clone();
        let (replay_grant, mut replay_socket, replay_epoch) =
            connect_customer_process(&recovering_customer, "process:stable").await;
        let hydration = {
            let recovering = recovering.clone();
            let session_id = session_id.clone();
            tokio::spawn(async move { hydrate(&recovering, &session_id).await })
        };
        let Some(crate::customer::CustomerCommand::Offer(replay_offer)) =
            replay_socket.recv().await
        else {
            panic!("replayed customer offer")
        };
        assert_eq!(replay_offer.operation_id, call);
        assert_eq!(replay_offer.request_digest, intent.request_digest);
        // Retained application terminal replay: no handler/effect runs a second time.
        recovering_customer
            .observation(
                &replay_grant.grant_id,
                &replay_grant.observation_token,
                crate::customer::CustomerObservation::Terminal {
                    epoch: replay_epoch,
                    operation_id: replay_offer.operation_id,
                    request_digest: replay_offer.request_digest,
                    ok: true,
                    output: Some(json!({"value":7})),
                    error: None,
                },
            )
            .await
            .unwrap();
        let Some(crate::customer::CustomerCommand::Ack {
            operation_id,
            request_digest,
            ..
        }) = replay_socket.recv().await
        else {
            panic!("post-commit customer ack")
        };
        assert_eq!(operation_id, call);
        assert_eq!(request_digest, intent.request_digest);
        let resident = tokio::time::timeout(Duration::from_secs(3), hydration)
            .await
            .expect("customer recovery completed")
            .unwrap()
            .unwrap();
        assert!(resident.st.head.pending_customer_acks.is_empty());
        assert!(resident.st.head.turn.is_none());
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
        let recovered = journal
            .read_records(&session_id, first_seq + 4)
            .await
            .unwrap();
        assert_eq!(
            recovered
                .iter()
                .filter(|entry| matches!(entry.record, Record::ToolResult { ref call, .. } if call == &operation_id))
                .count(),
            1
        );
        assert!(
            recovered
                .iter()
                .any(|entry| matches!(entry.record, Record::CustomerTerminalReceived { .. }))
        );
        assert!(
            recovered
                .iter()
                .any(|entry| matches!(entry.record, Record::CustomerTerminalAcknowledged { .. }))
        );
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn journaled_customer_terminal_is_reacked_after_process_restart() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-customer-ack-recovery-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).unwrap();
        let journal = Journal::new_memory("brain-customer-ack-crashed");
        let transport = crate::customer::CustomerTransportConfig::new(
            "ws://127.0.0.1:3210/v1/customer-hand/socket",
            "http://127.0.0.1:3210",
        )
        .unwrap();
        let crashed = Brain::with_parts_and_services(
            BrainConfig::default(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                customer_transport: Some(transport.clone()),
                ..BrainServices::default()
            },
            None,
        );
        let created = crashed
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic","name":"unused","api_key":"sk-test"},
                    "client":{"id":"app"}
                })),
                None,
            )
            .await
            .unwrap();
        let session_id = created.id.to_string();
        let mut head = journal.claim(&session_id).await.unwrap();
        let call = "op_ackrestart0000".to_owned();
        let request_digest = "a".repeat(64);
        let terminal_digest = "b".repeat(64);
        head.doc
            .pending_customer_acks
            .push(crate::journal::CustomerTerminalAckDoc {
                turn: "trn_ackrestart00000000".into(),
                call: call.clone(),
                client_id: "app".into(),
                process_id: "process:stable".into(),
                request_digest: request_digest.clone(),
                terminal_digest: terminal_digest.clone(),
            });
        let seq = head.last_seq + 1;
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        journal
            .commit(
                &session_id,
                &mut lease,
                &[(
                    seq,
                    Record::CustomerTerminalReceived {
                        turn: "trn_ackrestart00000000".into(),
                        call: call.clone(),
                        client_id: "app".into(),
                        process_id: "process:stable".into(),
                        request_digest: request_digest.clone(),
                        terminal_digest: terminal_digest.clone(),
                    },
                )],
                &head.doc,
                seq,
            )
            .await
            .unwrap();
        journal.release(&session_id, &lease).await.unwrap();
        drop(crashed);

        let recovering = Brain::with_parts_and_services(
            BrainConfig::default(),
            journal.cloned_as("brain-customer-ack-recovering"),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                customer_transport: Some(transport),
                ..BrainServices::default()
            },
            None,
        );
        let customer = recovering.customer.as_ref().unwrap().clone();
        let (_, mut socket, epoch) = connect_customer_process(&customer, "process:stable").await;
        let hydration = {
            let recovering = recovering.clone();
            let session_id = session_id.clone();
            tokio::spawn(async move { hydrate(&recovering, &session_id).await })
        };
        let Some(crate::customer::CustomerCommand::Ack {
            epoch: ack_epoch,
            operation_id,
            request_digest: ack_request,
            terminal_digest: ack_terminal,
        }) = socket.recv().await
        else {
            panic!("durable ack replay")
        };
        assert_eq!(ack_epoch, epoch);
        assert_eq!(operation_id, call);
        assert_eq!(ack_request, request_digest);
        assert_eq!(ack_terminal, terminal_digest);
        let resident = hydration.await.unwrap().unwrap();
        assert!(resident.st.head.pending_customer_acks.is_empty());
        let records = journal.read_records(&session_id, seq).await.unwrap();
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::CustomerTerminalAcknowledged { .. }))
        );
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn managed_submit_unknown_survives_cleanup_crash_without_resubmission() {
        let journal = Journal::new_memory("brain-managed-submit-unknown");
        let ports = Arc::new(UnknownManagedPorts::default());
        ports.fail_next_dematerialize.store(true, Ordering::Release);
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::Text("continued after an honest unknown".into())]);
        let provider = fake.clone();
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                hand: Some(ports.clone()),
                session_preparation: Some(ports.clone()),
                sandbox_files: Some(ports.clone()),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| provider.clone())),
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic","name":"managed-unknown","api_key":"key"}
                })),
                Some("managed-submit-unknown"),
            )
            .await
            .expect("create unknown-submit recovery session");
        let session_id = created.id.to_string();
        let mut resident = hydrate(&brain, &session_id)
            .await
            .expect("claim initial crash fold");

        let turn = "trn_managedunknown000000".to_owned();
        let call = "op_managedunknown0000".to_owned();
        let name = "managed_unknown_test".to_owned();
        let binding_ref = "bnd_managedunknown0000";
        let input = json!({"effect":"already_may_have_run"});
        let mut envelope: brain_protocol::hand::OperationEnvelope = serde_json::from_value(json!({
            "operation_id":call,
            "request_digest":"0".repeat(64),
            "session_id":session_id,
            "root_id":resident.st.head.root_id,
            "turn_id":turn,
            "caller_id":"agent_root",
            "fence":resident.st.lease.fence,
            "generation":null,
            "binding_ref":binding_ref,
            "capability":name,
            "input":{"kind":"inline","value":input},
            "target_ref":null,
            "deadline_at_ms":crate::wall_ms() + 60_000,
            "resources":managed_hand_resources().unwrap(),
            "network":sealed_sandbox_network(&resident.st.head).unwrap(),
            "trace":{}
        }))
        .expect("valid managed operation envelope");
        envelope.request_digest = brain_protocol::contract::operation_request_digest(&envelope);
        let binding: brain_protocol::hand::ResolvedBinding = serde_json::from_value(json!({
            "binding_ref":binding_ref,
            "capabilities":["execution","session_preparation"],
            "hand_id":"hand_managedunknown",
            "limits":{
                "max_inline_input_bytes":brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES,
                "max_inline_result_bytes":brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                "max_wait_ms":1
            },
            "realm":"aex_managed",
            "recovery":"retained"
        }))
        .expect("valid resolved binding");
        resident.managed_bindings = Arc::new(HashMap::from([(name.clone(), binding)]));
        resident.st.head.state = "open".into();
        resident.st.head.turn = Some(turn.clone());
        resident.st.head.active_phase = Some("managed_running".into());
        resident.st.head.active_rounds = 1;
        resident.st.head.active_tool_calls = 1;
        resident.st.head.turns += 1;
        let first_seq = resident.st.take_seq();
        let turn_started_seq = resident.st.take_seq();
        let assistant_seq = resident.st.take_seq();
        let tool_call_seq = resident.st.take_seq();
        let managed_intent_seq = resident.st.take_seq();
        let records = vec![
            (
                first_seq,
                Record::UserMessage {
                    turn: turn.clone(),
                    content: vec![ContentBlock::text("run the managed effect")],
                    starts_turn: false,
                    metadata: HashMap::new(),
                    idempotency_key_hash: None,
                    request_hash: None,
                },
            ),
            (turn_started_seq, Record::TurnStarted { turn: turn.clone() }),
            (
                assistant_seq,
                Record::Assistant {
                    turn: turn.clone(),
                    agent: "root".into(),
                    attempt_id: "att_managedunknown00000".into(),
                    content: vec![ContentBlock::ToolUse {
                        id: call.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }],
                    stop: crate::message::StopReason::ToolUse,
                },
            ),
            (
                tool_call_seq,
                Record::ToolCall {
                    turn: turn.clone(),
                    agent: "root".into(),
                    call: call.clone(),
                    name: name.clone(),
                    input,
                    detach: false,
                },
            ),
            (
                managed_intent_seq,
                Record::ManagedCallIntent {
                    turn: turn.clone(),
                    call: call.clone(),
                    name: name.clone(),
                    envelope,
                },
            ),
        ];
        commit(&brain, &session_id, &mut resident.st, records)
            .await
            .expect("commit crash-after-Submit intent");

        let crash_entries = journal.read_records(&session_id, 0).await.unwrap();
        assert_eq!(
            pending_managed(&crash_entries).unwrap().len(),
            1,
            "the simulated crash leaves one durable managed intent; records={:?}",
            crash_entries
                .iter()
                .map(|entry| (entry.seq, entry.record.kind_name()))
                .collect::<Vec<_>>()
        );
        let error = recover_managed_calls(&brain, &session_id, &mut resident, &crash_entries)
            .await
            .expect_err("inject a crash boundary after the unknown marker and status commit");
        assert!(matches!(error, BrainError::HandUnavailable(_)), "{error:?}");
        assert_eq!(ports.submits.load(Ordering::Acquire), 1);
        let after_unknown = journal.read_records(&session_id, 0).await.unwrap();
        assert_eq!(
            after_unknown
                .iter()
                .filter(|entry| matches!(entry.record, Record::ManagedCallUnknown { .. }))
                .count(),
            1,
            "OperationUnknown is a single durable revocation of submit replay"
        );
        assert!(after_unknown.iter().any(|entry| matches!(
            &entry.record,
            Record::DefaultSandboxChanged { status }
                if status.reason.as_ref().is_some_and(|reason| reason.as_str() == MANAGED_UNKNOWN_SANDBOX_REASON)
                    && status.generation.as_ref().is_some_and(|generation| generation.as_str() == "gen_unknown_submit")
                    && status.target_ref.as_ref().is_some_and(|target_ref| target_ref.as_str() == "tgt_unknown_submit")
                    && status.expires_at_ms.is_some()
        )));
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .expect("release simulated crashed recovery owner");
        drop(resident);
        drop(brain);

        let provider = fake.clone();
        let recovering = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.cloned_as("brain-managed-submit-unknown-restart"),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                hand: Some(ports.clone()),
                session_preparation: Some(ports.clone()),
                sandbox_files: Some(ports.clone()),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| provider.clone())),
        );
        let recovered = hydrate(&recovering, &session_id)
            .await
            .expect("restart consumes the unknown marker without Submit");
        assert_eq!(
            ports.submits.load(Ordering::Acquire),
            1,
            "the journaled unknown marker permanently forbids a second Submit"
        );
        assert_eq!(ports.dematerialize_calls.load(Ordering::Acquire), 2);
        assert_eq!(
            recovered
                .st
                .head
                .default_sandbox
                .as_ref()
                .expect("reconciled default sandbox")
                .state,
            brain_protocol::hand::SandboxState::Terminated
        );
        assert!(recovered.st.head.turn.is_none());
        let final_records = journal.read_records(&session_id, 0).await.unwrap();
        assert_eq!(
            final_records
                .iter()
                .filter(|entry| matches!(
                    &entry.record,
                    Record::ToolResult { call: result_call, outcome, .. }
                        if result_call == &call && outcome == "interrupted"
                ))
                .count(),
            1
        );
        fake.assert_drained(1, "managed OperationUnknown recovery")
            .unwrap();
        recovering
            .journal
            .release(&session_id, &recovered.st.lease)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ending_session_reconciles_stale_managed_intent_without_resubmission() {
        let journal = Journal::new_memory("brain-stale-managed-ending");
        let ports = Arc::new(UnknownManagedPorts::default());
        ports.fail_next_dematerialize.store(true, Ordering::Release);
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        let provider = fake.clone();
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                hand: Some(ports.clone()),
                session_preparation: Some(ports.clone()),
                sandbox_files: Some(ports.clone()),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| provider.clone())),
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic","name":"stale-managed","api_key":"key"}
                })),
                Some("stale-managed-ending"),
            )
            .await
            .expect("create stale managed recovery session");
        let session_id = created.id.to_string();
        let mut resident = hydrate(&brain, &session_id)
            .await
            .expect("claim stale managed session");
        let turn = "trn_stalemanaged0000000".to_owned();
        let call = "op_stalemanaged000000".to_owned();
        let name = "managed_stale_test".to_owned();
        let mut envelope: brain_protocol::hand::OperationEnvelope = serde_json::from_value(json!({
            "operation_id":call,
            "request_digest":"0".repeat(64),
            "session_id":session_id,
            "root_id":resident.st.head.root_id,
            "turn_id":turn,
            "caller_id":"agent_root",
            "fence":resident.st.lease.fence,
            "generation":null,
            "binding_ref":"bnd_stalemanaged0000",
            "capability":name,
            "input":{"kind":"inline","value":{"effect":"may_have_started"}},
            "target_ref":null,
            "deadline_at_ms":crate::wall_ms() + 60_000,
            "resources":managed_hand_resources().unwrap(),
            "network":sealed_sandbox_network(&resident.st.head).unwrap(),
            "trace":{}
        }))
        .expect("valid stale managed envelope");
        envelope.request_digest = brain_protocol::contract::operation_request_digest(&envelope);

        resident.st.head.turn = Some(turn.clone());
        resident.st.head.active_phase = Some("managed_running".into());
        resident.st.head.active_rounds = 1;
        resident.st.head.active_tool_calls = 1;
        let intent_records = vec![
            (
                resident.st.take_seq(),
                Record::TurnStarted { turn: turn.clone() },
            ),
            (
                resident.st.take_seq(),
                Record::ToolCall {
                    turn: turn.clone(),
                    agent: "root".into(),
                    call: call.clone(),
                    name: name.clone(),
                    input: json!({"effect":"may_have_started"}),
                    detach: false,
                },
            ),
            (
                resident.st.take_seq(),
                Record::ManagedCallIntent {
                    turn: turn.clone(),
                    call: call.clone(),
                    name: name.clone(),
                    envelope,
                },
            ),
        ];
        commit(&brain, &session_id, &mut resident.st, intent_records)
            .await
            .expect("commit managed intent");

        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        resident.st.head.active_rounds = 0;
        resident.st.head.active_tool_calls = 0;
        let failed_records = vec![
            (
                resident.st.take_seq(),
                Record::TurnFailed {
                    turn: turn.clone(),
                    code: "internal".into(),
                    message: "sandbox capacity is exhausted".into(),
                    details: None,
                },
            ),
            (
                resident.st.take_seq(),
                Record::State {
                    state: "open".into(),
                    turn: None,
                },
            ),
        ];
        commit(&brain, &session_id, &mut resident.st, failed_records)
            .await
            .expect("commit failed turn without a managed result");
        resident.st.head.ended = true;
        resident.st.head.state = "ending".into();
        let ending = vec![(
            resident.st.take_seq(),
            Record::State {
                state: "ending".into(),
                turn: None,
            },
        )];
        commit(&brain, &session_id, &mut resident.st, ending)
            .await
            .expect("commit ending lifecycle");

        let crash_entries = journal.read_records(&session_id, 0).await.unwrap();
        let error = recover_managed_calls(&brain, &session_id, &mut resident, &crash_entries)
            .await
            .expect_err("inject cleanup loss after submit replay is revoked");
        assert!(matches!(error, BrainError::HandUnavailable(_)), "{error:?}");
        assert_eq!(ports.submits.load(Ordering::Acquire), 0);
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .expect("release simulated cleanup crash owner");
        drop(resident);
        drop(brain);

        let provider = fake.clone();
        let recovering = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.cloned_as("brain-stale-managed-ending-restart"),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                hand: Some(ports.clone()),
                session_preparation: Some(ports.clone()),
                sandbox_files: Some(ports.clone()),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| provider.clone())),
        );
        let recovered = hydrate(&recovering, &session_id)
            .await
            .expect("restart reconciles the stale managed intent");
        assert_eq!(ports.submits.load(Ordering::Acquire), 0);
        assert_eq!(ports.dematerialize_calls.load(Ordering::Acquire), 2);
        assert_eq!(recovered.st.head.state, "ending");
        assert!(recovered.st.head.turn.is_none());
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|entry| matches!(entry.record, Record::ManagedCallUnknown { .. }))
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|entry| matches!(
                    &entry.record,
                    Record::ToolResult { call: result_call, outcome, .. }
                        if result_call == &call && outcome == "interrupted"
                ))
                .count(),
            1
        );
        assert!(pending_managed(&records).unwrap().is_empty());

        let mut resident = Some(recovered);
        assert!(
            continue_end_session(&recovering, &session_id, &mut resident)
                .await
                .expect("ending cleanup converges")
        );
        assert_eq!(
            journal.get_head(&session_id).await.unwrap().doc.state,
            "ended"
        );
        if let Some(resident) = resident {
            recovering
                .journal
                .release(&session_id, &resident.st.lease)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn deleting_managed_session_hydrates_without_repreparing_hand_definitions() {
        let journal = Journal::new_memory("brain-deleting-managed-hydrate");
        let brain = Brain::with_parts(
            BrainConfig::default(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            None,
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model":{"provider":"anthropic","name":"deleting-managed","api_key":"key"}
                })),
                Some("deleting-managed-hydrate"),
            )
            .await
            .expect("create deleting managed session");
        let session_id = created.id.to_string();
        let mut resident = hydrate(&brain, &session_id)
            .await
            .expect("claim deleting managed session");
        let bundle_digest = "a".repeat(64);
        resident.st.head.prefix.managed_bundles.push(
            serde_json::from_value(json!({
                "bundle_digest":bundle_digest,
                "bytes":1,
                "contract_digest":"b".repeat(64),
                "object":{
                    "bytes":1,
                    "media_type":"application/javascript+esm",
                    "object_id":format!("bundle_{bundle_digest}"),
                    "sha256":bundle_digest,
                },
                "required_env":[],
                "runtime":"node22",
                "tool_name":"managed_delete_test",
            }))
            .expect("valid managed bundle descriptor"),
        );
        resident.st.head.state = "deleting".into();
        resident.st.head.ended = true;
        resident.st.head.turn = None;
        resident.st.head.active_phase = None;
        let state_seq = resident.st.take_seq();
        commit(
            &brain,
            &session_id,
            &mut resident.st,
            vec![(
                state_seq,
                Record::State {
                    state: "deleting".into(),
                    turn: None,
                },
            )],
        )
        .await
        .expect("commit deleting lifecycle with a managed descriptor");
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .expect("release deleting session before cold hydration");
        drop(resident);

        let recovered = hydrate(&brain, &session_id)
            .await
            .expect("deleting hydration must not require or recreate Hand definitions");
        assert_eq!(recovered.st.head.state, "deleting");
        assert!(!recovered.st.head.prefix.managed_bundles.is_empty());
        assert!(recovered.managed_bindings.is_empty());
        journal
            .release(&session_id, &recovered.st.lease)
            .await
            .expect("release recovered deleting session");
    }

    async fn simulate_provider_only_crash(
        retries: u32,
        attempt_state: &str,
    ) -> (Arc<Brain>, Journal, Arc<FakeProvider>, String, u64, PathBuf) {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-provider-recovery-{}-{}-{retries}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create provider recovery data dir");
        let journal = Journal::new_memory(format!("brain-provider-recovery-{retries}"));
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::Text("replacement completed".into())]);
        let provider = fake.clone();
        let provider_factory: ProviderFactory = Arc::new(move |_| provider.clone());
        let brain = Brain::with_parts(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Some(provider_factory),
        );
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "provider-recovery-test",
                        "api_key": "sk-test"
                    },
                    "provider_recovery_retries": retries
                }))
                .expect("valid provider recovery create"),
                None,
            )
            .await
            .expect("create provider recovery session");
        let session_id = created.id.to_string();
        for _ in 0..100 {
            if journal
                .get_head(&session_id)
                .await
                .expect("recovery head")
                .last_seq
                >= 2
            {
                break;
            }
            tokio::task::yield_now().await;
        }

        let mut head = journal.claim(&session_id).await.expect("claim crash owner");
        let turn = "trn_providercrash00000000".to_owned();
        let content = vec![ContentBlock::text("recover provider-only phase")];
        let history = vec![Message {
            role: crate::message::Role::User,
            content: content.clone(),
        }];
        let (prefix, _) = build_prefix(&head.doc.prefix, 512).expect("rebuild sealed prefix");
        let request = fake
            .build_request(
                &prefix,
                &history,
                &ProviderKey::new("sk-test"),
                head.doc.prefix.base_url.as_deref().expect("base URL"),
            )
            .expect("build crashed request");
        let request_digest = crate::turn::model_request_digest(&request);
        let logical_operation_id = "mdl_providercrash00000000".to_owned();
        let attempt_id = "att_providercrash00000000".to_owned();
        head.doc.state = "open".into();
        head.doc.turn = Some(turn.clone());
        head.doc.active_phase = Some(if attempt_state == "intent" {
            "model_intent_committed".into()
        } else {
            "model_running".into()
        });
        head.doc.provider_attempt = Some(crate::journal::ProviderAttemptDoc {
            logical_operation_id: logical_operation_id.clone(),
            attempt_id: attempt_id.clone(),
            request_digest: request_digest.clone(),
            state: attempt_state.into(),
            replacements_used: 0,
        });
        head.doc.active_context = HashMap::new();
        head.doc.active_rounds = 0;
        head.doc.active_tool_calls = 0;
        head.doc.turns += 1;
        let first_seq = head.last_seq + 1;
        let records = vec![
            (
                first_seq,
                Record::UserMessage {
                    turn: turn.clone(),
                    content,
                    starts_turn: false,
                    metadata: HashMap::new(),
                    idempotency_key_hash: None,
                    request_hash: None,
                },
            ),
            (first_seq + 1, Record::TurnStarted { turn: turn.clone() }),
            (
                first_seq + 2,
                Record::ModelCallIntent {
                    turn: turn.clone(),
                    logical_operation_id,
                    attempt_id,
                    request_digest,
                    replacement: 0,
                },
            ),
        ];
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
            retention: head.retention,
        };
        journal
            .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 2)
            .await
            .expect("commit simulated provider crash");
        journal
            .defer_recovery(&session_id)
            .await
            .expect("release failed provider owner and schedule recovery");
        (brain, journal, fake, session_id, first_seq + 2, data_dir)
    }

    #[tokio::test]
    async fn provider_only_crash_replaces_the_same_logical_request_by_default() {
        for state in ["intent", "running"] {
            let (brain, journal, fake, session_id, crash_seq, data_dir) =
                simulate_provider_only_crash(1, state).await;
            let resident = hydrate(&brain, &session_id)
                .await
                .expect("hydrate provider-only crash");
            assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
            assert_eq!(resident.st.head.state, "open");
            assert!(resident.st.head.turn.is_none());
            let records = journal
                .read_records(&session_id, crash_seq)
                .await
                .expect("read provider recovery records");
            assert!(records.iter().any(|entry| matches!(
                &entry.record,
                Record::ModelCallUnknown {
                    possibly_duplicated: true,
                    ..
                }
            )));
            assert!(records.iter().any(|entry| matches!(
                &entry.record,
                Record::ModelCallIntent { replacement: 1, .. }
            )));
            assert!(
                records
                    .iter()
                    .any(|entry| matches!(&entry.record, Record::ModelCallCompleted { .. }))
            );
            journal
                .release(&session_id, &resident.st.lease)
                .await
                .expect("release recovered provider lease");
            drop(resident);
            let _ = std::fs::remove_dir_all(data_dir);
        }
    }

    #[tokio::test]
    async fn provider_only_crash_with_zero_retries_commits_honest_interruption() {
        let (brain, journal, fake, session_id, crash_seq, data_dir) =
            simulate_provider_only_crash(0, "running").await;
        let resident = hydrate(&brain, &session_id)
            .await
            .expect("hydrate provider-only crash");
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 0);
        assert_eq!(resident.st.head.state, "open");
        assert!(resident.st.head.turn.is_none());
        let records = journal
            .read_records(&session_id, crash_seq)
            .await
            .expect("read strict interruption records");
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::ModelCallUnknown {
                possibly_duplicated: true,
                ..
            }
        )));
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if stop_reason == "interrupted"
        )));
        assert!(!records.iter().any(|entry| matches!(
            &entry.record,
            Record::ModelCallIntent { replacement: 1, .. }
        )));
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .expect("release interrupted provider lease");
        drop(resident);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn recovery_worker_resumes_provider_only_crash_without_customer_traffic() {
        let (_crashed_brain, journal, fake, session_id, crash_seq, data_dir) =
            simulate_provider_only_crash(1, "running").await;
        let provider = fake.clone();
        let recovering = Brain::with_parts(
            BrainConfig {
                recovery_poll_interval: Duration::from_millis(5),
                recovery_shards_per_poll: crate::journal::RECOVERY_SHARDS,
                recovery_page_size: 16,
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.cloned_as("brain-recovery-worker"),
            Arc::new(crate::keys::PlainCustody),
            Some(Arc::new(move |_| provider.clone())),
        );
        recovering.start_recovery_worker();

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let head = journal.get_head(&session_id).await.unwrap();
                if head.doc.turn.is_none() && head.last_seq > crash_seq {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("due recovery completed without a follow-up request");
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
        let records = journal.read_records(&session_id, crash_seq).await.unwrap();
        assert!(
            records.iter().any(|entry| matches!(
                entry.record,
                Record::ModelCallIntent { replacement: 1, .. }
            ))
        );
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::TurnCompleted { .. }))
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    async fn run_live_provider_case(
        retries: u32,
        script: Vec<Scripted>,
    ) -> (Journal, Arc<FakeProvider>, String, PathBuf) {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-live-provider-recovery-{}-{}-{retries}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create live provider recovery dir");
        let journal = Journal::new_memory(format!("brain-live-provider-{retries}"));
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script(script);
        let provider = fake.clone();
        let brain = Brain::with_parts(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Some(Arc::new(move |_| provider.clone())),
        );
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {"provider":"anthropic", "name":"live-recovery", "api_key":"sk-test"},
                    "provider_recovery_retries": retries
                }))
                .unwrap(),
                None,
            )
            .await
            .unwrap();
        let session_id = created.id.to_string();
        let (_, admitted_seq) = brain
            .message(
                &session_id,
                serde_json::from_value(json!("exercise recovery")).unwrap(),
            )
            .await
            .unwrap();
        // Generous budget: live-retry cases sleep through jittered backoff before finishing.
        for _ in 0..2_500 {
            let head = journal.get_head(&session_id).await.unwrap();
            if head.doc.turn.is_none() && head.last_seq > admitted_seq {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let head = journal.get_head(&session_id).await.unwrap();
        assert!(
            head.doc.turn.is_none(),
            "live provider turn remained active"
        );
        (journal, fake, session_id, data_dir)
    }

    #[tokio::test]
    async fn live_unknown_before_or_after_stream_bytes_uses_the_same_replacement_budget() {
        for partial_text in [None, Some("provisional bytes".to_owned())] {
            let (journal, fake, session_id, data_dir) = run_live_provider_case(
                1,
                vec![
                    Scripted::TransportError {
                        partial_text,
                        message: "ambiguous reset".into(),
                    },
                    Scripted::Text("replacement completed".into()),
                ],
            )
            .await;
            assert_eq!(fake.call_count.load(Ordering::Relaxed), 2);
            let records = journal.read_records(&session_id, 0).await.unwrap();
            let intents = records
                .iter()
                .filter_map(|entry| match &entry.record {
                    Record::ModelCallIntent {
                        logical_operation_id,
                        request_digest,
                        replacement,
                        ..
                    } => Some((logical_operation_id, request_digest, replacement)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(intents.len(), 2);
            assert_eq!(intents[0].0, intents[1].0);
            assert_eq!(intents[0].1, intents[1].1);
            assert_eq!(*intents[1].2, 1);
            assert!(
                records
                    .iter()
                    .any(|entry| matches!(entry.record, Record::ModelCallCompleted { .. }))
            );
            let _ = std::fs::remove_dir_all(data_dir);
        }
    }

    #[tokio::test]
    async fn live_unknown_zero_or_exhausted_budget_interrupts_honestly() {
        for (retries, script, expected_calls) in [
            (
                0,
                vec![Scripted::TransportError {
                    partial_text: None,
                    message: "strict reset".into(),
                }],
                1,
            ),
            (
                1,
                vec![
                    Scripted::TransportError {
                        partial_text: None,
                        message: "first reset".into(),
                    },
                    Scripted::TransportError {
                        partial_text: Some("then reset".into()),
                        message: "second reset".into(),
                    },
                ],
                2,
            ),
        ] {
            let (journal, fake, session_id, data_dir) =
                run_live_provider_case(retries, script).await;
            assert_eq!(fake.call_count.load(Ordering::Relaxed), expected_calls);
            let records = journal.read_records(&session_id, 0).await.unwrap();
            assert!(records.iter().any(|entry| matches!(
                &entry.record,
                Record::TurnCompleted { stop_reason, .. } if stop_reason == "interrupted"
            )));
            assert_eq!(
                records
                    .iter()
                    .filter(|entry| matches!(entry.record, Record::ModelCallUnknown { .. }))
                    .count(),
                expected_calls as usize
            );
            let _ = std::fs::remove_dir_all(data_dir);
        }
    }

    #[tokio::test]
    async fn the_agentloop_selector_seals_and_unavailable_loops_refuse_at_create() {
        let journal = Journal::new_memory("agentloop-selector");
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        let provider = fake.clone();
        let brain = Brain::with_parts(
            BrainConfig::default(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Some(Arc::new(move |_| provider.clone())),
        );
        let model = json!({"provider":"anthropic", "name":"selector-test", "api_key":"sk-test"});

        let created = brain
            .create_session(
                serde_json::from_value(json!({"model": model, "agentloop": "aex"})).unwrap(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&created).unwrap()["agentloop"],
            json!({"kind": "official", "name": "aex", "version": "1"}),
            "the sealed loop identity is echoed on the session resource"
        );

        let refused = brain
            .create_session(
                serde_json::from_value(json!({"model": model, "agentloop": "pi"})).unwrap(),
                None,
            )
            .await;
        assert!(
            matches!(&refused, Err(BrainError::Invalid(message)) if message.contains("not available")),
            "an official this composition lacks refuses at create: {refused:?}"
        );

        let bundle = b"export function activate() { return \"{}\" }";
        let encoded = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(bundle)
        };
        let digest = hex::encode(Sha256::digest(bundle));
        let wrong_digest = brain
            .create_session(
                serde_json::from_value(json!({"model": model, "agentloop": {
                    "source_bundle_sha256": "0".repeat(64),
                    "toolchain": "loopchain-1",
                    "bundle_base64": encoded,
                }}))
                .unwrap(),
                None,
            )
            .await;
        assert!(
            matches!(&wrong_digest, Err(BrainError::Invalid(message)) if message.contains("digest")),
            "a bundle that does not match its declared digest never reaches the registry"
        );
        let custom = brain
            .create_session(
                serde_json::from_value(json!({"model": model, "agentloop": {
                    "source_bundle_sha256": digest,
                    "toolchain": "loopchain-1",
                    "bundle_base64": encoded,
                }}))
                .unwrap(),
                None,
            )
            .await;
        assert!(
            matches!(&custom, Err(BrainError::Invalid(message)) if message.contains("not enabled")),
            "the default registry refuses customs honestly: {custom:?}"
        );
    }

    #[tokio::test]
    async fn a_composition_registry_resolves_per_session_loops() {
        struct TwoOfficials;
        impl crate::agentloop::AgentloopRegistry for TwoOfficials {
            fn resolve(
                &self,
                selector: &crate::journal::AgentloopSelectorDoc,
            ) -> Result<Arc<dyn crate::agentloop::Agentloop>> {
                match selector {
                    crate::journal::AgentloopSelectorDoc::Official { name, .. }
                        if name == "aex" || name == "echo-loop" =>
                    {
                        Ok(Arc::new(crate::agentloop::BuiltinAexLoop))
                    }
                    _ => Err(BrainError::Invalid("unknown loop".into())),
                }
            }
            fn pin_official(&self, name: &str) -> Result<crate::journal::AgentloopSelectorDoc> {
                match name {
                    "aex" => Ok(crate::journal::AgentloopSelectorDoc::official_aex()),
                    "echo-loop" => Ok(crate::journal::AgentloopSelectorDoc::Official {
                        name: "echo-loop".into(),
                        version: "9".into(),
                    }),
                    _ => Err(BrainError::Invalid("unknown loop".into())),
                }
            }
        }
        let journal = Journal::new_memory("agentloop-registry");
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::Text("resolved answer".into())]);
        let provider = fake.clone();
        let brain = Brain::with_parts_and_services(
            BrainConfig::default(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                agentloop_registry: Some(Arc::new(TwoOfficials)),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| {
                provider.clone() as Arc<dyn crate::provider::Provider>
            })),
        );
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {"provider":"anthropic", "name":"registry-test", "api_key":"sk-test"},
                    "agentloop": "echo-loop"
                }))
                .unwrap(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&created).unwrap()["agentloop"]["version"],
            "9",
            "the registry's pinned version seals"
        );
        let session_id = created.id.to_string();
        let (_, admitted_seq) = brain
            .message(
                &session_id,
                serde_json::from_value(json!("drive the sealed loop")).unwrap(),
            )
            .await
            .unwrap();
        for _ in 0..1_000 {
            let head = journal.get_head(&session_id).await.unwrap();
            if head.doc.turn.is_none() && head.last_seq > admitted_seq {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(
            records.iter().any(|entry| matches!(
                &entry.record,
                Record::TurnCompleted { stop_reason, .. } if stop_reason == "end_turn"
            )),
            "the per-session loop resolved at turn time and drove the turn"
        );
    }

    #[tokio::test]
    async fn draining_refuses_new_work_while_admitted_turns_finish() {
        let journal = Journal::new_memory("brain-drain");
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::Text("x".repeat(800))]);
        // Paced emission gives the turn a real duration so the drain window is observable.
        fake.tokens_per_second.store(400, Ordering::Relaxed);
        let provider = fake.clone();
        let brain = Brain::with_parts(
            BrainConfig::default(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Some(Arc::new(move |_| provider.clone())),
        );
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {"provider":"anthropic", "name":"drain-test", "api_key":"sk-test"}
                }))
                .unwrap(),
                None,
            )
            .await
            .unwrap();
        let session_id = created.id.to_string();
        brain
            .message(
                &session_id,
                serde_json::from_value(json!("one slow answer")).unwrap(),
            )
            .await
            .unwrap();
        assert!(brain.active_turns() > 0, "the turn holds its permit");

        brain.begin_drain();
        let refused = brain
            .message(
                &session_id,
                serde_json::from_value(json!("refused")).unwrap(),
            )
            .await;
        assert!(matches!(refused, Err(BrainError::Draining)));
        let refused_create = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {"provider":"anthropic", "name":"drain-test", "api_key":"sk-test"}
                }))
                .unwrap(),
                None,
            )
            .await;
        assert!(matches!(refused_create, Err(BrainError::Draining)));

        // The admitted turn is never interrupted: it runs to its durable terminal.
        for _ in 0..2_000 {
            if brain.active_turns() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(brain.active_turns(), 0, "the admitted turn drained");
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if stop_reason == "end_turn"
        )));
    }

    #[tokio::test]
    async fn clean_provider_failures_retry_in_place_without_replacement_budget() {
        // Replacement budget ZERO: live retry is a separate mechanism from the durable
        // digest-identical replacement path and must not consume it.
        let (journal, fake, session_id, data_dir) = run_live_provider_case(
            0,
            vec![
                Scripted::ProviderStatus {
                    status: 500,
                    body: "transient upstream failure".into(),
                    retry_after_ms: None,
                },
                Scripted::ProviderStatus {
                    status: 429,
                    body: "rate limited".into(),
                    retry_after_ms: Some(100),
                },
                Scripted::Text("recovered in place".into()),
            ],
        )
        .await;
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 3);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if stop_reason == "end_turn"
        )));
        assert!(
            !records
                .iter()
                .any(|entry| matches!(entry.record, Record::ModelCallUnknown { .. })),
            "a complete HTTP error response is definitive, never an unknown outcome"
        );
        assert_eq!(
            records
                .iter()
                .filter(|entry| matches!(entry.record, Record::ModelCallIntent { .. }))
                .count(),
            1,
            "in-place retries reuse the committed intent"
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn persistent_clean_failures_exhaust_live_retries_and_fail_honestly() {
        let script = (0..4)
            .map(|_| Scripted::ProviderStatus {
                status: 503,
                body: "unavailable".into(),
                retry_after_ms: Some(50),
            })
            .collect();
        let (journal, fake, session_id, data_dir) = run_live_provider_case(1, script).await;
        assert_eq!(
            fake.call_count.load(Ordering::Relaxed),
            4,
            "one attempt plus exactly three live retries"
        );
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnFailed { code, .. } if code == "provider_error"
        )));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn quota_exhaustion_fails_fast_despite_the_429_status() {
        let (journal, fake, session_id, data_dir) = run_live_provider_case(
            1,
            vec![Scripted::ProviderStatus {
                status: 429,
                body: r#"{"error":{"code":"insufficient_quota"}}"#.into(),
                retry_after_ms: Some(50),
            }],
        )
        .await;
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnFailed { code, .. } if code == "provider_error"
        )));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn deterministic_provider_4xx_is_not_unknown_or_retried() {
        let (journal, fake, session_id, data_dir) = run_live_provider_case(
            2,
            vec![
                Scripted::ProviderStatus {
                    status: 400,
                    body: "invalid model request".into(),
                    retry_after_ms: None,
                },
                Scripted::Text("must not run".into()),
            ],
        )
        .await;
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(
            !records
                .iter()
                .any(|entry| matches!(entry.record, Record::ModelCallUnknown { .. }))
        );
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::TurnFailed { .. }))
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    async fn run_compaction_case(
        retries: u32,
        compactor_failures: usize,
    ) -> (
        Journal,
        Arc<FakeProvider>,
        Arc<ScriptedCompactor>,
        String,
        PathBuf,
    ) {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-compaction-journal-{}-{}-{retries}-{compactor_failures}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).unwrap();
        let journal = Journal::new_memory(format!("brain-compaction-{retries}"));
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([
            Scripted::Text(format!(
                "early exact fact /workspace/file.rs id=alpha {}",
                "x".repeat(2_000)
            )),
            Scripted::Text("provider ran after compaction".into()),
        ]);
        let provider = fake.clone();
        let compactor = Arc::new(ScriptedCompactor {
            failures_remaining: AtomicUsize::new(compactor_failures),
            calls: AtomicUsize::new(0),
        });
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                context_soft_tokens: 100,
                context_hard_tokens: 5_000,
                context_tail_tokens: 1,
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: None,
                customer_delivery: None,
                customer_transport: None,
                compactor: Some(compactor.clone()),
                ..BrainServices::default()
            },
            Some(Arc::new(move |_| provider.clone())),
        );
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {"provider":"anthropic", "name":"compact", "api_key":"sk-test"},
                    "provider_recovery_retries": retries
                }))
                .unwrap(),
                None,
            )
            .await
            .unwrap();
        let session_id = created.id.to_string();
        for message in ["first", "second"] {
            let (_, admitted) = brain
                .message(&session_id, serde_json::from_value(json!(message)).unwrap())
                .await
                .unwrap();
            for _ in 0..500 {
                let head = journal.get_head(&session_id).await.unwrap();
                if head.doc.turn.is_none() && head.last_seq > admitted {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            assert!(
                journal
                    .get_head(&session_id)
                    .await
                    .unwrap()
                    .doc
                    .turn
                    .is_none()
            );
        }
        (journal, fake, compactor, session_id, data_dir)
    }

    #[tokio::test]
    async fn compaction_unknown_retries_with_one_canonical_usage_and_atomic_checkpoint() {
        let (journal, fake, compactor, session_id, data_dir) = run_compaction_case(1, 1).await;
        assert_eq!(compactor.calls.load(Ordering::Relaxed), 2);
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 2);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        let intents = records
            .iter()
            .filter_map(|entry| match &entry.record {
                Record::CompactionIntent {
                    logical_operation_id,
                    request_digest,
                    replacement,
                    ..
                } => Some((logical_operation_id, request_digest, replacement)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(intents.len(), 2);
        assert_eq!(intents[0].0, intents[1].0);
        assert_eq!(intents[0].1, intents[1].1);
        assert_eq!(*intents[1].2, 1);
        assert_eq!(
            records
                .iter()
                .filter(|entry| matches!(entry.record, Record::CompactionUnknown { .. }))
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|entry| matches!(
                    &entry.record,
                    Record::Usage { agent, usage, .. }
                        if agent == "compactor" && usage.input_tokens == Some(7)
                ))
                .count(),
            1,
            "only the installed compaction usage is canonical"
        );
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::CompactionCompleted { .. }))
        );
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::ContextInstalled { .. }))
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn failed_strict_compaction_installs_no_partial_checkpoint_or_usage() {
        let (journal, fake, compactor, session_id, data_dir) = run_compaction_case(0, 1).await;
        assert_eq!(compactor.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::CompactionUnknown { .. }))
        );
        assert!(!records.iter().any(|entry| matches!(
            entry.record,
            Record::ContextInstalled { .. } | Record::ContextChunk { .. }
        )));
        assert!(!records.iter().any(|entry| matches!(
            &entry.record,
            Record::Usage { agent, .. } if agent == "compactor"
        )));
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if stop_reason == "interrupted"
        )));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn tenant_storage_quota_rejection_restores_the_live_actor_fold() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-storage-tenant-quota-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create tenant quota data dir");
        let journal = Journal::new_memory("brain-storage-tenant-quota");
        let storage = Arc::new(ReservationStorage::new(journal.clone()));
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                storage_max_object_bytes: 8,
                storage_max_session_bytes: 20,
                storage_max_tenant_bytes: 8,
                storage_transfer_ttl: Duration::from_secs(60 * 60),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage.clone()),
                ..BrainServices::default()
            },
            None,
        );
        let create = || {
            serde_json::from_value(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "storage-quota-test",
                    "api_key": "sk-test"
                }
            }))
            .expect("valid storage quota create")
        };
        let first = brain
            .create_session(create(), None)
            .await
            .expect("create first root");
        let second = brain
            .create_session(create(), None)
            .await
            .expect("create second root");
        brain
            .storage_prepare_upload(
                &first.id,
                crate::storage::StorageUploadIntent {
                    key: "full.bin".into(),
                    bytes: 8,
                    sha256: Some("a".repeat(64)),
                    content_type: None,
                    overwrite: false,
                },
            )
            .await
            .expect("first root consumes tenant quota");

        for _ in 0..2 {
            let error = brain
                .storage_prepare_upload(
                    &second.id,
                    crate::storage::StorageUploadIntent {
                        key: "rejected.bin".into(),
                        bytes: 1,
                        sha256: Some("b".repeat(64)),
                        content_type: None,
                        overwrite: false,
                    },
                )
                .await
                .expect_err("the same live actor must re-run tenant admission on retry");
            assert!(matches!(
                error,
                BrainError::TenantStorageQuotaExceeded {
                    requested: 1,
                    limit: 8
                }
            ));
        }
        assert_eq!(
            storage.prepares.load(Ordering::Relaxed),
            1,
            "a rejected resident reservation must never reach the storage adapter"
        );
        let rejected = journal
            .get_head(&second.id)
            .await
            .expect("authoritative rejected root");
        assert_eq!(rejected.doc.session_storage_bytes, 0);
        assert_eq!(rejected.doc.storage_reserved_bytes, 0);
        assert_eq!(rejected.doc.tenant_metered_storage_bytes, 0);
        assert!(rejected.doc.storage_upload.is_none());
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn deep_ancestor_end_fence_discards_the_mutated_resident_before_retry() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-deep-ancestor-fence-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create ancestor fence data dir");
        let journal = Journal::new_memory("brain-deep-ancestor-fence");
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        let provider = fake.clone();
        let brain = Brain::with_parts(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Some(Arc::new(move |_| provider.clone())),
        );
        let root = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "ancestor-fence-test",
                        "api_key": "sk-test"
                    }
                })),
                Some("ancestor-fence-root"),
            )
            .await
            .expect("create ancestor fence root");
        let root_id = root.id.to_string();
        let root_head = journal.get_head(&root_id).await.expect("root head");

        let child_id = "ses_ancestorfencechild000";
        let mut child = root_head.doc.clone();
        child.root_id = root_id.clone();
        child.parent_id = Some(root_id.clone());
        child.ancestor_ids = vec![root_id.clone()];
        child.depth = 1;
        child.create_key_hash = None;
        child.create_request_hash = None;
        child.turn = None;
        child.turns = 0;
        child.last_seq = 1;
        child.context_fork = None;
        child.default_sandbox = None;
        journal
            .create(
                child_id,
                &child,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .expect("create depth-one child");

        let grandchild_id = "ses_ancestorfencegrand000";
        let mut grandchild = child.clone();
        grandchild.parent_id = Some(child_id.into());
        grandchild.ancestor_ids = vec![root_id.clone(), child_id.into()];
        grandchild.depth = 2;
        journal
            .create(
                grandchild_id,
                &grandchild,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .expect("create depth-two child");

        // The descendant can claim and hydrate before the root fence. Its next admission mutates
        // a local TurnStarted projection, but the atomic journal decision must observe this root
        // transition and fence that stale resident.
        let _ = brain
            .cancel(grandchild_id)
            .await
            .expect("hydrate descendant before root fence");
        let mut fenced_root = journal.claim(&root_id).await.expect("claim root fence");
        fenced_root.doc.ended = true;
        fenced_root.doc.state = "ending".into();
        let mut root_lease = Lease {
            fence: fenced_root.fence,
            last_seq: fenced_root.last_seq,
            retention: fenced_root.retention,
        };
        let root_fence_seq = fenced_root.last_seq + 1;
        journal
            .commit(
                &root_id,
                &mut root_lease,
                &[(
                    root_fence_seq,
                    Record::State {
                        state: "ending".into(),
                        turn: None,
                    },
                )],
                &fenced_root.doc,
                root_fence_seq,
            )
            .await
            .expect("commit constant-size root admission fence");

        for attempt in 0..2 {
            let error = brain
                .message(
                    grandchild_id,
                    MessageRequestContent::String(
                        format!("late message {attempt}").parse().unwrap(),
                    ),
                )
                .await
                .expect_err("root fence rejects every deep admission");
            assert!(matches!(error, BrainError::Fenced));
            if attempt == 0 {
                tokio::time::timeout(Duration::from_secs(1), async {
                    loop {
                        let closed = brain
                            .sessions
                            .lock()
                            .expect("session actors")
                            .get(grandchild_id)
                            .is_none_or(tokio::sync::mpsc::Sender::is_closed);
                        if closed {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("fenced descendant actor is reclaimed");
            }
        }
        let durable = journal
            .get_head(grandchild_id)
            .await
            .expect("durable descendant remains quiescent");
        assert_eq!(durable.last_seq, 1);
        assert_eq!(durable.doc.turns, 0);
        assert!(durable.doc.turn.is_none());
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 0);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn staged_overwrite_never_adopts_an_older_byte_identical_object() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-storage-provenance-staged-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create staged provenance data dir");
        let journal = Journal::new_memory("brain-storage-provenance-staged");
        let storage = Arc::new(ReservationStorage::new(journal.clone()));
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                storage_max_object_bytes: 16,
                storage_max_session_bytes: 32,
                storage_max_tenant_bytes: 32,
                storage_transfer_ttl: Duration::from_secs(60 * 60),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage.clone()),
                ..BrainServices::default()
            },
            None,
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "staged-provenance-test",
                        "api_key": "sk-test"
                    }
                })),
                None,
            )
            .await
            .expect("create staged provenance root");
        let session_id = created.id.to_string();
        let bytes = b"same";
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let old = brain
            .storage_write_inline(
                &session_id,
                "same.bin".into(),
                encoded,
                Some("text/plain".into()),
                false,
            )
            .await
            .expect("publish old object");
        let ticket = brain
            .storage_prepare_upload(
                &session_id,
                crate::storage::StorageUploadIntent {
                    key: "same.bin".into(),
                    bytes: bytes.len() as u64,
                    sha256: Some(hex::encode(Sha256::digest(bytes))),
                    content_type: Some("application/json".into()),
                    overwrite: true,
                },
            )
            .await
            .expect("reserve byte-identical overwrite");

        assert!(matches!(
            brain
                .storage_complete_upload(&session_id, &ticket.transfer_id)
                .await,
            Err(BrainError::FileNotFound(_))
        ));
        brain
            .storage_reconcile(&session_id)
            .await
            .expect("a future reservation remains pending, not falsely completed");
        let pending = journal.get_head(&session_id).await.unwrap();
        assert!(pending.doc.storage_upload.as_ref().is_some_and(|upload| {
            upload.transfer_id == ticket.transfer_id && upload.state == "reserved"
        }));
        let still_old = storage.stat(&session_id, "same.bin").await.unwrap();
        assert_eq!(still_old.publication_id, old.publication_id);
        assert_eq!(still_old.content_type.as_deref(), Some("text/plain"));

        // Even a buggy destination carrying the new publication id is not exact proof if its
        // sealed content type differs.
        storage
            .objects
            .lock()
            .expect("storage objects")
            .get_mut(&format!("{session_id}\0same.bin"))
            .expect("old object")
            .publication_id = Some(ticket.transfer_id.clone());
        assert!(matches!(
            brain
                .storage_complete_upload(&session_id, &ticket.transfer_id)
                .await,
            Err(BrainError::FileNotFound(_))
        ));
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(!records.iter().any(|entry| matches!(
            &entry.record,
            Record::StorageUploadPublished { transfer_id, .. }
                if transfer_id == &ticket.transfer_id
        )));

        storage
            .staged
            .lock()
            .expect("staged uploads")
            .insert(ticket.transfer_id.clone());
        let published = brain
            .storage_complete_upload(&session_id, &ticket.transfer_id)
            .await
            .expect("the exact staged transfer publishes");
        assert_eq!(
            published.publication_id.as_deref(),
            Some(ticket.transfer_id.as_str())
        );
        assert_eq!(published.content_type.as_deref(), Some("application/json"));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn inline_overwrite_retry_reexecutes_after_pre_publication_crash() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-storage-provenance-inline-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create inline provenance data dir");
        let journal = Journal::new_memory("brain-storage-provenance-inline");
        let storage = Arc::new(ReservationStorage::new(journal.clone()));
        let brain = Brain::with_parts_and_services(
            BrainConfig {
                idle_discard: Duration::from_secs(300),
                storage_max_object_bytes: 16,
                storage_max_session_bytes: 32,
                storage_max_tenant_bytes: 32,
                storage_transfer_ttl: Duration::from_secs(60 * 60),
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage.clone()),
                ..BrainServices::default()
            },
            None,
        );
        let created = brain
            .create_session(
                typed_create(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "inline-provenance-test",
                        "api_key": "sk-test"
                    }
                })),
                None,
            )
            .await
            .expect("create inline provenance root");
        let session_id = created.id.to_string();
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"same");
        let old = brain
            .storage_write_inline(
                &session_id,
                "same.bin".into(),
                encoded.clone(),
                Some("text/plain".into()),
                false,
            )
            .await
            .expect("publish old inline object");
        storage
            .fail_next_write_before_effect
            .store(true, Ordering::Relaxed);
        assert!(
            brain
                .storage_write_inline(
                    &session_id,
                    "same.bin".into(),
                    encoded.clone(),
                    Some("application/json".into()),
                    true,
                )
                .await
                .is_err()
        );
        let reserved = journal.get_head(&session_id).await.unwrap();
        let intent_id = reserved
            .doc
            .storage_upload
            .as_ref()
            .filter(|upload| upload.state == "inline_reserved")
            .expect("durable inline intent survives pre-effect crash")
            .transfer_id
            .clone();
        let unchanged = storage.stat(&session_id, "same.bin").await.unwrap();
        assert_eq!(unchanged.publication_id, old.publication_id);
        brain
            .storage_reconcile(&session_id)
            .await
            .expect("future inline intent remains pending");
        assert_eq!(
            journal
                .get_head(&session_id)
                .await
                .unwrap()
                .doc
                .storage_upload
                .as_ref()
                .map(|upload| upload.state.as_str()),
            Some("inline_reserved")
        );

        let retried = brain
            .storage_write_inline(
                &session_id,
                "same.bin".into(),
                encoded,
                Some("application/json".into()),
                true,
            )
            .await
            .expect("retry executes the unpublished inline intent");
        assert_eq!(retried.publication_id.as_deref(), Some(intent_id.as_str()));
        assert_eq!(retried.content_type.as_deref(), Some("application/json"));
        assert_eq!(storage.writes.load(Ordering::Relaxed), 3);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn copied_upload_is_adopted_after_crash_and_expiry_without_customer_traffic() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-storage-copy-crash-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create copy crash data dir");
        let journal = Journal::new_memory("brain-storage-copy-crash");
        let storage = Arc::new(ReservationStorage::new(journal.clone()));
        let cfg = BrainConfig {
            recovery_poll_interval: Duration::from_millis(5),
            recovery_shards_per_poll: crate::journal::RECOVERY_SHARDS,
            recovery_page_size: 16,
            idle_discard: Duration::from_secs(300),
            storage_max_object_bytes: 8,
            storage_max_session_bytes: 20,
            storage_max_tenant_bytes: 8,
            storage_transfer_ttl: Duration::from_secs(60 * 60),
            ..BrainConfig::default()
        };
        let compose = |owner: &str| {
            Brain::with_parts_and_services(
                cfg.clone(),
                journal.cloned_as(owner),
                Arc::new(crate::keys::PlainCustody),
                Arc::new(crate::adapter::DisabledToolExecutor),
                BrainServices {
                    session_storage: Some(storage.clone()),
                    ..BrainServices::default()
                },
                None,
            )
        };
        let crashed = compose("brain-storage-copy-owner-a");
        let created = crashed
            .create_session(
                serde_json::from_value(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "storage-copy-crash-test",
                        "api_key": "sk-test"
                    }
                }))
                .expect("valid copy crash create"),
                None,
            )
            .await
            .expect("create copy crash root");
        let session_id = created.id.to_string();
        let ticket = crashed
            .storage_prepare_upload(
                &session_id,
                crate::storage::StorageUploadIntent {
                    key: "copied.bin".into(),
                    bytes: 8,
                    sha256: Some("c".repeat(64)),
                    content_type: Some("application/octet-stream".into()),
                    overwrite: false,
                },
            )
            .await
            .expect("reserve copy crash upload");

        // The adapter publishes destination bytes, then the Brain process loses the response
        // before it can journal StorageUploadPublished.
        storage
            .staged
            .lock()
            .expect("staged uploads")
            .insert(ticket.transfer_id.clone());
        crate::storage::SessionStoragePort::complete_upload(
            storage.as_ref(),
            &session_id,
            &ticket.transfer_id,
        )
        .await
        .expect("simulate successful CopyObject with lost response");
        let mut expired = journal
            .cloned_as("brain-storage-copy-owner-a")
            .claim(&session_id)
            .await
            .expect("claim copied reservation");
        expired
            .doc
            .storage_upload
            .as_mut()
            .expect("copied reservation")
            .expires_at_ms = crate::wall_ms().saturating_sub(1);
        let mut lease = Lease {
            fence: expired.fence,
            last_seq: expired.last_seq,
            retention: expired.retention,
        };
        let expirer = journal.cloned_as("brain-storage-copy-owner-a");
        expirer
            .commit(&session_id, &mut lease, &[], &expired.doc, expired.last_seq)
            .await
            .expect("persist post-crash expired reservation");
        expirer
            .release(&session_id, &lease)
            .await
            .expect("release crashed owner");

        let recovering = compose("brain-storage-copy-owner-b");
        recovering.start_recovery_worker();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let head = journal.get_head(&session_id).await.unwrap();
                if head
                    .doc
                    .storage_upload
                    .as_ref()
                    .is_some_and(|upload| upload.state == "completed")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("due worker adopts copied bytes without customer traffic");

        let adopted = journal.get_head(&session_id).await.expect("adopted head");
        assert_eq!(adopted.doc.session_storage_bytes, 8);
        assert_eq!(adopted.doc.storage_reserved_bytes, 0);
        assert_eq!(adopted.doc.tenant_metered_storage_bytes, 8);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::StorageUploadPublished { .. }))
        );
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::StorageUploadCompleted { .. }))
        );
        assert!(
            !records
                .iter()
                .any(|entry| matches!(entry.record, Record::StorageUploadExpired { .. }))
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn storage_upload_reservation_is_durable_bounded_and_retried_after_restart() {
        let data_dir = std::env::temp_dir().join(format!(
            "brain-storage-reservation-{}-{}",
            std::process::id(),
            crate::wall_ms()
        ));
        std::fs::create_dir_all(&data_dir).expect("create storage reservation data dir");
        let journal = Journal::new_memory("brain-storage-reservation");
        let storage = Arc::new(ReservationStorage::new(journal.clone()));
        let cfg = BrainConfig {
            idle_discard: Duration::from_secs(300),
            storage_max_object_bytes: 8,
            storage_max_session_bytes: 20,
            storage_transfer_ttl: Duration::from_secs(60 * 60),
            ..BrainConfig::default()
        };
        let compose = |storage: Arc<ReservationStorage>| {
            Brain::with_parts_and_services(
                cfg.clone(),
                journal.clone(),
                Arc::new(crate::keys::PlainCustody),
                Arc::new(crate::adapter::DisabledToolExecutor),
                BrainServices {
                    session_storage: Some(storage),
                    customer_delivery: None,
                    customer_transport: None,
                    compactor: None,
                    ..BrainServices::default()
                },
                None,
            )
        };
        let brain = compose(storage.clone());
        let created = brain
            .create_session(
                serde_json::from_value(json!({
                    "model": {
                        "provider": "anthropic",
                        "name": "storage-test",
                        "api_key": "sk-test"
                    }
                }))
                .expect("valid storage test create"),
                None,
            )
            .await
            .expect("create storage test session");
        let session_id = created.id.to_string();
        let too_large = brain
            .storage_prepare_upload(
                &session_id,
                crate::storage::StorageUploadIntent {
                    key: "large.bin".into(),
                    bytes: 9,
                    sha256: Some("a".repeat(64)),
                    content_type: None,
                    overwrite: false,
                },
            )
            .await;
        assert!(matches!(
            too_large,
            Err(BrainError::StorageObjectTooLarge { limit: 8 })
        ));
        assert_eq!(storage.prepares.load(Ordering::Relaxed), 0);

        let ticket = brain
            .storage_prepare_upload(
                &session_id,
                crate::storage::StorageUploadIntent {
                    key: "bounded.bin".into(),
                    bytes: 8,
                    sha256: Some("b".repeat(64)),
                    content_type: Some("application/octet-stream".into()),
                    overwrite: false,
                },
            )
            .await
            .expect("reserve bounded upload");
        assert!(storage.saw_durable_reservation.load(Ordering::Relaxed));
        let reserved = journal.get_head(&session_id).await.expect("reserved head");
        assert_eq!(reserved.doc.session_storage_bytes, 0);
        assert_eq!(reserved.doc.storage_reserved_bytes, 8);
        assert_eq!(
            reserved
                .doc
                .storage_upload
                .as_ref()
                .map(|upload| upload.transfer_id.as_str()),
            Some(ticket.transfer_id.as_str())
        );
        let competing = brain
            .storage_prepare_upload(
                &session_id,
                crate::storage::StorageUploadIntent {
                    key: "other.bin".into(),
                    bytes: 1,
                    sha256: Some("c".repeat(64)),
                    content_type: None,
                    overwrite: false,
                },
            )
            .await;
        assert!(matches!(
            competing,
            Err(BrainError::StorageUploadInProgress { .. })
        ));

        // Simulate process loss after the reservation by advancing only its persisted deadline.
        let mut crashed = journal
            .claim(&session_id)
            .await
            .expect("claim expired upload");
        crashed
            .doc
            .storage_upload
            .as_mut()
            .expect("reservation")
            .expires_at_ms = crate::wall_ms().saturating_sub(1);
        let mut lease = Lease {
            fence: crashed.fence,
            last_seq: crashed.last_seq,
            retention: crashed.retention,
        };
        journal
            .commit(&session_id, &mut lease, &[], &crashed.doc, crashed.last_seq)
            .await
            .expect("persist expired deadline");
        journal
            .release(&session_id, &lease)
            .await
            .expect("release crashed storage owner");

        let restarted = compose(storage.clone());
        storage.fail_next_abort.store(true, Ordering::Relaxed);
        assert!(restarted.storage_reconcile(&session_id).await.is_err());
        let after_failure = journal
            .get_head(&session_id)
            .await
            .expect("head after abort failure");
        assert_eq!(after_failure.doc.storage_reserved_bytes, 8);
        assert!(after_failure.doc.storage_upload.is_some());
        restarted
            .storage_reconcile(&session_id)
            .await
            .expect("retry expired staging deletion");
        let after_retry = journal
            .get_head(&session_id)
            .await
            .expect("head after cleanup");
        assert_eq!(after_retry.doc.storage_reserved_bytes, 0);
        assert!(after_retry.doc.storage_upload.is_none());
        assert_eq!(storage.aborts.load(Ordering::Relaxed), 2);
        let storage_records = journal
            .read_records(&session_id, 0)
            .await
            .expect("storage journal");
        assert!(
            storage_records
                .iter()
                .any(|entry| matches!(entry.record, Record::StorageUploadExpired { .. }))
        );
        let gauges: Vec<_> = storage_records
            .iter()
            .filter_map(|entry| {
                crate::events::derive(&session_id, entry.seq, entry.ts_ms, &entry.record)
            })
            .filter_map(|event| match event {
                brain_protocol::session::Event::StorageUsage { storage, .. } => {
                    Some((storage.session_storage_bytes, storage.upload_reserved_bytes))
                }
                _ => None,
            })
            .collect();
        assert_eq!(gauges, vec![(0, 8), (0, 0)]);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
