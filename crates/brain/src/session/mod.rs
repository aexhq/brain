//! Sessions as spawned tasks: one actor per resident session, hydrate-act-commit-discard.
//!
//! An idle session is nothing but its journal. The actor holds the cached fold (history,
//! head and lease); after `idle_discard` without traffic it releases the lease and exits -- the
//! next message hydrates from the journal (measured at roughly
//! 4 ms). Everything the actor holds is rebuildable; everything
//! durable went through `Journal::commit` first.
//!
//! The brain is COMPOSED, not configured into a cloud: [`Brain::with_parts`] takes a journal
//! store, key custody, typed Environment services and (optionally) a provider factory -- all trait
//! objects (see [`crate::adapter`]). Durable local and cloud implementations live behind the
//! same public ports.

use crate::adapter::{CallOutcome, DisabledToolExecutor, ToolExecutor, TurnTerminal};
use crate::compact::{
    DEFAULT_CONTEXT_HARD_TOKENS, DEFAULT_CONTEXT_SOFT_TOKENS, DEFAULT_CONTEXT_TAIL_TOKENS,
};
use crate::config::{AgentDef, Dialect, GenOpts, OutputTokenParameter, ProviderKey, SessionConfig};
use crate::environment::{
    managed_environment_resources, map_environment_port_error, sealed_sandbox_network,
};
use crate::events::EventHub;
use crate::journal::{
    ContextForkDoc, DELETION_TOMBSTONE_TTL_MS, DeletionState, DeletionStatusDoc, Entry, FailureDoc,
    Head, HeadDoc, Journal, Lease, PrefixDoc, ProviderAttemptState, Record, SessionLifecycle,
    StorageDeleteReservationDoc, StorageUploadReservationDoc, TurnPhase, TurnStopReason,
    UploadReservationState,
};
use crate::keys::{KeyCustody, blob_from_b64, blob_to_b64, validate_custody_plaintext};
use crate::message::{ContentBlock, Message, Role};
use crate::provider::Provider;
use crate::turn::{TurnRun, TurnState};
use crate::{BrainError, Result};
use base64::Engine;
use brain_protocol::environment::TerminalOutcome;
use brain_protocol::session::ToolOutcome;
use brain_protocol::session::{self, CreateSessionRequest, MessageRequestContent};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Notify, OnceCell, Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

mod view;
pub use view::*;

mod storage_state;
use storage_state::*;

#[path = "engine/subagents.rs"]
mod engine_subagents;

mod config;
pub use config::*;
mod recovery;
use recovery::*;

/// The supervisor: the composed parts and the resident-session map.
pub struct Brain {
    pub cfg: BrainConfig,
    pub journal: Journal,
    pub custody: Arc<dyn KeyCustody>,
    pub hub: Arc<EventHub>,
    pub model_permits: Arc<Semaphore>,
    /// Guarded transport used for user-controlled provider base URLs.
    pub outbound: crate::outbound::OutboundPolicy,
    pub external_executor: Arc<dyn ToolExecutor>,
    /// Resolves each session's sealed selector to the loop implementation driving its turns.
    pub agentloop_registry: Arc<dyn crate::agentloop::AgentloopRegistry>,
    /// Resolves each session's sealed selector to its Model component.
    pub model_registry: Arc<dyn crate::provider::ModelRegistry>,
    /// Admits and invokes precompiled Tool components. It may be absent only when no component
    /// Tool is declared.
    pub tool_registry: Option<Arc<dyn crate::tools::ToolRegistry>>,
    /// Admits and invokes generic precompiled Environment components.
    pub component_environment_registry:
        Option<Arc<dyn crate::environment::ComponentEnvironmentRegistry>>,
    /// Durable per-session object storage. Hosted composition supplies this adapter; `None` means
    /// the storage resource is unavailable, never an in-memory production fallback.
    pub session_storage: Option<Arc<dyn crate::storage::SessionStoragePort>>,
    pub bundle_storage: Option<Arc<dyn crate::storage::BundleStoragePort>>,
    pub environments: crate::environment::EnvironmentRegistry,
    /// Hosted customer-app delivery (for example API Gateway Management API). Absence means
    /// customer-app Tools are unavailable; Brain never silently routes them elsewhere.
    pub customer_delivery: Option<Arc<dyn crate::customer::CustomerEnvironmentDeliveryPort>>,
    /// Customer-app connection/receipt coordinator. Present only when the composition supplied
    /// absolute socket and observation callback URLs.
    pub customer: Option<Arc<crate::customer::CustomerCoordinator>>,
    pub compactor: Arc<dyn crate::compact::CompactionPort>,
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
    environment_env: HashMap<String, String>,
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
    environment_id: String,
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
    environment_name: String,
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
    Completed(brain_protocol::environment::FileEntry),
    Ambiguous,
}

