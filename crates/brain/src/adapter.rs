//! The adapter seams: everything substrate-specific lives behind these traits.
//!
//! The brain core is generic — it owns sessions, the journal semantics, the sealed prefix,
//! the provider dialects, the turn loop and the API. WHERE tools run, WHERE the journal
//! persists and WHERE keys rest are adapters:
//!
//! - [`HandFactory`] / [`HandAdapter`] — tool execution + workspace lifecycle,
//! - [`crate::journal::JournalStore`] — journal persistence,
//! - [`crate::keys::KeyCustody`] — BYOK key custody,
//! - [`crate::session::ProviderFactory`] — model providers.
//!
//! Built-ins: the explicitly unsafe development adapters in this crate and the durable
//! single-node adapters in `brain-standalone`. `brain-aws` supplies neutral persistence and
//! custody adapters. A Hand implementation lives downstream and implements these traits; Brain
//! never depends on it. The `custom_adapter` integration test is the living example.
//!
//! Contract an adapter must hold:
//! - `call` runs ONE tool call to a terminal outcome, streams output through the sink as it
//!   happens, honours the cancel token, and NEVER returns an empty `content` (providers
//!   reject empty error results; say what happened);
//! - all methods take `&self`: parallel `call`s race, adapters carry their own interior
//!   mutability;
//! - everything an adapter must remember across process restarts goes in [`Self::state`]
//!   (opaque JSON, persisted in the journal head and handed back on [`HandFactory::open`]);
//! - a lost substrate is reported via `ensure_ready` (the core journals `hand_lost`; work is
//!   never replayed, I10) — never by hanging.

use crate::Result;
use brain_protocol::abi::ToolManifest;
use brain_protocol::session::{
    ExternalToolCallRequest, ExternalToolCallResponse, FileEntry, FileListSource, HandInfo,
};
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// A live output chunk sink: (stream, byte offset, text). The core turns these into
/// `tool.output` events; adapters never mint event seqs themselves.
pub type OutputSink = Arc<dyn Fn(&str, u64, String) + Send + Sync>;

/// One tool call as the adapter sees it.
#[derive(Debug, Clone)]
pub struct CallRequest {
    /// Brain-minted, durable, unique per attempt (the ABI operation id for remote hands).
    pub call_id: String,
    pub tool: String,
    pub input: Value,
    /// True when this call is one of several dispatched from a single assistant message.
    /// Substrates with lane semantics isolate parallel calls from each other.
    pub parallel: bool,
}

/// What one call produced. `outcome` uses the contract vocabulary:
/// `completed | failed | cancelled | deadline_exceeded | interrupted`.
#[derive(Debug, Clone)]
pub struct CallOutcome {
    pub outcome: String,
    /// Successful structured value before presentation formatting. Brain validates this against
    /// the sealed output schema immediately before it commits the result.
    pub value: Option<Value>,
    pub content: String,
    pub is_error: bool,
    pub exit_code: Option<i64>,
    pub duration_ms: u64,
    pub truncated: bool,
    /// Present only when a host-executed return-direct tool asks Brain to end the turn.
    pub terminal: Option<TerminalOutcome>,
}

/// A generic external executor may return a replayable client value or a structured turn error.
/// Brain does not interpret either payload; it only journals it with the turn terminal.
#[derive(Debug, Clone)]
pub enum TerminalOutcome {
    Complete {
        value: Value,
        metadata: std::collections::HashMap<String, String>,
    },
    Fail {
        error: brain_protocol::session::ApiError,
    },
}

impl CallOutcome {
    pub fn failed(content: impl Into<String>) -> Self {
        CallOutcome {
            outcome: "failed".into(),
            value: None,
            content: content.into(),
            is_error: true,
            exit_code: None,
            duration_ms: 0,
            truncated: false,
            terminal: None,
        }
    }
}

/// Trusted host-side execution registered under stable capability identifiers. A composition
/// advertises availability before a session is created; model-visible names never select code.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    fn supports(&self, capability: &str) -> bool;

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse>;
}

/// Default composition for deployments that do not expose host-executed tools.
pub struct DisabledToolExecutor;

#[async_trait::async_trait]
impl ToolExecutor for DisabledToolExecutor {
    fn supports(&self, _capability: &str) -> bool {
        false
    }

    async fn call(
        &self,
        _capability: &str,
        _request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        Err(crate::BrainError::Invalid(
            "no external tool executor is configured on this Brain host".into(),
        ))
    }
}

/// Why a substrate was lost (surfaced as the `hand.lost` event; never replayed).
#[derive(Debug, Clone)]
pub struct LostReport {
    pub reason: String,
}

/// A persisted artifact's metadata. `location` is adapter-defined (an S3 key, a local path);
/// the core stores it verbatim and hands it back for [`HandFactory::artifact_url`].
#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    pub bytes: u64,
    pub sha256: String,
    pub media_type: String,
    pub location: String,
}

/// A workspace listing returned by a substrate. The core owns the public list envelope;
/// adapters provide entries and whether they came from a live hand or its durable manifest.
#[derive(Debug, Clone)]
pub struct WorkspaceListing {
    pub entries: Vec<FileEntry>,
    pub source: FileListSource,
    pub synced_ms: Option<u64>,
}

