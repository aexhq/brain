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
//! objects (see [`crate::adapter`]). [`Brain::local`] is the zero-setup composition; the AWS
//! one lives in `brain-aws`; yours is whatever you hand in.

use crate::adapter::{
    HandAdapter, HandFactory, HandSpec, SeedFile, WorkspaceFile, WorkspaceListing,
};
use crate::compact::DEFAULT_HISTORY_BUDGET_BYTES;
use crate::config::{AgentDef, Dialect, GenOpts, ProviderKey, SealedPrefix, SessionConfig};
use crate::events::EventHub;
use crate::journal::{
    ArtifactDoc, Entry, FailureDoc, Head, HeadDoc, Journal, Lease, OutputRequestDoc, PrefixDoc,
    Record,
};
use crate::keys::{KeyCustody, blob_from_b64, blob_to_b64};
use crate::local::LocalFactory;
use crate::message::{ContentBlock, Message};
use crate::provider::Provider;
use crate::tools::TodoState;
use crate::turn::{TurnRun, TurnState};
use crate::{BrainError, Result};
use aex_contracts::session::{
    self, CreateSessionRequest, MessageRequestContent, OutputRequest, OutputRequestInput,
    Provider as ApiProvider,
};
use base64::Engine;
use std::collections::HashMap;
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
    /// Serper's fixed managed-search endpoint and redacted process credential.
    pub web_search_endpoint: String,
    pub web_search_api_key: Option<ProviderKey>,
    pub web_call_timeout: Duration,
    pub web_max_result_bytes: usize,
    pub web_search_max_response_bytes: usize,
    pub web_fetch_max_response_bytes: usize,
    pub web_fetch_max_chars: usize,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            max_concurrent_model_rounds: env_num("AEX_MAX_MODEL_ROUNDS", 64),
            max_concurrent_turns: env_num("AEX_MAX_TURNS", 64),
            idle_discard: Duration::from_secs(env_num("AEX_IDLE_DISCARD_SECONDS", 900) as u64),
            history_budget_bytes: DEFAULT_HISTORY_BUDGET_BYTES,
            outbound_allow_private: env_bool("AEX_OUTBOUND_ALLOW_PRIVATE", false),
            mcp_create_timeout: Duration::from_millis(
                env_num("AEX_MCP_CREATE_TIMEOUT_MS", 10_000) as u64
            ),
            mcp_call_timeout: Duration::from_millis(
                env_num("AEX_MCP_CALL_TIMEOUT_MS", 60_000) as u64
            ),
            mcp_max_result_bytes: env_num("AEX_MCP_MAX_RESULT_BYTES", 128 * 1024),
            max_file_bytes: env_num("AEX_MAX_FILE_BYTES", 64 * 1024 * 1024),
            web_search_endpoint: std::env::var("AEX_WEB_SEARCH_ENDPOINT")
                .unwrap_or_else(|_| "https://google.serper.dev/search".into()),
            web_search_api_key: std::env::var("SERPER_API_KEY")
                .ok()
                .filter(|value| !value.is_empty())
                .map(ProviderKey::new),
            web_call_timeout: Duration::from_millis(
                env_num("AEX_WEB_CALL_TIMEOUT_MS", 30_000) as u64
            ),
            web_max_result_bytes: env_num("AEX_WEB_MAX_RESULT_BYTES", 128 * 1024),
            web_search_max_response_bytes: env_num(
                "AEX_WEB_SEARCH_MAX_RESPONSE_BYTES",
                1024 * 1024,
            ),
            web_fetch_max_response_bytes: env_num(
                "AEX_WEB_FETCH_MAX_RESPONSE_BYTES",
                5 * 1024 * 1024,
            ),
            web_fetch_max_chars: env_num("AEX_WEB_FETCH_MAX_CHARS", 50_000),
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
    pub web: Arc<crate::web::WebRuntime>,
    provider_factory: ProviderFactory,
    turn_permits: Arc<Semaphore>,
    sessions: Mutex<HashMap<String, mpsc::Sender<Command>>>,
}

enum Command {
    Message {
        content: Vec<ContentBlock>,
        reply: oneshot::Sender<Result<(String, u64)>>, // (turn_id, seq of the user message)
    },
    Output {
        schema: serde_json::Value,
        schema_hash: String,
        request_hash: String,
        idempotency_key_hash: Option<String>,
        input: Option<Vec<ContentBlock>>,
        reply: oneshot::Sender<Result<OutputAdmission>>,
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
        reply: oneshot::Sender<Result<aex_contracts::session::FileEntry>>,
    },
    Snapshot {
        reply: oneshot::Sender<HeadDoc>,
    },
}