#[derive(Default)]
pub struct BrainServices {
    pub session_storage: Option<Arc<dyn crate::storage::SessionStoragePort>>,
    pub bundle_storage: Option<Arc<dyn crate::storage::BundleStoragePort>>,
    pub environments: crate::environment::EnvironmentRegistry,
    pub customer_delivery: Option<Arc<dyn crate::customer::CustomerEnvironmentDeliveryPort>>,
    pub customer_transport: Option<crate::customer::CustomerTransportConfig>,
    pub compactor: Option<Arc<dyn crate::compact::CompactionPort>>,
    /// Selector-to-loop resolution. Compositions must supply it explicitly.
    pub agentloop_registry: Option<Arc<dyn crate::agentloop::AgentloopRegistry>>,
    /// Selector-to-Model resolution. Production compositions must supply it explicitly.
    pub model_registry: Option<Arc<dyn crate::provider::ModelRegistry>>,
    /// Precompiled Tool component admission and invocation.
    pub tool_registry: Option<Arc<dyn crate::tools::ToolRegistry>>,
    /// Precompiled Environment component admission and invocation.
    pub component_environment_registry:
        Option<Arc<dyn crate::environment::ComponentEnvironmentRegistry>>,
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

struct DecodedToolBundle {
    checksum: String,
    bytes: usize,
    target: brain_protocol::session::ToolBundleTarget,
    execute_path: String,
    setup_path: Option<String>,
    layers: Vec<DecodedToolLayer>,
}

struct DecodedToolLayer {
    checksum: String,
    bytes: Vec<u8>,
    media_type: brain_protocol::session::ToolArtifactLayerRefMediaType,
    mount_path: String,
    unpack: brain_protocol::session::ToolArtifactLayerRefUnpack,
}

fn managed_bundle_descriptors(
    environment_tools: &[(
        &crate::config::ToolDecl,
        &crate::config::EnvironmentToolSeal,
    )],
    decoded_bundles: &[DecodedToolBundle],
) -> Result<Vec<brain_protocol::environment::BundleDescriptor>> {
    environment_tools
        .iter()
        .map(|(decl, seal)| {
            let bundle = decoded_bundles
                .iter()
                .find(|bundle| bundle.checksum == seal.checksum)
                .ok_or_else(|| {
                    BrainError::Invalid(format!(
                        "missing verified bundle for managed Tool {}",
                        decl.name
                    ))
                })?;
            serde_json::from_value(serde_json::json!({
                "bundle_digest": seal.checksum,
                "bytes": bundle.bytes,
                "contract_digest": decl.contract_digest,
                "description": (!decl.description.is_empty()).then_some(decl.description.as_str()),
                "layers": bundle.layers.iter().map(|layer| serde_json::json!({
                    "digest": layer.checksum,
                    "bytes": layer.bytes.len(),
                    "media_type": layer.media_type,
                    "mount_path": layer.mount_path,
                    "unpack": layer.unpack,
                    "object": {
                        "bytes": layer.bytes.len(),
                        "media_type": layer.media_type.to_string(),
                        "object_id": format!("bundle_{}", layer.checksum),
                        "sha256": layer.checksum,
                    },
                })).collect::<Vec<_>>(),
                "required_env": seal.required_env,
                "target": bundle.target,
                "execute_path": bundle.execute_path,
                "setup_path": bundle.setup_path,
                "tool_name": decl.name,
                "environment_name": seal.environment,
            }))
            .map_err(BrainError::from)
        })
        .collect()
}

fn bundle_object_matches_descriptor(
    object: &brain_protocol::environment::ObjectReference,
    descriptor: &brain_protocol::environment::ArtifactLayerDescriptor,
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

fn sealed_managed_binding(
    session_id: &str,
    doc: &HeadDoc,
    descriptor: &brain_protocol::environment::BundleDescriptor,
    declaration: &brain_protocol::session::EnvironmentConfig,
) -> Result<brain_protocol::environment::SealedBinding> {
    let declaration = legacy_environment(declaration)?;
    let network = sealed_sandbox_network(doc)?;
    let resources = managed_environment_resources()?;
    let policy_digest = brain_protocol::contract::canonical_digest(&serde_json::json!({
        "network": network,
        "required_env": descriptor.required_env,
        "resources": resources,
    }))?;
    let binding_identity = brain_protocol::contract::canonical_digest(&serde_json::json!({
        "bundle": descriptor,
        "environment": declaration,
        "root_id": doc.root_id,
        "session_id": session_id,
    }))?;
    let implementation_identity = brain_protocol::contract::canonical_digest(declaration)?;
    serde_json::from_value(serde_json::json!({
        "binding_id": format!("bnd_{}", &binding_identity.as_str()[..24]),
        "bundle": descriptor,
        "capability": descriptor.tool_name,
        "contract_digest": descriptor.contract_digest,
        "implementation_identity": implementation_identity,
        "extension": declaration.extension,
        "protocol": declaration.protocol,
        "profile": declaration.profile,
        "configuration": declaration.configuration,
        "policy_digest": policy_digest,
        "environment_name": descriptor.environment_name,
        "required_capabilities": ["execution", "session_preparation"],
        "root_id": doc.root_id,
        "session_id": session_id,
    }))
    .map_err(BrainError::from)
}

fn legacy_environment(
    declaration: &brain_protocol::session::EnvironmentConfig,
) -> Result<&brain_protocol::session::LegacyEnvironmentConfig> {
    match declaration {
        brain_protocol::session::EnvironmentConfig::LegacyEnvironmentConfig(declaration) => {
            Ok(declaration)
        }
        brain_protocol::session::EnvironmentConfig::ComponentEnvironmentConfig(_) => Err(
            BrainError::Invalid("a legacy managed Tool cannot use a component Environment".into()),
        ),
    }
}

fn component_environment(
    declaration: &brain_protocol::session::EnvironmentConfig,
) -> Result<&brain_protocol::session::ComponentEnvironmentConfig> {
    match declaration {
        brain_protocol::session::EnvironmentConfig::ComponentEnvironmentConfig(declaration) => {
            Ok(declaration)
        }
        brain_protocol::session::EnvironmentConfig::LegacyEnvironmentConfig(_) => Err(
            BrainError::Invalid("a component Tool cannot use a legacy Environment".into()),
        ),
    }
}

pub(crate) fn environment_target(
    root_id: &str,
    environment_name: &str,
) -> Result<brain_protocol::environment::SandboxTarget> {
    serde_json::from_value(serde_json::json!({
        "kind": "environment",
        "session_id": root_id,
        "root_id": root_id,
        "binding_ref": brain_protocol::contract::environment_binding_ref(root_id, environment_name),
    }))
    .map_err(BrainError::from)
}

fn initial_environment(
    root_id: &str,
    environment_name: &str,
) -> Result<brain_protocol::environment::SandboxStatus> {
    serde_json::from_value(serde_json::json!({
        "state": "never_materialized",
        "target": environment_target(root_id, environment_name)?,
        "expires_at_ms": null,
    }))
    .map_err(BrainError::from)
}

fn environment_create_request(
    doc: &HeadDoc,
    environment_name: &str,
    generation_intent: &str,
) -> Result<brain_protocol::environment::CreateSandboxRequest> {
    sandbox_create_request(
        doc,
        environment_target(&doc.root_id, environment_name)?,
        generation_intent,
    )
}

fn sandbox_create_request(
    doc: &HeadDoc,
    target: brain_protocol::environment::SandboxTarget,
    generation_intent: &str,
) -> Result<brain_protocol::environment::CreateSandboxRequest> {
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
    MaterializeEnvironment {
        environment_name: String,
        reply: oneshot::Sender<Result<brain_protocol::environment::SandboxStatus>>,
    },
    WriteEnvironmentFile {
        environment_name: String,
        operation_id: String,
        generation: String,
        path: String,
        content_base64: String,
        overwrite: bool,
        reply: oneshot::Sender<Result<brain_protocol::environment::FileEntry>>,
    },
    CopyStorageToEnvironment {
        environment_name: String,
        operation_id: String,
        generation: String,
        key: String,
        path: String,
        overwrite: bool,
        reply: oneshot::Sender<Result<brain_protocol::environment::FileEntry>>,
    },
    CopyEnvironmentToStorage {
        environment_name: String,
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
    /// The whole composition surface -- bring your own backends; a custom substrate needs no
    /// core change.
    pub fn with_parts_and_services(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        external_executor: Arc<dyn ToolExecutor>,
        services: BrainServices,
        provider_factory: ProviderFactory,
    ) -> Arc<Self> {
        let outbound = crate::outbound::OutboundPolicy::new(cfg.outbound_allow_private);
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
        let agentloop_registry = services.agentloop_registry;
        #[cfg(test)]
        let agentloop_registry =
            agentloop_registry.or_else(|| Some(Arc::new(crate::agentloop::TestAgentloopRegistry)));
        let model_registry = services.model_registry.unwrap_or_else(|| {
            Arc::new(FactoryModelRegistry {
                factory: provider_factory.clone(),
            })
        });
        Arc::new(Self {
            agentloop_registry: agentloop_registry
                .expect("BrainServices.agentloop_registry is required"),
            model_registry,
            tool_registry: services.tool_registry,
            component_environment_registry: services.component_environment_registry,
            model_permits: Arc::new(Semaphore::new(cfg.max_concurrent_model_rounds)),
            turn_permits: Arc::new(Semaphore::new(cfg.max_concurrent_turns)),
            create_permits: Arc::new(Semaphore::new(cfg.max_concurrent_creates)),
            journal,
            custody,
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
            environments: services.environments,
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
    pub fn in_memory_test(
        data_dir: impl Into<PathBuf>,
        cfg: BrainConfig,
        provider_factory: ProviderFactory,
    ) -> Result<Arc<Self>> {
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
        Ok(Self::with_parts_and_services(
            cfg,
            Journal::new_memory(owner),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(DisabledToolExecutor),
            BrainServices {
                agentloop_registry: Some(Arc::new(crate::agentloop::TestAgentloopRegistry)),
                ..BrainServices::default()
            },
            provider_factory,
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
                let environment_env = if doc.environment_secrets_b64.is_empty() {
                    HashMap::new()
                } else {
                    let plain = self
                        .custody
                        .decrypt(&doc.root_id, &blob_from_b64(&doc.environment_secrets_b64)?)
                        .await?;
                    serde_json::from_str(plain.expose()).map_err(|error| {
                        BrainError::Custody(format!("managed Tool secret document: {error}"))
                    })?
                };
                Ok::<_, BrainError>(Arc::new(RootExecutionSecrets {
                    key,
                    environment_env,
                }))
            })
            .await?
            .clone();
        Ok((cell, value))
    }

    fn mint_managed_secret_capability(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        environment_id: &str,
        binding_refs: HashSet<String>,
        mut env_names: Vec<String>,
    ) -> Result<Option<brain_protocol::environment::SecretCapability>> {
        env_names.sort_unstable();
        env_names.dedup();
        if env_names.is_empty() {
            return Ok(None);
        }
        if env_names.len() > brain_protocol::MAX_SESSION_SECRET_NAMES
            || env_names
                .iter()
                .any(|name| !doc.prefix.environment_env_keys.contains(name))
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
                    environment_id: environment_id.to_owned(),
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
    ) -> Result<Arc<HashMap<String, crate::environment::ManagedBinding>>> {
        if doc.prefix.managed_bundles.is_empty() {
            return Ok(Arc::new(HashMap::new()));
        }
        let bundle_storage = self.bundle_storage.as_ref().ok_or_else(|| {
            BrainError::Invalid("managed Tools require durable Tool-bundle custody".into())
        })?;

        let mut resolved_by_tool = HashMap::with_capacity(doc.prefix.managed_bundles.len());
        struct Preparation {
            adapter: crate::environment::EnvironmentAdapter,
            environment_id: String,
            bindings: Vec<serde_json::Value>,
            binding_refs: HashSet<String>,
            env_names: Vec<String>,
            bundle_digests: Vec<String>,
        }
        let mut preparations = HashMap::<String, Preparation>::new();
        for descriptor in &doc.prefix.managed_bundles {
            let logical_name = descriptor.environment_name.as_str();
            let declaration = doc.prefix.environments.get(logical_name).ok_or_else(|| {
                BrainError::Invalid(format!(
                    "managed Tool {} names undeclared environment {logical_name}",
                    descriptor.tool_name.as_str()
                ))
            })?;
            let adapter = self
                .environments
                .resolve(legacy_environment(declaration)?.extension.as_str())?
                .clone();
            let binding = sealed_managed_binding(session_id, doc, descriptor, declaration)?;
            let resolved = adapter
                .execution
                .resolve_binding(binding)
                .await
                .map_err(map_environment_port_error)?;
            if resolved.recovery != brain_protocol::environment::RecoveryClass::Retained
                || !resolved
                    .capabilities
                    .contains(&brain_protocol::environment::EnvironmentCapability::Execution)
                || !resolved.capabilities.contains(
                    &brain_protocol::environment::EnvironmentCapability::SessionPreparation,
                )
                || resolved.limits.max_inline_input_bytes.get()
                    < brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES as u64
                || resolved.limits.max_inline_result_bytes.get()
                    < brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES as u64
            {
                return Err(BrainError::EnvironmentUnavailable(
                    "resolved managed binding cannot enforce the immutable execution seal".into(),
                ));
            }
            let preparation = preparations
                .entry(logical_name.to_owned())
                .or_insert_with(|| Preparation {
                    adapter: adapter.clone(),
                    environment_id: resolved.environment_id.to_string(),
                    bindings: Vec::new(),
                    binding_refs: HashSet::new(),
                    env_names: Vec::new(),
                    bundle_digests: Vec::new(),
                });
            if preparation.environment_id != resolved.environment_id.as_str() {
                return Err(BrainError::EnvironmentUnavailable(format!(
                    "logical environment {logical_name} resolved to more than one physical environment"
                )));
            }
            preparation.env_names.extend(
                descriptor
                    .required_env
                    .iter()
                    .map(|name| name.as_str().to_owned()),
            );
            preparation
                .binding_refs
                .insert(resolved.binding_ref.to_string());
            if resolved_by_tool
                .insert(
                    descriptor.tool_name.to_string(),
                    crate::environment::ManagedBinding {
                        environment_name: logical_name.to_owned(),
                        resolved: resolved.clone(),
                        environment: adapter.execution.clone(),
                    },
                )
                .is_some()
            {
                return Err(BrainError::Invalid(format!(
                    "managed Tool {} has more than one immutable binding",
                    descriptor.tool_name.as_str()
                )));
            }
            preparation.bindings.push(serde_json::json!({
                "binding_ref": resolved.binding_ref.clone(),
                "bundle_digests": descriptor.layers.iter().map(|layer| layer.digest.as_str()).collect::<Vec<_>>(),
            }));
            preparation.bundle_digests.extend(
                descriptor
                    .layers
                    .iter()
                    .map(|layer| layer.digest.to_string()),
            );
        }

        for (_, preparation) in preparations {
            let mut seen = HashSet::new();
            let mut bundles = Vec::new();
            for digest in preparation.bundle_digests {
                if seen.insert(digest.clone()) {
                    bundles.push(
                        bundle_storage
                            .prepare_bundle_fetch(&doc.root_id, &digest)
                            .await?,
                    );
                }
            }
            let secret_capability = self.mint_managed_secret_capability(
                session_id,
                doc,
                &preparation.environment_id,
                preparation.binding_refs,
                preparation.env_names,
            )?;
            let request: brain_protocol::environment::PrepareSessionRequest =
                serde_json::from_value(serde_json::json!({
                    "bindings": preparation.bindings,
                    "bundles": bundles,
                    "network": sealed_sandbox_network(doc)?,
                    "resources": managed_environment_resources()?,
                    "root_id": doc.root_id,
                    "secret_capability": secret_capability,
                    "session_id": session_id,
                }))?;
            preparation
                .adapter
                .preparation
                .prepare(request)
                .await
                .map_err(map_environment_port_error)?;
        }
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
    pub fn try_admit_create(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.refuse_while_draining()?;
        self.create_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| BrainError::Overloaded)
    }

    pub async fn create_session_for_admitted(
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
        let provider = req.model.provider.as_str();
        let base_url = req
            .model
            .base_url
            .as_ref()
            .map(|value| value.as_str().trim_end_matches('/').to_owned())
            .unwrap_or_default();
        if !base_url.is_empty() {
            let checked_base = self.outbound.check_url(&base_url)?;
            if checked_base.query().is_some() {
                return Err(BrainError::Invalid(
                    "model.base_url must not contain a query".into(),
                ));
            }
        }
        if req.model.api_key.is_empty() {
            return Err(BrainError::Invalid(
                "model.api_key must not be empty".into(),
            ));
        }
        validate_custody_plaintext("model.api_key", req.model.api_key.as_ref())?;
        validate_header_value(req.model.api_key.as_ref())
            .then_some(())
            .ok_or_else(|| {
                BrainError::Invalid("model.api_key is not a valid HTTP header value".into())
            })?;
        let mut component_artifacts = HashMap::with_capacity(req.component_artifacts.len());
        for (index, artifact) in req.component_artifacts.iter().enumerate() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(artifact.component_base64.as_bytes())
                .map_err(|_| {
                    BrainError::Invalid(format!(
                        "component_artifacts[{index}].component_base64 is not valid base64"
                    ))
                })?;
            if bytes.len() != artifact.bytes.get() as usize {
                return Err(BrainError::Invalid(format!(
                    "component_artifacts[{index}].bytes does not match decoded content"
                )));
            }
            let digest = hex::encode(Sha256::digest(&bytes));
            if digest != artifact.component_digest.as_str() {
                return Err(BrainError::Invalid(format!(
                    "component_artifacts[{index}] digest is {digest}, not the declared {}",
                    artifact.component_digest.as_str()
                )));
            }
            if component_artifacts.insert(digest, bytes).is_some() {
                return Err(BrainError::Invalid(format!(
                    "component_artifacts[{index}] repeats a component digest"
                )));
            }
        }
        let model_digest = req.model.component_digest.as_str();
        let model_component = component_artifacts.get(model_digest).ok_or_else(|| {
            BrainError::Invalid(format!(
                "Model component {model_digest} has no supplied artifact"
            ))
        })?;
        let model_selector = self.model_registry.admit(
            model_digest,
            &req.model.world,
            model_component,
            provider,
            &req.model.config,
        )?;
        self.model_registry.resolve(&model_selector)?;
        if req.metadata.len() > 16 {
            return Err(BrainError::Invalid("metadata: at most 16 pairs".into()));
        }
        let tools_cfg = req.tools.clone().unwrap_or_default();
        if !req.secrets.is_empty() {
            validate_custody_plaintext("secrets", &serde_json::to_string(&req.secrets)?)?;
        }
        let tool_items = tools_cfg.items.clone();
        let mut decls = crate::tools::resolve(&tool_items)?;
        let mut referenced_tool_components = HashSet::new();
        for decl in &mut decls {
            let crate::config::ToolRoute::Component(selector) = &mut decl.route else {
                continue;
            };
            let registry = self.tool_registry.as_ref().ok_or_else(|| {
                BrainError::Invalid(format!(
                    "tool {} requires component execution, which is unavailable in this Brain composition",
                    decl.name
                ))
            })?;
            let digest = selector.component_digest.clone();
            let component = component_artifacts.get(&digest).ok_or_else(|| {
                BrainError::Invalid(format!("Tool component {digest} has no supplied artifact"))
            })?;
            *selector = registry.admit(
                &digest,
                &selector.world,
                component,
                &selector.config,
                &selector.grants,
                selector.environment.as_deref(),
            )?;
            referenced_tool_components.insert(digest);
        }
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

        let environment_env: HashMap<String, String> = req
            .secrets
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_str().to_owned()))
            .collect();
        let environments: HashMap<String, brain_protocol::session::EnvironmentConfig> = req
            .environments
            .as_ref()
            .into_iter()
            .flat_map(|environments| environments.iter())
            .map(|(name, environment)| (name.as_str().to_owned(), environment.clone()))
            .collect();
        let mut referenced_environment_components = HashSet::new();
        for (name, declaration) in &environments {
            let brain_protocol::session::EnvironmentConfig::ComponentEnvironmentConfig(declaration) =
                declaration
            else {
                continue;
            };
            let registry = self.component_environment_registry.as_ref().ok_or_else(|| {
                BrainError::Invalid(format!(
                    "environment {name:?} requires component execution, which is unavailable in this Brain composition"
                ))
            })?;
            let digest = declaration.component_digest.as_str();
            let component = component_artifacts.get(digest).ok_or_else(|| {
                BrainError::Invalid(format!(
                    "Environment component {digest} has no supplied artifact"
                ))
            })?;
            registry.admit(digest, &declaration.world, component)?;
            referenced_environment_components.insert(digest.to_owned());
        }
        for decl in &decls {
            if let crate::config::ToolRoute::Component(selector) = &decl.route {
                if let Some(environment_name) = selector.environment.as_deref() {
                    let environment = environments.get(environment_name).ok_or_else(|| {
                        BrainError::Invalid(format!(
                            "tool {} is bound to undeclared environment {environment_name:?}",
                            decl.name
                        ))
                    })?;
                    component_environment(environment)?;
                }
                continue;
            }
            let (environment_name, expected_profile) = match &decl.route {
                crate::config::ToolRoute::Environment(seal) => {
                    (seal.environment.as_str(), "computer")
                }
                crate::config::ToolRoute::Customer { environment, .. } => {
                    (environment.as_str(), "callbacks")
                }
                _ => continue,
            };
            let environment = environments.get(environment_name).ok_or_else(|| {
                BrainError::Invalid(format!(
                    "tool {} is bound to undeclared environment {environment_name:?}",
                    decl.name
                ))
            })?;
            let environment = legacy_environment(environment)?;
            let actual_profile = match environment.profile.kind {
                brain_protocol::session::EnvironmentProfileKind::Computer => "computer",
                brain_protocol::session::EnvironmentProfileKind::Callbacks => "callbacks",
            };
            if actual_profile != expected_profile {
                return Err(BrainError::Invalid(format!(
                    "tool {} requires a {expected_profile} environment, but {environment_name:?} is {actual_profile}",
                    decl.name
                )));
            }
        }
        let shape = "1gb".to_string();
        let environment_tools: Vec<_> = decls
            .iter()
            .filter_map(|decl| match &decl.route {
                crate::config::ToolRoute::Environment(seal) => Some((decl, seal)),
                _ => None,
            })
            .collect();
        for (decl, seal) in &environment_tools {
            let missing: Vec<_> = seal
                .required_env
                .iter()
                .filter(|key| !environment_env.contains_key(*key))
                .collect();
            if !missing.is_empty() {
                return Err(BrainError::Invalid(format!(
                    "tool {} is missing required Environment environment keys: {}",
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
        let mut layer_checksums = HashSet::new();
        let mut layer_payloads = HashMap::with_capacity(req.tool_artifact_layers.len());
        for (index, layer) in req.tool_artifact_layers.iter().enumerate() {
            let checksum = layer.checksum.to_string();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(layer.content_base64.as_bytes())
                .map_err(|error| {
                    BrainError::Invalid(format!(
                        "tool_artifact_layers[{index}].content_base64: {error}"
                    ))
                })?;
            if bytes.len() != layer.bytes.get() as usize {
                return Err(BrainError::Invalid(format!(
                    "tool_artifact_layers[{index}].bytes does not match decoded content"
                )));
            }
            if hex::encode(Sha256::digest(&bytes)) != checksum {
                return Err(BrainError::Invalid(format!(
                    "tool_artifact_layers[{index}] checksum mismatch"
                )));
            }
            if layer_payloads
                .insert(checksum, (bytes, layer.media_type))
                .is_some()
            {
                return Err(BrainError::Invalid(format!(
                    "tool_artifact_layers[{index}] repeats a checksum"
                )));
            }
        }
        for (index, bundle) in req.tool_bundles.iter().enumerate() {
            let checksum = bundle.checksum.to_string();
            if !bundle_checksums.insert(checksum.clone()) {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}]: duplicate checksum"
                )));
            }
            let manifest = serde_json::json!({
                "profile": "computer/v1",
                "target": bundle.target,
                "execute_path": bundle.execute_path,
                "setup_path": bundle.setup_path,
                "layers": bundle.layers.iter().map(|layer| serde_json::json!({
                    "checksum": layer.checksum,
                    "bytes": layer.bytes,
                    "media_type": layer.media_type,
                    "mount_path": layer.mount_path,
                    "unpack": layer.unpack,
                })).collect::<Vec<_>>(),
            });
            let actual_manifest = brain_protocol::contract::canonical_digest(&manifest)?;
            if actual_manifest.as_str() != checksum {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}] manifest checksum mismatch"
                )));
            }
            let mut decoded_layers = Vec::with_capacity(bundle.layers.len());
            let mut bundle_bytes = 0usize;
            for (layer_index, layer) in bundle.layers.iter().enumerate() {
                let (bytes, media_type) = layer_payloads
                    .get(layer.checksum.as_str())
                    .ok_or_else(|| BrainError::Invalid(format!(
                        "tool_bundles[{index}].layers[{layer_index}] has no supplied artifact layer"
                    )))?;
                if bytes.len() != layer.bytes.get() as usize {
                    return Err(BrainError::Invalid(format!(
                        "tool_bundles[{index}].layers[{layer_index}].bytes does not match decoded content"
                    )));
                }
                if media_type.to_string() != layer.media_type.to_string() {
                    return Err(BrainError::Invalid(format!(
                        "tool_bundles[{index}].layers[{layer_index}] media type conflicts with its payload"
                    )));
                }
                bundle_bytes = bundle_bytes.saturating_add(bytes.len());
                if layer_checksums.insert(layer.checksum.to_string()) {
                    total_bundle_bytes = total_bundle_bytes.saturating_add(bytes.len());
                }
                decoded_layers.push(DecodedToolLayer {
                    checksum: layer.checksum.to_string(),
                    bytes: bytes.clone(),
                    media_type: layer.media_type,
                    mount_path: layer.mount_path.to_string(),
                    unpack: layer.unpack,
                });
            }
            if bundle_bytes != bundle.bytes.get() as usize {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}].bytes does not match its artifact layers"
                )));
            }
            if bundle_bytes > brain_protocol::MAX_SESSION_BUNDLE_BYTES {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles[{index}] exceeds {} bytes",
                    brain_protocol::MAX_SESSION_BUNDLE_BYTES,
                )));
            }
            if total_bundle_bytes > brain_protocol::MAX_SESSION_BUNDLE_BYTES {
                return Err(BrainError::Invalid(format!(
                    "tool_bundles exceed the {}-byte session limit",
                    brain_protocol::MAX_SESSION_BUNDLE_BYTES,
                )));
            }
            decoded_bundles.push(DecodedToolBundle {
                checksum,
                bytes: bundle_bytes,
                target: bundle.target,
                execute_path: bundle.execute_path.to_string(),
                setup_path: bundle
                    .setup_path
                    .as_ref()
                    .map(|path| path.as_str().to_owned()),
                layers: decoded_layers,
            });
        }
        if let Some(unused) = layer_payloads
            .keys()
            .find(|checksum| !layer_checksums.contains(*checksum))
        {
            return Err(BrainError::Invalid(format!(
                "unreferenced tool artifact layer {unused}"
            )));
        }
        let referenced_bundle_checksums: HashSet<_> = environment_tools
            .iter()
            .map(|(_, seal)| seal.checksum.as_str())
            .collect();
        for (_, seal) in &environment_tools {
            if !bundle_checksums.contains(&seal.checksum) {
                return Err(BrainError::Invalid(format!(
                    "Environment bundle {} was not supplied",
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
        let managed_bundles = managed_bundle_descriptors(&environment_tools, &decoded_bundles)?;

        let mut environment_env_keys: Vec<_> = environment_env.keys().cloned().collect();
        environment_env_keys.sort();

        // Seal the loop identity before anything else commits, rejecting a loop this
        // composition cannot run while the request is still refusable.
        let brain_protocol::session::AgentloopConfig {
            component_digest,
            world,
            config,
        } = &req.agentloop;
        let digest = component_digest.as_str();
        let component = component_artifacts.get(digest).ok_or_else(|| {
            BrainError::Invalid(format!(
                "Agentloop component {digest} has no supplied artifact"
            ))
        })?;
        let agentloop_selector =
            self.agentloop_registry
                .admit(digest, world.as_str(), component, config)?;
        self.agentloop_registry.resolve(&agentloop_selector)?;
        let mut referenced_components = HashSet::from([model_digest, digest]);
        referenced_components.extend(referenced_tool_components.iter().map(String::as_str));
        referenced_components.extend(referenced_environment_components.iter().map(String::as_str));
        if let Some(unused) = component_artifacts
            .keys()
            .find(|candidate| !referenced_components.contains(candidate.as_str()))
        {
            return Err(BrainError::Invalid(format!(
                "unreferenced component artifact {unused}"
            )));
        }

        let now = crate::wall_ms();
        let mut prefix = PrefixDoc {
            agentloop: Some(agentloop_selector),
            system_prompt: None,
            provider: provider.to_string(),
            model_component: Some(model_selector),
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
            network: merge_session_network(req.network.as_ref(), &decls)?,
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
            prompt_cache_key: format!("brain:{session_id}"),
            tools: tool_items,
            environments,
            managed_bundles,
            official_capabilities,
            environment_enabled: true,
            shape: shape.clone(),
            sync_interval_seconds: 0,
            environment_env_keys,
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
            let mut stored_layers = HashSet::new();
            for bundle in &decoded_bundles {
                let descriptor = prefix
                    .managed_bundles
                    .iter()
                    .find(|descriptor| descriptor.bundle_digest.as_str() == bundle.checksum)
                    .ok_or_else(|| {
                        BrainError::Journal("stored Tool bundle has no immutable descriptor".into())
                    })?;
                for layer in &bundle.layers {
                    if !stored_layers.insert(layer.checksum.clone()) {
                        continue;
                    }
                    let object = bundle_storage
                        .store_bundle(&session_id, &layer.checksum, &layer.bytes)
                        .await?;
                    let layer_descriptor = descriptor
                        .layers
                        .iter()
                        .find(|candidate| candidate.digest.as_str() == layer.checksum)
                        .ok_or_else(|| {
                            BrainError::Journal(
                                "stored Tool artifact layer has no immutable descriptor".into(),
                            )
                        })?;
                    if !bundle_object_matches_descriptor(&object, layer_descriptor)? {
                        return Err(BrainError::Journal(
                            "Tool artifact storage returned an object outside the immutable descriptor"
                                .into(),
                        ));
                    }
                }
            }
        }

        // Only after every pure request/prefix/budget validation succeeds may custody perform an
        // external effect. The plaintext key and session secrets never reach the journal.
        let key = ProviderKey::new(req.model.api_key.to_string());
        let blob = self.custody.encrypt(&session_id, &key).await?;
        let environment_secrets_b64 = if environment_env.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(&environment_env)?;
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
            state: SessionLifecycle::Open,
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
            environment_secrets_b64,
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
            environment_targets: HashMap::new(),
            tool_setups: HashMap::new(),
        };
        if let Err(error) = self
            .journal
            .create(
                &session_id,
                &doc,
                &Record::State {
                    state: SessionLifecycle::Open,
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
        if head.doc.state == SessionLifecycle::Deleted {
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

    /// Current logical status of one declared environment. Reading status never materializes
    /// compute. Descendants resolve through their immutable `root_id`.
    pub async fn environment_status(
        &self,
        session_id: &str,
        environment_name: &str,
    ) -> Result<brain_protocol::environment::SandboxStatus> {
        let head = self.journal.get_head(session_id).await?;
        let root = if head.doc.root_id == session_id {
            head
        } else {
            self.journal.get_head(&head.doc.root_id).await?
        };
        if !root.doc.prefix.environments.contains_key(environment_name) {
            return Err(BrainError::Invalid(format!(
                "session has no environment named {environment_name:?}"
            )));
        }
        root.doc
            .environment_targets
            .get(environment_name)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| initial_environment(&root.doc.root_id, environment_name))
    }

    /// Idempotently materialize one declared computer environment.
    pub async fn materialize_environment(
        self: &Arc<Self>,
        session_id: &str,
        environment_name: &str,
    ) -> Result<brain_protocol::environment::SandboxStatus> {
        let head = self.journal.get_head(session_id).await?;
        let root_id = head.doc.root_id.clone();
        let environment_name = environment_name.to_owned();
        self.deliver(&root_id, |reply| Command::MaterializeEnvironment {
            environment_name: environment_name.clone(),
            reply,
        })
        .await?
    }

    async fn environment_file_target(
        &self,
        session_id: &str,
        environment_name: &str,
        expected_generation: &str,
    ) -> Result<brain_protocol::environment::SandboxTarget> {
        let session = self.journal.get_head(session_id).await?;
        ensure_storage_readable(&session.doc, session_id)?;
        let root = if session.doc.root_id == session_id {
            session
        } else {
            self.journal.get_head(&session.doc.root_id).await?
        };
        if root.doc.state != SessionLifecycle::Open || root.doc.ended {
            return Err(BrainError::SessionDeleted(root.doc.root_id.clone()));
        }
        let status = root
            .doc
            .environment_targets
            .get(environment_name)
            .ok_or(BrainError::SandboxNotMaterialized)?;
        if status.generation.as_ref().map(|value| value.as_str()) != Some(expected_generation) {
            return Err(
                if matches!(
                    status.state,
                    brain_protocol::environment::SandboxState::Gone
                        | brain_protocol::environment::SandboxState::Terminated
                ) {
                    BrainError::SandboxGone
                } else {
                    BrainError::SandboxGenerationConflict
                },
            );
        }
        match status.state {
            brain_protocol::environment::SandboxState::Running
            | brain_protocol::environment::SandboxState::Suspended => Ok(status.target.clone()),
            brain_protocol::environment::SandboxState::Gone
            | brain_protocol::environment::SandboxState::Terminated => Err(BrainError::SandboxGone),
            brain_protocol::environment::SandboxState::NeverMaterialized
            | brain_protocol::environment::SandboxState::Creating => {
                Err(BrainError::SandboxNotMaterialized)
            }
        }
    }

    fn environment_adapter(
        &self,
        doc: &HeadDoc,
        environment_name: &str,
    ) -> Result<crate::environment::EnvironmentAdapter> {
        let declaration = doc
            .prefix
            .environments
            .get(environment_name)
            .ok_or_else(|| {
                BrainError::Invalid(format!(
                    "session has no environment named {environment_name:?}"
                ))
            })?;
        self.environments
            .resolve(legacy_environment(declaration)?.extension.as_str())
            .cloned()
    }

    fn environment_files_port(
        &self,
        doc: &HeadDoc,
        environment_name: &str,
    ) -> Result<Arc<dyn crate::environment::SandboxFilesPort>> {
        self.environment_adapter(doc, environment_name)?
            .files
            .ok_or_else(|| {
                BrainError::Invalid(format!(
                    "environment {environment_name:?} does not provide files"
                ))
            })
    }

    pub async fn sandbox_file_list(
        &self,
        session_id: &str,
        environment_name: &str,
        generation: &str,
        path: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::environment::SandboxFileList> {
        let path = normalize_workspace_path(path)?;
        let head = self.journal.get_head(session_id).await?;
        let target = self
            .environment_file_target(session_id, environment_name, generation)
            .await?;
        self.environment_files_port(&head.doc, environment_name)?
            .list(crate::environment::SandboxFileListRequest {
                target,
                expected_generation: generation.to_owned(),
                path,
                cursor: cursor.map(str::to_owned),
                limit: limit.clamp(1, 100),
            })
            .await
            .map_err(map_environment_port_error)
    }

    pub async fn sandbox_file_stat(
        &self,
        session_id: &str,
        environment_name: &str,
        generation: &str,
        path: &str,
    ) -> Result<brain_protocol::environment::FileEntry> {
        let path = normalize_workspace_path(path)?;
        let head = self.journal.get_head(session_id).await?;
        let target = self
            .environment_file_target(session_id, environment_name, generation)
            .await?;
        self.environment_files_port(&head.doc, environment_name)?
            .stat(sandbox_file_request(&target, generation, &path)?)
            .await
            .map_err(map_environment_port_error)
    }

    pub async fn sandbox_file_read_inline(
        &self,
        session_id: &str,
        environment_name: &str,
        generation: &str,
        path: &str,
        max_bytes: u64,
    ) -> Result<crate::environment::SandboxFileContent> {
        let path = normalize_workspace_path(path)?;
        let head = self.journal.get_head(session_id).await?;
        let target = self
            .environment_file_target(session_id, environment_name, generation)
            .await?;
        let content = self
            .environment_files_port(&head.doc, environment_name)?
            .read(sandbox_file_request(&target, generation, &path)?)
            .await
            .map_err(map_environment_port_error)?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&content.content_base64)
            .map_err(|_| BrainError::Environment("sandbox returned invalid base64".into()))?;
        let limit = max_bytes.min(1024 * 1024) as usize;
        if decoded.len() > limit || decoded.len() as u64 != content.entry.bytes {
            return Err(BrainError::FileTooLarge { limit });
        }
        Ok(content)
    }

    pub async fn sandbox_file_write_inline(
        self: &Arc<Self>,
        session_id: &str,
        environment_name: &str,
        generation: String,
        path: String,
        content_base64: String,
        overwrite: bool,
        idempotency_key: &str,
    ) -> Result<brain_protocol::environment::FileEntry> {
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
            "brain.environment-file-write.v1\0{session_id}\0{environment_name}\0{idempotency_key}"
        ));
        let operation_id = format!("file_{}", &identity[..24]);
        let environment_name = environment_name.to_owned();
        self.deliver(session_id, |reply| Command::WriteEnvironmentFile {
            environment_name: environment_name.clone(),
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
        environment_name: &str,
        generation: &str,
        path: &str,
        expression: &str,
        cursor: Option<&str>,
        limit: u32,
        grep: bool,
    ) -> Result<crate::environment::SandboxFileList> {
        let path = normalize_workspace_path(path)?;
        if expression.is_empty() || expression.len() > 4096 {
            return Err(BrainError::Invalid(
                "sandbox search expression must contain 1 to 4096 UTF-8 bytes".into(),
            ));
        }
        let head = self.journal.get_head(session_id).await?;
        let target = self
            .environment_file_target(session_id, environment_name, generation)
            .await?;
        let request = sandbox_search_request(
            &target,
            generation,
            &path,
            expression,
            cursor,
            limit.clamp(1, 100),
        )?;
        let files = self.environment_files_port(&head.doc, environment_name)?;
        if grep {
            files.grep(request).await
        } else {
            files.find(request).await
        }
        .map_err(map_environment_port_error)
    }

    /// Prepare a short-lived direct download of one generation-fenced sandbox file. Bytes are
    /// exported through the existing exact-pair Environment copy operation into quota-metered hidden
    /// session storage; the public ticket never exposes that storage key.
    pub async fn sandbox_file_prepare_download(
        self: &Arc<Self>,
        session_id: &str,
        environment_name: &str,
        generation: String,
        path: String,
    ) -> Result<crate::storage::StorageTransferTicket> {
        self.storage_port()?;
        let head = self.journal.get_head(session_id).await?;
        self.environment_files_port(&head.doc, environment_name)?;
        let path = normalize_workspace_path(&path)?;
        let entry = self
            .sandbox_file_stat(session_id, environment_name, &generation, &path)
            .await?;
        if entry.kind != brain_protocol::environment::FileEntryKind::File {
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
                environment_name,
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
                return Err(BrainError::Environment(
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
        environment_name: &str,
        generation: String,
        path: String,
        bytes: u64,
        sha256: String,
        overwrite: bool,
    ) -> Result<crate::storage::StorageTransferTicket> {
        self.storage_port()?;
        let head = self.journal.get_head(session_id).await?;
        self.environment_files_port(&head.doc, environment_name)?;
        let path = normalize_workspace_path(&path)?;
        // This read-only lookup both authenticates the session's root and rejects a stale/gone
        // generation before reserving shared process or storage quota.
        self.environment_file_target(session_id, environment_name, &generation)
            .await?;
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
                    environment_name: environment_name.to_owned(),
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
    ) -> Result<brain_protocol::environment::FileEntry> {
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
                &destination.environment_name,
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
        environment_name: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        idempotency_key: &str,
    ) -> Result<brain_protocol::environment::FileEntry> {
        crate::storage::validate_storage_key(&key)?;
        self.storage_copy_to_sandbox_admitted(
            session_id,
            environment_name,
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
        environment_name: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        operation_key: &str,
    ) -> Result<brain_protocol::environment::FileEntry> {
        crate::storage::validate_internal_storage_key(&key)?;
        self.storage_copy_to_sandbox_admitted(
            session_id,
            environment_name,
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
        environment_name: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        operation_key: &str,
    ) -> Result<brain_protocol::environment::FileEntry> {
        let operation_id = sandbox_file_effect_id(
            session_id,
            operation_key,
            &format!("storage-to-{environment_name}"),
        )?;
        let path = normalize_workspace_path(&path)?;
        let environment_name = environment_name.to_owned();
        self.deliver(session_id, |reply| Command::CopyStorageToEnvironment {
            environment_name: environment_name.clone(),
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
        environment_name: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        idempotency_key: &str,
    ) -> Result<crate::storage::StorageObject> {
        crate::storage::validate_storage_key(&key)?;
        self.storage_copy_from_sandbox_admitted(
            session_id,
            environment_name,
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
        environment_name: &str,
        key: String,
        path: String,
        generation: String,
        operation_key: &str,
    ) -> Result<crate::storage::StorageObject> {
        crate::storage::validate_internal_storage_key(&key)?;
        self.storage_copy_from_sandbox_admitted(
            session_id,
            environment_name,
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
        environment_name: &str,
        key: String,
        path: String,
        generation: String,
        overwrite: bool,
        operation_key: &str,
    ) -> Result<crate::storage::StorageObject> {
        let operation_id = sandbox_file_effect_id(
            session_id,
            operation_key,
            &format!("{environment_name}-to-storage"),
        )?;
        let path = normalize_workspace_path(&path)?;
        let environment_name = environment_name.to_owned();
        self.deliver(session_id, |reply| Command::CopyEnvironmentToStorage {
            environment_name: environment_name.clone(),
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
            page.sessions
                .iter()
                .map(session_doc_summary)
                .collect::<Result<Vec<_>>>()?,
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
            .is_some_and(|status| status.state == DeletionState::Succeeded)
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
        if head.doc.state == SessionLifecycle::Deleted {
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
            .filter(|h| h.doc.state != SessionLifecycle::Deleted)
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
        let state = state
            .map(|value| {
                value.parse::<SessionLifecycle>().map_err(|_| {
                    BrainError::Invalid(format!("unknown session state filter {value:?}"))
                })
            })
            .transpose()?;
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
            page.sessions
                .iter()
                .map(session_doc_summary)
                .collect::<Result<Vec<_>>>()?,
            page.next_cursor,
        ))
    }

    pub async fn list_changes_for(
        self: &Arc<Self>,
        principal: &TrustedPrincipal,
        after_ms: u64,
        partition: u16,
        partitions: u16,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<(Vec<session::Session>, Option<String>, u64)> {
        if partitions == 0 || partitions > 256 || partition >= partitions {
            return Err(BrainError::Invalid(
                "changefeed partition must be lower than a partitions value from 1 through 256"
                    .into(),
            ));
        }
        let limit = limit.clamp(1, 100);
        let mut cursor = cursor.map(str::to_owned);
        let mut changes = Vec::with_capacity(limit);
        let mut watermark_ms = after_ms;
        loop {
            let remaining = limit - changes.len();
            let page = self
                .journal
                .list_session_page(&crate::journal::SessionListQuery {
                    tenant_id: principal.as_str(),
                    state: None,
                    limit: remaining,
                    cursor: cursor.as_deref(),
                })
                .await?;
            let mut reached_lower_bound = false;
            for summary in &page.sessions {
                if summary.updated_ms <= after_ms {
                    reached_lower_bound = true;
                    break;
                }
                if change_partition(&summary.session_id, partitions) != partition {
                    continue;
                }
                watermark_ms = watermark_ms.max(summary.updated_ms);
                changes.push(crate::session::view::session_doc_summary(summary)?);
                if changes.len() == limit {
                    return Ok((changes, page.next_cursor, watermark_ms));
                }
            }
            if reached_lower_bound {
                return Ok((changes, None, watermark_ms));
            }
            let Some(next) = page.next_cursor else {
                return Ok((changes, None, watermark_ms));
            };
            cursor = Some(next);
        }
    }

    pub async fn head(&self, session_id: &str) -> Result<Head> {
        self.journal.get_head(session_id).await
    }
}

fn change_partition(session_id: &str, partitions: u16) -> u16 {
    use sha2::{Digest as _, Sha256};

    let digest = Sha256::digest(session_id.as_bytes());
    u16::from_be_bytes([digest[0], digest[1]]) % partitions
}

struct FactoryModelRegistry {
    factory: ProviderFactory,
}

impl crate::provider::ModelRegistry for FactoryModelRegistry {
    fn resolve(&self, selector: &crate::journal::ModelSelectorDoc) -> Result<Arc<dyn Provider>> {
        Ok((self.factory)(dialect_of(&selector.provider)))
    }

    fn admit(
        &self,
        component_digest: &str,
        world: &str,
        component: &[u8],
        provider: &str,
        config: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<crate::journal::ModelSelectorDoc> {
        Ok(crate::journal::ModelSelectorDoc {
            component_digest: component_digest.into(),
            component_bytes: component.len() as u64,
            world: world.into(),
            provider: provider.into(),
            config: config.clone(),
        })
    }
}

fn secret_delivery_error(
    code: brain_protocol::environment::EnvironmentErrorCode,
    retryable: bool,
    message: &str,
) -> brain_protocol::environment::EnvironmentError {
    serde_json::from_value(serde_json::json!({
        "code": code,
        "details": {},
        "message": message,
        "retryable": retryable,
    }))
    .expect("static secret-delivery Environment errors satisfy the contract")
}

#[async_trait::async_trait]
impl crate::turn::EngineServices for Brain {
    async fn prepare_managed_session(
        &self,
        session_id: &str,
        doc: &HeadDoc,
    ) -> Result<Arc<HashMap<String, crate::environment::ManagedBinding>>> {
        Brain::prepare_managed_session(self, session_id, doc).await
    }

    async fn execute_child_capability(
        self: Arc<Self>,
        parent_id: &str,
        operation_id: &str,
        input: serde_json::Value,
        cancel: CancellationToken,
    ) -> CallOutcome {
        Brain::execute_child_capability(&self, parent_id, operation_id, input, cancel).await
    }

    async fn execute_tool_capability(
        self: Arc<Self>,
        session_id: &str,
        environment: Option<&str>,
        capability: &str,
        operation_id: &str,
        request: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, crate::tools::ToolCapabilityFailure> {
        self.execute_component_tool_capability(
            session_id,
            environment,
            capability,
            operation_id,
            request,
        )
        .await
        .map_err(|error| crate::tools::ToolCapabilityFailure {
            code: match error {
                BrainError::Invalid(_) => "invalid_request",
                BrainError::NoSuchSession(_) => "not_found",
                BrainError::Cancelled => "cancelled",
                _ => "tool_capability_failed",
            }
            .into(),
            message: error.to_string(),
            retryable: false,
        })
    }

    async fn reconcile_managed_unknown_environment(
        self: Arc<Self>,
        session_id: &str,
        tool_name: &str,
        st: &mut crate::turn::TurnState,
    ) -> Result<()> {
        reconcile_managed_unknown_environment(&self, session_id, tool_name, st).await
    }
}

impl Brain {
    async fn execute_component_tool_capability(
        self: &Arc<Self>,
        session_id: &str,
        environment: Option<&str>,
        capability: &str,
        operation_id: &str,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let head = self.journal.get_head(session_id).await?;
        match capability {
            "tool.journal.read" => {
                let after = request
                    .get("after_seq")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let limit = request
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(32)
                    .clamp(1, 100) as usize;
                let page = self
                    .journal
                    .read_record_page(&crate::journal::RecordPageQuery {
                        session_id,
                        after,
                        through_seq: head.last_seq,
                        limit,
                        max_bytes: crate::journal::DEFAULT_RECORD_PAGE_BYTES,
                    })
                    .await?;
                let items = page
                    .entries
                    .into_iter()
                    .map(|entry| {
                        serde_json::json!({
                            "seq": entry.seq,
                            "ts_ms": entry.ts_ms,
                            "record": entry.record,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(serde_json::json!({
                    "items_json": serde_json::to_string(&items)?,
                    "next_cursor": page.next_after.map(|cursor| cursor.to_string()),
                }))
            }
            "tool.storage.list" => {
                let prefix = request.get("prefix").and_then(serde_json::Value::as_str);
                let cursor = request.get("cursor").and_then(serde_json::Value::as_str);
                let limit = request
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(32) as u32;
                let page = self.storage_list(session_id, prefix, cursor, limit).await?;
                Ok(serde_json::json!({
                    "items_json": serde_json::to_string(&page.objects)?,
                    "next_cursor": page.next_cursor,
                }))
            }
            "tool.storage.stat" => {
                let key = required_tool_capability_string(&request, "key")?;
                Ok(serde_json::Value::String(serde_json::to_string(
                    &self.storage_stat(session_id, key).await?,
                )?))
            }
            "tool.storage.read" => {
                let key = required_tool_capability_string(&request, "key")?;
                let offset = request
                    .get("offset")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let length = request
                    .get("length")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let through = offset
                    .checked_add(length)
                    .ok_or_else(|| BrainError::Invalid("storage read range overflows".into()))?;
                if through > 1024 * 1024 {
                    return Err(BrainError::Invalid(
                        "Tool storage reads are limited to the first 1 MiB; use an Environment for larger transfers".into(),
                    ));
                }
                let (_, bytes) = self.storage_read_inline(session_id, key, through).await?;
                let start = usize::try_from(offset)
                    .expect("1 MiB bound")
                    .min(bytes.len());
                let end = usize::try_from(through)
                    .expect("1 MiB bound")
                    .min(bytes.len());
                Ok(serde_json::to_value(&bytes[start..end])?)
            }
            "tool.storage.write" => {
                let key = required_tool_capability_string(&request, "key")?.to_owned();
                let bytes: Vec<u8> =
                    serde_json::from_value(request.get("bytes").cloned().ok_or_else(|| {
                        BrainError::Invalid("storage write bytes are required".into())
                    })?)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(BrainError::FileTooLarge { limit: 1024 * 1024 });
                }
                let object = self
                    .storage_write_inline(
                        session_id,
                        key,
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                        None,
                        true,
                    )
                    .await?;
                Ok(serde_json::Value::String(serde_json::to_string(&object)?))
            }
            "tool.storage.delete" => {
                let key = required_tool_capability_string(&request, "key")?.to_owned();
                self.storage_delete(session_id, key).await?;
                Ok(serde_json::Value::Null)
            }
            capability if capability.starts_with("tool.children.") => {
                self.execute_component_child_capability(
                    session_id,
                    capability,
                    operation_id,
                    request,
                )
                .await
            }
            capability if capability.starts_with("tool.parent.") => {
                let parent_id =
                    head.doc.parent_id.as_deref().ok_or_else(|| {
                        BrainError::Invalid("the root session has no parent".into())
                    })?;
                self.execute_component_parent_capability(
                    session_id,
                    parent_id,
                    capability,
                    operation_id,
                    request,
                )
                .await
            }
            "tool.environment.invoke" => {
                let environment_id = environment.ok_or_else(|| {
                    BrainError::Invalid("Tool Environment binding is absent".into())
                })?;
                let declaration = head
                    .doc
                    .prefix
                    .environments
                    .get(environment_id)
                    .ok_or_else(|| {
                        BrainError::Invalid(format!(
                            "session has no environment named {environment_id:?}"
                        ))
                    })?;
                let declaration = component_environment(declaration)?;
                let registry = self
                    .component_environment_registry
                    .as_ref()
                    .ok_or_else(|| {
                        BrainError::EnvironmentUnavailable(
                            "component Environment execution is unavailable".into(),
                        )
                    })?;
                let deadline_at_ms = required_tool_capability_string(&request, "deadline_at_ms")?
                    .parse::<u64>()
                    .map_err(|_| BrainError::Invalid("Environment deadline is invalid".into()))?;
                let bundle = request
                    .get("bundle_base64")
                    .and_then(serde_json::Value::as_str)
                    .map(|encoded| {
                        base64::engine::general_purpose::STANDARD
                            .decode(encoded)
                            .map_err(|_| {
                                BrainError::Invalid("Environment bundle is invalid base64".into())
                            })
                    })
                    .transpose()?;
                let result = registry
                    .invoke(
                        declaration,
                        crate::environment::ComponentEnvironmentInvocation {
                            tenant_id: head.doc.tenant_id.clone(),
                            session_id: session_id.to_owned(),
                            root_id: head.doc.root_id.clone(),
                            parent_id: head.doc.parent_id.clone(),
                            environment_id: environment_id.to_owned(),
                            policy: serde_json::json!({
                                "network": head.doc.prefix.network,
                            }),
                            operation_id: operation_id.to_owned(),
                            descriptor_json: required_tool_capability_string(
                                &request,
                                "descriptor_json",
                            )?
                            .to_owned(),
                            bundle,
                            input_json: required_tool_capability_string(&request, "input_json")?
                                .to_owned(),
                            deadline_at_ms,
                        },
                    )
                    .await?;
                Ok(serde_json::Value::String(result))
            }
            _ => Err(BrainError::Invalid(format!(
                "unknown Tool capability {capability:?}"
            ))),
        }
    }

    async fn execute_component_child_capability(
        self: &Arc<Self>,
        parent_id: &str,
        capability: &str,
        operation_id: &str,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        match capability {
            "tool.children.spawn" => {
                let body = parse_tool_request_json(&request)?;
                let prompt = required_tool_capability_string(&body, "prompt")?.to_owned();
                let name = body
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let fork_turns = body.get("fork_turns").and_then(serde_json::Value::as_str);
                let child = self
                    .create_child(
                        parent_id,
                        prompt,
                        name,
                        fork_turns.map(str::to_owned),
                        Some(operation_id),
                    )
                    .await?;
                Ok(serde_json::Value::String(serde_json::to_string(&child)?))
            }
            "tool.children.send" => {
                let child_id = required_tool_capability_string(&request, "child_id")?;
                self.get_child(parent_id, child_id).await?;
                let body = parse_tool_request_json(&request)?;
                let content = tool_message_content(&body)?;
                let (turn_id, seq) = self
                    .message_with_metadata_idempotent(
                        child_id,
                        content,
                        HashMap::new(),
                        Some(operation_id),
                    )
                    .await?;
                Ok(serde_json::Value::String(serde_json::to_string(
                    &serde_json::json!({"turn_id": turn_id, "seq": seq}),
                )?))
            }
            "tool.children.inspect" => {
                let child_id = required_tool_capability_string(&request, "child_id")?;
                Ok(serde_json::Value::String(serde_json::to_string(
                    &self.get_child(parent_id, child_id).await?,
                )?))
            }
            "tool.children.wait" => {
                let child_id = required_tool_capability_string(&request, "child_id")?;
                self.get_child(parent_id, child_id).await?;
                let timeout_ms = request
                    .get("timeout_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(30_000)
                    .min(300_000);
                Ok(serde_json::Value::String(serde_json::to_string(
                    &self
                        .wait_child(parent_id, child_id, Duration::from_millis(timeout_ms))
                        .await?,
                )?))
            }
            "tool.children.events" => {
                let child_id = required_tool_capability_string(&request, "child_id")?;
                self.get_child(parent_id, child_id).await?;
                self.component_event_page(child_id, &request).await
            }
            "tool.children.manage" => {
                let child_id = required_tool_capability_string(&request, "child_id")?;
                self.get_child(parent_id, child_id).await?;
                let child = match required_tool_capability_string(&request, "action")? {
                    "cancel" => self.cancel(child_id).await?,
                    "end" => self.end(child_id).await?,
                    action => {
                        return Err(BrainError::Invalid(format!(
                            "unknown child action {action:?}"
                        )));
                    }
                };
                Ok(serde_json::Value::String(serde_json::to_string(&child)?))
            }
            "tool.children.list" => {
                let cursor = request.get("cursor").and_then(serde_json::Value::as_str);
                let limit = request
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(32) as usize;
                let (items, next_cursor) = self.list_children(parent_id, cursor, limit).await?;
                Ok(serde_json::json!({
                    "items_json": serde_json::to_string(&items)?,
                    "next_cursor": next_cursor,
                }))
            }
            _ => Err(BrainError::Invalid(format!(
                "unknown child capability {capability:?}"
            ))),
        }
    }

    async fn execute_component_parent_capability(
        self: &Arc<Self>,
        session_id: &str,
        parent_id: &str,
        capability: &str,
        operation_id: &str,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        match capability {
            "tool.parent.metadata" => Ok(serde_json::Value::String(serde_json::to_string(
                &serde_json::json!({"session_id": session_id, "parent_id": parent_id}),
            )?)),
            "tool.parent.inspect" => Ok(serde_json::Value::String(serde_json::to_string(
                &self.get(parent_id).await?,
            )?)),
            "tool.parent.events" => self.component_event_page(parent_id, &request).await,
            "tool.parent.send" => {
                let body = parse_tool_request_json(&request)?;
                let content = tool_message_content(&body)?;
                let (turn_id, seq) = self
                    .message_with_metadata_idempotent(
                        parent_id,
                        content,
                        HashMap::new(),
                        Some(operation_id),
                    )
                    .await?;
                Ok(serde_json::Value::String(serde_json::to_string(
                    &serde_json::json!({"turn_id": turn_id, "seq": seq}),
                )?))
            }
            _ => Err(BrainError::Invalid(format!(
                "unknown parent capability {capability:?}"
            ))),
        }
    }

    async fn component_event_page(
        &self,
        session_id: &str,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let head = self.journal.get_head(session_id).await?;
        let after = request
            .get("after_seq")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let limit = request
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(32)
            .clamp(1, 100) as usize;
        let page = self
            .journal
            .read_record_page(&crate::journal::RecordPageQuery {
                session_id,
                after,
                through_seq: head.last_seq,
                limit,
                max_bytes: crate::journal::DEFAULT_RECORD_PAGE_BYTES,
            })
            .await?;
        let items = page
            .entries
            .into_iter()
            .map(|entry| {
                serde_json::json!({
                    "seq": entry.seq,
                    "ts_ms": entry.ts_ms,
                    "record": entry.record,
                })
            })
            .collect::<Vec<_>>();
        Ok(serde_json::json!({
            "items_json": serde_json::to_string(&items)?,
            "next_cursor": page.next_after.map(|cursor| cursor.to_string()),
        }))
    }
}

fn required_tool_capability_string<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BrainError::Invalid(format!("Tool capability field {key:?} is required")))
}

fn parse_tool_request_json(request: &serde_json::Value) -> Result<serde_json::Value> {
    serde_json::from_str(required_tool_capability_string(request, "request_json")?)
        .map_err(|error| BrainError::Invalid(format!("Tool request_json: {error}")))
}

fn tool_message_content(value: &serde_json::Value) -> Result<MessageRequestContent> {
    let content = value
        .as_str()
        .or_else(|| value.get("content").and_then(serde_json::Value::as_str))
        .ok_or_else(|| {
            BrainError::Invalid("Tool message request must be a string or contain content".into())
        })?;
    Ok(MessageRequestContent::String(content.parse().map_err(
        |error| BrainError::Invalid(format!("Tool message content: {error}")),
    )?))
}

#[async_trait::async_trait]
impl crate::environment::SecretDeliveryPort for Brain {
    async fn redeem(
        &self,
        request: brain_protocol::environment::SecretDeliveryRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SecretMaterial> {
        use brain_protocol::environment::EnvironmentErrorCode;

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
                EnvironmentErrorCode::CapabilityUnavailable,
                false,
                "secret capability is absent, expired, or already redeemed",
            )
        })?;

        if grant.root_id != request.root_id.as_str()
            || grant.session_id != request.session_id.as_str()
            || grant.environment_id != request.environment_id.as_str()
            || request.target.root_id.as_str() != grant.root_id
            || request.target.session_id.as_str() != grant.session_id
            || !grant
                .binding_refs
                .contains(request.target.binding_ref.as_str())
            || request.target.kind != brain_protocol::environment::TargetKind::Environment
        {
            return Err(secret_delivery_error(
                EnvironmentErrorCode::BindingConflict,
                false,
                "secret capability does not match the exact Environment/session/target scope",
            ));
        }

        let head = self
            .journal
            .get_head(&grant.session_id)
            .await
            .map_err(|_| {
                secret_delivery_error(
                    EnvironmentErrorCode::TemporarilyUnavailable,
                    true,
                    "secret custody is temporarily unavailable",
                )
            })?;
        if head.doc.root_id != grant.root_id || head.doc.state != SessionLifecycle::Open {
            return Err(secret_delivery_error(
                EnvironmentErrorCode::CapabilityUnavailable,
                false,
                "session no longer permits managed secret delivery",
            ));
        }
        let (_, secrets) = self.root_execution_secrets(&head.doc).await.map_err(|_| {
            secret_delivery_error(
                EnvironmentErrorCode::TemporarilyUnavailable,
                true,
                "secret custody is temporarily unavailable",
            )
        })?;
        let mut values = HashMap::with_capacity(grant.env_names.len());
        for name in grant.env_names {
            let value = secrets.environment_env.get(&name).ok_or_else(|| {
                secret_delivery_error(
                    EnvironmentErrorCode::CapabilityUnavailable,
                    false,
                    "immutable managed secret material is incomplete",
                )
            })?;
            values.insert(name, value.clone());
        }
        Ok(crate::environment::SecretMaterial::new(values))
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
        "brain.sandbox-file-effect.v1\0{action}\0{session_id}\0{key}"
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

/// HTTP header-value grammar without pulling an HTTP client into the core: visible ASCII plus
/// space and tab, no control bytes (RFC 9110 field-value).
fn validate_header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || (b' '..=b'~').contains(&byte) || byte >= 0x80)
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
    target: &brain_protocol::environment::SandboxTarget,
    generation: &str,
    path: &str,
) -> Result<brain_protocol::environment::SandboxFileRequest> {
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
) -> Result<brain_protocol::environment::ObjectReference> {
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
    target: &brain_protocol::environment::SandboxTarget,
    generation: &str,
    path: &str,
    object: Option<brain_protocol::environment::ObjectReference>,
    ticket: &crate::storage::StorageTransferTicket,
    direction: &str,
    overwrite: bool,
) -> Result<brain_protocol::environment::SandboxCopyRequest> {
    let mut request: brain_protocol::environment::SandboxCopyRequest =
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
    result: &brain_protocol::environment::SandboxCopyResult,
    operation_id: &str,
    request_digest: &brain_protocol::environment::Digest,
) -> Result<()> {
    if result.operation_id.as_str() != operation_id || &result.request_digest != request_digest {
        return Err(BrainError::Environment(
            "sandbox copy receipt identity mismatch".into(),
        ));
    }
    Ok(())
}

fn sandbox_status_matches_target(
    status: &brain_protocol::environment::SandboxStatus,
    target: &brain_protocol::environment::SandboxTarget,
) -> Result<bool> {
    Ok(serde_json::to_value(&status.target)? == serde_json::to_value(target)?)
}

fn sandbox_status_releases_slot(status: &brain_protocol::environment::SandboxStatus) -> bool {
    matches!(
        status.state,
        brain_protocol::environment::SandboxState::Gone
            | brain_protocol::environment::SandboxState::Terminated
    )
}

fn sandbox_gone_status(
    current: &brain_protocol::environment::SandboxStatus,
    reason: &str,
) -> Result<brain_protocol::environment::SandboxStatus> {
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
    target: &brain_protocol::environment::SandboxTarget,
    generation: &str,
    path: &str,
    content: &[u8],
    overwrite: bool,
) -> Result<brain_protocol::environment::SandboxFileWriteRequest> {
    let mut request: brain_protocol::environment::SandboxFileWriteRequest =
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
    result: &brain_protocol::environment::SandboxFileWriteResult,
    operation_id: &str,
    request_digest: &brain_protocol::environment::Digest,
) -> Result<()> {
    if result.operation_id.as_str() != operation_id || &result.request_digest != request_digest {
        return Err(BrainError::Environment(
            "sandbox file write receipt identity mismatch".into(),
        ));
    }
    Ok(())
}

fn sandbox_search_request(
    target: &brain_protocol::environment::SandboxTarget,
    generation: &str,
    path: &str,
    expression: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<crate::environment::SandboxSearchRequest> {
    Ok(crate::environment::SandboxSearchRequest {
        target: target.clone(),
        expected_generation: generation.to_owned(),
        path: path.to_owned(),
        expression: expression.to_owned(),
        cursor: cursor.map(str::to_owned),
        limit,
    })
}

// ---------------------------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------------------------

struct Resident {
    st: TurnState,
    key: ProviderKey,
    managed_bindings: Arc<HashMap<String, crate::environment::ManagedBinding>>,
    /// Keeps the root-scoped OnceCell alive while any root/descendant actor is resident.
    _root_secrets: Arc<RootSecretCell>,
    message_replays: HashMap<String, MessageReplay>,
}

struct Running {
    handle: tokio::task::JoinHandle<(TurnState, RunningOutcome)>,
    cancel: CancellationToken,
    key: ProviderKey,
    managed_bindings: Arc<HashMap<String, crate::environment::ManagedBinding>>,
    root_secrets: Arc<RootSecretCell>,
    message_replays: HashMap<String, MessageReplay>,
    _heartbeat: LeaseHeartbeatGuard,
}

/// A lease renewal is deliberately independent from recovery scheduling. Long provider, Environment,
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

struct RecoveredTurn {
    turn: String,
    context: HashMap<String, String>,
    rounds: u64,
    tool_calls: u64,
}

const MANAGED_UNKNOWN_SANDBOX_REASON: &str = "managed_operation_unknown_cleanup";

fn managed_operation_running_status(
    operation: &brain_protocol::environment::OperationRef,
    expires_at_ms: std::num::NonZeroU64,
) -> Result<brain_protocol::environment::SandboxStatus> {
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
    operation: &brain_protocol::environment::OperationRef,
) -> Result<brain_protocol::environment::SandboxStatus> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActorStartup {
    Lazy,
    Recovery,
}

fn can_discard_under_pressure(resident: &Option<Resident>) -> bool {
    !has_pending_terminal_ack(resident)
        && !has_reserved_storage_upload(resident)
        && resident.as_ref().is_none_or(|resident| {
            resident.st.head.state != SessionLifecycle::Deleting
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
                if r.st.head.state == SessionLifecycle::Deleting {
                    resident = Some(r);
                    if let Err(error) = delete_session(&brain, &session_id, &mut resident).await {
                        tracing::warn!(session = %session_id, error = %error, "background session deletion will retry");
                    }
                    return;
                } else if r.st.head.state == SessionLifecycle::Ending {
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
                    // environment materialization crosses that boundary.
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
                                // signal cancellation. A cancellation-resistant provider/Environment can
                                // finish its external wait, but every late journal decision loses
                                // the fence and descendants already observe admission closed.
                                if let Some(task) = running.take() {
                                    task.cancel.cancel();
                                    drop(task);
                                }
                                let pending = doc.state == SessionLifecycle::Ending;
                                // The response proves the constant-size durable admission fence;
                                // descendant traversal and Environment release happen only afterwards.
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
                    Command::MaterializeEnvironment { environment_name, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_materialize_environment(
                            &brain,
                            &session_id,
                            &mut resident,
                            &environment_name,
                        )
                        .await;
                        discard_if_fenced(&out, &mut resident);
                        let _ = reply.send(out);
                    }
                    Command::WriteEnvironmentFile {
                        environment_name,
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
                        let out = do_write_environment_file(
                            &brain,
                            &session_id,
                            &mut resident,
                            &environment_name,
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
                    Command::CopyStorageToEnvironment {
                        environment_name,
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
                        let out = do_copy_storage_to_environment(
                            &brain,
                            &session_id,
                            &mut resident,
                            &environment_name,
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
                    Command::CopyEnvironmentToStorage {
                        environment_name,
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
                        let out = do_copy_environment_to_storage(
                            &brain,
                            &session_id,
                            &mut resident,
                            &environment_name,
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
    managed_bindings: Arc<HashMap<String, crate::environment::ManagedBinding>>,
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
        )) if matches!(
            st.head.active_phase,
            Some(TurnPhase::ManagedRunning | TurnPhase::ManagedCancelling)
        ) && matches!(
            error,
            BrainError::EnvironmentUnavailable(_) | BrainError::Cancelled
        ) =>
        {
            // The managed intent is already durable and the effect may have crossed the Environment
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
    if head.doc.state == SessionLifecycle::Deleted {
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
                .is_some_and(|upload| upload.state == UploadReservationState::Published),
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
                    outcome: ToolOutcome::Interrupted,
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
                    stop_reason: TurnStopReason::Interrupted,
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
    let fork_context = if head.doc.context.is_none() {
        materialize_context_fork(brain, &head.doc).await?
    } else {
        Vec::new()
    };
    let history = materialize_session_history(&head.doc, &entries, &fork_context)?;
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
    // Re-preparing its managed bindings here recreates Environment definition rows immediately before
    // `purge_tree` removes them, so every cold deletion retry sees another nonempty purge page.
    let managed_bindings = if head.doc.state == SessionLifecycle::Deleting {
        Arc::new(HashMap::new())
    } else {
        brain.prepare_managed_session(session_id, &head.doc).await?
    };
    let persisted_head = head.doc.clone();
    let mut resident = Resident {
        st: TurnState {
            history,
            fork_context,
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
            materialize_session_history(&resident.st.head, &entries, &resident.st.fork_context)?;
    }
    if recover_managed_calls(brain, session_id, &mut resident, &entries).await? {
        entries = brain
            .journal
            .read_records_through(session_id, context_after, resident.st.head.last_seq)
            .await?;
        resident.st.history =
            materialize_session_history(&resident.st.head, &entries, &resident.st.fork_context)?;
    }
    if resident.st.head.root_id == session_id {
        let environments = resident
            .st
            .head
            .environment_targets
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for environment in environments {
            let state = resident.st.head.environment_targets[&environment].state;
            if state == brain_protocol::environment::SandboxState::Creating {
                materialize_environment_resident(brain, session_id, &mut resident, &environment)
                    .await?;
            } else {
                reconcile_environment_expiry(brain, session_id, &mut resident, &environment)
                    .await?;
            }
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

async fn do_materialize_environment(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    environment_name: &str,
) -> Result<brain_protocol::environment::SandboxStatus> {
    let r = ensure_resident(brain, session_id, resident).await?;
    materialize_environment_resident(brain, session_id, r, environment_name).await
}

async fn environment_for_effect(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &TurnState,
    environment_name: &str,
    generation: &str,
) -> Result<brain_protocol::environment::SandboxTarget> {
    use brain_protocol::environment::SandboxState;

    if st.head.ended || st.head.state != SessionLifecycle::Open {
        return Err(BrainError::SessionDeleted(session_id.to_owned()));
    }
    let (root_state, root_ended, status) = if st.head.root_id == session_id {
        (
            st.head.state,
            st.head.ended,
            st.head.environment_targets.get(environment_name).cloned(),
        )
    } else {
        let root = brain.journal.get_head(&st.head.root_id).await?;
        (
            root.doc.state,
            root.doc.ended,
            root.doc.environment_targets.get(environment_name).cloned(),
        )
    };
    if root_ended || root_state != SessionLifecycle::Open {
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
async fn do_write_environment_file(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    environment_name: &str,
    operation_id: String,
    generation: String,
    path: String,
    content_base64: String,
    overwrite: bool,
) -> Result<brain_protocol::environment::FileEntry> {
    let r = ensure_resident(brain, session_id, resident).await?;
    let target =
        environment_for_effect(brain, session_id, &r.st, environment_name, &generation).await?;
    let files = brain.environment_files_port(&r.st.head, environment_name)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&content_base64)
        .map_err(|_| BrainError::Invalid("environment file content is not valid base64".into()))?;
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
                action: format!("environment:{environment_name}:write_inline"),
                path: path.clone(),
            },
        )],
    )
    .await?;
    let result = files.write(request).await.map_err(|error| {
        if error.code == brain_protocol::environment::EnvironmentErrorCode::BindingConflict {
            BrainError::IdempotencyConflict
        } else {
            map_environment_port_error(error)
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
                action: format!("environment:{environment_name}:write_inline"),
                path,
                replayed: result.replayed,
            },
        )],
    )
    .await?;
    Ok(result.file)
}

#[allow(clippy::too_many_arguments)]
async fn do_copy_storage_to_environment(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    environment_name: &str,
    operation_id: String,
    generation: String,
    key: String,
    path: String,
    overwrite: bool,
) -> Result<brain_protocol::environment::FileEntry> {
    let r = ensure_resident(brain, session_id, resident).await?;
    let target =
        environment_for_effect(brain, session_id, &r.st, environment_name, &generation).await?;
    let files = brain.environment_files_port(&r.st.head, environment_name)?;
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
    let result = files.transfer(request).await.map_err(|error| {
        if error.code == brain_protocol::environment::EnvironmentErrorCode::BindingConflict {
            BrainError::IdempotencyConflict
        } else {
            map_environment_port_error(error)
        }
    })?;
    validate_sandbox_copy_result(&result, &operation_id, &request_digest)?;
    if result.object.is_some() {
        return Err(BrainError::Environment(
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
async fn do_copy_environment_to_storage(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    environment_name: &str,
    operation_id: String,
    generation: String,
    key: String,
    path: String,
    overwrite: bool,
) -> Result<crate::storage::StorageObject> {
    let r = ensure_resident(brain, session_id, resident).await?;
    let target =
        environment_for_effect(brain, session_id, &r.st, environment_name, &generation).await?;
    let files = brain.environment_files_port(&r.st.head, environment_name)?;
    let entry = files
        .stat(sandbox_file_request(&target, &generation, &path)?)
        .await
        .map_err(map_environment_port_error)?;
    if entry.kind != brain_protocol::environment::FileEntryKind::File {
        return Err(BrainError::Invalid(
            "sandbox copy source must be a regular file".into(),
        ));
    }
    let transfer_digest = hash_create_key(&format!(
        "brain.sandbox-storage-transfer.v1\0{session_id}\0{operation_id}"
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
        if error.code == brain_protocol::environment::EnvironmentErrorCode::BindingConflict {
            BrainError::IdempotencyConflict
        } else {
            map_environment_port_error(error)
        }
    })?;
    validate_sandbox_copy_result(&result, &operation_id, &request_digest)?;
    let exported = result.object.as_ref().ok_or_else(|| {
        BrainError::Environment("sandbox export omitted its uploaded object identity".into())
    })?;
    if exported.object_id.as_str() != ticket.object_id || exported.bytes != entry.bytes {
        return Err(BrainError::Environment(
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

async fn materialize_environment_resident(
    brain: &Arc<Brain>,
    session_id: &str,
    r: &mut Resident,
    environment_name: &str,
) -> Result<brain_protocol::environment::SandboxStatus> {
    use brain_protocol::environment::SandboxState;

    if r.st.head.root_id != session_id {
        return Err(BrainError::Invalid(
            "environment materialization must be driven by the root actor".into(),
        ));
    }
    if r.st.head.ended || matches!(r.st.head.state.as_str(), "ending" | "ended" | "deleting") {
        return Err(BrainError::SessionDeleted(session_id.to_owned()));
    }
    let declaration =
        r.st.head
            .prefix
            .environments
            .get(environment_name)
            .ok_or_else(|| {
                BrainError::Invalid(format!(
                    "session has no environment named {environment_name:?}"
                ))
            })?;
    let declaration = legacy_environment(declaration)?;
    if declaration.profile.kind != brain_protocol::session::EnvironmentProfileKind::Computer {
        return Err(BrainError::Invalid(format!(
            "environment {environment_name:?} is not a computer environment"
        )));
    }
    let adapter = brain
        .environments
        .resolve(declaration.extension.as_str())?
        .clone();
    let now = crate::wall_ms();
    let current =
        r.st.head
            .environment_targets
            .get(environment_name)
            .cloned()
            .unwrap_or(initial_environment(session_id, environment_name)?);
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
                BrainError::Journal("creating environment lacks generation intent".into())
            })?
    } else {
        crate::mint_id("gen", 20)
    };
    let target = environment_target(session_id, environment_name)?;
    let creating: brain_protocol::environment::SandboxStatus =
        serde_json::from_value(serde_json::json!({
            "state": "creating",
            "target": target,
            "generation": generation_intent,
            "changed_at_ms": now,
            "expires_at_ms": null,
        }))?;
    if current.state != SandboxState::Creating {
        r.st.head
            .environment_targets
            .insert(environment_name.to_owned(), creating.clone());
        let seq = r.st.take_seq();
        commit(
            brain,
            session_id,
            &mut r.st,
            vec![(
                seq,
                Record::EnvironmentChanged {
                    environment: environment_name.to_owned(),
                    status: creating,
                },
            )],
        )
        .await?;
    }

    let request = environment_create_request(&r.st.head, environment_name, &generation_intent)?;
    let status = adapter
        .preparation
        .materialize(request)
        .await
        .map_err(map_environment_port_error)?;
    if serde_json::to_value(&status.target)?
        != serde_json::to_value(environment_target(session_id, environment_name)?)?
    {
        return Err(BrainError::Environment(
            "Environment returned a different logical target".into(),
        ));
    }
    if matches!(
        status.state,
        SandboxState::Running | SandboxState::Suspended
    ) && (status.generation.is_none()
        || status.target_ref.is_none()
        || status.expires_at_ms.is_none())
    {
        return Err(BrainError::Environment(
            "materialized environment receipt lacks generation, target_ref, or expiry".into(),
        ));
    }
    r.st.head
        .environment_targets
        .insert(environment_name.to_owned(), status.clone());
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::EnvironmentChanged {
                environment: environment_name.to_owned(),
                status: status.clone(),
            },
        )],
    )
    .await?;
    Ok(status)
}

async fn reconcile_environment_expiry(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
    environment_name: &str,
) -> Result<()> {
    use brain_protocol::environment::SandboxState;

    let Some(current) = resident
        .st
        .head
        .environment_targets
        .get(environment_name)
        .cloned()
    else {
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
    let adapter = brain.environment_adapter(&resident.st.head, environment_name)?;
    let status = if let Some(files) = adapter.files {
        match files.status(current.target.clone()).await {
            Ok(status) => status,
            Err(error)
                if error.code == brain_protocol::environment::EnvironmentErrorCode::SandboxGone =>
            {
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
            Err(error) => return Err(map_environment_port_error(error)),
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
    resident
        .st
        .head
        .environment_targets
        .insert(environment_name.to_owned(), status.clone());
    let seq = resident.st.take_seq();
    commit(
        brain,
        session_id,
        &mut resident.st,
        vec![(
            seq,
            Record::EnvironmentChanged {
                environment: environment_name.to_owned(),
                status,
            },
        )],
    )
    .await
}

/// Admits one message: journals the decision, pokes the adapter, environments back the turn
/// identity. 202 semantics: the reply happens after this commit succeeds.
async fn admit(
    brain: &Arc<Brain>,
    session_id: &str,
    r: &mut Resident,
    content: Vec<ContentBlock>,
    metadata: HashMap<String, String>,
    idempotency: Option<MessageIdentity>,
) -> Result<(String, u64, CancellationToken)> {
    if r.st.head.state == SessionLifecycle::Failed {
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
    r.st.head.state = SessionLifecycle::Open;
    r.st.head.turn = Some(turn_id.clone());
    r.st.head.active_phase = Some(TurnPhase::ReadyToBuildModelRequest);
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
    let (prefix, _) = build_prefix(&r.st.head.prefix, brain.cfg.default_max_rounds)?;
    let base_url = r.st.head.prefix.base_url.clone().unwrap_or_default();
    let session = SessionConfig::new(prefix.clone(), r.key.clone(), base_url);
    Ok(TurnRun {
        engine: {
            let services: Arc<dyn crate::turn::EngineServices> = brain.clone();
            Arc::downgrade(&services)
        },
        agentloop: brain.agentloop_registry.resolve(
            r.st.head
                .prefix
                .agentloop
                .as_ref()
                .ok_or_else(|| BrainError::Journal("session has no sealed agent loop".into()))?,
        )?,
        message,
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        prefix,
        session,
        provider: brain.model_registry.resolve(
            r.st.head.prefix.model_component.as_ref().ok_or_else(|| {
                BrainError::Journal("session has no sealed Model component".into())
            })?,
        )?,
        provider_name: r.st.head.prefix.provider.clone(),
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
        tool_registry: brain.tool_registry.clone(),
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
            tracing::info!(session = %session_id, turn = %turn_id, stop = report.stop_reason.as_str(), rounds = report.rounds, "turn done");
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
        BrainError::EnvironmentUnavailable(_) => ("environment_unavailable", false),
        BrainError::Agentloop(_) => ("agentloop_error", false),
        BrainError::SessionFailed(_) => ("session_failed", true),
        BrainError::Fenced => return Ok(()), // a newer owner exists; nothing to write
        _ => ("internal", false),
    };
    let failed_seq = st.take_seq();
    let state_seq = st.take_seq();
    if session_fatal && !st.head.ended {
        st.head.state = SessionLifecycle::Failed;
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
                state: st.head.state,
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
    let environment_secrets_b64 = root.doc.environment_secrets_b64.clone();
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
        state: SessionLifecycle::Open,
        failure: None,
        turn: Some(turn_id.clone()),
        active_phase: Some(TurnPhase::ReadyToBuildModelRequest),
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
        environment_secrets_b64,
        session_storage_bytes: 0,
        storage_reserved_bytes: 0,
        tenant_metered_storage_bytes: 0,
        storage_upload: None,
        storage_delete: None,
        pending_customer_acks: Vec::new(),
        pending_managed_acks: Vec::new(),
        environment_targets: HashMap::new(),
        tool_setups: HashMap::new(),
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

/// W6.1: the sealed session network is the create-time merge of the session's requested
/// policy with every granted tool's declared needs. Effective allowlist =
/// (union of tool declarations and session allows) minus session denies. Product-specific
/// infrastructure denials are supplied by the hosting composition and enforced by its environment.
/// Declaration and merge only — no per-tool runtime isolation is claimed.
pub(crate) fn merge_session_network(
    requested: Option<&brain_protocol::session::NetworkPolicy>,
    decls: &[crate::config::ToolDecl],
) -> Result<serde_json::Value> {
    fn host_of(destination: &serde_json::Value) -> Option<&str> {
        destination.get("host").and_then(serde_json::Value::as_str)
    }
    fn denied(host: &str, deny: &[String]) -> bool {
        deny.iter().any(|rule| {
            if let Some(suffix) = rule.strip_prefix("*.") {
                host.len() > suffix.len() + 1
                    && host
                        .to_ascii_lowercase()
                        .ends_with(&suffix.to_ascii_lowercase())
                    && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
            } else {
                host.eq_ignore_ascii_case(rule)
            }
        })
    }

    let mut declared: Vec<serde_json::Value> = Vec::new();
    let mut seen: std::collections::HashSet<Vec<u8>> = std::collections::HashSet::new();
    for decl in decls {
        for destination in &decl.network_needs {
            if seen.insert(serde_jcs::to_vec(destination)?) {
                declared.push(destination.clone());
            }
        }
    }

    let requested = requested.map(serde_json::to_value).transpose()?;
    let (outbound, session_destinations, deny): (&str, Vec<serde_json::Value>, Vec<String>) =
        match &requested {
            None => ("none", Vec::new(), Vec::new()),
            Some(value) => {
                let outbound = value
                    .get("outbound")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        BrainError::Invalid("network policy needs an outbound mode".into())
                    })?;
                let destinations = value
                    .get("destinations")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let deny = value
                    .get("deny")
                    .and_then(serde_json::Value::as_array)
                    .map(|rules| {
                        rules
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                (outbound, destinations, deny)
            }
        };

    match outbound {
        "public" => {
            if !deny.is_empty() {
                return Err(BrainError::Invalid(
                    "network deny rules require outbound \"none\" or \"allowlist\": nothing enforces a deny off the gateway path".into(),
                ));
            }
            // Public already covers every declared need.
            Ok(serde_json::json!({"outbound": "public"}))
        }
        "none" | "allowlist" => {
            let mut merged: Vec<serde_json::Value> = Vec::new();
            for destination in session_destinations.into_iter().chain(declared) {
                if let Some(host) = host_of(&destination) {
                    if denied(host, &deny) {
                        continue;
                    }
                }
                if !merged.contains(&destination) {
                    merged.push(destination);
                }
            }
            if merged.is_empty() {
                if outbound == "allowlist" {
                    return Err(BrainError::Invalid(
                        "the merged network allowlist is empty after session denies".into(),
                    ));
                }
                Ok(serde_json::json!({"outbound": "none"}))
            } else {
                Ok(serde_json::json!({"outbound": "allowlist", "destinations": merged}))
            }
        }
        other => Err(BrainError::Invalid(format!(
            "unknown network outbound mode {other:?}"
        ))),
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

fn materialize_session_history(
    head: &HeadDoc,
    entries: &[Entry],
    fork_context: &[Message],
) -> Result<Vec<Message>> {
    let own_history = crate::compact::materialize_history(entries, head.context.as_ref())?;
    let mut history = if head.context.is_none() {
        fork_context.to_vec()
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
/// traversal or Environment operation is allowed before this decision: a successful response therefore
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
            state: SessionLifecycle::Ending,
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
    if r.st.head.state != SessionLifecycle::Ending {
        drop(heartbeat);
        return Ok(r.st.head.state == SessionLifecycle::Ended
            || r.st.head.state == SessionLifecycle::Deleting);
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

    // Root environments belong to the whole tree. Ending a child never releases state shared
    // with its parent or siblings.
    if r.st.head.root_id == session_id {
        dematerialize_environments_for_end(brain, session_id, r).await?;
    }
    r.st.head.state = SessionLifecycle::Ended;
    r.st.head.active_phase = None;
    r.st.head.provider_attempt = None;
    let seq = r.st.take_seq();
    let rec = Record::State {
        state: SessionLifecycle::Ended,
        turn: None,
    };
    commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;
    drop(heartbeat);
    Ok(true)
}

async fn dematerialize_environments_for_end(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Resident,
) -> Result<()> {
    use brain_protocol::environment::SandboxState;

    let environments = resident
        .st
        .head
        .environment_targets
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for environment in environments {
        let current = resident.st.head.environment_targets[&environment].clone();
        if matches!(
            current.state,
            SandboxState::Gone | SandboxState::Terminated | SandboxState::NeverMaterialized
        ) {
            continue;
        }
        let adapter = brain.environment_adapter(&resident.st.head, &environment)?;
        let status = match adapter
            .preparation
            .dematerialize(current.target.clone())
            .await
        {
            Ok(status) => status,
            Err(error)
                if matches!(
                    error.code,
                    brain_protocol::environment::EnvironmentErrorCode::SandboxGone
                        | brain_protocol::environment::EnvironmentErrorCode::SandboxNotMaterialized
                ) =>
            {
                sandbox_gone_status(&current, "environment_reported_gone")?
            }
            Err(error) => return Err(map_environment_port_error(error)),
        };
        if !matches!(status.state, SandboxState::Gone | SandboxState::Terminated) {
            return Err(BrainError::Environment(format!(
                "environment {environment:?} dematerialization did not return a terminal state"
            )));
        }
        resident
            .st
            .head
            .environment_targets
            .insert(environment.clone(), status.clone());
        let seq = resident.st.take_seq();
        commit(
            brain,
            session_id,
            &mut resident.st,
            vec![(
                seq,
                Record::EnvironmentChanged {
                    environment,
                    status,
                },
            )],
        )
        .await?;
    }
    release_component_environments(brain, session_id, &resident.st.head).await?;
    Ok(())
}

async fn release_component_environments(
    brain: &Arc<Brain>,
    session_id: &str,
    head: &HeadDoc,
) -> Result<()> {
    let declarations = head
        .prefix
        .environments
        .iter()
        .filter_map(|(environment_id, declaration)| {
            component_environment(declaration)
                .ok()
                .map(|declaration| (environment_id.clone(), declaration.clone()))
        })
        .collect::<Vec<_>>();
    if declarations.is_empty() {
        return Ok(());
    }
    let registry = brain
        .component_environment_registry
        .as_ref()
        .ok_or_else(|| {
            BrainError::EnvironmentUnavailable(
                "component Environment release is unavailable".into(),
            )
        })?;
    for (environment_id, declaration) in declarations {
        registry
            .release(
                &declaration,
                crate::environment::ComponentEnvironmentRelease {
                    tenant_id: head.tenant_id.clone(),
                    session_id: session_id.to_owned(),
                    root_id: head.root_id.clone(),
                    parent_id: head.parent_id.clone(),
                    environment_id,
                    policy: serde_json::json!({ "network": head.prefix.network }),
                },
            )
            .await?;
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
                status.state = DeletionState::Retrying;
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
        BrainError::Environment(_) | BrainError::EnvironmentUnavailable(_) => {
            "sandbox_cleanup_failed"
        }
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
        r.st.head.state == SessionLifecycle::Ending
    };
    if end_pending && !continue_end_session(brain, session_id, resident).await? {
        return Err(BrainError::EnvironmentUnavailable(
            "session subtree end remains pending".into(),
        ));
    }
    let r = ensure_resident(brain, session_id, resident).await?;
    if r.st.head.state != SessionLifecycle::Deleting {
        // This decision is the admission fence for every later mutation. It lands before any
        // external cleanup, and remains indexed until all cleanup has succeeded.
        r.st.head.state = SessionLifecycle::Deleting;
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
                    state: SessionLifecycle::Deleting,
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
        state: DeletionState::Deleting,
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
        if r.st.head.state != SessionLifecycle::Deleting {
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
        release_component_environments(brain, session_id, &r.st.head).await?;
        let extensions =
            r.st.head
                .prefix
                .environments
                .values()
                .filter_map(|environment| legacy_environment(environment).ok())
                .filter(|environment| {
                    environment.profile.kind
                        == brain_protocol::session::EnvironmentProfileKind::Computer
                })
                .map(|environment| environment.extension.to_string())
                .collect::<HashSet<_>>();
        for extension in extensions {
            brain
                .environments
                .resolve(&extension)?
                .preparation
                .purge_tree(session_id)
                .await
                .map_err(map_environment_port_error)?;
        }
        if let Some(bundle_storage) = &brain.bundle_storage {
            bundle_storage.purge_root_bundles(session_id).await?;
        } else if !r.st.head.prefix.managed_bundles.is_empty() {
            return Err(BrainError::EnvironmentUnavailable(
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
            state: DeletionState::Succeeded,
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
            if upload.state == UploadReservationState::Published {
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

pub(crate) async fn complete_storage_upload_state(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    transfer_id: String,
) -> Result<crate::storage::StorageObject> {
    ensure_storage_readable(&st.head, session_id)?;
    let requested_expired = st.head.storage_upload.as_ref().is_some_and(|upload| {
        upload.transfer_id == transfer_id
            && upload.state == UploadReservationState::Reserved
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

    if upload.state == UploadReservationState::Completed {
        return storage.stat(session_id, &upload.key).await;
    }

    let object = if upload.state == UploadReservationState::Published {
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
        published.state = UploadReservationState::Published;
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
    completed.state = UploadReservationState::Completed;
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
        if upload.state == UploadReservationState::Completed && same {
            return storage.stat(session_id, &key).await;
        }
        if upload.state == UploadReservationState::InlineReserved && same {
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
            completed.state = UploadReservationState::Completed;
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
        if upload.state != UploadReservationState::Completed {
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
        state: UploadReservationState::InlineReserved,
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
    completed.state = UploadReservationState::Completed;
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

// ---------------------------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------------------------

fn default_system_prompt() -> String {
    "You are an autonomous engineering agent running in an isolated Linux workspace \
     (/workspace, ARM64). Use the tools to inspect and change files and run commands. \
     Sandbox files are live compute state, not durable storage. Copy anything that must survive \
     target loss into session storage explicitly."
        .to_string()
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
#[path = "../session_tests.rs"]
mod tests;
