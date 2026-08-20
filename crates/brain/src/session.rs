//! Sessions as spawned tasks (D9): one actor per resident session, hydrate-act-commit-discard.
//!
//! An idle session is nothing but its journal. The actor holds the cached fold (history,
//! head, lease, hand adapter); after `idle_discard` without traffic it releases the lease,
//! drops the adapter and exits -- the next message hydrates from the journal (PD-11 measured
//! the rehydrate at constant ~4 ms). Everything the actor holds is rebuildable; everything
//! durable went through `Journal::commit` first.
//!
//! The brain is COMPOSED, not configured into a cloud: [`Brain::with_parts`] takes a journal
//! store, a key custody, a hand factory and (optionally) a provider factory -- all trait
//! objects (see [`crate::adapter`]). [`Brain::local`] is the explicitly unsafe development
//! composition; durable standalone and cloud implementations live behind the same public ports.

use crate::adapter::{
    CallOutcome, DisabledToolExecutor, HandAdapter, HandFactory, HandSpec, SeedFile,
    TerminalOutcome, ToolBundleFile, ToolExecutor, WorkspaceFile, WorkspaceListing,
};
use crate::compact::DEFAULT_HISTORY_BUDGET_BYTES;
use crate::config::{AgentDef, Dialect, GenOpts, ProviderKey, SessionConfig};
use crate::events::EventHub;
use crate::journal::{
    ArtifactDoc, Entry, FailureDoc, Head, HeadDoc, Journal, Lease, PrefixDoc, Record,
};
use crate::keys::{KeyCustody, blob_from_b64, blob_to_b64};
use crate::local::LocalFactory;
use crate::message::{ContentBlock, Message};
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
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Process configuration: the knobs that are NOT adapters.
#[derive(Debug, Clone)]
pub struct BrainConfig {
    /// Admission: concurrent model rounds across the process.
    pub max_concurrent_model_rounds: usize,
    /// Admission: concurrent active turns across the process.
    pub max_concurrent_turns: usize,
    /// Idle residency before the actor discards its fold and exits.
    pub idle_discard: Duration,
    pub history_budget_bytes: usize,
    /// Whether brain-originated requests to user-controlled URLs (MCP) may reach loopback /
    /// private / link-local addresses. `false` is the production invariant (the SSRF guard,
    /// D14); [`Brain::local`] defaults it to `true` so a developer's MCP server on
    /// `127.0.0.1` works zero-config, and `brain-aws` refuses to start with `true`.
    pub outbound_allow_private: bool,
    /// Per-server budget for the create-time MCP probe + `tools/list`.
    pub mcp_create_timeout: Duration,
    /// Deadline for one MCP tool call.
    pub mcp_call_timeout: Duration,
    /// Bound on one MCP tool result as shown to the model.
    pub mcp_max_result_bytes: usize,
    /// Maximum bytes buffered by one public file upload or download.
    pub max_file_bytes: usize,
    /// Optional host executor shared by every external tool declaration. The URL and service
    /// credential are process configuration and never enter the sealed model prefix.
    pub external_executor_url: Option<String>,
    pub external_executor_token: Option<ProviderKey>,
    /// Stable capabilities registered by the HTTP executor. An empty set advertises none.
    pub external_executor_capabilities: HashSet<String>,
    pub external_call_timeout: Duration,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            max_concurrent_model_rounds: env_num("BRAIN_MAX_MODEL_ROUNDS", 64),
            max_concurrent_turns: env_num("BRAIN_MAX_TURNS", 64),
            idle_discard: Duration::from_secs(env_num("BRAIN_IDLE_DISCARD_SECONDS", 900) as u64),
            history_budget_bytes: DEFAULT_HISTORY_BUDGET_BYTES,
            outbound_allow_private: env_bool("BRAIN_OUTBOUND_ALLOW_PRIVATE", false),
            mcp_create_timeout: Duration::from_millis(
                env_num("BRAIN_MCP_CREATE_TIMEOUT_MS", 10_000) as u64,
            ),
            mcp_call_timeout: Duration::from_millis(
                env_num("BRAIN_MCP_CALL_TIMEOUT_MS", 60_000) as u64
            ),
            mcp_max_result_bytes: env_num("BRAIN_MCP_MAX_RESULT_BYTES", 128 * 1024),
            max_file_bytes: env_num("BRAIN_MAX_FILE_BYTES", 64 * 1024 * 1024),
            external_executor_url: std::env::var("BRAIN_EXTERNAL_TOOL_EXECUTOR_URL")
                .ok()
                .filter(|value| !value.is_empty()),
            external_executor_token: std::env::var("BRAIN_EXTERNAL_TOOL_EXECUTOR_TOKEN")
                .ok()
                .filter(|value| !value.is_empty())
                .map(ProviderKey::new),
            external_executor_capabilities: std::env::var("BRAIN_EXTERNAL_TOOL_CAPABILITIES")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect(),
            external_call_timeout: Duration::from_millis(env_num(
                "BRAIN_EXTERNAL_TOOL_TIMEOUT_MS",
                30_000,
            ) as u64),
        }
    }
}