#[derive(Debug, Clone)]
pub struct OutputAdmission {
    pub output_id: String,
    pub schema_hash: String,
    pub seq: u64,
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
        let outbound = crate::outbound::Outbound::new(cfg.outbound_allow_private);
        let web = Arc::new(crate::web::WebRuntime::new(
            outbound.clone(),
            cfg.web_search_endpoint.clone(),
            cfg.web_search_api_key.clone(),
            cfg.web_call_timeout,
            cfg.web_max_result_bytes,
            cfg.web_search_max_response_bytes,
            cfg.web_fetch_max_response_bytes,
            cfg.web_fetch_max_chars,
        ));
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
            web,
            cfg,
        })
    }

    /// The zero-setup composition: in-memory journal (NOT durable), local subprocess tools
    /// (NOT a sandbox), in-memory custody. Everything under `data_dir`.
    pub fn local(data_dir: impl Into<PathBuf>, cfg: BrainConfig) -> Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| BrainError::Invalid(format!("data dir: {e}")))?;
        // Local mode is permissive-by-default toward private addresses: a developer's MCP
        // server lives on 127.0.0.1. Setting AEX_OUTBOUND_ALLOW_PRIVATE explicitly wins;
        // this is a composition choice of THIS constructor, never of the guard itself.
        let mut cfg = cfg;
        if std::env::var("AEX_OUTBOUND_ALLOW_PRIVATE").is_err() {
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

    fn hand_spec(session_id: &str, prefix: &PrefixDoc, manifest_digest: &str) -> HandSpec {
        HandSpec {
            session_id: session_id.to_string(),
            hand_enabled: prefix.hand_enabled,
            shape: prefix.shape.clone(),
            env: prefix.env.clone(),
            manifest_digest: manifest_digest.to_string(),
        }
    }

    async fn open_adapter(&self, session_id: &str, doc: &HeadDoc) -> Result<Arc<dyn HandAdapter>> {
        let spec = Self::hand_spec(session_id, &doc.prefix, &doc.manifest_digest);
        self.hand_factory.open(&spec, doc.hand_state.clone()).await
    }

    // -- create ------------------------------------------------------------------------------

    pub async fn create_session(
        self: &Arc<Self>,
        req: CreateSessionRequest,
    ) -> Result<session::Session> {
        let session_id = crate::mint_id("ses", 24);

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
        let builtins = tools_cfg
            .builtin
            .clone()
            .unwrap_or_else(crate::tools::default_builtins);
        if builtins
            .iter()
            .any(|tool| matches!(tool, aex_contracts::session::BuiltinTool::WebSearch))
            && self.cfg.web_search_api_key.is_none()
        {
            return Err(BrainError::Invalid(
                "tool web_search requires the managed search credential on this plane".into(),
            ));
        }
        let decls = crate::tools::resolve(&builtins)?;

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
            tools: crate::tools::names(&decls),
            mcp: mcp.servers,
            mcp_tools: mcp.tools,
            hand_enabled: hand_cfg.enabled,
            shape: shape.clone(),
            sync_interval_seconds: hand_cfg.sync_interval_seconds.max(60) as u64,
            env: hand_cfg.env.clone(),
            metadata: req.metadata.clone(),
        };
        let manifest_digest = crate::tools::manifest_digest();

        // The adapter stages seeds and validates the spec (an unsupported shape is refused
        // HERE, loudly, before anything is journaled).
        let spec = Self::hand_spec(&session_id, &prefix, &manifest_digest);
        let seeds: Vec<SeedFile<'_>> = seed_bytes
            .iter()
            .map(|(path, bytes, mode)| SeedFile {
                path,
                bytes,
                mode: *mode,
            })
            .collect();
        let hand_state = self.hand_factory.create(&spec, &seeds).await?;

        let doc = HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: now,
            updated_ms: now,
            last_message_ms: None,
            ended: false,
            prefix,
            key_b64: blob_to_b64(&blob),
            mcp_secrets_b64,
            manifest_digest,
            hand_info: HeadDoc::initial_hand_info(&shape),
            hand_state,
            workspace_bytes: 0,
            artifacts: Vec::new(),
            output_requests: Vec::new(),
        };
        self.journal
            .create(
                &session_id,
                &doc,
                &Record::State {
                    state: "idle".into(),
                    turn: None,
                },
            )
            .await?;

        // Eager hand creation (D16): the actor starts now and readies the substrate without
        // the caller waiting.
        self.spawn_actor(&session_id, true).await;

        Ok(session_doc(&session_id, &doc))
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
        let blocks = content_blocks(content)?;
        self.deliver(session_id, |reply| Command::Message {
            content: blocks.clone(),
            reply,
        })
        .await?
    }

    pub async fn output(
        self: &Arc<Self>,
        session_id: &str,
        req: OutputRequest,
        idempotency_key: Option<&str>,
    ) -> Result<OutputAdmission> {
        if let Some(key) = idempotency_key
            && (key.is_empty() || key.len() > 128)
        {
            return Err(BrainError::Invalid(
                "Idempotency-Key must contain 1 to 128 bytes".into(),
            ));
        }
        let request_hash = crate::output::jcs_sha256(&req)?;
        let schema_hash = req.schema_hash.to_string();
        let schema = serde_json::Value::Object(req.schema.0);
        crate::output::validate_schema(&schema, &schema_hash)?;
        let input = req.input.map(output_content_blocks).transpose()?;
        let idempotency_key_hash = idempotency_key.map(crate::output::jcs_sha256).transpose()?;
        self.deliver(session_id, |reply| Command::Output {
            schema: schema.clone(),
            schema_hash: schema_hash.clone(),
            request_hash: request_hash.clone(),
            idempotency_key_hash: idempotency_key_hash.clone(),
            input: input.clone(),
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
    ) -> Result<aex_contracts::session::FileEntry> {
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
}

struct Running {
    handle: tokio::task::JoinHandle<(TurnState, RunningOutcome)>,
    cancel: CancellationToken,
    key: ProviderKey,
}

enum RunningOutcome {
    Turn {
        turn_id: String,
        outcome: Result<crate::turn::TurnReport>,
    },
    Output {
        output_id: String,
        outcome: Result<OutputJobReport>,
    },
}

#[derive(Debug)]
struct OutputJobReport {
    completed: bool,
}

struct OutputRuntime {
    prefix: Arc<SealedPrefix>,
    session: SessionConfig,
    provider: Arc<dyn Provider>,
}

enum OutputAdmit {
    Replay(OutputAdmission),
    Started {
        admission: OutputAdmission,
        turn_id: Option<String>,
        cancel: CancellationToken,
    },
}

#[derive(Debug)]
struct PendingTask {
    seq: u64,
    turn: String,
    agent: String,
    call: String,
}

#[derive(Debug)]
struct PendingOutput {
    seq: u64,
    output: String,
    turn: Option<String>,
    schema_hash: String,
}

fn pending_outputs(entries: &[Entry]) -> Vec<PendingOutput> {
    let mut pending = HashMap::<String, PendingOutput>::new();
    for entry in entries {
        match &entry.record {
            Record::OutputStarted {
                output,
                turn,
                schema_hash,
                ..
            } => {
                pending.insert(
                    output.clone(),
                    PendingOutput {
                        seq: entry.seq,
                        output: output.clone(),
                        turn: turn.clone(),
                        schema_hash: schema_hash.clone(),
                    },
                );
            }
            Record::OutputCompleted { output, .. } | Record::OutputFailed { output, .. } => {
                pending.remove(output);
            }
            _ => {}
        }
    }
    let mut pending: Vec<_> = pending.into_values().collect();
    pending.sort_by_key(|output| output.seq);
    pending
}

/// Finds `task` intents with no result. Once a new owner can claim the
/// session, the in-process child that owned each intent is gone and must never
/// be replayed.
fn pending_tasks(entries: &[Entry]) -> Vec<PendingTask> {
    let mut pending = HashMap::<String, PendingTask>::new();
    for entry in entries {
        match &entry.record {
            Record::ToolCall {
                turn,
                agent,
                call,
                name,
                detach,
                ..
            } if name == "task" && !detach => {
                pending.insert(
                    call.clone(),
                    PendingTask {
                        seq: entry.seq,
                        turn: turn.clone(),
                        agent: agent.clone(),
                        call: call.clone(),
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
    pending.sort_by_key(|task| task.seq);
    pending
}

fn task_identity_count(entries: &[Entry]) -> u64 {
    entries
        .iter()
        .filter(|entry| {
            matches!(
                &entry.record,
                Record::ToolCall { name, detach: false, .. } if name == "task"
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
                resident = settle_running(&brain, &session_id, task.key, done).await;
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
                    Command::Message { content, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let r = match ensure_resident(&brain, &session_id, &mut resident).await {
                            Ok(r) => r,
                            Err(e) => { let _ = reply.send(Err(e)); continue; }
                        };
                        let permit = match brain.turn_permits.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => { let _ = reply.send(Err(BrainError::Overloaded)); continue; }
                        };
                        match admit(&brain, &session_id, r, content).await {
                            Ok((turn_id, seq, cancel)) => {
                                let _ = reply.send(Ok((turn_id.clone(), seq)));
                                // Park the resident state into the turn task; the key rides
                                // the running tuple until the task returns the state.
                                let mut parked = resident.take().expect("resident");
                                let run = match turn_run(&brain, &session_id, &turn_id, &parked, cancel.clone()) {
                                    Ok(run) => run,
                                    Err(e) => {
                                        let _ = fail_turn_now(&brain, &session_id, &turn_id, &mut parked.st, &e).await;
                                        resident = Some(parked);
                                        continue;
                                    }
                                };
                                let key = parked.key.clone();
                                let handle = tokio::spawn(async move {
                                    let _permit = permit; // held for the whole turn (admission)
                                    let mut st = parked.st;
                                    let out = run.run(&mut st).await;
                                    (st, RunningOutcome::Turn { turn_id: turn_id.clone(), outcome: out })
                                });
                                running = Some(Running { handle, cancel, key });
                            }
                            Err(e) => { let _ = reply.send(Err(e)); }
                        }
                    }
                    Command::Output {
                        schema,
                        schema_hash,
                        request_hash,
                        idempotency_key_hash,
                        input,
                        reply,
                    } => {
                        if running.is_some() {
                            let replay = match brain.journal.get_head(&session_id).await {
                                Ok(head) => replay_output(
                                    &head.doc,
                                    idempotency_key_hash.as_deref(),
                                    &request_hash,
                                ),
                                Err(error) => Err(error),
                            };
                            match replay {
                                Ok(Some(admission)) => { let _ = reply.send(Ok(admission)); }
                                Ok(None) => { let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone()))); }
                                Err(error) => { let _ = reply.send(Err(error)); }
                            }
                            continue;
                        }
                        let resident_ref = match ensure_resident(&brain, &session_id, &mut resident).await {
                            Ok(resident) => resident,
                            Err(error) => { let _ = reply.send(Err(error)); continue; }
                        };
                        // Replays are reads of an already-admitted identity. They must not need
                        // provider configuration or a fresh global turn permit.
                        match replay_output(
                            &resident_ref.st.head,
                            idempotency_key_hash.as_deref(),
                            &request_hash,
                        ) {
                            Ok(Some(admission)) => {
                                let _ = reply.send(Ok(admission));
                                continue;
                            }
                            Ok(None) => {}
                            Err(error) => {
                                let _ = reply.send(Err(error));
                                continue;
                            }
                        }
                        let runtime = match output_runtime(&brain, resident_ref) {
                            Ok(runtime) => runtime,
                            Err(error) => { let _ = reply.send(Err(error)); continue; }
                        };
                        let permit = match brain.turn_permits.clone().try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => { let _ = reply.send(Err(BrainError::Overloaded)); continue; }
                        };
                        let admitted = admit_output(
                            &brain,
                            &session_id,
                            resident_ref,
                            &schema_hash,
                            &request_hash,
                            idempotency_key_hash.as_deref(),
                            input,
                        ).await;
                        let OutputAdmit::Started { admission, turn_id, cancel } = (match admitted {
                            Ok(OutputAdmit::Replay(admission)) => {
                                let _ = reply.send(Ok(admission));
                                continue;
                            }
                            Ok(started) => started,
                            Err(error) => { let _ = reply.send(Err(error)); continue; }
                        }) else { unreachable!("replay handled above") };

                        let _ = reply.send(Ok(admission.clone()));
                        let parked = resident.take().expect("resident");
                        let work_run = turn_id.as_ref().map(|turn_id| TurnRun {
                            session_id: session_id.clone(),
                            turn_id: turn_id.clone(),
                            prefix: runtime.prefix.clone(),
                            session: runtime.session.clone(),
                            provider: runtime.provider.clone(),
                            provider_name: parked.st.head.prefix.provider.clone(),
                            journal: brain.journal.clone(),
                            hub: brain.hub.clone(),
                            cancel: cancel.clone(),
                            model_permits: brain.model_permits.clone(),
                            history_budget_bytes: brain.cfg.history_budget_bytes,
                            web: brain.web.clone(),
                        });
                        let key = parked.key.clone();
                        let output_id = admission.output_id.clone();
                        let brain_for_job = brain.clone();
                        let session_for_job = session_id.clone();
                        let schema_hash_for_job = schema_hash.clone();
                        let cancel_for_job = cancel.clone();
                        let handle = tokio::spawn(async move {
                            let _permit = permit;
                            let mut state = parked.st;
                            let outcome = run_output_job(
                                &brain_for_job,
                                &session_for_job,
                                &output_id,
                                &schema_hash_for_job,
                                turn_id,
                                schema,
                                runtime,
                                work_run,
                                cancel_for_job,
                                &mut state,
                            ).await;
                            (
                                state,
                                RunningOutcome::Output {
                                    output_id,
                                    outcome,
                                },
                            )
                        });
                        running = Some(Running { handle, cancel, key });
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
                            let done = task.handle.await;
                            resident = settle_running(&brain, &session_id, key, done).await;
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
    done: std::result::Result<(TurnState, RunningOutcome), tokio::task::JoinError>,
) -> Option<Resident> {
    match done {
        Ok((st, outcome)) => {
            let mut resident = Resident { st, key };
            match outcome {
                RunningOutcome::Turn { turn_id, outcome } => {
                    finish_turn(brain, session_id, &turn_id, &mut resident.st, outcome).await;
                }
                RunningOutcome::Output { output_id, outcome } => {
                    match outcome {
                        Ok(report) => tracing::info!(
                            session = %session_id,
                            output = %output_id,
                            completed = report.completed,
                            "output done"
                        ),
                        Err(error) => tracing::error!(
                            session = %session_id,
                            output = %output_id,
                            error = %error,
                            "output task could not commit its terminal event"
                        ),
                    }
                    match resident.st.hand.checkpoint().await {
                        Ok(()) => {
                            if let Err(error) =
                                commit(brain, session_id, &mut resident.st, vec![]).await
                            {
                                tracing::warn!(session = %session_id, error = %error, "output checkpoint commit failed");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(session = %session_id, error = %error, "output checkpoint failed")
                        }
                    }
                    resident.st.hand.idle();
                }
            }
            Some(resident)
        }
        Err(join_error) => {
            tracing::error!(session = %session_id, error = %join_error, "session task panicked");
            // The fold moved into the task and is gone. Rehydrate immediately so an admitted
            // output gets a durable interrupted terminal instead of leaving its Promise open
            // until some unrelated future request happens to touch the session.
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

    // A successfully claimed session has no live previous owner. Any task
    // intent without a result belonged to an in-process child that disappeared
    // with that owner: answer it as interrupted, never replay it. Nested task
    // results remain audit-only because Fold excludes non-root agents.
    let interrupted = pending_tasks(&entries);
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
                    name: "task".into(),
                    outcome: "interrupted".into(),
                    content: "subagent interrupted while the session was not resident".into(),
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

    // An output commit is a model-side effect and must never be replayed after ownership was
    // lost. Close any admitted request that lacks a terminal event as interrupted; an
    // idempotent retry then replays this same failure instead of asking the model twice.
    let interrupted_outputs = pending_outputs(&entries);
    if !interrupted_outputs.is_empty() {
        let mut next_seq = head.last_seq + 1;
        let mut records = Vec::with_capacity(interrupted_outputs.len() * 2 + 1);
        for output in &interrupted_outputs {
            records.push((
                next_seq,
                Record::OutputFailed {
                    output: output.output.clone(),
                    turn: output.turn.clone(),
                    schema_hash: output.schema_hash.clone(),
                    code: "cancelled".into(),
                    message:
                        "output interrupted when its previous owner stopped; it was not replayed"
                            .into(),
                    issues: Vec::new(),
                    usage: crate::message::Usage::default(),
                },
            ));
            next_seq += 1;
            if let Some(turn) = &output.turn
                && head.doc.turn.as_deref() == Some(turn)
            {
                records.push((
                    next_seq,
                    Record::TurnFailed {
                        turn: turn.clone(),
                        code: "cancelled".into(),
                        message: "turn interrupted with its output request; it was not replayed"
                            .into(),
                    },
                ));
                next_seq += 1;
            }
        }
        head.doc.state = "idle".into();
        let previous_turn = head.doc.turn.take();
        records.push((
            next_seq,
            Record::State {
                state: "idle".into(),
                turn: previous_turn,
            },
        ));
        head.doc.updated_ms = crate::wall_ms();
        let high_water = next_seq;
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
        };
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

    let fold = crate::journal::fold(&entries);
    let key = brain
        .custody
        .decrypt(session_id, &blob_from_b64(&head.doc.key_b64)?)
        .await?;
    let mcp = build_mcp_runtime(brain, session_id, &head.doc).await?;
    let hand = brain.open_adapter(session_id, &head.doc).await?;
    // Every journaled `task` intent minted one child identity, including an
    // interrupted call. Rebuilding the count here makes the D11 lifetime cap
    // survive discard and process restart.
    let identities = task_identity_count(&entries);
    Ok(Resident {
        st: TurnState {
            history: fold.history,
            hand,
            head: head.doc,
            lease: Lease {
                fence: head.fence,
                last_seq: head.last_seq,
            },
            todo: Arc::new(TodoState::default()),
            mcp,
            seq: Arc::new(std::sync::atomic::AtomicU64::new(head.last_seq + 1)),
            identities: Arc::new(std::sync::atomic::AtomicU64::new(identities)),
        },
        key,
    })
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

fn output_runtime(brain: &Arc<Brain>, resident: &Resident) -> Result<OutputRuntime> {
    let (prefix, dialect) = build_prefix(&resident.st.head.prefix)?;
    let base_url = resident.st.head.prefix.base_url.clone().unwrap_or_default();
    let session = SessionConfig::new(prefix.clone(), resident.key.clone(), base_url);
    Ok(OutputRuntime {
        prefix,
        session,
        provider: (brain.provider_factory)(dialect),
    })
}

fn replay_output(
    doc: &HeadDoc,
    idempotency_key_hash: Option<&str>,
    request_hash: &str,
) -> Result<Option<OutputAdmission>> {
    let Some(key_hash) = idempotency_key_hash else {
        return Ok(None);
    };
    let Some(previous) = doc
        .output_requests
        .iter()
        .find(|request| request.key_hash == key_hash)
    else {
        return Ok(None);
    };
    if previous.request_hash != request_hash {
        return Err(BrainError::IdempotencyConflict);
    }
    Ok(Some(OutputAdmission {
        output_id: previous.output_id.clone(),
        schema_hash: previous.schema_hash.clone(),
        seq: previous.started_seq,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn admit_output(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    schema_hash: &str,
    request_hash: &str,
    idempotency_key_hash: Option<&str>,
    input: Option<Vec<ContentBlock>>,
) -> Result<OutputAdmit> {
    if let Some(admission) = replay_output(&resident.st.head, idempotency_key_hash, request_hash)? {
        return Ok(OutputAdmit::Replay(admission));
    }
    if resident.st.head.state == "failed" {
        return Err(BrainError::SessionFailed(
            resident
                .st
                .head
                .failure
                .as_ref()
                .map(|failure| failure.message.clone())
                .unwrap_or_default(),
        ));
    }

    let now = crate::wall_ms();
    resident
        .st
        .head
        .output_requests
        .retain(|request| now.saturating_sub(request.admitted_ms) <= 24 * 60 * 60 * 1000);

    if input.is_some() && resident.st.hand.must_release() {
        let _ = resident.st.hand.release().await;
        let seq = resident.st.take_seq();
        let record = Record::State {
            state: resident.st.head.state.clone(),
            turn: None,
        };
        commit(brain, session_id, &mut resident.st, vec![(seq, record)]).await?;
    }

    let source_seq = resident.st.lease.last_seq;
    let output_id = crate::mint_id("out", 24);
    let turn_id = input.as_ref().map(|_| crate::mint_id("trn", 24));
    let mut records = Vec::with_capacity(if input.is_some() { 3 } else { 1 });

    if let (Some(content), Some(turn)) = (input.as_ref(), turn_id.as_ref()) {
        records.push((
            resident.st.take_seq(),
            Record::UserMessage {
                turn: turn.clone(),
                content: content.clone(),
            },
        ));
        records.push((
            resident.st.take_seq(),
            Record::TurnStarted { turn: turn.clone() },
        ));
        resident.st.head.turn = Some(turn.clone());
        resident.st.head.turns += 1;
        resident.st.head.last_message_ms = Some(now);
    }
    let started_seq = resident.st.take_seq();
    records.push((
        started_seq,
        Record::OutputStarted {
            output: output_id.clone(),
            turn: turn_id.clone(),
            schema_hash: schema_hash.to_string(),
            source_seq,
        },
    ));
    resident.st.head.state = "active".into();

    if let Some(key_hash) = idempotency_key_hash {
        if resident.st.head.output_requests.len() >= 256 {
            resident.st.head.output_requests.remove(0);
        }
        resident.st.head.output_requests.push(OutputRequestDoc {
            key_hash: key_hash.to_string(),
            request_hash: request_hash.to_string(),
            output_id: output_id.clone(),
            schema_hash: schema_hash.to_string(),
            started_seq,
            admitted_ms: now,
        });
    }
    commit(brain, session_id, &mut resident.st, records).await?;

    if let Some(content) = input {
        if let Some(last) = resident.st.history.last_mut()
            && last.role == crate::message::Role::User
            && !last.content.is_empty()
            && last
                .content
                .iter()
                .all(|block| matches!(block, ContentBlock::ToolResult { .. }))
        {
            last.content.extend(content);
        } else {
            resident.st.history.push(Message {
                role: crate::message::Role::User,
                content,
            });
        }
        resident.st.hand.on_message_admitted();
    }

    Ok(OutputAdmit::Started {
        admission: OutputAdmission {
            output_id,
            schema_hash: schema_hash.to_string(),
            seq: started_seq,
        },
        turn_id,
        cancel: CancellationToken::new(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_output_job(
    brain: &Arc<Brain>,
    session_id: &str,
    output_id: &str,
    schema_hash: &str,
    turn_id: Option<String>,
    schema: serde_json::Value,
    runtime: OutputRuntime,
    work_run: Option<TurnRun>,
    cancel: CancellationToken,
    state: &mut TurnState,
) -> Result<OutputJobReport> {
    let work_report = if let Some(run) = work_run {
        match run.run_work(state).await {
            Ok(report) => Some(report),
            Err(error) => {
                return finish_output_failure(
                    brain,
                    session_id,
                    state,
                    output_id,
                    schema_hash,
                    turn_id,
                    None,
                    error,
                    Vec::new(),
                    crate::message::Usage::default(),
                    true,
                )
                .await;
            }
        }
    } else {
        None
    };

    if work_report
        .as_ref()
        .is_some_and(|report| report.stop_reason == "refusal")
    {
        let message = last_assistant_text(&state.history)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "the provider refused the output request".into());
        return finish_output_failure(
            brain,
            session_id,
            state,
            output_id,
            schema_hash,
            turn_id,
            work_report,
            BrainError::OutputRefused(message),
            Vec::new(),
            crate::message::Usage::default(),
            false,
        )
        .await;
    }

    if work_report
        .as_ref()
        .is_some_and(|report| report.stop_reason == "cancelled")
    {
        return finish_output_failure(
            brain,
            session_id,
            state,
            output_id,
            schema_hash,
            turn_id,
            work_report,
            BrainError::Cancelled,
            Vec::new(),
            crate::message::Usage::default(),
            false,
        )
        .await;
    }

    let context = crate::output::CommitContext {
        provider: runtime.provider,
        prefix: runtime.prefix,
        session: runtime.session,
        history: state.history.clone(),
        model_permits: brain.model_permits.clone(),
        cancel,
    };
    match crate::output::commit(context, schema).await {
        Ok(success) => {
            let output_seq = state.take_seq();
            let mut records = vec![(
                output_seq,
                Record::OutputCompleted {
                    output: output_id.to_string(),
                    turn: turn_id.clone(),
                    schema_hash: schema_hash.to_string(),
                    value: success.value.clone(),
                    usage: success.usage,
                },
            )];
            append_turn_terminal(
                state,
                &mut records,
                turn_id.as_deref(),
                work_report.as_ref(),
            );
            state.head.state = "idle".into();
            state.head.turn = None;
            let state_seq = state.take_seq();
            records.push((
                state_seq,
                Record::State {
                    state: "idle".into(),
                    turn: turn_id.clone(),
                },
            ));
            commit(brain, session_id, state, records).await?;
            append_output_history(state, schema_hash, success.value);
            Ok(OutputJobReport { completed: true })
        }
        Err(failure) => {
            finish_output_failure(
                brain,
                session_id,
                state,
                output_id,
                schema_hash,
                turn_id,
                work_report,
                failure.error,
                failure.issues,
                failure.usage,
                false,
            )
            .await
        }
    }
}

fn append_turn_terminal(
    state: &TurnState,
    records: &mut Vec<(u64, Record)>,
    turn_id: Option<&str>,
    report: Option<&crate::turn::TurnReport>,
) {
    if let (Some(turn), Some(report)) = (turn_id, report) {
        records.push((
            state.take_seq(),
            Record::TurnCompleted {
                turn: turn.to_string(),
                stop_reason: report.stop_reason.clone(),
                rounds: report.rounds,
                tool_calls: report.tool_calls,
            },
        ));
    }
}

#[allow(clippy::too_many_arguments)]
async fn finish_output_failure(
    brain: &Arc<Brain>,
    session_id: &str,
    state: &mut TurnState,
    output_id: &str,
    schema_hash: &str,
    turn_id: Option<String>,
    work_report: Option<crate::turn::TurnReport>,
    error: BrainError,
    issues: Vec<crate::output::ValidationIssue>,
    usage: crate::message::Usage,
    work_failed: bool,
) -> Result<OutputJobReport> {
    let message = error.to_string();
    let code = output_error_code(&error).to_string();
    let mut records = vec![(
        state.take_seq(),
        Record::OutputFailed {
            output: output_id.to_string(),
            turn: turn_id.clone(),
            schema_hash: schema_hash.to_string(),
            code,
            message: message.clone(),
            issues,
            usage,
        },
    )];
    if work_failed {
        if let Some(turn) = turn_id.as_ref() {
            records.push((
                state.take_seq(),
                Record::TurnFailed {
                    turn: turn.clone(),
                    code: turn_error_code(&error).into(),
                    message,
                },
            ));
        }
    } else {
        append_turn_terminal(
            state,
            &mut records,
            turn_id.as_deref(),
            work_report.as_ref(),
        );
    }
    state.head.state = "idle".into();
    state.head.turn = None;
    records.push((
        state.take_seq(),
        Record::State {
            state: "idle".into(),
            turn: turn_id,
        },
    ));
    commit(brain, session_id, state, records).await?;
    Ok(OutputJobReport { completed: false })
}

fn output_error_code(error: &BrainError) -> &'static str {
    match error {
        BrainError::OutputSchema(_) => "output_schema_error",
        BrainError::OutputRefused(_) => "output_refused",
        BrainError::OutputValidation(_) => "output_validation_error",
        BrainError::Cancelled => "cancelled",
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => {
            "provider_error"
        }
        BrainError::HandUnavailable(_) | BrainError::Hand(_) => "hand_unavailable",
        BrainError::Overloaded => "rate_limited",
        BrainError::SessionDeleted(_) | BrainError::SessionFailed(_) => "session_failed",
        _ => "internal",
    }
}

fn turn_error_code(error: &BrainError) -> &'static str {
    match error {
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => {
            "provider_error"
        }
        BrainError::HandUnavailable(_) | BrainError::Hand(_) => "hand_unavailable",
        BrainError::SessionFailed(_) => "session_failed",
        BrainError::Cancelled => "cancelled",
        _ => "internal",
    }
}

fn append_output_history(state: &mut TurnState, schema_hash: &str, value: serde_json::Value) {
    let block = ContentBlock::Output {
        schema_hash: schema_hash.to_string(),
        value,
    };
    if let Some(last) = state.history.last_mut()
        && last.role == crate::message::Role::Assistant
    {
        last.content.push(block);
    } else {
        state.history.push(Message::assistant(vec![block]));
    }
}

fn last_assistant_text(history: &[Message]) -> Option<String> {
    let message = history.last()?;
    (message.role == crate::message::Role::Assistant).then(|| {
        message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>()
    })
}

fn turn_run(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    r: &Resident,
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
        web: brain.web.clone(),
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
    // A released Lambda hand can serve its committed manifest without waking compute.
    if r.st.hand.hand_info().state != aex_contracts::session::HandState::Released {
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
) -> Result<aex_contracts::session::FileEntry> {
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
    let builtins: Vec<_> = p
        .tools
        .iter()
        .filter_map(|n| crate::tools::parse_builtin(n))
        .collect();
    let decls = crate::tools::resolve(&builtins)?;
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
    // Sealed MCP tools render after the builtins, in create order, from the schemas the doc
    // carries -- no I/O, so the digest is a pure function of the doc (same doc, same digest).
    for t in &p.mcp_tools {
        def = def.tool(crate::config::ToolDecl {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
            route: crate::config::ToolRoute::Mcp,
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

fn output_content_blocks(content: OutputRequestInput) -> Result<Vec<ContentBlock>> {
    match content {
        OutputRequestInput::String(value) => {
            let value = value.to_string();
            if value.is_empty() {
                return Err(BrainError::Invalid("input must not be empty".into()));
            }
            Ok(vec![ContentBlock::text(value)])
        }
        OutputRequestInput::Array(parts) => {
            if parts.is_empty() {
                return Err(BrainError::Invalid("input must not be empty".into()));
            }
            let mut blocks = Vec::with_capacity(parts.len());
            for part in parts {
                match part {
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
            tools: vec!["bash".into(), "todo".into()],
            mcp: vec![],
            mcp_tools: vec![],
            hand_enabled: true,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            env: HashMap::new(),
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
    fn pending_task_scan_tracks_results_and_the_lifetime_count() {
        let task = |seq: u64, agent: &str, call: &str, detach: bool| Entry {
            seq,
            ts_ms: 0,
            record: Record::ToolCall {
                turn: "trn_test".into(),
                agent: agent.into(),
                call: call.into(),
                name: "task".into(),
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
                    name: "task".into(),
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
        let pending = pending_tasks(&entries);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].call, "op_pending");
        assert_eq!(task_identity_count(&entries), 2);
    }

    #[test]
    fn pending_output_scan_never_replays_a_model_side_effect() {
        let started = |seq: u64, output: &str| Entry {
            seq,
            ts_ms: 0,
            record: Record::OutputStarted {
                output: output.into(),
                turn: None,
                schema_hash: "0".repeat(64),
                source_seq: seq.saturating_sub(1),
            },
        };
        let entries = vec![
            started(4, "out_interrupted"),
            started(5, "out_done"),
            Entry {
                seq: 6,
                ts_ms: 0,
                record: Record::OutputCompleted {
                    output: "out_done".into(),
                    turn: None,
                    schema_hash: "0".repeat(64),
                    value: serde_json::json!({"ok":true}),
                    usage: crate::message::Usage::default(),
                },
            },
        ];
        let pending = pending_outputs(&entries);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].output, "out_interrupted");
        assert_eq!(pending[0].seq, 4);
    }
}
