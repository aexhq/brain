//! The session journal: every decision durable in one DynamoDB write (D9).
//!
//! One item collection per session. `HEAD` carries ownership (lease + fence), the sealed
//! configuration and the mutable session facts; `E#<seq>` items carry the decision records.
//! The journal is also the event log: SSE replay is a derivation over these records
//! (`events::derive`), and `seq` is both the journal order and the SSE `id:`.
//!
//! Concurrency rules (donor: aex-brain-domain, kept because each one answers a real outage):
//! - the item key is the idempotency barrier: every record put is conditioned
//!   `attribute_not_exists(sk)` -- a redelivered decision loses the put, it never duplicates;
//! - the fence advances on claim only, never on renew (renewing must not fence out the owner);
//! - a conditional failure on commit means a newer owner exists: the local fold is stale and
//!   must be discarded, never patched;
//! - `BatchWriteItem` is never used for records (it cannot be conditioned).

use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::{BrainError, Result};
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Zero-padded so lexicographic order is numeric order. Without the padding `E#10` sorts
/// before `E#9` and a paged read replays out of sequence.
fn record_sk(seq: u64) -> String {
    format!("E#{seq:020}")
}

fn session_pk(session_id: &str) -> String {
    format!("S#{session_id}")
}

/// How long a lease lives without renewal, and how much longer a steal waits beyond expiry.
/// The grace absorbs clock skew between instances; the fence, not the clock, decides whether
/// a stale owner can write.
pub const LEASE_MS: u64 = 60_000;
pub const STEAL_GRACE_MS: u64 = 5_000;

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
    },
    TurnFailed {
        turn: String,
        code: String,
        message: String,
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
        match record {
            Record::UserMessage { content, .. } => {
                self.flush_results();
                self.history.push(Message {
                    role: Role::User,
                    content: content.clone(),
                });
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
    /// True once `end` ran: the hand is released for good, the workspace is kept.
    #[serde(default)]
    pub ended: bool,
    pub prefix: PrefixDoc,
    /// KMS ciphertext of the BYOK key, base64. Never the plaintext.
    pub key_b64: String,
    pub manifest_digest: String,
    pub hand: HandDoc,
    pub sync: SyncDoc,
    #[serde(default)]
    pub artifacts: Vec<ArtifactDoc>,
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
    pub hand_enabled: bool,
    pub shape: String,
    pub sync_interval_seconds: u64,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Seed files staged to S3 at create; applied on the first hello, captured by the first
    /// sync. Content never enters DynamoDB (400 KiB item cap; 1 MiB files).
    #[serde(default)]
    pub seed_files: Vec<SeedFileDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedFileDoc {
    pub path: String,
    pub s3_key: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<i64>,
}

/// The hand as the brain last knew it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandDoc {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microvm_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Incarnation count across the session's life (HandInfo.generation).
    pub incarnations: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launched_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_deadline_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_ms: Option<u64>,
    /// Packs referenced by the last manifest, as the hand reported. Drives the "ask for a
    /// full sync when the chain grows" policy.
    #[serde(default)]
    pub packs_referenced: u64,
    /// Total workspace bytes as of the last sync (the hand's manifest total). Storage info,
    /// not billing authority (I9).
    #[serde(default)]
    pub bytes_total: u64,
}

/// One persisted artifact: metadata only; the bytes live in S3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDoc {
    pub name: String,
    pub s3_key: String,
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
// Store
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Journal {
    db: aws_sdk_dynamodb::Client,
    table: String,
    /// This brain instance's identity as a lease owner.
    owner: String,
}

impl Journal {
    pub fn new(
        db: aws_sdk_dynamodb::Client,
        table: impl Into<String>,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            db,
            table: table.into(),
            owner: owner.into(),
        }
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Creates the session: HEAD plus the first record, atomically, refused if it exists.
    pub async fn create(&self, session_id: &str, doc: &HeadDoc, first: &Record) -> Result<()> {
        let now = crate::wall_ms();
        let head = Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(session_pk(session_id)))
            .item("sk", AttributeValue::S("HEAD".into()))
            .item("owner_id", AttributeValue::S(self.owner.clone()))
            .item("fence", AttributeValue::N("1".into()))
            .item(
                "lease_expires_ms",
                AttributeValue::N((now + LEASE_MS).to_string()),
            )
            .item("last_seq", AttributeValue::N("1".into()))
            .item("doc", AttributeValue::S(serde_json::to_string(doc)?))
            .condition_expression("attribute_not_exists(sk)")
            .build()
            .map_err(|e| BrainError::Journal(format!("head put: {e}")))?;
        let rec = self.record_put(session_id, 1, now, first)?;
        self.db
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(head).build())
            .transact_items(TransactWriteItem::builder().put(rec).build())
            .send()
            .await
            .map_err(|e| match conditional_failure(&e) {
                true => BrainError::Invalid(format!("session {session_id} already exists")),
                false => BrainError::Journal(format!("create: {}", describe(&e))),
            })?;
        Ok(())
    }

    /// Claims (or re-claims) ownership. One round trip: the conditional update returns the
    /// head, so becoming owner and learning the state are the same call.
    pub async fn claim(&self, session_id: &str) -> Result<Head> {
        let now = crate::wall_ms();
        let out = self
            .db
            .update_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression(
                "attribute_exists(pk) AND (attribute_not_exists(lease_expires_ms) \
                 OR lease_expires_ms < :stealable OR owner_id = :me)",
            )
            .update_expression("SET owner_id = :me, lease_expires_ms = :expires ADD fence :one")
            .expression_attribute_values(":me", AttributeValue::S(self.owner.clone()))
            .expression_attribute_values(
                ":stealable",
                AttributeValue::N(now.saturating_sub(STEAL_GRACE_MS).to_string()),
            )
            .expression_attribute_values(
                ":expires",
                AttributeValue::N((now + LEASE_MS).to_string()),
            )
            .expression_attribute_values(":one", AttributeValue::N("1".into()))
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
            .send()
            .await
            .map_err(|e| match conditional_failure(&e) {
                true => BrainError::Fenced,
                false => match not_found(&e) {
                    true => BrainError::NoSuchSession(session_id.into()),
                    false => BrainError::Journal(format!("claim: {}", describe(&e))),
                },
            })?;
        let attrs = out
            .attributes()
            .ok_or_else(|| BrainError::Journal("claim returned no head".into()))?;
        parse_head(session_id, attrs)
    }

    /// Reads the head without claiming. Strongly consistent (session facts, not a cache).
    pub async fn get_head(&self, session_id: &str) -> Result<Head> {
        let out = self
            .db
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| BrainError::Journal(format!("get head: {}", describe(&e))))?;
        let attrs = out
            .item()
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        parse_head(session_id, attrs)
    }

    /// Reads records `after < seq <= last`, in order, paged, strongly consistent.
    pub async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        let mut start_key = None;
        loop {
            let out = self
                .db
                .query()
                .table_name(&self.table)
                .key_condition_expression("pk = :pk AND sk BETWEEN :lo AND :hi")
                .expression_attribute_values(":pk", AttributeValue::S(session_pk(session_id)))
                .expression_attribute_values(":lo", AttributeValue::S(record_sk(after + 1)))
                .expression_attribute_values(":hi", AttributeValue::S(record_sk(u64::MAX)))
                .consistent_read(true)
                .set_exclusive_start_key(start_key)
                .send()
                .await
                .map_err(|e| BrainError::Journal(format!("read: {}", describe(&e))))?;
            for item in out.items() {
                entries.push(parse_entry(item)?);
            }
            start_key = out.last_evaluated_key().cloned();
            if start_key.is_none() {
                return Ok(entries);
            }
        }
    }

    /// One decision, one durable write. Puts every record (each `attribute_not_exists(sk)`)
    /// and updates HEAD (fenced) in a single transaction. `high_water` is the highest seq
    /// allocated by the session -- including ephemeral (never-journaled) event seqs -- so a
    /// rehydrated session never re-issues an id a client may already have seen.
    pub async fn commit(
        &self,
        session_id: &str,
        lease: &mut Lease,
        records: &[(u64, Record)],
        doc: &HeadDoc,
        high_water: u64,
    ) -> Result<()> {
        let now = crate::wall_ms();
        let mut tx = self.db.transact_write_items();
        for (seq, record) in records {
            let put = self.record_put(session_id, *seq, now, record)?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());
        }
        let update = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("fence = :fence AND owner_id = :me")
            // Deliberately no `ADD fence`: renewing must not fence out the renewer.
            .update_expression("SET last_seq = :hw, doc = :doc, lease_expires_ms = :expires")
            .expression_attribute_values(":fence", AttributeValue::N(lease.fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(self.owner.clone()))
            .expression_attribute_values(":hw", AttributeValue::N(high_water.to_string()))
            .expression_attribute_values(":doc", AttributeValue::S(serde_json::to_string(doc)?))
            .expression_attribute_values(
                ":expires",
                AttributeValue::N((now + LEASE_MS).to_string()),
            )
            .build()
            .map_err(|e| BrainError::Journal(format!("head update: {e}")))?;
        tx = tx.transact_items(TransactWriteItem::builder().update(update).build());
        tx.send().await.map_err(|e| match conditional_failure(&e) {
            true => BrainError::Fenced,
            false => BrainError::Journal(format!("commit: {}", describe(&e))),
        })?;
        lease.last_seq = high_water;
        Ok(())
    }

    /// Releases the lease. A conditional failure maps to Ok: a release that lost its fence
    /// has already been superseded.
    pub async fn release(&self, session_id: &str, lease: &Lease) -> Result<()> {
        let r = self
            .db
            .update_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("fence = :fence AND owner_id = :me")
            .update_expression("REMOVE owner_id, lease_expires_ms")
            .expression_attribute_values(":fence", AttributeValue::N(lease.fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(self.owner.clone()))
            .send()
            .await;
        match r {
            Ok(_) => Ok(()),
            Err(e) if conditional_failure(&e) => Ok(()),
            Err(e) => Err(BrainError::Journal(format!("release: {}", describe(&e)))),
        }
    }

    /// Deletes every item of the session. Plain deletes: delete is the one irreversible
    /// transition and the caller has already committed `state = deleted`.
    pub async fn purge(&self, session_id: &str) -> Result<u64> {
        let mut deleted = 0u64;
        let mut start_key = None;
        loop {
            let out = self
                .db
                .query()
                .table_name(&self.table)
                .key_condition_expression("pk = :pk")
                .expression_attribute_values(":pk", AttributeValue::S(session_pk(session_id)))
                .projection_expression("pk, sk")
                .consistent_read(true)
                .set_exclusive_start_key(start_key)
                .send()
                .await
                .map_err(|e| BrainError::Journal(format!("purge query: {}", describe(&e))))?;
            for item in out.items() {
                let sk = item
                    .get("sk")
                    .and_then(|v| v.as_s().ok())
                    .cloned()
                    .unwrap_or_default();
                self.db
                    .delete_item()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(session_id)))
                    .key("sk", AttributeValue::S(sk))
                    .send()
                    .await
                    .map_err(|e| BrainError::Journal(format!("purge delete: {}", describe(&e))))?;
                deleted += 1;
            }
            start_key = out.last_evaluated_key().cloned();
            if start_key.is_none() {
                return Ok(deleted);
            }
        }
    }

    /// Lists session ids by scanning HEAD items. Dev-plane listing; an index arrives with the
    /// control plane (slice 4).
    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        let mut heads = Vec::new();
        let mut start_key = None;
        loop {
            let out = self
                .db
                .scan()
                .table_name(&self.table)
                .filter_expression("sk = :head")
                .expression_attribute_values(":head", AttributeValue::S("HEAD".into()))
                .set_exclusive_start_key(start_key)
                .send()
                .await
                .map_err(|e| BrainError::Journal(format!("list: {}", describe(&e))))?;
            for item in out.items() {
                let pk = item
                    .get("pk")
                    .and_then(|v| v.as_s().ok())
                    .cloned()
                    .unwrap_or_default();
                let sid = pk.strip_prefix("S#").unwrap_or(&pk).to_string();
                heads.push(parse_head(&sid, item)?);
                if heads.len() >= limit {
                    return Ok(heads);
                }
            }
            start_key = out.last_evaluated_key().cloned();
            if start_key.is_none() {
                return Ok(heads);
            }
        }
    }

    fn record_put(&self, session_id: &str, seq: u64, ts_ms: u64, record: &Record) -> Result<Put> {
        Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(session_pk(session_id)))
            .item("sk", AttributeValue::S(record_sk(seq)))
            .item("kind", AttributeValue::S(record.kind_name().into()))
            .item("ts_ms", AttributeValue::N(ts_ms.to_string()))
            .item("body", AttributeValue::S(serde_json::to_string(record)?))
            .condition_expression("attribute_not_exists(sk)")
            .build()
            .map_err(|e| BrainError::Journal(format!("record put: {e}")))
    }
}

