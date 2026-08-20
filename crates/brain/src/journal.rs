//! The session journal: every decision durable in one DynamoDB write (D9).
//!
//! One item collection per session. `HEAD` carries ownership (lease + fence), the sealed
//! configuration and the mutable session facts; `E#<seq>` items carry the decision records.
//! The journal is also the event log: SSE replay is a derivation over these records
//! (`events::derive`), and `seq` is both the journal order and the SSE `id:`.
//!
//! Concurrency rules (donor: aex-brain-domain, kept because each one answers a real outage):
//! - the (session, seq) key is the idempotency barrier: a redelivered decision loses the
//!   write, it never duplicates;
//! - the fence advances on claim only, never on renew (renewing must not fence out the owner);
//! - a `Fenced` failure on commit means a newer owner exists: the local fold is stale and
//!   must be discarded, never patched.
//!
//! Persistence is a seam: [`JournalStore`]. [`MemoryStore`] is the reference backend;
//! `brain-aws` carries the DynamoDB one; custom backends implement the trait.

use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::{BrainError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Bound on the tool-result content a single record may carry. DynamoDB items cap at 400 KiB;
/// this leaves generous room for the envelope and the parallel records of one decision.
pub const MAX_RECORD_CONTENT_BYTES: usize = 96 * 1024;

// ---------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------

/// One journaled decision. A closed enum: an unknown kind on read is a typed error, never a
/// silent passthrough (passthrough only ever hid corruption).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    /// The admitted user message. Not an SSE event (the caller knows what they sent); it is
    /// what rebuilds the user side of the model history.
    UserMessage {
        turn: String,
        content: Vec<ContentBlock>,
        /// Trusted turn context forwarded to host-executed tools. It is never rendered as model
        /// input and must survive replay with the admitted message.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        metadata: HashMap<String, String>,
    },
    TurnStarted {
        turn: String,
    },
    /// A complete assistant message, full-fidelity blocks. Only complete messages are
    /// journaled -- a stream that dies mid-message journals nothing.
    Assistant {
        turn: String,
        agent: String,
        content: Vec<ContentBlock>,
        stop: StopReason,
    },
    /// Raw provider usage for one round. Absent counters stay absent.
    Usage {
        turn: String,
        agent: String,
        provider: String,
        model: String,
        usage: Usage,
    },
    /// Journaled BEFORE dispatch: an ambiguous outcome is recorded as possibly-run.
    ToolCall {
        turn: String,
        agent: String,
        call: String,
        name: String,
        input: serde_json::Value,
        detach: bool,
    },
    ToolResult {
        turn: String,
        agent: String,
        call: String,
        name: String,
        /// `completed | failed | cancelled | deadline_exceeded | interrupted`.
        outcome: String,
        /// What the model was shown, bounded by [`MAX_RECORD_CONTENT_BYTES`].
        content: String,
        is_error: bool,
        exit_code: Option<i64>,
        duration_ms: u64,
        truncated: bool,
    },
    TurnCompleted {
        turn: String,
        stop_reason: String,
        rounds: u64,
        tool_calls: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<aex_contracts::session::TurnResult>,
    },
    TurnFailed {
        turn: String,
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
    /// A session/hand state transition worth telling clients about (`session.updated`).
    State {
        state: String,
        turn: Option<String>,
    },
    HandLost {
        turn: Option<String>,
        interrupted: Vec<String>,
        synced_ms: Option<u64>,
    },
    /// Linear, prefix-stable compaction: on fold, everything but the trailing `kept` messages
    /// is replaced by one user message carrying `summary`. The sealed prefix is untouched.
    Compacted {
        summary: String,
        kept: u64,
    },
}

impl Record {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Record::UserMessage { .. } => "user_message",
            Record::TurnStarted { .. } => "turn_started",
            Record::Assistant { .. } => "assistant",
            Record::Usage { .. } => "usage",
            Record::ToolCall { .. } => "tool_call",
            Record::ToolResult { .. } => "tool_result",
            Record::TurnCompleted { .. } => "turn_completed",
            Record::TurnFailed { .. } => "turn_failed",
            Record::State { .. } => "state",
            Record::HandLost { .. } => "hand_lost",
            Record::Compacted { .. } => "compacted",
        }
    }
}