fn env_num(k: &str, default: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_bool(k: &str, default: bool) -> bool {
    std::env::var(k)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Process-wide reclaim policy (PD-13: >=97% of a dropped session's memory returns with an
/// explicit `malloc_trim`; no allocator does it unprompted).
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
    pub hand_factory: Arc<dyn HandFactory>,
    pub hub: Arc<EventHub>,
    pub model_permits: Arc<Semaphore>,
    /// The D14 egress seam: every brain-originated request to a user-controlled URL (MCP)
    /// goes through this client and its SSRF guard.
    pub outbound: crate::outbound::Outbound,
    pub external_executor: Arc<dyn ToolExecutor>,
    pub attached: Arc<crate::attached::AttachedHub>,
    provider_factory: ProviderFactory,
    turn_permits: Arc<Semaphore>,
    sessions: Mutex<HashMap<String, mpsc::Sender<Command>>>,
}

fn hash_create_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

pub(crate) fn idempotent_session_id(key: &str) -> String {
    let hash = hash_create_key(key);
    format!("ses_{}", &hash[..24])
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
    Delete {
        reply: oneshot::Sender<Result<()>>,
    },
    Persist {
        name: String,
        path: String,
        media_type: Option<String>,
        reply: oneshot::Sender<Result<ArtifactDoc>>,
    },
    ListFiles {
        path: String,
        recursive: bool,
        reply: oneshot::Sender<Result<WorkspaceListing>>,
    },
    ReadFile {
        path: String,
        reply: oneshot::Sender<Result<WorkspaceFile>>,
    },
    WriteFile {
        path: String,
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<brain_protocol::session::FileEntry>>,
    },
    Snapshot {
        reply: oneshot::Sender<HeadDoc>,
    },
}

impl Brain {
    /// The general constructor: bring your own backends. This is the whole composition
    /// surface -- a custom substrate needs no core change.
    pub fn with_parts(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        hand_factory: Arc<dyn HandFactory>,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        let external_executor: Arc<dyn ToolExecutor> = match &cfg.external_executor_url {
            Some(endpoint) => Arc::new(crate::external::HttpExternalToolExecutor::new(
                endpoint.clone(),
                cfg.external_executor_token
                    .as_ref()
                    .map(|token| token.expose().to_string()),
                cfg.external_call_timeout,
                cfg.external_executor_capabilities.iter().cloned(),
            )),
            None => Arc::new(DisabledToolExecutor),
        };
        Self::with_parts_and_external(
            cfg,
            journal,
            custody,
            hand_factory,
            external_executor,
            provider_factory,
        )
    }

    /// General composition including a host-owned executor for sealed external tools.
    pub fn with_parts_and_external(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        hand_factory: Arc<dyn HandFactory>,
        external_executor: Arc<dyn ToolExecutor>,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        let outbound = crate::outbound::Outbound::new(cfg.outbound_allow_private);
        Arc::new(Self {
            model_permits: Arc::new(Semaphore::new(cfg.max_concurrent_model_rounds)),
            turn_permits: Arc::new(Semaphore::new(cfg.max_concurrent_turns)),
            journal,
            custody,
            hand_factory,
            provider_factory: provider_factory.unwrap_or_else(default_provider_factory),
            hub: Arc::new(EventHub::new()),
            sessions: Mutex::new(HashMap::new()),
            outbound,
            external_executor,
            attached: Arc::new(crate::attached::AttachedHub::default()),
            cfg,
        })
    }

    pub async fn attached_callbacks(&self, session_id: &str) -> Result<HashSet<String>> {
        let head = self.journal.get_head(session_id).await?;
        Ok(crate::tools::resolve(&head.doc.prefix.tools)?
            .into_iter()
            .filter_map(|tool| match tool.route {
                crate::config::ToolRoute::Attached { callback_id } => Some(callback_id),
                _ => None,
            })
            .collect())
    }

    /// The zero-setup composition: in-memory journal (NOT durable), local subprocess tools
    /// (NOT a sandbox), in-memory custody. Everything under `data_dir`.
    pub fn local(data_dir: impl Into<PathBuf>, cfg: BrainConfig) -> Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| BrainError::Invalid(format!("data dir: {e}")))?;
        // Local mode is permissive-by-default toward private addresses: a developer's MCP
        // server lives on 127.0.0.1. Setting BRAIN_OUTBOUND_ALLOW_PRIVATE explicitly wins;
        // this is a composition choice of THIS constructor, never of the guard itself.
        let mut cfg = cfg;
        if std::env::var("BRAIN_OUTBOUND_ALLOW_PRIVATE").is_err() {
            cfg.outbound_allow_private = true;
        }
        let owner = format!("brain-{}", crate::mint_id("i", 12));
        Ok(Self::with_parts(
            cfg,
            Journal::new_memory(owner),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(LocalFactory::new(data_dir)),
            None,
        ))
    }

    /// A retrievable URL for a persisted artifact, if the substrate can mint one.
    pub async fn artifact_url(&self, session_id: &str, doc: &ArtifactDoc) -> Option<String> {
        self.hand_factory
            .artifact_url(session_id, &doc.location)
            .await
    }

    async fn hand_spec(&self, session_id: &str, doc: &HeadDoc) -> Result<HandSpec> {
        let env = if doc.hand_secrets_b64.is_empty() {
            HashMap::new()
        } else {
            let blob = blob_from_b64(&doc.hand_secrets_b64)?;
            let secret = self.custody.decrypt(session_id, &blob).await?;
            serde_json::from_str(secret.expose())
                .map_err(|error| BrainError::Custody(format!("Hand environment: {error}")))?
        };
        Ok(HandSpec {
            session_id: session_id.to_string(),
            hand_enabled: doc.prefix.hand_enabled,
            shape: doc.prefix.shape.clone(),
            env,
            tool_manifest: doc.hand_manifest.clone(),
            manifest_digest: doc.manifest_digest.clone(),
        })
    }

    async fn open_adapter(&self, session_id: &str, doc: &HeadDoc) -> Result<Arc<dyn HandAdapter>> {
        let spec = self.hand_spec(session_id, doc).await?;
        self.hand_factory.open(&spec, doc.hand_state.clone()).await
    }

    // -- create ------------------------------------------------------------------------------

    pub async fn create_session(
        self: &Arc<Self>,
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
        let create_key_hash = idempotency_key.map(hash_create_key);
        let create_request_hash = create_key_hash
            .as_ref()
            .map(|_| {
                let canonical = serde_jcs::to_vec(&req)?;
                Ok::<_, BrainError>(hex::encode(Sha256::digest(canonical)))
            })
            .transpose()?;
        let session_id = idempotency_key
            .map(idempotent_session_id)
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
        let base_url = resolve_base_url(&req.model.provider, req.model.base_url.as_deref())?;
        if req.model.api_key.is_empty() {
            return Err(BrainError::Invalid(
                "model.api_key must not be empty".into(),
            ));
        }
        if req.metadata.len() > 16 {
            return Err(BrainError::Invalid("metadata: at most 16 pairs".into()));
        }
        let tools_cfg = req.tools.clone().unwrap_or_default();
        let tool_items = tools_cfg.items.clone();
        let decls = crate::tools::resolve(&tool_items)?;

        for decl in &decls {
            match &decl.route {
                crate::config::ToolRoute::Server(policy)
                    if !self.external_executor.supports(&policy.capability) =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} requires unavailable server capability {}",
                        decl.name, policy.capability
                    )));
                }
                crate::config::ToolRoute::Intrinsic(capability)
                    if !matches!(
                        capability.as_str(),
                        "brain.subagents" | "brain.subagents.v1"
                    ) =>
                {
                    return Err(BrainError::Invalid(format!(
                        "tool {} requires unavailable intrinsic capability {}",
                        decl.name, capability
                    )));
                }
                _ => {}
            }
        }

        // Resolve the declared MCP servers NOW: the tool list is sealed at create (§1.12),
        // so `tools/list` runs here, concurrently per server, and never again for this
        // session. Strict: an unreachable server fails the create.
        let mcp = if tools_cfg.mcp.is_empty() {
            crate::mcp::ResolvedMcp {
                servers: Vec::new(),
                tools: Vec::new(),
            }
        } else {
            crate::mcp::resolve_at_create(
                &self.outbound,
                &tools_cfg.mcp,
                self.cfg.mcp_create_timeout,
            )
            .await?
        };
        let mut tool_names = std::collections::HashSet::new();
        for name in crate::tools::names(&decls)
            .into_iter()
            .chain(mcp.tools.iter().map(|tool| tool.name.clone()))
        {
            if !tool_names.insert(name.clone()) {
                return Err(BrainError::Invalid(format!(
                    "tools: duplicate model-visible name {name:?}"
                )));
            }
        }
        // Per-server headers are credentials: custody, never the journal plaintext.
        let mcp_headers: HashMap<String, HashMap<String, String>> = tools_cfg
            .mcp
            .iter()
            .filter(|c| !c.headers.is_empty())
            .map(|c| (c.name.to_string(), c.headers.clone()))
            .collect();
        let mcp_secrets_b64 = if mcp_headers.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(&mcp_headers)?;
            let blob = self
                .custody
                .encrypt(&session_id, &ProviderKey::new(json))
                .await?;
            blob_to_b64(&blob)
        };
        let hand_cfg = req.hand.clone().unwrap_or_default();
        let shape = match hand_cfg.shape {
            None => "1gb".to_string(),
            Some(s) => format!("{s}"),
        };

        let hand_tools: Vec<_> = decls
            .iter()
            .filter_map(|decl| match &decl.route {
                crate::config::ToolRoute::Hand(seal) => Some((decl, seal)),
                _ => None,
            })
            .collect();
        if !hand_tools.is_empty() && !hand_cfg.enabled {
            return Err(BrainError::Invalid(
                "Hand tools require hand.enabled=true".into(),
            ));
        }
        for (decl, seal) in &hand_tools {
            let missing: Vec<_> = seal
                .required_env
                .iter()
                .filter(|key| !hand_cfg.env.contains_key(*key))
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

        // Verify bundle integrity before any journal write. Exact bytes are passed once to the
        // Hand factory for staging and are never included in PrefixDoc, HeadDoc, records, events,
        // logs, or model requests.
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
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}] exceeds 4 MiB"
                )));
            }
            total_bundle_bytes = total_bundle_bytes.saturating_add(bytes.len());
            if total_bundle_bytes > 16 * 1024 * 1024 {
                return Err(BrainError::Invalid(
                    "tool_bundles exceed the 16 MiB session limit".into(),
                ));
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
            .filter(|(_, seal)| seal.source == brain_protocol::session::HandToolSource::Bundle)
            .map(|(_, seal)| seal.checksum.as_str())
            .collect();
        for (_, seal) in &hand_tools {
            let supplied = bundle_checksums.contains(&seal.checksum);
            match (seal.source, supplied) {
                (brain_protocol::session::HandToolSource::Bundle, false) => {
                    return Err(BrainError::Invalid(format!(
                        "Hand bundle {} was not supplied",
                        seal.checksum
                    )));
                }
                (brain_protocol::session::HandToolSource::Preinstalled, true) => {
                    return Err(BrainError::Invalid(format!(
                        "preinstalled Hand tool unexpectedly supplied bundle {}",
                        seal.checksum
                    )));
                }
                _ => {}
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

        let mut hand_manifest = crate::tools::hand_manifest(&decls)?;
        for spec in &mut hand_manifest.tools {
            if spec.executable.source == brain_protocol::abi::ToolExecutableSource::Bundle {
                let bytes = decoded_bundles
                    .iter()
                    .find(|(checksum, _, _)| checksum == &spec.executable.checksum.to_string())
                    .map(|(_, bytes, _)| bytes.len() as u64)
                    .ok_or_else(|| {
                        BrainError::Invalid(format!(
                            "missing verified bundle for tool {}",
                            *spec.name
                        ))
                    })?;
                spec.executable.bytes = std::num::NonZeroU64::new(bytes);
            }
        }
        let manifest_digest = crate::tools::manifest_digest(&hand_manifest);

        // Decode seed files; staging is the adapter's business (S3, local disk, yours).
        let mut seed_bytes = Vec::with_capacity(req.files.len());
        for (i, f) in req.files.iter().enumerate() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&f.content_base64)
                .map_err(|e| BrainError::Invalid(format!("files[{i}].content_base64: {e}")))?;
            if bytes.len() > 1024 * 1024 {
                return Err(BrainError::Invalid(format!("files[{i}] exceeds 1 MiB")));
            }
            bytes_check_path(&f.path)?;
            seed_bytes.push((f.path.clone(), bytes, f.mode));
        }

        // Encrypt the BYOK key; the plaintext never reaches the journal.
        let key = ProviderKey::new(req.model.api_key.to_string());
        let blob = self.custody.encrypt(&session_id, &key).await?;
        let hand_secrets_b64 = if hand_cfg.env.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(&hand_cfg.env)?;
            let encrypted = self
                .custody
                .encrypt(&session_id, &ProviderKey::new(json))
                .await?;
            blob_to_b64(&encrypted)
        };
        let mut hand_env_keys: Vec<_> = hand_cfg.env.keys().cloned().collect();
        hand_env_keys.sort();

        let now = crate::wall_ms();
        let prefix = PrefixDoc {
            system_prompt: req.system_prompt.clone(),
            provider: provider.to_string(),
            model: req.model.name.to_string(),
            base_url: Some(base_url),
            max_output_tokens: req.model.max_output_tokens.map(|n| n.get()),
            temperature: req.model.temperature,
            reasoning_effort: req
                .model
                .reasoning_effort
                .as_ref()
                .map(|r| format!("{r:?}").to_lowercase()),
            tools: tool_items,
            mcp: mcp.servers,
            mcp_tools: mcp.tools,
            hand_enabled: hand_cfg.enabled,
            shape: shape.clone(),
            sync_interval_seconds: hand_cfg.sync_interval_seconds.max(60) as u64,
            hand_env_keys,
            metadata: req.metadata.clone(),
        };

        // The adapter stages seeds and validates the spec (an unsupported shape is refused
        // HERE, loudly, before anything is journaled).
        let spec = HandSpec {
            session_id: session_id.clone(),
            hand_enabled: prefix.hand_enabled,
            shape: prefix.shape.clone(),
            env: hand_cfg.env.clone(),
            tool_manifest: hand_manifest.clone(),
            manifest_digest: manifest_digest.clone(),
        };
        let seeds: Vec<SeedFile<'_>> = seed_bytes
            .iter()
            .map(|(path, bytes, mode)| SeedFile {
                path,
                bytes,
                mode: *mode,
            })
            .collect();
        let bundles: Vec<ToolBundleFile<'_>> = decoded_bundles
            .iter()
            .map(|(checksum, bytes, media_type)| ToolBundleFile {
                checksum,
                bytes,
                media_type,
            })
            .collect();
        let hand_state = self.hand_factory.create(&spec, &seeds, &bundles).await?;

        let doc = HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: now,
            updated_ms: now,
            create_key_hash: create_key_hash.clone(),
            create_request_hash: create_request_hash.clone(),
            last_message_ms: None,
            ended: false,
            prefix,
            key_b64: blob_to_b64(&blob),
            mcp_secrets_b64,
            hand_secrets_b64,
            hand_manifest,
            manifest_digest,
            hand_info: HeadDoc::initial_hand_info(&shape),
            hand_state,
            workspace_bytes: 0,
            artifacts: Vec::new(),
        };
        if let Err(error) = self
            .journal
            .create(
                &session_id,
                &doc,
                &Record::State {
                    state: "idle".into(),
                    turn: None,
                },
            )
            .await
        {
            if create_key_hash.is_some()
                && let Some(doc) = self
                    .replay_create(&session_id, &create_key_hash, &create_request_hash)
                    .await?
            {
                return Ok(doc);
            }
            return Err(error);
        }

        // Eager hand creation (D16): the actor starts now and readies the substrate without
        // the caller waiting.
        self.spawn_actor(&session_id, true).await;

        Ok(session_doc(&session_id, &doc))
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
        Ok(Some(session_doc(session_id, &head.doc)))
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
        Ok(self.spawn_actor(session_id, false).await)
    }

    async fn spawn_actor(
        self: &Arc<Self>,
        session_id: &str,
        freshly_created: bool,
    ) -> mpsc::Sender<Command> {
        let (tx, rx) = mpsc::channel(16);
        {
            let mut map = self.sessions.lock().expect("sessions lock");
            map.insert(session_id.to_string(), tx.clone());
        }
        let brain = self.clone();
        let sid = session_id.to_string();
        tokio::spawn(async move {
            actor(brain.clone(), sid.clone(), rx, freshly_created).await;
            let mut map = brain.sessions.lock().expect("sessions lock");
            if let Some(cur) = map.get(&sid)
                && cur.is_closed()
            {
                map.remove(&sid);
            }
        });
        tx
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
        Ok(session_doc(session_id, &doc))
    }

    pub async fn end(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        let doc = self
            .deliver(session_id, |reply| Command::End { reply })
            .await??;
        Ok(session_doc(session_id, &doc))
    }

    pub async fn delete(self: &Arc<Self>, session_id: &str) -> Result<()> {
        self.deliver(session_id, |reply| Command::Delete { reply })
            .await?
    }

    pub async fn persist(
        self: &Arc<Self>,
        session_id: &str,
        name: String,
        path: String,
        media_type: Option<String>,
    ) -> Result<ArtifactDoc> {
        self.deliver(session_id, |reply| Command::Persist {
            name: name.clone(),
            path: path.clone(),
            media_type: media_type.clone(),
            reply,
        })
        .await?
    }

    pub async fn list_files(
        self: &Arc<Self>,
        session_id: &str,
        path: String,
        recursive: bool,
    ) -> Result<WorkspaceListing> {
        let path = normalize_workspace_path(&path)?;
        self.deliver(session_id, |reply| Command::ListFiles {
            path: path.clone(),
            recursive,
            reply,
        })
        .await?
    }

    pub async fn read_file(
        self: &Arc<Self>,
        session_id: &str,
        path: String,
    ) -> Result<WorkspaceFile> {
        let path = normalize_workspace_path(&path)?;
        self.deliver(session_id, |reply| Command::ReadFile {
            path: path.clone(),
            reply,
        })
        .await?
    }

    pub async fn write_file(
        self: &Arc<Self>,
        session_id: &str,
        path: String,
        bytes: Vec<u8>,
    ) -> Result<brain_protocol::session::FileEntry> {
        let path = normalize_workspace_path(&path)?;
        if bytes.len() > self.cfg.max_file_bytes {
            return Err(BrainError::FileTooLarge {
                limit: self.cfg.max_file_bytes,
            });
        }
        self.deliver(session_id, |reply| Command::WriteFile {
            path: path.clone(),
            bytes: bytes.clone(),
            reply,
        })
        .await?
    }

    /// GET never hydrates: a resident actor answers from memory, otherwise the head is read
    /// straight from the journal.
    pub async fn get(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        if let Ok(tx) = self.sender(session_id).await {
            let (reply, rx) = oneshot::channel();
            if tx.send(Command::Snapshot { reply }).await.is_ok()
                && let Ok(doc) = rx.await
            {
                return Ok(session_doc(session_id, &doc));
            }
        }
        let head = self.journal.get_head(session_id).await?;
        if head.doc.state == "deleted" {
            return Err(BrainError::NoSuchSession(session_id.into()));
        }
        Ok(session_doc(session_id, &head.doc))
    }

    pub async fn list(self: &Arc<Self>, limit: usize) -> Result<Vec<session::Session>> {
        let heads = self.journal.list_sessions(limit).await?;
        Ok(heads
            .iter()
            .filter(|h| h.doc.state != "deleted")
            .map(|h| session_doc(&h.session_id, &h.doc))
            .collect())
    }

    pub async fn head(&self, session_id: &str) -> Result<Head> {
        self.journal.get_head(session_id).await
    }
}