fn parse_head(session_id: &str, attrs: &HashMap<String, AttributeValue>) -> Result<Head> {
    let n = |k: &str| -> Result<u64> {
        attrs
            .get(k)
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| BrainError::Journal(format!("head missing numeric {k}")))
    };
    let doc_s = attrs
        .get("doc")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| BrainError::Journal("head missing doc".into()))?;
    let doc: HeadDoc = serde_json::from_str(doc_s)
        .map_err(|e| BrainError::Journal(format!("head doc does not parse: {e}")))?;
    Ok(Head {
        session_id: session_id.to_string(),
        doc,
        fence: n("fence")?,
        last_seq: n("last_seq")?,
    })
}

fn parse_entry(item: &HashMap<String, AttributeValue>) -> Result<Entry> {
    let sk = item
        .get("sk")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| BrainError::Journal("record missing sk".into()))?;
    let seq: u64 = sk
        .strip_prefix("E#")
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| BrainError::Journal(format!("record sk malformed: {sk}")))?;
    let ts_ms = item
        .get("ts_ms")
        .and_then(|v| v.as_n().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = item
        .get("body")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| BrainError::Journal(format!("record {seq} missing body")))?;
    let record: Record = serde_json::from_str(body)
        .map_err(|e| BrainError::Journal(format!("record {seq} does not parse: {e}")))?;
    Ok(Entry { seq, ts_ms, record })
}