/// A record with its journal position, as read back.
#[derive(Debug, Clone)]
pub struct Entry {
    pub seq: u64,
    pub ts_ms: u64,
    pub record: Record,
}

impl Record {
    /// The agent an activity record belongs to; `None` for session-level records.
    pub fn agent(&self) -> Option<&str> {
        match self {
            Record::Assistant { agent, .. }
            | Record::Usage { agent, .. }
            | Record::ToolCall { agent, .. }
            | Record::ToolResult { agent, .. } => Some(agent),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fold
// ---------------------------------------------------------------------------------------------

/// The model-visible history rebuilt from records. `fold` is a loop over `apply` so the cold
/// (rehydrate) and hot (in-turn append) paths cannot drift.
#[derive(Debug, Default, Clone)]
pub struct Fold {
    pub history: Vec<Message>,
    /// Consecutive tool_result records group into one user message (Anthropic requires tool
    /// results to arrive as one user message per batch); flushed by the next non-result record.
    pending_results: Vec<ContentBlock>,
    pub turns: u64,
}

impl Fold {
    /// Resumes a fold from an already-rebuilt history (in-turn compaction).
    pub fn from_history(history: Vec<Message>) -> Self {
        Fold {
            history,
            pending_results: Vec::new(),
            turns: 0,
        }
    }

    pub fn apply(&mut self, record: &Record) {
        // Subagent records (slice 8) never enter the ROOT history: a child's assistant
        // message is not the parent's, and -- load-bearing -- a child record landing
        // between two root tool results of one batch must not flush them into separate
        // user messages (providers require one user message per result batch). The
        // parent's own `task` ToolCall/ToolResult carry the parent's agent id and fold
        // normally.
        if let Some(agent) = record.agent()
            && agent != "root"
        {
            return;
        }
        match record {
            Record::UserMessage { content, .. } => {
                if self.pending_results.is_empty() {
                    self.history.push(Message {
                        role: Role::User,
                        content: content.clone(),
                    });
                } else {
                    // A recovered/cancelled turn may end immediately after its
                    // tool results. Merge the next real user text into that same
                    // user message so provider histories still alternate roles.
                    let mut merged = std::mem::take(&mut self.pending_results);
                    merged.extend(content.clone());
                    self.history.push(Message {
                        role: Role::User,
                        content: merged,
                    });
                }
            }
            Record::TurnStarted { .. } => self.turns += 1,
            Record::Assistant { content, .. } => {
                self.flush_results();
                self.history.push(Message {
                    role: Role::Assistant,
                    content: content.clone(),
                });
            }
            Record::ToolResult {
                call,
                content,
                is_error,
                ..
            } => {
                self.pending_results.push(ContentBlock::ToolResult {
                    tool_use_id: call.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                });
            }
            Record::Compacted { summary, kept } => {
                self.flush_results();
                let kept = (*kept as usize).min(self.history.len());
                let tail = self.history.split_off(self.history.len() - kept);
                self.history = Vec::with_capacity(tail.len() + 1);
                self.history.push(Message::user_text(summary.clone()));
                self.history.extend(tail);
            }
            Record::Usage { .. }
            | Record::ToolCall { .. }
            | Record::TurnCompleted { .. }
            | Record::TurnFailed { .. }
            | Record::State { .. }
            | Record::HandLost { .. } => {}
        }
    }

    fn flush_results(&mut self) {
        if !self.pending_results.is_empty() {
            self.history.push(Message::tool_results(std::mem::take(
                &mut self.pending_results,
            )));
        }
    }

    /// Terminal flush: called once all records are applied.
    pub fn finish(&mut self) {
        self.flush_results();
    }
}

pub fn fold(entries: &[Entry]) -> Fold {
    let mut f = Fold::default();
    for e in entries {
        f.apply(&e.record);
    }
    f.finish();
    f
}

// ---------------------------------------------------------------------------------------------
// HEAD
// ---------------------------------------------------------------------------------------------

/// Everything durable about a session that is not a record. Rewritten whole on each commit
/// (single writer under the fence), carried as one JSON attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadDoc {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<FailureDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    pub turns: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_ms: Option<u64>,
    /// True once `end` ran: compute released for good, the workspace is kept.
    #[serde(default)]
    pub ended: bool,
    pub prefix: PrefixDoc,
    /// Custody blob of the BYOK key, base64. Never the plaintext.
    pub key_b64: String,
    /// Custody blob of the per-server MCP header maps (`{server: {header: value}}`), base64.
    /// Present only when a declared server carries headers. Never the plaintext -- the
    /// contract marks `McpServerConfig.headers` writeOnly.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mcp_secrets_b64: String,
    pub manifest_digest: String,
    /// The contract-facing hand snapshot, refreshed from the adapter on every commit --
    /// what `session.updated` replay and `GET /sessions/{id}` serve.
    pub hand_info: aex_contracts::session::HandInfo,
    /// The hand adapter's own durable state. Opaque to the core: persisted verbatim, handed
    /// back on the next `HandFactory::open`.
    #[serde(default)]
    pub hand_state: serde_json::Value,
    /// Workspace bytes as the adapter last reported them. Storage info, never billing
    /// authority (I9).
    #[serde(default)]
    pub workspace_bytes: u64,
    #[serde(default)]
    pub artifacts: Vec<ArtifactDoc>,
}

impl HeadDoc {
    /// The hand snapshot before any adapter has reported: preparing, on the given shape.
    pub fn initial_hand_info(shape: &str) -> aex_contracts::session::HandInfo {
        use aex_contracts::session::{HandInfo, HandShape, HandState};
        HandInfo {
            generation: None,
            last_sync_at: None,
            live_jobs: None,
            shape: match shape {
                "2gb" => HandShape::X2gb,
                "4gb" => HandShape::X4gb,
                "8gb" => HandShape::X8gb,
                _ => HandShape::X1gb,
            },
            started_at: None,
            state: HandState::Preparing,
            wall_deadline_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureDoc {
    pub code: String,
    pub message: String,
    pub at_ms: u64,
}

/// The sealed create-time configuration, minus the credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixDoc {
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Builtin tool names, in declaration order (order is cache-visible).
    pub tools: Vec<String>,
    /// Declared MCP servers with their negotiated spec versions. Digested via the sealed
    /// prefix; empty for sessions without MCP.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<McpServerDoc>,
    /// The FULL resolved MCP tool declarations, sealed at create. Carrying the schemas here
    /// is what makes rehydration deterministic and I/O-free: a server-side schema drift can
    /// change nothing until the customer forks a new session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_tools: Vec<McpToolDoc>,
    /// Exact host-executed tool declarations and policies, sealed at create. The executor
    /// address and credential are deliberately process configuration and never live here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_tools: Vec<aex_contracts::session::ExternalToolConfig>,
    pub hand_enabled: bool,
    pub shape: String,
    pub sync_interval_seconds: u64,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// One declared MCP server, minus its credentials (those live in custody via
/// `HeadDoc::mcp_secrets_b64`). `spec_version` is the NEGOTIATED protocol revision --
/// `2026-07-28` for the stateless spec, an initialization-era date for the legacy adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerDoc {
    pub name: String,
    pub url: String,
    pub spec_version: String,
}

/// One sealed MCP tool: the namespaced name the model sees, the routing identity
/// (server + remote name), and the schema rendered verbatim into every request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolDoc {
    /// `server__tool`, validated against the provider tool-name rule at create.
    pub name: String,
    pub server: String,
    pub remote_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// One persisted artifact: metadata only; the bytes live wherever the adapter put them
/// (`location` is adapter-defined and fed back to `HandFactory::artifact_url`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDoc {
    pub name: String,
    pub location: String,
    pub bytes: u64,
    pub sha256: String,
    pub media_type: String,
    pub created_ms: u64,
}

/// The claim a hydrating owner holds. Every commit is conditioned on it.
#[derive(Debug, Clone)]
pub struct Lease {
    pub fence: u64,
    pub last_seq: u64,
}

#[derive(Debug, Clone)]
pub struct Head {
    pub session_id: String,
    pub doc: HeadDoc,
    pub fence: u64,
    pub last_seq: u64,
}

// ---------------------------------------------------------------------------------------------
// Store: the persistence seam
// ---------------------------------------------------------------------------------------------
//
// [`JournalStore`] is the adapter trait: any backend that can honour these semantics can
// carry the journal. The semantics are not negotiable --
// - `create` refuses an existing session;
// - `claim` is the ONLY operation that advances the fence; it fails (`Fenced`) while another
//   live owner holds the lease, and may steal an expired one (plus grace);
// - `commit` is atomic: all records plus the head update land or nothing does; it fails
//   `Fenced` when the owner/fence does not match OR any record seq already exists (the
//   (session, seq) key is the idempotency barrier -- a redelivered decision loses the write,
//   it never duplicates);
// - `release` with a stale fence is a silent no-op (the releaser was superseded).
//
// Built-ins: [`MemoryStore`] here (local mode: full semantics, no durability) and
// `brain_aws::DynamoJournal` (production). The shared tests in this module run against any
// store; run them against yours.

/// Key shape shared by every backend that keys records textually: zero-padded so that
/// lexicographic order is numeric order (`E#10` must not sort before `E#9`).
pub fn record_sk(seq: u64) -> String {
    format!("E#{seq:020}")
}

pub fn session_pk(session_id: &str) -> String {
    format!("S#{session_id}")
}

/// How long a lease lives without renewal, and how much longer a steal waits beyond expiry.
/// The grace absorbs clock skew between instances; the fence, not the clock, decides whether
/// a stale owner can write.
pub const LEASE_MS: u64 = 60_000;
pub const STEAL_GRACE_MS: u64 = 5_000;

#[async_trait::async_trait]
pub trait JournalStore: Send + Sync {
    async fn create(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        owner: &str,
        now_ms: u64,
    ) -> Result<()>;
    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head>;
    async fn get_head(&self, session_id: &str) -> Result<Head>;
    async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>>;
    #[allow(clippy::too_many_arguments)]
    async fn commit(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
        now_ms: u64,
    ) -> Result<()>;
    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()>;
    async fn purge(&self, session_id: &str) -> Result<u64>;
    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>>;
}

/// The journal as the rest of the brain sees it: a store plus this instance's owner
/// identity. All fence/lease bookkeeping the caller needs rides in [`Lease`].
#[derive(Clone)]
pub struct Journal {
    store: Arc<dyn JournalStore>,
    owner: String,
}

impl Journal {
    pub fn new(store: Arc<dyn JournalStore>, owner: impl Into<String>) -> Self {
        Self {
            store,
            owner: owner.into(),
        }
    }

    /// The local-mode journal: full semantics, no durability, no dependencies.
    pub fn new_memory(owner: impl Into<String>) -> Self {
        Self::new(Arc::new(MemoryStore::default()), owner)
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The same store under a different owner identity. Exists to test (and later simulate)
    /// multi-instance fencing; production instances each construct their own `Journal`.
    pub fn cloned_as(&self, owner: impl Into<String>) -> Journal {
        Journal {
            store: self.store.clone(),
            owner: owner.into(),
        }
    }

    pub async fn create(&self, session_id: &str, doc: &HeadDoc, first: &Record) -> Result<()> {
        self.store
            .create(session_id, doc, first, &self.owner, crate::wall_ms())
            .await
    }

    pub async fn claim(&self, session_id: &str) -> Result<Head> {
        self.store
            .claim(session_id, &self.owner, crate::wall_ms())
            .await
    }

    pub async fn get_head(&self, session_id: &str) -> Result<Head> {
        self.store.get_head(session_id).await
    }

    pub async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>> {
        self.store.read_records(session_id, after).await
    }

    /// One decision, one durable write. `high_water` is the highest seq allocated by the
    /// session -- including ephemeral (never-journaled) event seqs -- so a rehydrated
    /// session never re-issues an id a client may already have seen.
    pub async fn commit(
        &self,
        session_id: &str,
        lease: &mut Lease,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
    ) -> Result<()> {
        self.store
            .commit(
                session_id,
                &self.owner,
                lease.fence,
                records,
                doc,
                high_water,
                crate::wall_ms(),
            )
            .await?;
        lease.last_seq = high_water;
        Ok(())
    }

    pub async fn release(&self, session_id: &str, lease: &Lease) -> Result<()> {
        self.store
            .release(session_id, &self.owner, lease.fence)
            .await
    }

    pub async fn purge(&self, session_id: &str) -> Result<u64> {
        self.store.purge(session_id).await
    }

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        self.store.list_sessions(limit).await
    }
}

// ---------------------------------------------------------------------------------------------
// The in-memory backend
// ---------------------------------------------------------------------------------------------

struct MemSession {
    doc: HeadDoc,
    fence: u64,
    last_seq: u64,
    owner: Option<String>,
    lease_expires_ms: u64,
    records: std::collections::BTreeMap<u64, (u64, Record)>,
}

/// The reference store: exact semantics, zero durability, zero dependencies.
#[derive(Default)]
pub struct MemoryStore {
    sessions: std::sync::Mutex<HashMap<String, MemSession>>,
}

#[async_trait::async_trait]
impl JournalStore for MemoryStore {
    async fn create(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        owner: &str,
        now_ms: u64,
    ) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        if map.contains_key(session_id) {
            return Err(BrainError::Invalid(format!(
                "session {session_id} already exists"
            )));
        }
        let mut records = std::collections::BTreeMap::new();
        records.insert(1, (now_ms, first.clone()));
        map.insert(
            session_id.to_string(),
            MemSession {
                doc: doc.clone(),
                fence: 1,
                last_seq: 1,
                owner: Some(owner.to_string()),
                lease_expires_ms: now_ms + LEASE_MS,
                records,
            },
        );
        Ok(())
    }

    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head> {
        let mut map = self.sessions.lock().expect("memory journal");
        let s = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        let claimable = match &s.owner {
            None => true,
            Some(o) if o == owner => true,
            Some(_) => s.lease_expires_ms < now_ms.saturating_sub(STEAL_GRACE_MS),
        };
        if !claimable {
            return Err(BrainError::Fenced);
        }
        s.owner = Some(owner.to_string());
        s.lease_expires_ms = now_ms + LEASE_MS;
        s.fence += 1;
        Ok(Head {
            session_id: session_id.to_string(),
            doc: s.doc.clone(),
            fence: s.fence,
            last_seq: s.last_seq,
        })
    }

    async fn get_head(&self, session_id: &str) -> Result<Head> {
        let map = self.sessions.lock().expect("memory journal");
        let s = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        Ok(Head {
            session_id: session_id.to_string(),
            doc: s.doc.clone(),
            fence: s.fence,
            last_seq: s.last_seq,
        })
    }

    async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>> {
        let map = self.sessions.lock().expect("memory journal");
        let s = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        Ok(s.records
            .range((after + 1)..)
            .map(|(seq, (ts_ms, record))| Entry {
                seq: *seq,
                ts_ms: *ts_ms,
                record: record.clone(),
            })
            .collect())
    }

    async fn commit(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
        now_ms: u64,
    ) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        let s = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if s.fence != fence || s.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        // The (session, seq) key is the idempotency barrier: a duplicate seq means a
        // superseded writer.
        if records.iter().any(|(seq, _)| s.records.contains_key(seq)) {
            return Err(BrainError::Fenced);
        }
        for (seq, record) in records {
            s.records.insert(*seq, (now_ms, record.clone()));
        }
        s.doc = doc.clone();
        s.last_seq = high_water;
        s.lease_expires_ms = now_ms + LEASE_MS; // renew; deliberately no fence bump
        Ok(())
    }

    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        if let Some(s) = map.get_mut(session_id)
            && s.fence == fence
            && s.owner.as_deref() == Some(owner)
        {
            s.owner = None;
            s.lease_expires_ms = 0;
        }
        Ok(())
    }