fn bytes_check_path(path: &str) -> Result<()> {
    if path.is_empty() || path.contains("..") {
        return Err(BrainError::Invalid(format!(
            "files path {path:?} is not allowed"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------------------------

struct Resident {
    st: TurnState,
    key: ProviderKey,
    message_replays: HashMap<String, MessageReplay>,
}

struct Running {
    handle: tokio::task::JoinHandle<(TurnState, RunningOutcome)>,
    cancel: CancellationToken,
    key: ProviderKey,
    message_replays: HashMap<String, MessageReplay>,
}

enum RunningOutcome {
    Turn {
        turn_id: String,
        outcome: Result<crate::turn::TurnReport>,
    },
}

fn collect_message_replays(entries: &[Entry]) -> Result<HashMap<String, MessageReplay>> {
    let mut replays = HashMap::new();
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
    let policies: HashMap<String, crate::config::ServerToolPolicy> =
        crate::tools::resolve(&prefix.tools)
            .unwrap_or_default()
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

/// Finds unanswered calls whose executor cannot be recovered unambiguously. A newly claimed
/// session never reassigns these calls: Hand, attached, MCP and in-process intrinsic effects may
/// already have happened. Replay-safe server capabilities are handled separately.
fn pending_volatile(entries: &[Entry], prefix: &PrefixDoc) -> Vec<PendingVolatile> {
    let volatile_names: HashSet<String> = crate::tools::resolve(&prefix.tools)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| match tool.route {
            crate::config::ToolRoute::Server(_) => None,
            _ => Some(tool.name),
        })
        .chain(prefix.mcp_tools.iter().map(|tool| tool.name.clone()))
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
            } if volatile_names.contains(name) && !detach => {
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
    if pending.is_empty() {
        return Ok(None);
    }

    let rounds = entries
        .iter()
        .filter(|entry| {
            matches!(
                &entry.record,
                Record::Assistant { turn, agent, .. }
                    if turn == &active_turn && agent == "root"
            )
        })
        .count() as u64;
    let tool_calls = entries
        .iter()
        .filter(|entry| {
            matches!(
                &entry.record,
                Record::ToolCall { turn, agent, .. }
                    if turn == &active_turn && agent == "root"
            )
        })
        .count() as u64;
    let context = pending[0].context.clone();
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
        let outcome = match sealed_tools.iter().find(|tool| tool.name == call.name) {
            Some(tool) => crate::tools::enforce_output(tool, outcome),
            None => CallOutcome::failed(format!(
                "tool {} is absent from the recovered execution seal",
                call.name
            )),
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
        resident.st.head.state = "idle".into();
        resident.st.head.turn = None;
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
                state: "idle".into(),
                turn: Some(active_turn.clone()),
            },
        ));
    } else if unreplayable {
        resident.st.head.state = "idle".into();
        resident.st.head.turn = None;
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
                state: "idle".into(),
                turn: Some(active_turn.clone()),
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

fn task_identity_count(entries: &[Entry], prefix: &PrefixDoc) -> u64 {
    let intrinsic_names: HashSet<String> = crate::tools::resolve(&prefix.tools)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| match tool.route {
            crate::config::ToolRoute::Intrinsic(capability)
                if matches!(
                    capability.as_str(),
                    "brain.subagents" | "brain.subagents.v1"
                ) =>
            {
                Some(tool.name)
            }
            _ => None,
        })
        .collect();
    entries
        .iter()
        .filter(|entry| {
            matches!(
                &entry.record,
                Record::ToolCall { name, detach: false, .. } if intrinsic_names.contains(name)
            )
        })
        .count() as u64
}

async fn actor(
    brain: Arc<Brain>,
    session_id: String,
    mut rx: mpsc::Receiver<Command>,
    eager_hand: bool,
) {
    let mut resident: Option<Resident> = None;
    let mut running: Option<Running> = None;

    if eager_hand {
        // Eager hand creation, D16: ready the substrate without any caller waiting.
        match hydrate(&brain, &session_id).await {
            Ok(mut r) => {
                match r.st.hand.ensure_ready().await {
                    Ok(_) => {
                        let seq = r.st.take_seq();
                        let rec = Record::State {
                            state: r.st.head.state.clone(),
                            turn: None,
                        };
                        if commit(&brain, &session_id, &mut r.st, vec![(seq, rec)])
                            .await
                            .is_err()
                        {
                            tracing::warn!(session = %session_id, "eager-hand commit failed");
                        }
                        // Same idle rule as turn end: nothing held open between turns.
                        r.st.hand.idle();
                    }
                    Err(e) => {
                        tracing::warn!(session = %session_id, error = %e, "eager hand launch failed");
                        // Not fatal at create: the first tool call retries and the failure
                        // then lands on the turn.
                    }
                }
                resident = Some(r);
            }
            Err(e) => tracing::warn!(session = %session_id, error = %e, "eager hydrate failed"),
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
                    task.message_replays,
                    done,
                )
                .await;
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    Command::Snapshot { reply } => {
                        let doc = match &resident {
                            Some(r) => r.st.head.clone(),
                            None => match brain.journal.get_head(&session_id).await {
                                Ok(h) => h.doc,
                                Err(_) => { drop(reply); continue; }
                            },
                        };
                        let _ = reply.send(doc);
                    }
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
                                // Park the resident state into the turn task; the key rides
                                // the running tuple until the task returns the state.
                                let mut parked = resident.take().expect("resident");
                                let run = match turn_run(&brain, &session_id, &turn_id, &parked, metadata, cancel.clone()) {
                                    Ok(run) => run,
                                    Err(e) => {
                                        let _ = fail_turn_now(&brain, &session_id, &turn_id, &mut parked.st, &e).await;
                                        resident = Some(parked);
                                        continue;
                                    }
                                };
                                let key = parked.key.clone();
                                let message_replays = std::mem::take(&mut parked.message_replays);
                                let handle = tokio::spawn(async move {
                                    let _permit = permit; // held for the whole turn (admission)
                                    let mut st = parked.st;
                                    let out = run.run(&mut st).await;
                                    (st, RunningOutcome::Turn { turn_id: turn_id.clone(), outcome: out })
                                });
                                running = Some(Running {
                                    handle,
                                    cancel,
                                    key,
                                    message_replays,
                                });
                            }
                            Err(e) => { let _ = reply.send(Err(e)); }
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
                        if let Some(task) = running.take() {
                            task.cancel.cancel();
                            let key = task.key;
                            let message_replays = task.message_replays;
                            let done = task.handle.await;
                            resident = settle_running(
                                &brain,
                                &session_id,
                                key,
                                message_replays,
                                done,
                            )
                            .await;
                        }
                        match end_session(&brain, &session_id, &mut resident).await {
                            Ok(doc) => { let _ = reply.send(Ok(doc)); }
                            Err(e) => { let _ = reply.send(Err(e)); }
                        }
                    }
                    Command::Delete { reply } => {
                        if let Some(task) = running.take() {
                            task.cancel.cancel();
                            let _ = task.handle.await;
                            resident = None;
                        }
                        let out = delete_session(&brain, &session_id, &mut resident).await;
                        let _ = reply.send(out);
                        break; // the actor is done; the session is gone
                    }
                    Command::Persist { name, path, media_type, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_persist(&brain, &session_id, &mut resident, name, path, media_type).await;
                        let _ = reply.send(out);
                    }
                    Command::ListFiles { path, recursive, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_list_files(&brain, &session_id, &mut resident, path, recursive).await;
                        let _ = reply.send(out);
                    }
                    Command::ReadFile { path, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_read_file(&brain, &session_id, &mut resident, path).await;
                        let _ = reply.send(out);
                    }
                    Command::WriteFile { path, bytes, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_write_file(&brain, &session_id, &mut resident, path, bytes).await;
                        let _ = reply.send(out);
                    }
                }
            }
            _ = tokio::time::sleep(brain.cfg.idle_discard), if running.is_none() => {
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
                rx.close();
                if let Some(r) = resident.take() {
                    r.st.hand.idle();
                    // Best-effort teardown of any live legacy MCP sessions (v2 has no session
                    // to tear down; close() is a no-op). Never blocks the discard.
                    if let Some(mcp) = &r.st.mcp {
                        mcp.close().await;
                    }
                    let _ = brain.journal.release(&session_id, &r.st.lease).await;
                    let freed: usize = r.st.history.iter().map(|m| m.heap_bytes()).sum();
                    drop(r);
                    // PD-13: no allocator returns memory on drop without an explicit trim.
                    // The policy batches trims so a burst of discards pays one stall.
                    if reclaim_policy().freed(freed as u64).is_some() {
                        tracing::debug!(freed, "malloc_trim after session drop");
                    }
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
    message_replays: HashMap<String, MessageReplay>,
    done: std::result::Result<(TurnState, RunningOutcome), tokio::task::JoinError>,
) -> Option<Resident> {
    match done {
        Ok((st, outcome)) => {
            let mut resident = Resident {
                st,
                key,
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
    let mut entries = brain.journal.read_records(session_id, 0).await?;

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
                    state: "idle".into(),
                    turn: Some(turn),
                },
            ));
            next_seq += 1;
            head.doc.state = "idle".into();
            head.doc.turn = None;
        }
        head.doc.updated_ms = crate::wall_ms();

        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
        };
        let high_water = next_seq - 1;
        brain
            .journal
            .commit(session_id, &mut lease, &records, &head.doc, high_water)
            .await?;
        let now = crate::wall_ms();
        for (seq, record) in &records {
            if let Some(event) =
                crate::events::derive(session_id, *seq, now, record, &head.doc.hand_info)
            {
                brain.hub.publish(session_id, event);
            }
        }
        head.last_seq = lease.last_seq;
        entries = brain.journal.read_records(session_id, 0).await?;
    }

    let message_replays = collect_message_replays(&entries)?;
    let fold = crate::journal::fold(&entries);
    let key = brain
        .custody
        .decrypt(session_id, &blob_from_b64(&head.doc.key_b64)?)
        .await?;
    let mcp = build_mcp_runtime(brain, session_id, &head.doc).await?;
    let hand = brain.open_adapter(session_id, &head.doc).await?;
    // Every journaled subagent intent minted one child identity, including an
    // interrupted call. Rebuilding the count here makes the D11 lifetime cap
    // survive discard and process restart.
    let identities = task_identity_count(&entries, &head.doc.prefix);
    let mut resident = Resident {
        st: TurnState {
            history: fold.history,
            hand,
            head: head.doc,
            lease: Lease {
                fence: head.fence,
                last_seq: head.last_seq,
            },
            mcp,
            seq: Arc::new(std::sync::atomic::AtomicU64::new(head.last_seq + 1)),
            identities: Arc::new(std::sync::atomic::AtomicU64::new(identities)),
        },
        key,
        message_replays,
    };
    if let Some(recovered) =
        recover_external_calls(brain, session_id, &mut resident, &entries).await?
    {
        let permit = brain
            .turn_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| BrainError::Overloaded)?;
        let run = turn_run(
            brain,
            session_id,
            &recovered.turn,
            &resident,
            recovered.context,
            CancellationToken::new(),
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

/// Rebuilds the session's MCP dispatch state from the sealed prefix doc: decrypt the header
/// custody blob (if any), rebuild the connections. Zero network I/O -- the tool list was
/// sealed at create.
async fn build_mcp_runtime(
    brain: &Arc<Brain>,
    session_id: &str,
    doc: &HeadDoc,
) -> Result<Option<Arc<crate::mcp::McpRuntime>>> {
    if doc.prefix.mcp.is_empty() {
        return Ok(None);
    }
    let secrets: HashMap<String, HashMap<String, String>> = if doc.mcp_secrets_b64.is_empty() {
        HashMap::new()
    } else {
        let plain = brain
            .custody
            .decrypt(session_id, &blob_from_b64(&doc.mcp_secrets_b64)?)
            .await?;
        serde_json::from_str(plain.expose())
            .map_err(|e| BrainError::Custody(format!("mcp secrets blob is not valid: {e}")))?
    };
    Ok(Some(Arc::new(crate::mcp::McpRuntime::build(
        &brain.outbound,
        &doc.prefix.mcp,
        &doc.prefix.mcp_tools,
        &secrets,
        brain.cfg.mcp_call_timeout,
        brain.cfg.mcp_max_result_bytes,
    )?)))
}

async fn commit(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    records: Vec<(u64, Record)>,
) -> Result<()> {
    st.snapshot_hand();
    st.head.updated_ms = crate::wall_ms();
    let high_water = st
        .seq
        .load(std::sync::atomic::Ordering::Relaxed)
        .saturating_sub(1);
    let mut lease = st.lease.clone();
    brain
        .journal
        .commit(session_id, &mut lease, &records, &st.head, high_water)
        .await?;
    st.lease = lease;
    let now = crate::wall_ms();
    for (seq, record) in &records {
        if let Some(e) = crate::events::derive(session_id, *seq, now, record, &st.head.hand_info) {
            brain.hub.publish(session_id, e);
        }
    }
    Ok(())
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
    // The substrate may demand a release before more work (e.g. a platform lifetime wall):
    // release now; the turn's first call re-materialises through ensure_ready.
    if r.st.hand.must_release() {
        tracing::info!(session = %session_id, "substrate wall: releasing before the turn");
        let _ = r.st.hand.release().await;
        let seq = r.st.take_seq();
        let rec = Record::State {
            state: r.st.head.state.clone(),
            turn: None,
        };
        commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;
    }

    let turn_id = crate::mint_id("trn", 24);
    let user_seq = r.st.take_seq();
    let started_seq = r.st.take_seq();
    r.st.head.state = "active".into();
    r.st.head.turn = Some(turn_id.clone());
    r.st.head.turns += 1;
    r.st.head.last_message_ms = Some(crate::wall_ms());
    let records = vec![
        (
            user_seq,
            Record::UserMessage {
                turn: turn_id.clone(),
                content: content.clone(),
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

    // e.g. the speculative resume (F-4): substrate traffic now, hidden behind the model round.
    r.st.hand.on_message_admitted();

    Ok((turn_id, user_seq, CancellationToken::new()))
}

fn turn_run(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    r: &Resident,
    context: HashMap<String, String>,
    cancel: CancellationToken,
) -> Result<TurnRun> {
    let (prefix, dialect) = build_prefix(&r.st.head.prefix)?;
    let base_url = r.st.head.prefix.base_url.clone().unwrap_or_default();
    let session = SessionConfig::new(prefix.clone(), r.key.clone(), base_url);
    Ok(TurnRun {
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        prefix,
        session,
        provider: (brain.provider_factory)(dialect),
        provider_name: r.st.head.prefix.provider.clone(),
        journal: brain.journal.clone(),
        hub: brain.hub.clone(),
        cancel,
        model_permits: brain.model_permits.clone(),
        history_budget_bytes: brain.cfg.history_budget_bytes,
        external_executor: brain.external_executor.clone(),
        attached: brain.attached.clone(),
        context,
    })
}

/// Applies the turn outcome that `TurnRun::run` could not commit itself (failures), then the
/// turn-end checkpoint (the workspace durability point, D7).
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
    match st.hand.checkpoint().await {
        Ok(()) => {
            if let Err(e) = commit(brain, session_id, st, vec![]).await {
                tracing::warn!(session = %session_id, error = %e, "checkpoint commit failed");
            }
        }
        Err(e) => tracing::warn!(session = %session_id, error = %e, "turn-end checkpoint failed"),
    }
    // Nothing held open between turns: the substrate idles/suspends on its own clock.
    st.hand.idle();
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
        BrainError::SessionFailed(_) => ("session_failed", true),
        BrainError::Fenced => return Ok(()), // a newer owner exists; nothing to write
        _ => ("internal", false),
    };
    let failed_seq = st.take_seq();
    let state_seq = st.take_seq();
    if session_fatal {
        st.head.state = "failed".into();
        st.head.failure = Some(FailureDoc {
            code: "tool_manifest_mismatch".into(),
            message: e.to_string(),
            at_ms: crate::wall_ms(),
        });
    } else {
        st.head.state = "idle".into();
    }
    st.head.turn = None;
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
                turn: Some(turn_id.to_string()),
            },
        ),
    ];
    commit(brain, session_id, st, records).await
}