fn conditional_failure<E: ProvideErrorMetadata, R>(e: &SdkError<E, R>) -> bool {
    // TransactWriteItems surfaces condition failures inside TransactionCanceledException;
    // single-item writes surface ConditionalCheckFailedException directly.
    match e {
        SdkError::ServiceError(s) => matches!(
            s.err().code(),
            Some("ConditionalCheckFailedException") | Some("TransactionCanceledException")
        ),
        _ => false,
    }
}

fn not_found<E: ProvideErrorMetadata, R>(e: &SdkError<E, R>) -> bool {
    matches!(e, SdkError::ServiceError(s) if s.err().code() == Some("ResourceNotFoundException"))
}

fn describe<E: ProvideErrorMetadata, R>(e: &SdkError<E, R>) -> String {
    match e {
        SdkError::ServiceError(s) => format!(
            "{}: {}",
            s.err().code().unwrap_or("service error"),
            s.err().message().unwrap_or("")
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(turn: &str, text: &str) -> Record {
        Record::UserMessage {
            turn: turn.into(),
            content: vec![ContentBlock::text(text)],
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
                hand_enabled: true,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                env: HashMap::new(),
                metadata: HashMap::new(),
                seed_files: vec![],
            },
            key_b64: "AAAA".into(),
            manifest_digest: "d".into(),
            hand: HandDoc::default(),
            sync: SyncDoc::default(),
            artifacts: vec![],
        };
        let s = serde_json::to_string(&doc).unwrap();
        let back: HeadDoc = serde_json::from_str(&s).unwrap();
        assert_eq!(back.prefix.model, "claude");
        assert_eq!(back.state, "idle");
    }
}