/// Exact bytes plus metadata for one workspace file.
#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub entry: FileEntry,
    pub bytes: Vec<u8>,
}

/// The create-time facts an adapter may care about.
#[derive(Debug, Clone)]
pub struct HandSpec {
    pub session_id: String,
    pub hand_enabled: bool,
    /// `1gb` etc. — advisory; adapters refuse what they cannot offer, loudly, at create.
    pub shape: String,
    pub env: std::collections::HashMap<String, String>,
    /// Exact ordered per-session Hand manifest. URLs may be filled by the concrete adapter from
    /// its staged state immediately before `hello`.
    pub tool_manifest: ToolManifest,
    /// The sealed tool-manifest digest; remote Hands must serve exactly this set (I1).
    pub manifest_digest: String,
}

/// A seed file staged at session create.
pub struct SeedFile<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
    pub mode: Option<i64>,
}

/// One verified bundle staged at session create. The bytes are borrowed only for the staging
/// call and must never be copied into adapter state or Brain's journal.
pub struct ToolBundleFile<'a> {
    pub checksum: &'a str,
    pub bytes: &'a [u8],
    pub media_type: &'a str,
}

/// One session's tool-execution substrate. Opened per residency by the factory; all state
/// that must outlive the process goes through [`Self::state`].
#[async_trait::async_trait]
pub trait HandAdapter: Send + Sync {
    /// Makes the substrate ready to execute calls (launch, reconnect, re-materialise...).
    /// Returns a report when a PREVIOUS incarnation was lost on the way.
    async fn ensure_ready(&self) -> Result<Option<LostReport>>;

    /// Executes one call to a terminal outcome. Must stream via `sink` and honour `cancel`.
    async fn call(
        &self,
        req: CallRequest,
        cancel: CancellationToken,
        sink: OutputSink,
    ) -> CallOutcome;

    /// Fired at message admission, before the model round (e.g. speculative resume).
    fn on_message_admitted(&self) {}

    /// The turn is over: release connections, let the substrate idle/suspend.
    fn idle(&self) {}

    /// The workspace durability point (turn end). Remote substrates sync; local ones no-op.
    async fn checkpoint(&self) -> Result<()> {
        Ok(())
    }

    /// True when compute must be released before more work is admitted (e.g. a platform
    /// lifetime wall). The core then calls [`Self::release`] and journals the transition.
    fn must_release(&self) -> bool {
        false
    }

    /// Releases compute, keeps the workspace restorable. (`end`, wall.)
    async fn release(&self) -> Result<()>;

    /// The results of these calls are durably journaled; the substrate may forget them
    /// (remote hands release spill files here). Default: nothing to forget.
    async fn acknowledge(&self, _call_ids: &[String]) {}

    /// Workspace bytes as last known, for `StorageInfo`. Never billing authority (I9).
    fn workspace_bytes(&self) -> u64 {
        0
    }

    /// Lists one workspace subtree. A released remote substrate should answer from its last
    /// committed manifest without waking compute.
    async fn list_files(&self, _path: &str, _recursive: bool) -> Result<WorkspaceListing> {
        Err(crate::BrainError::HandUnavailable(
            "workspace file listing is not supported by this substrate".into(),
        ))
    }

    /// Reads one regular file, refusing before buffering more than `max_bytes`.
    async fn read_file(&self, _path: &str, _max_bytes: usize) -> Result<WorkspaceFile> {
        Err(crate::BrainError::HandUnavailable(
            "workspace file download is not supported by this substrate".into(),
        ))
    }

    /// Atomically overwrites one regular file. The core checkpoints immediately afterwards.
    async fn write_file(&self, _path: &str, _bytes: &[u8]) -> Result<FileEntry> {
        Err(crate::BrainError::HandUnavailable(
            "workspace file upload is not supported by this substrate".into(),
        ))
    }

    /// Copies a workspace file into durable artifact storage.
    async fn persist(
        &self,
        name: &str,
        path: &str,
        media_type: Option<&str>,
    ) -> Result<ArtifactMeta>;

    /// The contract-facing snapshot for `session.updated` / `GET /sessions/{id}`.
    fn hand_info(&self) -> HandInfo;

    /// Adapter-owned durable state; persisted in the journal head on every commit and handed
    /// back verbatim on the next `open`.
    fn state(&self) -> Value;
}

/// Opens and disposes per-session adapters.
#[async_trait::async_trait]
pub trait HandFactory: Send + Sync {
    /// Called once at session create: stage seed files, validate the spec (refuse an
    /// unsupported shape loudly), return the adapter's initial state.
    async fn create(
        &self,
        spec: &HandSpec,
        seeds: &[SeedFile<'_>],
        bundles: &[ToolBundleFile<'_>],
    ) -> Result<Value>;

    /// Opens the adapter for a residency, from the state the last commit persisted.
    async fn open(&self, spec: &HandSpec, state: Value) -> Result<Arc<dyn HandAdapter>>;

    /// Deletes everything the substrate stored for this session (workspace, artifacts).
    async fn purge(&self, session_id: &str) -> Result<()>;

    /// A retrievable URL for a persisted artifact, if this substrate can mint one.
    async fn artifact_url(&self, _session_id: &str, _location: &str) -> Option<String> {
        None
    }
}