async fn end_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<HeadDoc> {
    let r = ensure_resident(brain, session_id, resident).await?;
    if !r.st.head.ended {
        // Checkpoint + release compute; keep the workspace, keep the journal. End is an
        // action, not a state.
        if let Err(e) = r.st.hand.checkpoint().await {
            tracing::warn!(session = %session_id, error = %e, "end checkpoint failed");
        }
        r.st.hand.release().await?;
        if let Some(mcp) = &r.st.mcp {
            mcp.close().await;
        }
        r.st.head.ended = true;
        r.st.head.state = "idle".into();
        let seq = r.st.take_seq();
        let rec = Record::State {
            state: "idle".into(),
            turn: None,
        };
        commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;
    }
    Ok(r.st.head.clone())
}

async fn delete_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<()> {
    let r = ensure_resident(brain, session_id, resident).await?;
    // Release compute without a checkpoint: the workspace is about to be deleted anyway.
    let _ = r.st.hand.release().await;
    if let Some(mcp) = &r.st.mcp {
        mcp.close().await;
    }
    r.st.head.state = "deleted".into();
    let seq = r.st.take_seq();
    let rec = Record::State {
        state: "deleted".into(),
        turn: None,
    };
    commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;

    // Purge storage (the adapter's), then the journal items. The state=deleted commit above
    // is the irreversible line; purge is cleanup.
    if let Err(e) = brain.hand_factory.purge(session_id).await {
        tracing::warn!(session = %session_id, error = %e, "substrate purge incomplete");
    }
    brain.journal.purge(session_id).await?;
    brain.hub.drop_session(session_id);
    *resident = None;
    Ok(())
}