    async fn purge(&self, session_id: &str) -> Result<u64> {
        let mut map = self.sessions.lock().expect("memory journal");
        Ok(match map.remove(session_id) {
            Some(s) => s.records.len() as u64 + 1,
            None => 0,
        })
    }

    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        let map = self.sessions.lock().expect("memory journal");
        Ok(map
            .iter()
            .take(limit)
            .map(|(sid, s)| Head {
                session_id: sid.clone(),
                doc: s.doc.clone(),
                fence: s.fence,
                last_seq: s.last_seq,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(turn: &str, text: &str) -> Record {
        Record::UserMessage {
            turn: turn.into(),
            content: vec![ContentBlock::text(text)],
            metadata: HashMap::new(),
        }
    }
    fn assistant(turn: &str, blocks: Vec<ContentBlock>) -> Record {
        Record::Assistant {
            turn: turn.into(),
            agent: "root".into(),
            content: blocks,
            stop: StopReason::EndTurn,
        }
    }
    fn result(call: &str, content: &str, is_error: bool) -> Record {
        Record::ToolResult {
            turn: "t1".into(),
            agent: "root".into(),
            call: call.into(),
            name: "bash".into(),
            outcome: if is_error { "failed" } else { "completed" }.into(),
            content: content.into(),
            is_error,
            exit_code: Some(if is_error { 1 } else { 0 }),
            duration_ms: 5,
            truncated: false,
        }
    }
    fn entries(records: Vec<Record>) -> Vec<Entry> {
        records
            .into_iter()
            .enumerate()
            .map(|(i, record)| Entry {
                seq: i as u64 + 1,
                ts_ms: 0,
                record,
            })
            .collect()
    }

    #[test]
    fn fold_rebuilds_the_conversation_and_groups_consecutive_tool_results() {
        let f = fold(&entries(vec![
            user("t1", "build it"),
            Record::TurnStarted { turn: "t1".into() },
            assistant(
                "t1",
                vec![
                    ContentBlock::text("running"),
                    ContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "bash".into(),
                        input: serde_json::json!({"command":"a"}),
                    },
                    ContentBlock::ToolUse {
                        id: "c2".into(),
                        name: "bash".into(),
                        input: serde_json::json!({"command":"b"}),
                    },
                ],
            ),
            result("c1", "ok-a", false),
            result("c2", "boom", true),
            assistant("t1", vec![ContentBlock::text("done")]),
        ]));
        assert_eq!(
            f.history.len(),
            4,
            "user, assistant, ONE grouped results message, assistant"
        );
        assert_eq!(f.history[2].role, Role::User);
        assert_eq!(f.history[2].content.len(), 2, "both results in one message");
        assert!(matches!(
            &f.history[2].content[1],
            ContentBlock::ToolResult { is_error: true, .. }
        ));
        assert_eq!(f.turns, 1);
    }

    #[test]
    fn fold_flushes_trailing_results_at_finish() {
        // A crash after committing results but before the next assistant message must still
        // rebuild a history the provider will accept.
        let f = fold(&entries(vec![
            user("t1", "x"),
            assistant(
                "t1",
                vec![ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                }],
            ),
            result("c1", "out", false),
        ]));
        assert_eq!(f.history.len(), 3);
        assert_eq!(f.history[2].role, Role::User);
    }

    #[test]
    fn subagent_records_never_split_or_pollute_root_history() {
        let mut child_assistant = assistant("t1", vec![ContentBlock::text("child")]);
        if let Record::Assistant { agent, .. } = &mut child_assistant {
            *agent = "agt_child".into();
        }
        let mut child_result = result("child-call", "child-out", false);
        if let Record::ToolResult { agent, .. } = &mut child_result {
            *agent = "agt_child".into();
        }
        let f = fold(&entries(vec![
            user("t1", "go"),
            assistant(
                "t1",
                vec![
                    ContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "task".into(),
                        input: serde_json::json!({}),
                    },
                    ContentBlock::ToolUse {
                        id: "c2".into(),
                        name: "task".into(),
                        input: serde_json::json!({}),
                    },
                ],
            ),
            result("c1", "one", false),
            child_assistant,
            child_result,
            result("c2", "two", false),
            assistant("t1", vec![ContentBlock::text("done")]),
        ]));
        assert_eq!(f.history.len(), 4);
        assert_eq!(f.history[2].content.len(), 2);
        assert!(f.history.iter().all(|message| {
            message
                .content
                .iter()
                .all(|block| !matches!(block, ContentBlock::Text { text } if text == "child"))
        }));
    }

