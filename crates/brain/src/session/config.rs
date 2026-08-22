//! Process-level configuration: env vocabulary, bounds and `BrainConfig`.

use super::*;

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
    pub(crate) fn from_env_values(
        mut read: impl FnMut(&str) -> Result<Option<String>>,
    ) -> Result<Self> {
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

pub(super) fn validate_external_executor_config(cfg: &BrainConfig) -> Result<()> {
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
        crate::outbound::validate_external_executor_url(endpoint)?;
    }
    if let Some(token) = &cfg.external_executor_token
        && !crate::outbound::validate_bearer_token(token.expose())
    {
        return Err(BrainError::Invalid(
            "external tool executor token is not a valid HTTP bearer value".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_timeout(
    name: &str,
    value: Duration,
    minimum_ms: u64,
    maximum_ms: u64,
) -> Result<()> {
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
pub(super) fn parse_strict_env_u64(k: &str, raw: Option<&str>, default: u64) -> Result<u64> {
    parse_env_u64(k, raw, default, 0, u64::MAX)
}

pub(super) fn parse_env_u64(
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

pub(super) fn parse_env_usize(
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

pub(super) fn parse_env_bool(name: &str, raw: Option<&str>, default: bool) -> Result<bool> {
    match raw {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(BrainError::Invalid(format!(
            "{name} must be exactly true or false"
        ))),
    }
}

pub(super) fn parse_optional_env_string(
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

pub(super) fn parse_capabilities(raw: Option<String>) -> Result<HashSet<String>> {
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

pub(super) fn validate_usize_range(
    name: &str,
    value: usize,
    minimum: usize,
    maximum: usize,
) -> Result<()> {
    if !(minimum..=maximum).contains(&value) {
        return Err(BrainError::Invalid(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    Ok(())
}

/// Process-wide reclaim policy. Measurements showed that allocators retain dropped session memory
/// unless reclamation is requested explicitly.
pub(super) fn reclaim_policy() -> &'static crate::reclaim::ReclaimPolicy {
    static POLICY: std::sync::OnceLock<crate::reclaim::ReclaimPolicy> = std::sync::OnceLock::new();
    POLICY
        .get_or_init(|| crate::reclaim::ReclaimPolicy::new(crate::reclaim::DEFAULT_THRESHOLD_BYTES))
}

/// How turns obtain a provider. Overridable so tests can inject the scripted fake.
pub type ProviderFactory = Arc<dyn Fn(Dialect) -> Arc<dyn Provider> + Send + Sync>;