async fn do_persist(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    name: String,
    path: String,
    media_type: Option<String>,
) -> Result<ArtifactDoc> {
    let r = ensure_resident(brain, session_id, resident).await?;
    r.st.hand.ensure_ready().await?;
    let meta =
        r.st.hand
            .persist(&name, &path, media_type.as_deref())
            .await?;
    let doc = ArtifactDoc {
        name: name.clone(),
        location: meta.location,
        bytes: meta.bytes,
        sha256: meta.sha256,
        media_type: meta.media_type,
        created_ms: crate::wall_ms(),
    };
    r.st.head.artifacts.retain(|a| a.name != name);
    r.st.head.artifacts.push(doc.clone());
    commit(brain, session_id, &mut r.st, vec![]).await?;
    // Same idle rule as turn end: nothing held open while nothing is running.
    r.st.hand.idle();
    Ok(doc)
}

async fn ready_for_file_operation(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    if let Some(lost) = st.hand.ensure_ready().await? {
        tracing::warn!(session = %session_id, reason = %lost.reason, "hand lost before file operation");
        let synced_ms = st
            .head
            .hand_info
            .last_sync_at
            .as_ref()
            .map(|t| t.0.timestamp_millis() as u64);
        let seq = st.take_seq();
        commit(
            brain,
            session_id,
            st,
            vec![(
                seq,
                Record::HandLost {
                    turn: None,
                    interrupted: vec![],
                    synced_ms,
                },
            )],
        )
        .await?;
    }
    Ok(())
}