    #[test]
    fn next_user_text_merges_with_an_interrupted_tool_result() {
        let f = fold(&entries(vec![
            user("t1", "start"),
            assistant(
                "t1",
                vec![ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "task".into(),
                    input: serde_json::json!({}),
                }],
            ),
            result("c1", "subagent interrupted", true),
            Record::TurnCompleted {
                turn: "t1".into(),
                stop_reason: "interrupted".into(),
                rounds: 1,
                tool_calls: 1,
                result: None,
            },
            user("t2", "continue"),
        ]));
        assert_eq!(f.history.len(), 3);
        assert_eq!(f.history[2].role, Role::User);
        assert!(matches!(
            &f.history[2].content[..],
            [ContentBlock::ToolResult { is_error: true, .. }, ContentBlock::Text { text }]
                if text == "continue"
        ));
    }

    #[test]
    fn fold_is_a_loop_over_apply() {
        // F1 (donor property): batch fold == incremental apply, at every prefix.
        let all = entries(vec![
            user("t1", "a"),
            assistant("t1", vec![ContentBlock::text("b")]),
            user("t2", "c"),
            result("c9", "r", false),
            assistant("t2", vec![ContentBlock::text("d")]),
        ]);
        for split in 0..=all.len() {
            let mut inc = Fold::default();
            for e in &all[..split] {
                inc.apply(&e.record);
            }
            inc.finish();
            let batch = fold(&all[..split]);
            assert_eq!(batch.history, inc.history, "split {split}");
        }
    }

    #[test]
    fn compaction_is_prefix_stable_and_keeps_the_tail() {
        let f = fold(&entries(vec![
            user("t1", "one"),
            assistant("t1", vec![ContentBlock::text("1")]),
            user("t2", "two"),
            assistant("t2", vec![ContentBlock::text("2")]),
            Record::Compacted {
                summary: "[compacted]".into(),
                kept: 2,
            },
            user("t3", "three"),
        ]));
        assert_eq!(f.history.len(), 4, "summary + kept tail (2) + new user");
        assert_eq!(f.history[0], Message::user_text("[compacted]"));
        assert_eq!(f.history[1], Message::user_text("two"));
    }

    #[test]
    fn unknown_record_kind_is_a_typed_error_not_a_passthrough() {
        let bad = r#"{"kind":"totally_new","x":1}"#;
        assert!(serde_json::from_str::<Record>(bad).is_err());
    }

    #[test]
    fn record_sks_sort_numerically() {
        assert!(record_sk(9) < record_sk(10));
        assert!(record_sk(999) < record_sk(1000));
    }

    #[test]
    fn head_doc_round_trips() {
        let doc = HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: 1,
            updated_ms: 2,
            last_message_ms: None,
            ended: false,
            prefix: PrefixDoc {
                system_prompt: Some("sp".into()),
                provider: "anthropic".into(),
                model: "claude".into(),
                base_url: None,
                max_output_tokens: Some(4096),
                temperature: None,
                reasoning_effort: None,
                tools: vec!["bash".into()],
                mcp: vec![],
                mcp_tools: vec![],
                external_tools: vec![],
                hand_enabled: true,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                env: HashMap::new(),
                metadata: HashMap::new(),
            },
            key_b64: "AAAA".into(),
            mcp_secrets_b64: String::new(),
            manifest_digest: "d".into(),
            hand_info: HeadDoc::initial_hand_info("1gb"),
            hand_state: serde_json::Value::Null,
            workspace_bytes: 0,
            artifacts: vec![],
        };
        let s = serde_json::to_string(&doc).unwrap();
        let back: HeadDoc = serde_json::from_str(&s).unwrap();
        assert_eq!(back.prefix.model, "claude");
        assert_eq!(back.state, "idle");
    }

    fn head_doc() -> HeadDoc {
        HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: 1,
            updated_ms: 1,
            last_message_ms: None,
            ended: false,
            prefix: PrefixDoc {
                system_prompt: None,
                provider: "anthropic".into(),
                model: "m".into(),
                base_url: None,
                max_output_tokens: None,
                temperature: None,
                reasoning_effort: None,
                tools: vec![],
                mcp: vec![],
                mcp_tools: vec![],
                external_tools: vec![],
                hand_enabled: false,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                env: HashMap::new(),
                metadata: HashMap::new(),
            },
            key_b64: String::new(),
            mcp_secrets_b64: String::new(),
            manifest_digest: String::new(),
            hand_info: HeadDoc::initial_hand_info("1gb"),
            hand_state: serde_json::Value::Null,
            workspace_bytes: 0,
            artifacts: vec![],
        }
    }

    #[tokio::test]
    async fn memory_journal_full_lifecycle() {
        let j = Journal::new_memory("brain-a");
        let doc = head_doc();
        j.create(
            "ses_m",
            &doc,
            &Record::State {
                state: "idle".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            j.create(
                "ses_m",
                &doc,
                &Record::State {
                    state: "idle".into(),
                    turn: None
                }
            )
            .await,
            Err(BrainError::Invalid(_))
        ));

        let head = j.claim("ses_m").await.unwrap();
        assert_eq!(head.fence, 2, "claim bumps the fence");
        assert_eq!(head.last_seq, 1);

        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
        };
        let rec = (2u64, Record::TurnStarted { turn: "t1".into() });
        j.commit("ses_m", &mut lease, std::slice::from_ref(&rec), &doc, 3)
            .await
            .unwrap();
        assert_eq!(
            lease.last_seq, 3,
            "high water persisted, ephemeral seq included"
        );

        let entries = j.read_records("ses_m", 0).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].seq, 2);

        // Re-committing the same seq is a superseded write, exactly like DynamoDB.
        assert!(matches!(
            j.commit("ses_m", &mut lease, std::slice::from_ref(&rec), &doc, 4)
                .await,
            Err(BrainError::Fenced)
        ));

        assert_eq!(j.purge("ses_m").await.unwrap(), 3);
        assert!(matches!(
            j.get_head("ses_m").await,
            Err(BrainError::NoSuchSession(_))
        ));
    }

    #[tokio::test]
    async fn memory_journal_fences_out_a_stale_owner() {
        let a = Journal::new_memory("brain-a");
        let b = a.cloned_as("brain-b");
        let doc = head_doc();
        a.create(
            "ses_f",
            &doc,
            &Record::State {
                state: "idle".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
        let head_a = a.claim("ses_f").await.unwrap();
        let mut lease_a = Lease {
            fence: head_a.fence,
            last_seq: head_a.last_seq,
        };

        // B cannot steal while A's lease is live...
        assert!(matches!(b.claim("ses_f").await, Err(BrainError::Fenced)));

        // ...but after A releases, B claims with a HIGHER fence, and A's writes are dead.
        a.release("ses_f", &lease_a).await.unwrap();
        let head_b = b.claim("ses_f").await.unwrap();
        assert!(head_b.fence > head_a.fence);
        let rec = (2u64, Record::TurnStarted { turn: "t".into() });
        assert!(matches!(
            a.commit("ses_f", &mut lease_a, std::slice::from_ref(&rec), &doc, 2)
                .await,
            Err(BrainError::Fenced)
        ));
        let mut lease_b = Lease {
            fence: head_b.fence,
            last_seq: head_b.last_seq,
        };
        b.commit("ses_f", &mut lease_b, std::slice::from_ref(&rec), &doc, 2)
            .await
            .unwrap();
    }
}