async fn do_list_files(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    path: String,
    recursive: bool,
) -> Result<WorkspaceListing> {
    let r = ensure_resident(brain, session_id, resident).await?;
    // A released remote Hand can serve its committed manifest without waking compute.
    if r.st.hand.hand_info().state != brain_protocol::session::HandState::Released {
        ready_for_file_operation(brain, session_id, &mut r.st).await?;
    }
    let out = r.st.hand.list_files(&path, recursive).await?;
    commit(brain, session_id, &mut r.st, vec![]).await?;
    r.st.hand.idle();
    Ok(out)
}

async fn do_read_file(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    path: String,
) -> Result<WorkspaceFile> {
    let r = ensure_resident(brain, session_id, resident).await?;
    ready_for_file_operation(brain, session_id, &mut r.st).await?;
    let out = r.st.hand.read_file(&path, brain.cfg.max_file_bytes).await?;
    commit(brain, session_id, &mut r.st, vec![]).await?;
    r.st.hand.idle();
    Ok(out)
}

async fn do_write_file(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    path: String,
    bytes: Vec<u8>,
) -> Result<brain_protocol::session::FileEntry> {
    let r = ensure_resident(brain, session_id, resident).await?;
    ready_for_file_operation(brain, session_id, &mut r.st).await?;
    let out = r.st.hand.write_file(&path, &bytes).await?;
    // Upload acknowledgement is the durability boundary: checkpoint and commit the new
    // adapter manifest pointer before returning 200.
    r.st.hand.checkpoint().await?;
    commit(brain, session_id, &mut r.st, vec![]).await?;
    r.st.hand.idle();
    Ok(out)
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

/// Rebuilds the sealed prefix from the HEAD prefix doc. Deterministic: the same doc always
/// seals to the same digest.
pub fn build_prefix(
    p: &PrefixDoc,
) -> Result<(crate::Shared<crate::config::SealedPrefix>, Dialect)> {
    let dialect = dialect_of(&p.provider);
    let decls = crate::tools::resolve(&p.tools)?;
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
    // Discovered MCP tools render after native tools, in create order, from schemas the doc
    // carries -- no I/O, so the digest is a pure function of the doc (same doc, same digest).
    for t in &p.mcp_tools {
        def = def.tool(crate::config::ToolDecl {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
            output_schema: serde_json::json!({"type": ["object", "array", "string", "number", "boolean", "null"]}),
            route: crate::config::ToolRoute::Mcp {
                server: t.server.clone(),
                remote_name: t.remote_name.clone(),
            },
        });
    }
    for s in &p.mcp {
        def = def.mcp(crate::config::McpServerDecl {
            name: s.name.clone(),
            url: s.url.clone(),
            spec_version: s.spec_version.clone(),
        });
    }
    def = def.sampling(GenOpts {
        max_tokens: p.max_output_tokens.unwrap_or(4096) as u32,
        temperature: p.temperature.map(|t| t as f32),
        stop_sequences: Vec::new(),
    });
    Ok((def.seal(), dialect))
}

fn default_system_prompt() -> String {
    "You are an autonomous engineering agent running in an isolated Linux workspace \
     (/workspace, ARM64). Use the tools to inspect and change files and run commands. \
     Software installed system-wide is ephemeral; anything under /workspace and your home \
     directory persists for the session's life."
        .to_string()
}

/// Builds the contract Session document from the head.
pub fn session_doc(session_id: &str, doc: &HeadDoc) -> session::Session {
    session::Session {
        created_at: crate::events::ts(doc.created_ms),
        current_turn: doc.turn.as_deref().and_then(|t| t.parse().ok()),
        failure: doc.failure.as_ref().map(|f| session::SessionFailure {
            at: crate::events::ts(f.at_ms),
            code: match f.code.as_str() {
                "tool_manifest_mismatch" => session::SessionFailureCode::ToolManifestMismatch,
                "provider_unusable" => session::SessionFailureCode::ProviderUnusable,
                "hand_unavailable" => session::SessionFailureCode::HandUnavailable,
                _ => session::SessionFailureCode::Internal,
            },
            message: f.message.clone(),
        }),
        hand: doc.hand_info.clone(),
        id: session_id
            .parse()
            .unwrap_or_else(|_| "ses_00000000000000000000".parse().expect("fallback id")),
        last_message_at: doc.last_message_ms.map(crate::events::ts),
        metadata: doc.prefix.metadata.clone(),
        model: session::ModelInfo {
            base_url: doc.prefix.base_url.clone(),
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
        object: session::SessionObject::Session,
        state: crate::events::session_state(&doc.state),
        storage: session::StorageInfo {
            artifact_bytes: doc.artifacts.iter().map(|a| a.bytes).sum(),
            // Suspended-memory metering lands with billing (slice 4); until then this is an
            // honest zero: nothing is billed off it.
            suspended_bytes: 0,
            workspace_bytes: doc.workspace_bytes,
        },
        turns: doc.turns,
        updated_at: crate::events::ts(doc.updated_ms),
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
    use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecoveryExecutor {
        calls: AtomicUsize,
        call_ids: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ToolExecutor for RecoveryExecutor {
        fn supports(&self, capability: &str) -> bool {
            capability == "submit"
        }

        async fn call(
            &self,
            capability: &str,
            request: ExternalToolCallRequest,
            _cancel: CancellationToken,
        ) -> Result<ExternalToolCallResponse> {
            assert_eq!(capability, "submit");
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
    fn prefix_rebuild_is_deterministic() {
        let p = PrefixDoc {
            system_prompt: Some("sp".into()),
            provider: "anthropic".into(),
            model: "claude-x".into(),
            base_url: Some("https://api.anthropic.com".into()),
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            reasoning_effort: None,
            tools: serde_json::from_value(json!([
                {
                    "definition": {
                        "name":"run", "description":"run",
                        "input_schema":{"type":"object"},
                        "output_schema":{"type":"object"}
                    },
                    "executor": {
                        "kind":"hand", "protocol":1,
                        "checksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "source":"preinstalled", "required_env":[]
                    }
                },
                {
                    "definition": {
                        "name":"delegate", "description":"delegate",
                        "input_schema":{"type":"object"},
                        "output_schema":{"type":"string"}
                    },
                    "executor":{"kind":"intrinsic", "capability":"brain.subagents"}
                }
            ])).unwrap(),
            mcp: vec![],
            mcp_tools: vec![],
            hand_enabled: true,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        };
        let (a, da) = build_prefix(&p).unwrap();
        let (b, db) = build_prefix(&p).unwrap();
        assert_eq!(a.digest(), b.digest());
        assert_eq!(da, db);
        assert_eq!(a.tools.len(), 2);
    }

    #[test]
    fn dialects_route_by_provider() {
        assert_eq!(dialect_of("anthropic"), Dialect::AnthropicMessages);
        assert_eq!(dialect_of("deepseek"), Dialect::OpenAiChat);
        assert_eq!(dialect_of("openai"), Dialect::OpenAiChat);
    }

    #[test]
    fn pending_volatile_scan_routes_by_the_seal_and_tracks_subagent_identity() {
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
        ];
        let prefix = PrefixDoc {
            system_prompt: None,
            provider: "anthropic".into(),
            model: "m".into(),
            base_url: None,
            max_output_tokens: None,
            temperature: None,
            reasoning_effort: None,
            tools: serde_json::from_value(json!([{
                "definition": {
                    "name":"delegate_under_any_name", "description":"delegate",
                    "input_schema":{"type":"object"}, "output_schema":{"type":"string"}
                },
                "executor":{"kind":"intrinsic", "capability":"brain.subagents"}
            }]))
            .unwrap(),
            mcp: vec![],
            mcp_tools: vec![],
            hand_enabled: false,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        };
        let pending = pending_volatile(&entries, &prefix);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].call, "op_pending");
        assert_eq!(task_identity_count(&entries, &prefix), 2);
    }

    #[test]
    fn pending_external_scan_recovers_only_unanswered_sealed_calls() {
        let prefix = PrefixDoc {
            system_prompt: None,
            provider: "anthropic".into(),
            model: "m".into(),
            base_url: None,
            max_output_tokens: None,
            temperature: None,
            reasoning_effort: None,
            tools: serde_json::from_value(json!([{
                "definition": {
                    "name":"submit", "description":"submit",
                    "input_schema":{"type":"object"},
                    "output_schema":{"type":"object"}
                },
                "executor": {
                    "kind":"server", "capability":"submit", "scope":"root",
                    "completion":"return_direct", "effect":"replay_safe",
                    "max_input_bytes":1024
                }
            }]))
            .unwrap(),
            mcp: vec![],
            mcp_tools: vec![],
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
                ..BrainConfig::default()
            },
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(LocalFactory::new(data_dir.clone())),
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
                    "hand": {"enabled": false},
                    "tools": {
                        "items": [{
                            "definition": {
                                "name": "submit",
                                "description": "Submit the final value",
                                "input_schema": {"type": "object"},
                                "output_schema": {"type": "object"}
                            },
                            "executor": {
                                "kind": "server",
                                "capability": "submit",
                                "scope": "root",
                                "completion": "return_direct",
                                "effect": "replay_safe",
                                "max_input_bytes": 1024
                            }
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
        head.doc.state = "active".into();
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
        };
        journal
            .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 3)
            .await
            .expect("commit pending external intent");
        let crash_entries = journal
            .read_records(&session_id, 0)
            .await
            .expect("read simulated crash records");
        let resolved = crate::tools::resolve(&head.doc.prefix.tools).expect("resolve sealed tools");
        assert!(
            resolved.iter().any(|tool| {
                tool.name == "submit"
                    && matches!(
                        &tool.route,
                        crate::config::ToolRoute::Server(policy) if policy.capability == "submit"
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
        assert_eq!(pending[0].policy.capability, "submit");
        journal
            .release(&session_id, &lease)
            .await
            .expect("release crashed owner");

        let resident = hydrate(&brain, &session_id)
            .await
            .expect("hydrate and replay pending call");
        assert_eq!(executor.calls.load(Ordering::Relaxed), 1);
        assert_eq!(resident.st.head.state, "idle");
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
}
