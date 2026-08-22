//! The DynamoDB journal store: the production implementation of
//! [`brain::journal::JournalStore`].
//!
//! One item collection per session (`S#<sid>` / `HEAD` + `E#<seq:020>`, shared key shape from
//! the core). The semantics the trait demands map onto DynamoDB primitives:
//! - record puts are conditioned `attribute_not_exists(sk)` -- the item key is the
//!   idempotency barrier;
//! - claim is one conditional `UpdateItem` with `ADD fence 1` (the only fence advance);
//! - commit is one `TransactWriteItems`: every record put plus the fenced HEAD update --
//!   one decision, one durable write;
//! - `BatchWriteItem` is never used (it cannot be conditioned).

use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_dynamodb::types::{
    AttributeValue, ConditionCheck, Delete, DeleteRequest, Put, TransactWriteItem, Update,
    WriteRequest,
};
use base64::Engine as _;
use brain::journal::{
    ChildListQuery, ChildPage, ConfigDoc, DeletionStatusDoc, EndFence, Entry, Head, HeadDoc,
    JournalRetention, JournalRetentionLimits, JournalStore, LEASE_MS, MAX_SERIALIZED_CONFIG_BYTES,
    Record, RecordPage, RecordPageQuery, RecoveryItem, RecoveryPage, RecoveryQuery, STEAL_GRACE_MS,
    SandboxInventoryDoc, SandboxListQuery, SandboxPage, SandboxReserveRequest,
    SandboxUpdateRequest, SessionListQuery, SessionPage, SessionSummary, child_admission_open,
    initial_retention, project_end_fence, project_retention, record_sk, recovery_due_key,
    recovery_shard, requires_ancestor_admission, retention_delta, session_id_from_list_cursor,
    session_pk, tenant_session_sort_key, validate_ancestor_path, validate_config_doc,
    validate_record_page_query,
};
use brain::{BrainError, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const TENANT_SESSIONS_INDEX: &str = "tenant-sessions";
pub const TENANT_STATE_SESSIONS_INDEX: &str = "tenant-state-sessions";
pub const RECOVERY_DUE_INDEX: &str = "recovery-due";
const CONFIG_SK: &str = "CONFIG#000000";
const CHILD_SK_PREFIX: &str = "CHILD#";
const SANDBOX_SK_PREFIX: &str = "SANDBOX#";
const TENANT_STORAGE_SK: &str = "STORAGE#METER";
const TENANT_RETENTION_SK: &str = "JOURNAL#METER";

fn tenant_pk(tenant_id: &str) -> String {
    format!("T#{tenant_id}")
}

fn tenant_state_pk(tenant_id: &str, state: &str) -> String {
    format!("T#{tenant_id}#S#{state}")
}

fn deletion_pk(session_id: &str) -> String {
    format!("D#{session_id}")
}

pub struct DynamoJournal {
    db: aws_sdk_dynamodb::Client,
    table: String,
}

impl DynamoJournal {
    pub fn new(db: aws_sdk_dynamodb::Client, table: impl Into<String>) -> Self {
        Self {
            db,
            table: table.into(),
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

    async fn load_config(&self, session_id: &str) -> Result<ConfigDoc> {
        let output = self
            .db
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S(CONFIG_SK.into()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|error| BrainError::Journal(format!("get config: {}", describe(&error))))?;
        parse_config(
            output
                .item()
                .ok_or_else(|| BrainError::Journal("session CONFIG item is missing".into()))?,
        )
    }

    async fn load_tenant_retention(&self, tenant_id: &str) -> Result<(u64, u64)> {
        let output = self
            .db
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(tenant_pk(tenant_id)))
            .key("sk", AttributeValue::S(TENANT_RETENTION_SK.into()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|error| {
                BrainError::Journal(format!("get tenant retention meter: {}", describe(&error)))
            })?;
        let Some(item) = output.item() else {
            return Ok((0, 0));
        };
        Ok((
            optional_number(item, "total_bytes")?.unwrap_or(0),
            optional_number(item, "session_count")?.unwrap_or(0),
        ))
    }

    async fn batch_delete_requests(
        &self,
        requests: Vec<WriteRequest>,
        context: &str,
    ) -> Result<u64> {
        let mut deleted = 0u64;
        for batch in requests.chunks(25) {
            let mut pending = batch.to_vec();
            let mut retry = 0u32;
            while !pending.is_empty() {
                let output = self
                    .db
                    .batch_write_item()
                    .request_items(&self.table, pending)
                    .send()
                    .await
                    .map_err(|error| {
                        BrainError::Journal(format!("{context}: {}", describe(&error)))
                    })?;
                pending = output
                    .unprocessed_items()
                    .and_then(|items| items.get(&self.table))
                    .cloned()
                    .unwrap_or_default();
                if pending.is_empty() {
                    break;
                }
                retry += 1;
                if retry > 8 {
                    return Err(BrainError::Journal(format!(
                        "{context} left {} DynamoDB items unprocessed after bounded retry",
                        pending.len()
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(
                    10u64.saturating_mul(1 << retry.min(7)),
                ))
                .await;
            }
            deleted += batch.len() as u64;
        }
        Ok(deleted)
    }
}

#[async_trait::async_trait]
impl JournalStore for DynamoJournal {
    async fn create(&self, decision: &brain::journal::CreateDecision<'_>) -> Result<()> {
        let &brain::journal::CreateDecision {
            session_id,
            doc,
            first,
            now_ms,
            tenant_storage_limit,
            retention,
            retention_limits,
        } = decision;
        validate_ancestor_path(doc)?;
        validate_config_doc(doc)?;
        if retention != initial_retention(first, retention_limits.session_bytes)? {
            return Err(BrainError::Journal(
                "create journal retention projection does not match the canonical charge".into(),
            ));
        }
        if doc.session_storage_bytes != 0 || doc.storage_reserved_bytes != 0 {
            return Err(BrainError::Invalid(
                "new sessions must start with zero public session storage".into(),
            ));
        }
        if doc.parent_id.is_some() && doc.tenant_metered_storage_bytes != 0 {
            return Err(BrainError::Invalid(
                "child sessions cannot reserve root-owned bundle storage".into(),
            ));
        }
        let (control, config_doc) = doc.split();
        let config_bytes = serde_json::to_vec(&config_doc)?;
        if config_bytes.len() > MAX_SERIALIZED_CONFIG_BYTES {
            return Err(BrainError::Invalid(format!(
                "sealed session configuration is {} bytes; maximum is {MAX_SERIALIZED_CONFIG_BYTES}",
                config_bytes.len()
            )));
        }
        let config_digest = hex::encode(Sha256::digest(&config_bytes));
        let mut head = Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(session_pk(session_id)))
            .item("sk", AttributeValue::S("HEAD".into()))
            .item("fence", AttributeValue::N("0".into()))
            .item("last_seq", AttributeValue::N("1".into()))
            .item("config_digest", AttributeValue::S(config_digest.clone()))
            .item("config_chunks", AttributeValue::N("1".into()))
            .item("doc", AttributeValue::S(serde_json::to_string(&control)?))
            .item(
                "listing_doc",
                AttributeValue::S(serde_json::to_string(&SessionSummary::from_head(
                    session_id, doc,
                ))?),
            )
            .item("tenant_pk", AttributeValue::S(tenant_pk(&doc.tenant_id)))
            .item(
                "tenant_state_pk",
                AttributeValue::S(tenant_state_pk(&doc.tenant_id, doc.state.as_str())),
            )
            .item(
                "tenant_sk",
                AttributeValue::S(tenant_session_sort_key(doc.updated_ms, session_id)),
            )
            .item("state", AttributeValue::S(doc.state.as_str().to_string()))
            .item("updated_ms", AttributeValue::N(doc.updated_ms.to_string()))
            .item("root_id", AttributeValue::S(doc.root_id.clone()))
            .item(
                "admission_open",
                AttributeValue::Bool(child_admission_open(doc)),
            )
            .item("direct_child_count", AttributeValue::N("0".into()))
            .item("descendant_count", AttributeValue::N("0".into()))
            .item("additional_sandbox_count", AttributeValue::N("0".into()))
            .item(
                "journal_metered_bytes",
                AttributeValue::N(retention.metered_bytes.to_string()),
            )
            .item(
                "journal_effect_reserve_bytes",
                AttributeValue::N(retention.effect_reserve_bytes.to_string()),
            )
            .item(
                "journal_lifecycle_reserve_bytes",
                AttributeValue::N(retention.lifecycle_reserve_bytes.to_string()),
            )
            .item(
                "max_direct_children",
                AttributeValue::N(doc.prefix.max_direct_children.to_string()),
            )
            .item(
                "max_descendants",
                AttributeValue::N(doc.prefix.max_descendants.to_string()),
            )
            .item(
                "max_additional_sandboxes",
                AttributeValue::N(doc.prefix.max_additional_sandboxes_per_root.to_string()),
            )
            .condition_expression("attribute_not_exists(sk)");
        if let Some(parent_id) = &doc.parent_id {
            head = head.item("parent_id", AttributeValue::S(parent_id.clone()));
        }
        if let Some(active_phase) = &doc.active_phase {
            head = head.item(
                "active_phase",
                AttributeValue::S(active_phase.as_str().to_string()),
            );
        }
        if let Some(due_ms) = doc.recovery_due_ms {
            head = head
                .item(
                    "recovery_shard",
                    AttributeValue::S(recovery_shard(session_id)),
                )
                .item(
                    "recovery_due_key",
                    AttributeValue::S(recovery_due_key(due_ms, session_id)),
                );
        }
        let head = head
            .build()
            .map_err(|e| BrainError::Journal(format!("head put: {e}")))?;
        let config = Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(session_pk(session_id)))
            .item("sk", AttributeValue::S(CONFIG_SK.into()))
            .item("sha256", AttributeValue::S(config_digest))
            .item(
                "content_base64",
                AttributeValue::S(base64::engine::general_purpose::STANDARD.encode(config_bytes)),
            )
            .condition_expression("attribute_not_exists(sk)")
            .build()
            .map_err(|error| BrainError::Journal(format!("config put: {error}")))?;
        let rec = self.record_put(session_id, 1, now_ms, first)?;
        let mut transaction = self
            .db
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(head).build())
            .transact_items(TransactWriteItem::builder().put(config).build())
            .transact_items(TransactWriteItem::builder().put(rec).build());
        if let Some(parent_id) = &doc.parent_id {
            let link = Put::builder()
                .table_name(&self.table)
                .item("pk", AttributeValue::S(session_pk(parent_id)))
                .item(
                    "sk",
                    AttributeValue::S(format!("{CHILD_SK_PREFIX}{session_id}")),
                )
                .item("child_id", AttributeValue::S(session_id.to_owned()))
                .item(
                    "listing_doc",
                    AttributeValue::S(serde_json::to_string(&SessionSummary::from_head(
                        session_id, doc,
                    ))?),
                )
                .condition_expression("attribute_not_exists(sk)")
                .build()
                .map_err(|error| BrainError::Journal(format!("child link put: {error}")))?;
            transaction =
                transaction.transact_items(TransactWriteItem::builder().put(link).build());
            if parent_id == &doc.root_id {
                let reserve = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(parent_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression(
                        "admission_open = :open AND root_id = :root AND \
                         direct_child_count < max_direct_children AND \
                         descendant_count < max_descendants",
                    )
                    .update_expression("ADD direct_child_count :one, descendant_count :one")
                    .expression_attribute_values(":open", AttributeValue::Bool(true))
                    .expression_attribute_values(":root", AttributeValue::S(doc.root_id.clone()))
                    .expression_attribute_values(":one", AttributeValue::N("1".into()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("reserve root child admission: {error}"))
                    })?;
                transaction = transaction
                    .transact_items(TransactWriteItem::builder().update(reserve).build());
            } else {
                let reserve_parent = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(parent_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression(
                        "admission_open = :open AND root_id = :root AND \
                         direct_child_count < max_direct_children",
                    )
                    .update_expression("ADD direct_child_count :one")
                    .expression_attribute_values(":open", AttributeValue::Bool(true))
                    .expression_attribute_values(":root", AttributeValue::S(doc.root_id.clone()))
                    .expression_attribute_values(":one", AttributeValue::N("1".into()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("reserve direct child admission: {error}"))
                    })?;
                let reserve_root = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(&doc.root_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression(
                        "admission_open = :open AND root_id = :root AND \
                         descendant_count < max_descendants",
                    )
                    .update_expression("ADD descendant_count :one")
                    .expression_attribute_values(":open", AttributeValue::Bool(true))
                    .expression_attribute_values(":root", AttributeValue::S(doc.root_id.clone()))
                    .expression_attribute_values(":one", AttributeValue::N("1".into()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("reserve root descendant admission: {error}"))
                    })?;
                transaction = transaction
                    .transact_items(TransactWriteItem::builder().update(reserve_parent).build())
                    .transact_items(TransactWriteItem::builder().update(reserve_root).build());
            }
            // Root and direct parent already carry equivalent conditions on their counter
            // updates above. Every intermediate ancestor gets a condition-only action in this
            // same transaction, closing a subtree-end race at arbitrary bounded depth.
            for ancestor_id in doc
                .ancestor_ids
                .iter()
                .filter(|ancestor| *ancestor != parent_id && *ancestor != &doc.root_id)
            {
                let condition = ConditionCheck::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(ancestor_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression("admission_open = :open AND root_id = :root")
                    .expression_attribute_values(":open", AttributeValue::Bool(true))
                    .expression_attribute_values(":root", AttributeValue::S(doc.root_id.clone()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("ancestor admission condition: {error}"))
                    })?;
                transaction = transaction.transact_items(
                    TransactWriteItem::builder()
                        .condition_check(condition)
                        .build(),
                );
            }
        }
        let create_meter_index = if doc.tenant_metered_storage_bytes == 0 {
            None
        } else {
            let requested = doc.tenant_metered_storage_bytes;
            let remaining = tenant_storage_limit.checked_sub(requested).ok_or(
                BrainError::TenantStorageQuotaExceeded {
                    requested,
                    limit: tenant_storage_limit,
                },
            )?;
            let meter = Update::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(tenant_pk(&doc.tenant_id)))
                .key("sk", AttributeValue::S(TENANT_STORAGE_SK.into()))
                .condition_expression(
                    "attribute_not_exists(total_bytes) OR total_bytes <= :remaining",
                )
                .update_expression(
                    "SET total_bytes = if_not_exists(total_bytes, :zero) + :requested",
                )
                .expression_attribute_values(":zero", AttributeValue::N("0".into()))
                .expression_attribute_values(":requested", AttributeValue::N(requested.to_string()))
                .expression_attribute_values(":remaining", AttributeValue::N(remaining.to_string()))
                .build()
                .map_err(|error| {
                    BrainError::Journal(format!("create tenant storage meter: {error}"))
                })?;
            let index = transaction
                .get_transact_items()
                .as_ref()
                .map_or(0, Vec::len);
            transaction =
                transaction.transact_items(TransactWriteItem::builder().update(meter).build());
            Some(index)
        };
        let remaining_journal = retention_limits
            .tenant_bytes
            .checked_sub(retention.metered_bytes)
            .ok_or(BrainError::TenantJournalQuotaExceeded {
                requested: retention.metered_bytes,
                limit: retention_limits.tenant_bytes,
            })?;
        if retention_limits.tenant_sessions == 0 {
            return Err(BrainError::TenantRetainedSessionQuotaExceeded { limit: 0 });
        }
        let retention_meter = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(tenant_pk(&doc.tenant_id)))
            .key("sk", AttributeValue::S(TENANT_RETENTION_SK.into()))
            .condition_expression(
                "(attribute_not_exists(total_bytes) OR total_bytes <= :remaining) AND \
                 (attribute_not_exists(session_count) OR session_count < :session_limit)",
            )
            .update_expression(
                "SET total_bytes = if_not_exists(total_bytes, :zero) + :requested, \
                 session_count = if_not_exists(session_count, :zero) + :one",
            )
            .expression_attribute_values(":zero", AttributeValue::N("0".into()))
            .expression_attribute_values(":one", AttributeValue::N("1".into()))
            .expression_attribute_values(
                ":requested",
                AttributeValue::N(retention.metered_bytes.to_string()),
            )
            .expression_attribute_values(
                ":remaining",
                AttributeValue::N(remaining_journal.to_string()),
            )
            .expression_attribute_values(
                ":session_limit",
                AttributeValue::N(retention_limits.tenant_sessions.to_string()),
            )
            .build()
            .map_err(|error| {
                BrainError::Journal(format!("create tenant retention meter: {error}"))
            })?;
        let create_retention_index = transaction
            .get_transact_items()
            .as_ref()
            .map_or(0, Vec::len);
        transaction = transaction
            .transact_items(TransactWriteItem::builder().update(retention_meter).build());
        // Same conflict-retry discipline as commit — create transacts on the shared tenant
        // meters and can collide with any sibling session's commit.
        let token = transaction_token(&[session_id.as_bytes(), b"create"]);
        let mut attempt = 0u32;
        let outcome = loop {
            match transaction
                .clone()
                .client_request_token(token.clone())
                .send()
                .await
            {
                Err(error) if transaction_conflicted(&error) && attempt < 5 => {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(20u64 << attempt)).await;
                }
                other => break other,
            }
        };
        match outcome {
            Ok(_) => Ok(()),
            Err(error)
                if create_meter_index
                    .is_some_and(|index| transaction_condition_failed_at(&error, index)) =>
            {
                Err(BrainError::TenantStorageQuotaExceeded {
                    requested: doc.tenant_metered_storage_bytes,
                    limit: tenant_storage_limit,
                })
            }
            Err(error) if transaction_condition_failed_at(&error, create_retention_index) => {
                let (used, sessions) = self.load_tenant_retention(&doc.tenant_id).await?;
                if sessions >= retention_limits.tenant_sessions {
                    Err(BrainError::TenantRetainedSessionQuotaExceeded {
                        limit: retention_limits.tenant_sessions,
                    })
                } else {
                    let _ = used;
                    Err(BrainError::TenantJournalQuotaExceeded {
                        requested: retention.metered_bytes,
                        limit: retention_limits.tenant_bytes,
                    })
                }
            }
            Err(error) if conditional_failure(&error) => {
                match self.get_head(session_id).await {
                    Ok(_) => {
                        return Err(BrainError::Invalid(format!(
                            "session {session_id} already exists"
                        )));
                    }
                    Err(BrainError::NoSuchSession(_)) => {}
                    Err(read_error) => return Err(read_error),
                }
                let Some(parent_id) = &doc.parent_id else {
                    // A root create failed a transaction condition, yet no HEAD exists for
                    // this id: the failed condition was one of the meter/quota items, not an
                    // id collision. Report it honestly instead of "already exists".
                    return Err(BrainError::Journal(format!(
                        "create failed a transaction condition without an existing HEAD: {}",
                        describe(&error)
                    )));
                };
                let parent = match self.get_head(parent_id).await {
                    Ok(parent) => parent,
                    Err(BrainError::NoSuchSession(_)) => {
                        return Err(BrainError::Invalid(
                            "child admission is closed or its rooted scope is stale".into(),
                        ));
                    }
                    Err(read_error) => return Err(read_error),
                };
                let root = match self.get_head(&doc.root_id).await {
                    Ok(root) => root,
                    Err(BrainError::NoSuchSession(_)) => {
                        return Err(BrainError::Invalid(
                            "child admission is closed or its rooted scope is stale".into(),
                        ));
                    }
                    Err(read_error) => return Err(read_error),
                };
                if child_admission_open(&parent.doc) && child_admission_open(&root.doc) {
                    Err(BrainError::Overloaded)
                } else {
                    Err(BrainError::Invalid(
                        "child admission is closed or its rooted scope is stale".into(),
                    ))
                }
            }
            Err(error) => Err(BrainError::Journal(format!("create: {}", describe(&error)))),
        }
    }

    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head> {
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
            .expression_attribute_values(":me", AttributeValue::S(owner.to_string()))
            .expression_attribute_values(
                ":stealable",
                AttributeValue::N(now_ms.saturating_sub(STEAL_GRACE_MS).to_string()),
            )
            .expression_attribute_values(
                ":expires",
                AttributeValue::N((now_ms + LEASE_MS).to_string()),
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
        parse_head(session_id, attrs, self.load_config(session_id).await?)
    }

    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<EndFence> {
        // A normal commit may win immediately before this transaction. Retry from a fresh strong
        // HEAD so that commit is retained and the eventual fence still closes admission. Once
        // this transaction wins, its fence increment invalidates every earlier owner atomically
        // with `admission_open=false` and the durable State record.
        for _ in 0..8 {
            let head = self.get_head(session_id).await?;
            let Some((doc, sequence, record)) = project_end_fence(&head, now_ms)? else {
                return Ok(EndFence {
                    head,
                    newly_fenced: false,
                });
            };
            let next_fence = head
                .fence
                .checked_add(1)
                .ok_or_else(|| BrainError::Journal("journal fence exhausted".into()))?;
            let due_ms = doc.recovery_due_ms.ok_or_else(|| {
                BrainError::Journal("ending session has no recovery projection".into())
            })?;
            let next_retention = project_retention(
                head.retention,
                &[(sequence, record.clone())],
                retention_limits.session_bytes,
            )?;
            let journal_delta = retention_delta(head.retention, next_retention)?;
            let update = Update::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(session_pk(session_id)))
                .key("sk", AttributeValue::S("HEAD".into()))
                .condition_expression(
                    "fence = :old_fence AND last_seq = :old_seq AND \
                     journal_metered_bytes = :old_journal_meter AND \
                     journal_effect_reserve_bytes = :old_effect_reserve AND \
                     journal_lifecycle_reserve_bytes = :old_lifecycle_reserve",
                )
                .update_expression(
                    "SET fence = :new_fence, last_seq = :seq, doc = :doc, \
                     listing_doc = :listing, lease_expires_ms = :zero, \
                     tenant_pk = :tenant, tenant_state_pk = :tenant_state, \
                     tenant_sk = :tenant_sk, #state = :state, updated_ms = :updated_ms, \
                     root_id = :root_id, active_phase = :active_phase, parent_id = :parent_id, \
                     admission_open = :closed, recovery_shard = :recovery_shard, \
                     recovery_due_key = :recovery_due_key, \
                     journal_metered_bytes = :journal_meter, \
                     journal_effect_reserve_bytes = :effect_reserve, \
                     journal_lifecycle_reserve_bytes = :lifecycle_reserve REMOVE owner_id",
                )
                .expression_attribute_names("#state", "state")
                .expression_attribute_values(
                    ":old_fence",
                    AttributeValue::N(head.fence.to_string()),
                )
                .expression_attribute_values(
                    ":old_seq",
                    AttributeValue::N(head.last_seq.to_string()),
                )
                .expression_attribute_values(
                    ":old_journal_meter",
                    AttributeValue::N(head.retention.metered_bytes.to_string()),
                )
                .expression_attribute_values(
                    ":old_effect_reserve",
                    AttributeValue::N(head.retention.effect_reserve_bytes.to_string()),
                )
                .expression_attribute_values(
                    ":old_lifecycle_reserve",
                    AttributeValue::N(head.retention.lifecycle_reserve_bytes.to_string()),
                )
                .expression_attribute_values(
                    ":new_fence",
                    AttributeValue::N(next_fence.to_string()),
                )
                .expression_attribute_values(":seq", AttributeValue::N(sequence.to_string()))
                .expression_attribute_values(
                    ":doc",
                    AttributeValue::S(serde_json::to_string(&doc.control_doc())?),
                )
                .expression_attribute_values(
                    ":listing",
                    AttributeValue::S(serde_json::to_string(&SessionSummary::from_head(
                        session_id, &doc,
                    ))?),
                )
                .expression_attribute_values(":zero", AttributeValue::N("0".into()))
                .expression_attribute_values(
                    ":tenant",
                    AttributeValue::S(tenant_pk(&doc.tenant_id)),
                )
                .expression_attribute_values(
                    ":tenant_state",
                    AttributeValue::S(tenant_state_pk(&doc.tenant_id, doc.state.as_str())),
                )
                .expression_attribute_values(
                    ":tenant_sk",
                    AttributeValue::S(tenant_session_sort_key(doc.updated_ms, session_id)),
                )
                .expression_attribute_values(
                    ":state",
                    AttributeValue::S(doc.state.as_str().to_string()),
                )
                .expression_attribute_values(
                    ":updated_ms",
                    AttributeValue::N(doc.updated_ms.to_string()),
                )
                .expression_attribute_values(":root_id", AttributeValue::S(doc.root_id.clone()))
                .expression_attribute_values(
                    ":active_phase",
                    AttributeValue::S(
                        doc.active_phase
                            .map(|phase| phase.as_str().to_string())
                            .unwrap_or_default(),
                    ),
                )
                .expression_attribute_values(
                    ":parent_id",
                    AttributeValue::S(doc.parent_id.clone().unwrap_or_default()),
                )
                .expression_attribute_values(":closed", AttributeValue::Bool(false))
                .expression_attribute_values(
                    ":recovery_shard",
                    AttributeValue::S(recovery_shard(session_id)),
                )
                .expression_attribute_values(
                    ":recovery_due_key",
                    AttributeValue::S(recovery_due_key(due_ms, session_id)),
                )
                .expression_attribute_values(
                    ":journal_meter",
                    AttributeValue::N(next_retention.metered_bytes.to_string()),
                )
                .expression_attribute_values(
                    ":effect_reserve",
                    AttributeValue::N(next_retention.effect_reserve_bytes.to_string()),
                )
                .expression_attribute_values(
                    ":lifecycle_reserve",
                    AttributeValue::N(next_retention.lifecycle_reserve_bytes.to_string()),
                )
                .build()
                .map_err(|error| BrainError::Journal(format!("end fence update: {error}")))?;
            let record_put = self.record_put(session_id, sequence, now_ms, &record)?;
            let mut transaction = self
                .db
                .transact_write_items()
                .transact_items(TransactWriteItem::builder().update(update).build())
                .transact_items(TransactWriteItem::builder().put(record_put).build());
            if let Some(parent_id) = &doc.parent_id {
                let link = Put::builder()
                    .table_name(&self.table)
                    .item("pk", AttributeValue::S(session_pk(parent_id)))
                    .item(
                        "sk",
                        AttributeValue::S(format!("{CHILD_SK_PREFIX}{session_id}")),
                    )
                    .item("child_id", AttributeValue::S(session_id.to_owned()))
                    .item(
                        "listing_doc",
                        AttributeValue::S(serde_json::to_string(&SessionSummary::from_head(
                            session_id, &doc,
                        ))?),
                    )
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("end fence child link: {error}"))
                    })?;
                transaction =
                    transaction.transact_items(TransactWriteItem::builder().put(link).build());
            }
            let retention_meter_index = if journal_delta == 0 {
                None
            } else {
                let mut meter = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(tenant_pk(&doc.tenant_id)))
                    .key("sk", AttributeValue::S(TENANT_RETENTION_SK.into()))
                    .expression_attribute_values(
                        ":delta",
                        AttributeValue::N(journal_delta.to_string()),
                    );
                if journal_delta > 0 {
                    let requested = journal_delta as u64;
                    let remaining = retention_limits.tenant_bytes.checked_sub(requested).ok_or(
                        BrainError::TenantJournalQuotaExceeded {
                            requested,
                            limit: retention_limits.tenant_bytes,
                        },
                    )?;
                    meter = meter
                        .condition_expression("total_bytes <= :remaining")
                        .update_expression("ADD total_bytes :delta")
                        .expression_attribute_values(
                            ":remaining",
                            AttributeValue::N(remaining.to_string()),
                        );
                } else {
                    meter = meter
                        .condition_expression("total_bytes >= :release")
                        .update_expression("ADD total_bytes :delta")
                        .expression_attribute_values(
                            ":release",
                            AttributeValue::N(journal_delta.unsigned_abs().to_string()),
                        );
                }
                let meter = meter.build().map_err(|error| {
                    BrainError::Journal(format!("end fence tenant retention meter: {error}"))
                })?;
                let index = transaction
                    .get_transact_items()
                    .as_ref()
                    .map_or(0, Vec::len);
                transaction =
                    transaction.transact_items(TransactWriteItem::builder().update(meter).build());
                Some(index)
            };
            match transaction.send().await {
                Ok(_) => {
                    return Ok(EndFence {
                        head: Head {
                            session_id: session_id.into(),
                            doc,
                            fence: next_fence,
                            last_seq: sequence,
                            retention: next_retention,
                        },
                        newly_fenced: true,
                    });
                }
                Err(error)
                    if retention_meter_index
                        .is_some_and(|index| transaction_condition_failed_at(&error, index)) =>
                {
                    if journal_delta > 0 {
                        return Err(BrainError::TenantJournalQuotaExceeded {
                            requested: journal_delta as u64,
                            limit: retention_limits.tenant_bytes,
                        });
                    }
                    return Err(BrainError::Journal(
                        "tenant journal meter rejected an impossible end-fence release".into(),
                    ));
                }
                Err(error) if conditional_failure(&error) => continue,
                Err(error) => {
                    return Err(BrainError::Journal(format!(
                        "end fence: {}",
                        describe(&error)
                    )));
                }
            }
        }
        Err(BrainError::Overloaded)
    }

    async fn get_head(&self, session_id: &str) -> Result<Head> {
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
        parse_head(session_id, attrs, self.load_config(session_id).await?)
    }

    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> Result<RecordPage> {
        let (limit, max_bytes) = validate_record_page_query(query)?;
        if query.after >= query.through_seq {
            return Ok(RecordPage {
                entries: Vec::new(),
                next_after: None,
            });
        }
        let mut entries = Vec::new();
        let out = self
            .db
            .query()
            .table_name(&self.table)
            .key_condition_expression("pk = :pk AND sk BETWEEN :lo AND :hi")
            .expression_attribute_values(":pk", AttributeValue::S(session_pk(query.session_id)))
            .expression_attribute_values(
                ":lo",
                AttributeValue::S(record_sk(query.after.saturating_add(1))),
            )
            .expression_attribute_values(":hi", AttributeValue::S(record_sk(query.through_seq)))
            .consistent_read(true)
            .limit((limit + 1) as i32)
            .send()
            .await
            .map_err(|e| BrainError::Journal(format!("read page: {}", describe(&e))))?;
        let mut bytes = 0usize;
        let mut more = out.last_evaluated_key().is_some();
        for item in out.items() {
            let entry = parse_entry(item)?;
            let record_bytes = serde_json::to_vec(&entry.record)?.len();
            if entries.len() >= limit || bytes.saturating_add(record_bytes) > max_bytes {
                more = true;
                break;
            }
            bytes = bytes.saturating_add(record_bytes);
            entries.push(entry);
        }
        let next_after = more.then(|| entries.last().expect("page limit admits one record").seq);
        Ok(RecordPage {
            entries,
            next_after,
        })
    }

    async fn commit(&self, decision: &brain::journal::CommitDecision<'_>) -> Result<()> {
        let &brain::journal::CommitDecision {
            session_id,
            owner,
            fence,
            records,
            doc,
            high_water,
            now_ms,
            tenant_storage_delta,
            tenant_storage_limit,
            retention,
            tenant_retention_delta,
            retention_limits,
        } = decision;
        if retention.metered_bytes > retention_limits.session_bytes {
            return Err(BrainError::SessionJournalQuotaExceeded {
                requested: retention.metered_bytes,
                limit: retention_limits.session_bytes,
            });
        }
        let old_journal_meter = if tenant_retention_delta >= 0 {
            retention
                .metered_bytes
                .checked_sub(tenant_retention_delta as u64)
        } else {
            retention
                .metered_bytes
                .checked_add(tenant_retention_delta.unsigned_abs())
        }
        .ok_or_else(|| {
            BrainError::Journal("journal retention delta does not match the projection".into())
        })?;
        let mut tx = self.db.transact_write_items();
        if requires_ancestor_admission(records) {
            for ancestor_id in &doc.ancestor_ids {
                let condition = ConditionCheck::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(ancestor_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression("admission_open = :open AND root_id = :root")
                    .expression_attribute_values(":open", AttributeValue::Bool(true))
                    .expression_attribute_values(":root", AttributeValue::S(doc.root_id.clone()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("ancestor turn admission condition: {error}"))
                    })?;
                tx = tx.transact_items(
                    TransactWriteItem::builder()
                        .condition_check(condition)
                        .build(),
                );
            }
        }
        for (seq, record) in records {
            let put = self.record_put(session_id, *seq, now_ms, record)?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());
        }
        let mut update = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression(
                "fence = :fence AND owner_id = :me AND \
                 journal_metered_bytes = :old_journal_meter",
            )
            // Deliberately no `ADD fence`: renewing must not fence out the renewer.
            .update_expression(if doc.recovery_due_ms.is_some() {
                "SET last_seq = :hw, doc = :doc, listing_doc = :listing, \
                 lease_expires_ms = :expires, tenant_pk = :tenant, \
                 tenant_state_pk = :tenant_state, tenant_sk = :tenant_sk, \
                 #state = :state, updated_ms = :updated_ms, root_id = :root_id, \
                 active_phase = :active_phase, parent_id = :parent_id, \
                 admission_open = :admission_open, journal_metered_bytes = :journal_meter, \
                 journal_effect_reserve_bytes = :effect_reserve, \
                 journal_lifecycle_reserve_bytes = :lifecycle_reserve, \
                 recovery_shard = :recovery_shard, recovery_due_key = :recovery_due_key"
            } else {
                "SET last_seq = :hw, doc = :doc, listing_doc = :listing, \
                 lease_expires_ms = :expires, tenant_pk = :tenant, \
                 tenant_state_pk = :tenant_state, tenant_sk = :tenant_sk, \
                 #state = :state, updated_ms = :updated_ms, root_id = :root_id, \
                 active_phase = :active_phase, parent_id = :parent_id, \
                 admission_open = :admission_open, journal_metered_bytes = :journal_meter, \
                 journal_effect_reserve_bytes = :effect_reserve, \
                 journal_lifecycle_reserve_bytes = :lifecycle_reserve \
                 REMOVE recovery_shard, recovery_due_key"
            })
            .expression_attribute_names("#state", "state")
            .expression_attribute_values(":fence", AttributeValue::N(fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(owner.to_string()))
            .expression_attribute_values(
                ":old_journal_meter",
                AttributeValue::N(old_journal_meter.to_string()),
            )
            .expression_attribute_values(":hw", AttributeValue::N(high_water.to_string()))
            .expression_attribute_values(
                ":doc",
                AttributeValue::S(serde_json::to_string(&doc.control_doc())?),
            )
            .expression_attribute_values(
                ":listing",
                AttributeValue::S(serde_json::to_string(&SessionSummary::from_head(
                    session_id, doc,
                ))?),
            )
            .expression_attribute_values(":tenant", AttributeValue::S(tenant_pk(&doc.tenant_id)))
            .expression_attribute_values(
                ":tenant_state",
                AttributeValue::S(tenant_state_pk(&doc.tenant_id, doc.state.as_str())),
            )
            .expression_attribute_values(
                ":tenant_sk",
                AttributeValue::S(tenant_session_sort_key(doc.updated_ms, session_id)),
            )
            .expression_attribute_values(
                ":expires",
                AttributeValue::N((now_ms + LEASE_MS).to_string()),
            )
            .expression_attribute_values(
                ":state",
                AttributeValue::S(doc.state.as_str().to_string()),
            )
            .expression_attribute_values(
                ":updated_ms",
                AttributeValue::N(doc.updated_ms.to_string()),
            )
            .expression_attribute_values(":root_id", AttributeValue::S(doc.root_id.clone()))
            .expression_attribute_values(
                ":admission_open",
                AttributeValue::Bool(child_admission_open(doc)),
            )
            .expression_attribute_values(
                ":active_phase",
                AttributeValue::S(
                    doc.active_phase
                        .map(|phase| phase.as_str().to_string())
                        .unwrap_or_default(),
                ),
            )
            .expression_attribute_values(
                ":parent_id",
                AttributeValue::S(doc.parent_id.clone().unwrap_or_default()),
            )
            .expression_attribute_values(
                ":journal_meter",
                AttributeValue::N(retention.metered_bytes.to_string()),
            )
            .expression_attribute_values(
                ":effect_reserve",
                AttributeValue::N(retention.effect_reserve_bytes.to_string()),
            )
            .expression_attribute_values(
                ":lifecycle_reserve",
                AttributeValue::N(retention.lifecycle_reserve_bytes.to_string()),
            );
        if let Some(due_ms) = doc.recovery_due_ms {
            update = update
                .expression_attribute_values(
                    ":recovery_shard",
                    AttributeValue::S(recovery_shard(session_id)),
                )
                .expression_attribute_values(
                    ":recovery_due_key",
                    AttributeValue::S(recovery_due_key(due_ms, session_id)),
                );
        }
        let update = update
            .build()
            .map_err(|e| BrainError::Journal(format!("head update: {e}")))?;
        tx = tx.transact_items(TransactWriteItem::builder().update(update).build());
        if let Some(parent_id) = &doc.parent_id {
            let link = Put::builder()
                .table_name(&self.table)
                .item("pk", AttributeValue::S(session_pk(parent_id)))
                .item(
                    "sk",
                    AttributeValue::S(format!("{CHILD_SK_PREFIX}{session_id}")),
                )
                .item("child_id", AttributeValue::S(session_id.to_owned()))
                .item(
                    "listing_doc",
                    AttributeValue::S(serde_json::to_string(&SessionSummary::from_head(
                        session_id, doc,
                    ))?),
                )
                .build()
                .map_err(|error| BrainError::Journal(format!("child link update: {error}")))?;
            tx = tx.transact_items(TransactWriteItem::builder().put(link).build());
        }
        let meter_index = if tenant_storage_delta == 0 {
            None
        } else {
            let delta = tenant_storage_delta;
            // `:zero` is referenced only by the growth expression; binding it on the release
            // branch made DynamoDB reject EVERY storage-releasing commit (session end) with
            // "ExpressionAttributeValues unused: {:zero}" — an endless end-retry loop.
            let mut meter = Update::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(tenant_pk(&doc.tenant_id)))
                .key("sk", AttributeValue::S(TENANT_STORAGE_SK.into()))
                .expression_attribute_values(":delta", AttributeValue::N(delta.to_string()));
            if delta > 0 {
                let requested = delta as u64;
                let remaining = tenant_storage_limit.checked_sub(requested).ok_or(
                    BrainError::TenantStorageQuotaExceeded {
                        requested,
                        limit: tenant_storage_limit,
                    },
                )?;
                meter = meter
                    .condition_expression(
                        "attribute_not_exists(total_bytes) OR total_bytes <= :remaining",
                    )
                    .update_expression(
                        "SET total_bytes = if_not_exists(total_bytes, :zero) + :delta",
                    )
                    .expression_attribute_values(":zero", AttributeValue::N("0".into()))
                    .expression_attribute_values(
                        ":remaining",
                        AttributeValue::N(remaining.to_string()),
                    );
            } else {
                meter = meter
                    .condition_expression("total_bytes >= :release")
                    .update_expression("ADD total_bytes :delta")
                    .expression_attribute_values(
                        ":release",
                        AttributeValue::N(delta.unsigned_abs().to_string()),
                    );
            }
            let meter = meter
                .build()
                .map_err(|error| BrainError::Journal(format!("tenant storage meter: {error}")))?;
            // Cancellation reasons preserve the transaction item order. Record the exact slot
            // before appending the meter so a quota condition is never confused with a fence.
            let index = tx.get_transact_items().as_ref().map_or(0, Vec::len);
            tx = tx.transact_items(TransactWriteItem::builder().update(meter).build());
            Some((index, delta))
        };
        let retention_meter_index = if tenant_retention_delta == 0 {
            None
        } else {
            let delta = tenant_retention_delta;
            let mut meter = Update::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(tenant_pk(&doc.tenant_id)))
                .key("sk", AttributeValue::S(TENANT_RETENTION_SK.into()))
                .expression_attribute_values(":delta", AttributeValue::N(delta.to_string()));
            if delta > 0 {
                let requested = delta as u64;
                let remaining = retention_limits.tenant_bytes.checked_sub(requested).ok_or(
                    BrainError::TenantJournalQuotaExceeded {
                        requested,
                        limit: retention_limits.tenant_bytes,
                    },
                )?;
                meter = meter
                    .condition_expression("total_bytes <= :remaining")
                    .update_expression("ADD total_bytes :delta")
                    .expression_attribute_values(
                        ":remaining",
                        AttributeValue::N(remaining.to_string()),
                    );
            } else {
                meter = meter
                    .condition_expression("total_bytes >= :release")
                    .update_expression("ADD total_bytes :delta")
                    .expression_attribute_values(
                        ":release",
                        AttributeValue::N(delta.unsigned_abs().to_string()),
                    );
            }
            let meter = meter
                .build()
                .map_err(|error| BrainError::Journal(format!("tenant journal meter: {error}")))?;
            let index = tx.get_transact_items().as_ref().map_or(0, Vec::len);
            tx = tx.transact_items(TransactWriteItem::builder().update(meter).build());
            Some((index, delta))
        };
        // Retry the identical transaction on cross-session TransactionConflict (shared
        // tenant-meter contention) under one stable client request token, so a retry of an
        // invisibly-committed attempt settles as success instead of failing its own
        // conditions. Only non-conflict failures reach classification.
        let classify = |error| {
            if let Some((index, delta)) = meter_index
                && transaction_condition_failed_at(&error, index)
            {
                if delta > 0 {
                    return BrainError::TenantStorageQuotaExceeded {
                        requested: delta as u64,
                        limit: tenant_storage_limit,
                    };
                }
                return BrainError::Journal(
                    "tenant storage meter rejected an impossible negative transition".into(),
                );
            }
            if let Some((index, delta)) = retention_meter_index
                && transaction_condition_failed_at(&error, index)
            {
                if delta > 0 {
                    return BrainError::TenantJournalQuotaExceeded {
                        requested: delta as u64,
                        limit: retention_limits.tenant_bytes,
                    };
                }
                return BrainError::Journal(
                    "tenant journal meter rejected an impossible negative transition".into(),
                );
            }
            match conditional_failure(&error) {
                true => BrainError::Fenced,
                false => BrainError::Journal(format!("commit: {}", describe(&error))),
            }
        };
        let token = transaction_token(&[
            session_id.as_bytes(),
            &fence.to_be_bytes(),
            &high_water.to_be_bytes(),
        ]);
        let mut attempt = 0u32;
        loop {
            match tx.clone().client_request_token(token.clone()).send().await {
                Ok(_) => break,
                Err(error) if transaction_conflicted(&error) && attempt < 5 => {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(20u64 << attempt)).await;
                }
                Err(error) => return Err(classify(error)),
            }
        }
        Ok(())
    }

    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()> {
        let r = self
            .db
            .update_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("fence = :fence AND owner_id = :me")
            .update_expression("REMOVE owner_id, lease_expires_ms")
            .expression_attribute_values(":fence", AttributeValue::N(fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(owner.to_string()))
            .send()
            .await;
        match r {
            Ok(_) => Ok(()),
            // A release that lost its fence has already been superseded.
            Err(e) if conditional_failure(&e) => Ok(()),
            Err(e) => Err(BrainError::Journal(format!("release: {}", describe(&e)))),
        }
    }

    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> Result<()> {
        let mut control = doc.control_doc();
        control.recovery_due_ms = Some(due_ms);
        self.db
            .update_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("fence = :fence AND owner_id = :me")
            .update_expression(
                "SET #doc = :doc, recovery_shard = :recovery_shard, \
                 recovery_due_key = :recovery_due_key REMOVE owner_id, lease_expires_ms",
            )
            .expression_attribute_names("#doc", "doc")
            .expression_attribute_values(":fence", AttributeValue::N(fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(owner.to_owned()))
            .expression_attribute_values(
                ":doc",
                AttributeValue::S(serde_json::to_string(&control)?),
            )
            .expression_attribute_values(
                ":recovery_shard",
                AttributeValue::S(recovery_shard(session_id)),
            )
            .expression_attribute_values(
                ":recovery_due_key",
                AttributeValue::S(recovery_due_key(due_ms, session_id)),
            )
            .send()
            .await
            .map_err(|error| match conditional_failure(&error) {
                true => BrainError::Fenced,
                false => BrainError::Journal(format!(
                    "release and schedule recovery: {}",
                    describe(&error)
                )),
            })?;
        Ok(())
    }

    async fn renew(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        now_ms: u64,
        recovery_due_ms: Option<u64>,
    ) -> Result<()> {
        let mut update = self
            .db
            .update_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("fence = :fence AND owner_id = :me")
            .expression_attribute_values(":fence", AttributeValue::N(fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(owner.to_owned()))
            .expression_attribute_values(
                ":expires",
                AttributeValue::N(now_ms.saturating_add(LEASE_MS).to_string()),
            );
        if let Some(recovery_due_ms) = recovery_due_ms {
            update = update
                .update_expression(
                    "SET lease_expires_ms = :expires, recovery_shard = :recovery_shard, \
                     recovery_due_key = :recovery_due_key",
                )
                .expression_attribute_values(
                    ":recovery_shard",
                    AttributeValue::S(recovery_shard(session_id)),
                )
                .expression_attribute_values(
                    ":recovery_due_key",
                    AttributeValue::S(recovery_due_key(recovery_due_ms, session_id)),
                );
        } else {
            update = update.update_expression("SET lease_expires_ms = :expires");
        }
        update
            .send()
            .await
            .map_err(|error| match conditional_failure(&error) {
                true => BrainError::Fenced,
                false => BrainError::Journal(format!("renew: {}", describe(&error))),
            })?;
        Ok(())
    }

    async fn purge_history(&self, session_id: &str) -> Result<u64> {
        let mut deleted = 0u64;
        loop {
            // Restart from the beginning after each page. A lost response or deleted
            // LastEvaluatedKey therefore cannot strand history, and CONFIG/HEAD remain as the
            // durable deletion-retry anchor until final `purge`.
            let out = self
                .db
                .query()
                .table_name(&self.table)
                .key_condition_expression("pk = :pk")
                .expression_attribute_values(":pk", AttributeValue::S(session_pk(session_id)))
                .projection_expression("pk, sk")
                .consistent_read(true)
                .limit(1_000)
                .send()
                .await
                .map_err(|e| {
                    BrainError::Journal(format!("purge history query: {}", describe(&e)))
                })?;
            let mut requests = Vec::new();
            for item in out.items() {
                let Some(sk) = item.get("sk").and_then(|value| value.as_s().ok()) else {
                    continue;
                };
                if sk == "HEAD" || sk.starts_with("CONFIG#") {
                    continue;
                }
                let pk = item
                    .get("pk")
                    .and_then(|value| value.as_s().ok())
                    .cloned()
                    .unwrap_or_else(|| session_pk(session_id));
                let delete = DeleteRequest::builder()
                    .key("pk", AttributeValue::S(pk))
                    .key("sk", AttributeValue::S(sk.clone()))
                    .build()
                    .map_err(|error| BrainError::Journal(format!("purge history key: {error}")))?;
                requests.push(WriteRequest::builder().delete_request(delete).build());
            }
            if requests.is_empty() {
                return Ok(deleted);
            }
            deleted += self
                .batch_delete_requests(requests, "purge history batch")
                .await?;
        }
    }

    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> Result<()> {
        let body = serde_json::to_string(status)?;
        let request = self
            .db
            .put_item()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(deletion_pk(&status.session_id)))
            .item("sk", AttributeValue::S("STATUS".into()))
            .item("deletion_state", AttributeValue::S(status.state.as_str().to_string()))
            .item("body", AttributeValue::S(body))
            .item(
                "ttl_epoch_s",
                AttributeValue::N((status.expires_at_ms / 1_000).to_string()),
            )
            .condition_expression(
                "attribute_not_exists(deletion_state) OR deletion_state <> :succeeded OR :new_state = :succeeded",
            )
            .expression_attribute_values(":succeeded", AttributeValue::S("succeeded".into()))
            .expression_attribute_values(":new_state", AttributeValue::S(status.state.as_str().to_string()))
            .send()
            .await;
        match request {
            Ok(_) => Ok(()),
            Err(error) if conditional_failure(&error) => Ok(()),
            Err(error) => Err(BrainError::Journal(format!(
                "put deletion status: {}",
                describe(&error)
            ))),
        }
    }

    async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>> {
        let output = self
            .db
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(deletion_pk(session_id)))
            .key("sk", AttributeValue::S("STATUS".into()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|error| {
                BrainError::Journal(format!("get deletion status: {}", describe(&error)))
            })?;
        let Some(item) = output.item else {
            return Ok(None);
        };
        let body = item
            .get("body")
            .and_then(|value| value.as_s().ok())
            .ok_or_else(|| BrainError::Journal("deletion status missing body".into()))?;
        let status = serde_json::from_str(body)
            .map_err(|error| BrainError::Journal(format!("deletion status: {error}")))?;
        Ok(Some(status))
    }

    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()> {
        let tombstone = Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(deletion_pk(&status.session_id)))
            .item("sk", AttributeValue::S("STATUS".into()))
            .item("deletion_state", AttributeValue::S("succeeded".into()))
            .item("body", AttributeValue::S(serde_json::to_string(status)?))
            .item(
                "ttl_epoch_s",
                AttributeValue::N((status.expires_at_ms / 1_000).to_string()),
            )
            // This condition is the exactly-once release barrier for the parent/root child
            // counters below. If the final response was lost, the retry observes the installed
            // tombstone instead of decrementing capacity a second time.
            .condition_expression(
                "attribute_not_exists(deletion_state) OR deletion_state <> :succeeded",
            )
            .expression_attribute_values(":succeeded", AttributeValue::S("succeeded".into()))
            .build()
            .map_err(|error| BrainError::Journal(format!("deletion tombstone: {error}")))?;
        let delete_head = Delete::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(&status.session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("journal_metered_bytes = :journal_bytes AND tenant_pk = :tenant")
            .expression_attribute_values(
                ":journal_bytes",
                AttributeValue::N(status.metered_journal_bytes.to_string()),
            )
            .expression_attribute_values(":tenant", AttributeValue::S(tenant_pk(&status.tenant_id)))
            .build()
            .map_err(|error| BrainError::Journal(format!("delete final HEAD: {error}")))?;
        let delete_config = Delete::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(&status.session_id)))
            .key("sk", AttributeValue::S(CONFIG_SK.into()))
            .build()
            .map_err(|error| BrainError::Journal(format!("delete final CONFIG: {error}")))?;
        let mut transaction = self
            .db
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(tombstone).build())
            .transact_items(TransactWriteItem::builder().delete(delete_head).build())
            .transact_items(TransactWriteItem::builder().delete(delete_config).build());
        if status.metered_storage_bytes > 0 {
            let release_meter = Update::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(tenant_pk(&status.tenant_id)))
                .key("sk", AttributeValue::S(TENANT_STORAGE_SK.into()))
                .condition_expression("total_bytes >= :release")
                .update_expression("ADD total_bytes :minus_release")
                .expression_attribute_values(
                    ":release",
                    AttributeValue::N(status.metered_storage_bytes.to_string()),
                )
                .expression_attribute_values(
                    ":minus_release",
                    AttributeValue::N(format!("-{}", status.metered_storage_bytes)),
                )
                .build()
                .map_err(|error| {
                    BrainError::Journal(format!("release tenant storage meter: {error}"))
                })?;
            transaction = transaction
                .transact_items(TransactWriteItem::builder().update(release_meter).build());
        }
        let mut release_retention = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(tenant_pk(&status.tenant_id)))
            .key("sk", AttributeValue::S(TENANT_RETENTION_SK.into()))
            .condition_expression("total_bytes >= :release AND session_count >= :one")
            .expression_attribute_values(":one", AttributeValue::N("1".into()))
            .expression_attribute_values(
                ":release",
                AttributeValue::N(status.metered_journal_bytes.to_string()),
            )
            .expression_attribute_values(":minus_one", AttributeValue::N("-1".into()));
        release_retention = if status.metered_journal_bytes == 0 {
            release_retention.update_expression("ADD session_count :minus_one")
        } else {
            release_retention
                .update_expression("ADD total_bytes :minus_release, session_count :minus_one")
                .expression_attribute_values(
                    ":minus_release",
                    AttributeValue::N(format!("-{}", status.metered_journal_bytes)),
                )
        };
        let release_retention = release_retention.build().map_err(|error| {
            BrainError::Journal(format!("release tenant journal retention meter: {error}"))
        })?;
        transaction = transaction.transact_items(
            TransactWriteItem::builder()
                .update(release_retention)
                .build(),
        );
        if let Some(parent_id) = &status.parent_id {
            let delete_link = Delete::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(session_pk(parent_id)))
                .key(
                    "sk",
                    AttributeValue::S(format!("{CHILD_SK_PREFIX}{}", status.session_id)),
                )
                .build()
                .map_err(|error| {
                    BrainError::Journal(format!("delete parent child adjacency: {error}"))
                })?;
            transaction = transaction
                .transact_items(TransactWriteItem::builder().delete(delete_link).build());
            if parent_id == &status.root_id {
                let release = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(parent_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression("direct_child_count >= :one AND descendant_count >= :one")
                    .update_expression(
                        "ADD direct_child_count :minus_one, descendant_count :minus_one",
                    )
                    .expression_attribute_values(":one", AttributeValue::N("1".into()))
                    .expression_attribute_values(":minus_one", AttributeValue::N("-1".into()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("release root child admission: {error}"))
                    })?;
                transaction = transaction
                    .transact_items(TransactWriteItem::builder().update(release).build());
            } else {
                let release_parent = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(parent_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression("direct_child_count >= :one")
                    .update_expression("ADD direct_child_count :minus_one")
                    .expression_attribute_values(":one", AttributeValue::N("1".into()))
                    .expression_attribute_values(":minus_one", AttributeValue::N("-1".into()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("release direct child admission: {error}"))
                    })?;
                let release_root = Update::builder()
                    .table_name(&self.table)
                    .key("pk", AttributeValue::S(session_pk(&status.root_id)))
                    .key("sk", AttributeValue::S("HEAD".into()))
                    .condition_expression("descendant_count >= :one")
                    .update_expression("ADD descendant_count :minus_one")
                    .expression_attribute_values(":one", AttributeValue::N("1".into()))
                    .expression_attribute_values(":minus_one", AttributeValue::N("-1".into()))
                    .build()
                    .map_err(|error| {
                        BrainError::Journal(format!("release root descendant admission: {error}"))
                    })?;
                transaction = transaction
                    .transact_items(TransactWriteItem::builder().update(release_parent).build())
                    .transact_items(TransactWriteItem::builder().update(release_root).build());
            }
        }
        match transaction.send().await {
            Ok(_) => Ok(()),
            Err(error) if conditional_failure(&error) => {
                if self
                    .get_deletion_status(&status.session_id)
                    .await?
                    .is_some_and(|existing| {
                        existing.state == brain::journal::DeletionState::Succeeded
                    })
                {
                    Ok(())
                } else {
                    Err(BrainError::Journal(format!(
                        "finalize deletion condition failed: {}",
                        describe(&error)
                    )))
                }
            }
            Err(error) => Err(BrainError::Journal(format!(
                "finalize deletion: {}",
                describe(&error)
            ))),
        }
    }

    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> Result<SessionPage> {
        let (index, partition_name, partition_value) = match query.state {
            Some(state) => (
                TENANT_STATE_SESSIONS_INDEX,
                "tenant_state_pk",
                tenant_state_pk(query.tenant_id, state.as_str()),
            ),
            None => (
                TENANT_SESSIONS_INDEX,
                "tenant_pk",
                tenant_pk(query.tenant_id),
            ),
        };
        let start_key = query
            .cursor
            .map(|cursor| {
                let session_id = session_id_from_list_cursor(cursor)?;
                let mut key = HashMap::from([
                    (
                        partition_name.to_string(),
                        AttributeValue::S(partition_value.clone()),
                    ),
                    ("tenant_sk".into(), AttributeValue::S(cursor.into())),
                    ("pk".into(), AttributeValue::S(session_pk(session_id))),
                    ("sk".into(), AttributeValue::S("HEAD".into())),
                ]);
                // DynamoDB may include both projected partition attributes in a LastEvaluatedKey
                // when the table projects ALL. Supplying the non-key one is unnecessary and is
                // rejected, so construct exactly the selected index key plus the base key.
                key.shrink_to_fit();
                Ok::<_, BrainError>(key)
            })
            .transpose()?;
        let fetch = query.limit.min(100).min(i32::MAX as usize) as i32;
        let out = self
            .db
            .query()
            .table_name(&self.table)
            .index_name(index)
            .key_condition_expression("#tenant = :tenant")
            .expression_attribute_names("#tenant", partition_name)
            .expression_attribute_values(":tenant", AttributeValue::S(partition_value))
            .limit(fetch)
            .scan_index_forward(true)
            .set_exclusive_start_key(start_key)
            .send()
            .await
            .map_err(|error| BrainError::Journal(format!("tenant list: {}", describe(&error))))?;
        let mut sessions = Vec::with_capacity(out.items().len().min(query.limit));
        for item in out.items() {
            let listing = item
                .get("listing_doc")
                .and_then(|value| value.as_s().ok())
                .ok_or_else(|| BrainError::Journal("listed HEAD missing listing_doc".into()))?;
            sessions.push(
                serde_json::from_str::<SessionSummary>(listing).map_err(|error| {
                    BrainError::Journal(format!("listing_doc does not parse: {error}"))
                })?,
            );
        }
        let has_more = out.last_evaluated_key().is_some();
        let next_cursor = has_more.then(|| {
            let last = sessions.last().expect("a page with more rows is non-empty");
            tenant_session_sort_key(last.updated_ms, &last.session_id)
        });
        Ok(SessionPage {
            sessions,
            next_cursor,
        })
    }

    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> Result<ChildPage> {
        let start_key = query.cursor.map(|child_id| {
            HashMap::from([
                (
                    "pk".to_owned(),
                    AttributeValue::S(session_pk(query.parent_id)),
                ),
                (
                    "sk".to_owned(),
                    AttributeValue::S(format!("{CHILD_SK_PREFIX}{child_id}")),
                ),
            ])
        });
        let limit = query.limit.clamp(1, 100);
        let output = self
            .db
            .query()
            .table_name(&self.table)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(session_pk(query.parent_id)))
            .expression_attribute_values(":prefix", AttributeValue::S(CHILD_SK_PREFIX.into()))
            .consistent_read(true)
            .limit((limit + 1) as i32)
            .set_exclusive_start_key(start_key)
            .send()
            .await
            .map_err(|error| {
                BrainError::Journal(format!("list direct children: {}", describe(&error)))
            })?;
        let mut sessions = output
            .items()
            .iter()
            .map(|item| {
                let body = item
                    .get("listing_doc")
                    .and_then(|value| value.as_s().ok())
                    .ok_or_else(|| BrainError::Journal("child link missing listing_doc".into()))?;
                serde_json::from_str::<SessionSummary>(body).map_err(BrainError::from)
            })
            .collect::<Result<Vec<_>>>()?;
        let has_more = sessions.len() > limit;
        sessions.truncate(limit);
        let next_cursor = has_more.then(|| {
            sessions
                .last()
                .expect("non-empty child page")
                .session_id
                .clone()
        });
        Ok(ChildPage {
            sessions,
            next_cursor,
        })
    }

    async fn reserve_sandbox(
        &self,
        request: &SandboxReserveRequest,
    ) -> Result<SandboxInventoryDoc> {
        let doc = SandboxInventoryDoc {
            root_id: request.root_id.clone(),
            owner_session_id: request.owner_session_id.clone(),
            sandbox_id: request.sandbox_id.clone(),
            operation_id: request.operation_id.clone(),
            request_digest: request.request_digest.clone(),
            generation_intent: request.generation_intent.clone(),
            status: request.initial_status.clone(),
            created_at_ms: request.now_ms,
            updated_at_ms: request.now_ms,
            version: 1,
            slot_released: false,
        };
        let put = Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(session_pk(&request.root_id)))
            .item(
                "sk",
                AttributeValue::S(format!("{SANDBOX_SK_PREFIX}{}", request.sandbox_id)),
            )
            .item("sandbox_id", AttributeValue::S(request.sandbox_id.clone()))
            .item("version", AttributeValue::N("1".into()))
            .item("body", AttributeValue::S(serde_json::to_string(&doc)?))
            .condition_expression("attribute_not_exists(sk)")
            .build()
            .map_err(|error| BrainError::Journal(format!("sandbox inventory put: {error}")))?;
        let reserve = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(&request.root_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression(
                "admission_open = :open AND root_id = :root AND \
                 additional_sandbox_count < max_additional_sandboxes",
            )
            .update_expression("ADD additional_sandbox_count :one")
            .expression_attribute_values(":open", AttributeValue::Bool(true))
            .expression_attribute_values(":root", AttributeValue::S(request.root_id.clone()))
            .expression_attribute_values(":one", AttributeValue::N("1".into()))
            .build()
            .map_err(|error| BrainError::Journal(format!("sandbox slot reserve: {error}")))?;
        match self
            .db
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(reserve).build())
            .transact_items(TransactWriteItem::builder().put(put).build())
            .send()
            .await
        {
            Ok(_) => Ok(doc),
            Err(error) if conditional_failure(&error) => {
                match self
                    .get_sandbox(&request.root_id, &request.sandbox_id)
                    .await
                {
                    Ok(existing)
                        if existing.operation_id == request.operation_id
                            && existing.request_digest == request.request_digest
                            && existing.owner_session_id == request.owner_session_id =>
                    {
                        return Ok(existing);
                    }
                    Ok(_) => return Err(BrainError::IdempotencyConflict),
                    Err(BrainError::FileNotFound(_)) => {}
                    Err(read_error) => return Err(read_error),
                }
                let root = self.get_head(&request.root_id).await?;
                if !child_admission_open(&root.doc) {
                    Err(BrainError::Invalid(
                        "additional sandbox admission is closed for this root".into(),
                    ))
                } else {
                    Err(BrainError::SandboxResourceExhausted)
                }
            }
            Err(error) => Err(BrainError::Journal(format!(
                "reserve additional sandbox: {}",
                describe(&error)
            ))),
        }
    }

    async fn get_sandbox(&self, root_id: &str, sandbox_id: &str) -> Result<SandboxInventoryDoc> {
        let output = self
            .db
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(root_id)))
            .key(
                "sk",
                AttributeValue::S(format!("{SANDBOX_SK_PREFIX}{sandbox_id}")),
            )
            .consistent_read(true)
            .send()
            .await
            .map_err(|error| {
                BrainError::Journal(format!("get sandbox inventory: {}", describe(&error)))
            })?;
        let item = output
            .item()
            .ok_or_else(|| BrainError::FileNotFound(format!("sandbox {sandbox_id}")))?;
        parse_sandbox(item)
    }

    async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> Result<SandboxPage> {
        let start_key = query.cursor.map(|sandbox_id| {
            HashMap::from([
                (
                    "pk".to_owned(),
                    AttributeValue::S(session_pk(query.root_id)),
                ),
                (
                    "sk".to_owned(),
                    AttributeValue::S(format!("{SANDBOX_SK_PREFIX}{sandbox_id}")),
                ),
            ])
        });
        let limit = query.limit.clamp(1, 100);
        let output = self
            .db
            .query()
            .table_name(&self.table)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(session_pk(query.root_id)))
            .expression_attribute_values(":prefix", AttributeValue::S(SANDBOX_SK_PREFIX.into()))
            .consistent_read(true)
            .limit((limit + 1) as i32)
            .set_exclusive_start_key(start_key)
            .send()
            .await
            .map_err(|error| {
                BrainError::Journal(format!("list sandbox inventory: {}", describe(&error)))
            })?;
        let mut sandboxes = output
            .items()
            .iter()
            .map(parse_sandbox)
            .collect::<Result<Vec<_>>>()?;
        let has_more = sandboxes.len() > limit;
        sandboxes.truncate(limit);
        let next_cursor = has_more.then(|| {
            sandboxes
                .last()
                .expect("sandbox page with more rows is non-empty")
                .sandbox_id
                .clone()
        });
        Ok(SandboxPage {
            sandboxes,
            next_cursor,
        })
    }

    async fn update_sandbox(&self, request: &SandboxUpdateRequest) -> Result<SandboxInventoryDoc> {
        let current = self
            .get_sandbox(&request.root_id, &request.sandbox_id)
            .await?;
        if current.version != request.expected_version {
            if serde_json::to_value(&current.status)? == serde_json::to_value(&request.status)? {
                return Ok(current);
            }
            return Err(BrainError::Fenced);
        }
        if serde_json::to_value(&current.status.target)?
            != serde_json::to_value(&request.status.target)?
        {
            return Err(BrainError::Journal(
                "sandbox lifecycle update changed its sealed target".into(),
            ));
        }
        if current.slot_released && !request.release_slot {
            return Err(BrainError::SandboxGone);
        }
        if request.release_slot
            && !matches!(
                request.status.state.to_string().as_str(),
                "gone" | "terminated"
            )
        {
            return Err(BrainError::Journal(
                "sandbox slot may be released only for a confirmed terminal target".into(),
            ));
        }
        let mut next = current.clone();
        next.status = request.status.clone();
        next.updated_at_ms = request.now_ms;
        next.version = next.version.saturating_add(1);
        if request.release_slot {
            next.slot_released = true;
        }
        let update_item = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(&request.root_id)))
            .key(
                "sk",
                AttributeValue::S(format!("{SANDBOX_SK_PREFIX}{}", request.sandbox_id)),
            )
            .condition_expression("version = :expected")
            .update_expression("SET version = :next, body = :body")
            .expression_attribute_values(
                ":expected",
                AttributeValue::N(request.expected_version.to_string()),
            )
            .expression_attribute_values(":next", AttributeValue::N(next.version.to_string()))
            .expression_attribute_values(":body", AttributeValue::S(serde_json::to_string(&next)?))
            .build()
            .map_err(|error| BrainError::Journal(format!("sandbox lifecycle update: {error}")))?;
        let mut transaction = self
            .db
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(update_item).build());
        if request.release_slot && !current.slot_released {
            let release = Update::builder()
                .table_name(&self.table)
                .key("pk", AttributeValue::S(session_pk(&request.root_id)))
                .key("sk", AttributeValue::S("HEAD".into()))
                .condition_expression("additional_sandbox_count >= :one")
                .update_expression("ADD additional_sandbox_count :minus_one")
                .expression_attribute_values(":one", AttributeValue::N("1".into()))
                .expression_attribute_values(":minus_one", AttributeValue::N("-1".into()))
                .build()
                .map_err(|error| BrainError::Journal(format!("sandbox slot release: {error}")))?;
            transaction =
                transaction.transact_items(TransactWriteItem::builder().update(release).build());
        }
        match transaction.send().await {
            Ok(_) => Ok(next),
            Err(error) if conditional_failure(&error) => {
                let observed = self
                    .get_sandbox(&request.root_id, &request.sandbox_id)
                    .await?;
                if serde_json::to_value(&observed.status)? == serde_json::to_value(&request.status)?
                {
                    Ok(observed)
                } else {
                    Err(BrainError::Fenced)
                }
            }
            Err(error) => Err(BrainError::Journal(format!(
                "update additional sandbox: {}",
                describe(&error)
            ))),
        }
    }

    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> Result<RecoveryPage> {
        let start_key = query
            .cursor
            .map(|cursor| {
                let session_id = cursor
                    .split_once('#')
                    .map(|(_, session_id)| session_id)
                    .filter(|session_id| !session_id.is_empty())
                    .ok_or_else(|| BrainError::Invalid("invalid recovery cursor".into()))?;
                Ok::<_, BrainError>(HashMap::from([
                    (
                        "recovery_shard".into(),
                        AttributeValue::S(query.shard.to_owned()),
                    ),
                    (
                        "recovery_due_key".into(),
                        AttributeValue::S(cursor.to_owned()),
                    ),
                    ("pk".into(), AttributeValue::S(session_pk(session_id))),
                    ("sk".into(), AttributeValue::S("HEAD".into())),
                ]))
            })
            .transpose()?;
        let upper = format!("{:020}#\u{10ffff}", query.due_before_ms);
        let output = self
            .db
            .query()
            .table_name(&self.table)
            .index_name(RECOVERY_DUE_INDEX)
            .key_condition_expression("recovery_shard = :shard AND recovery_due_key <= :upper")
            .expression_attribute_values(":shard", AttributeValue::S(query.shard.to_owned()))
            .expression_attribute_values(":upper", AttributeValue::S(upper))
            .limit(query.limit.clamp(1, 100) as i32)
            .scan_index_forward(true)
            .set_exclusive_start_key(start_key)
            .send()
            .await
            .map_err(|error| {
                BrainError::Journal(format!("recovery due query: {}", describe(&error)))
            })?;
        let mut items = Vec::with_capacity(output.items().len());
        for item in output.items() {
            let string = |name: &str| -> Result<String> {
                item.get(name)
                    .and_then(|value| value.as_s().ok())
                    .cloned()
                    .ok_or_else(|| BrainError::Journal(format!("recovery row missing {name}")))
            };
            let number = |name: &str| -> Result<u64> {
                item.get(name)
                    .and_then(|value| value.as_n().ok())
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(|| BrainError::Journal(format!("recovery row missing {name}")))
            };
            let pk = string("pk")?;
            let session_id = pk
                .strip_prefix("S#")
                .ok_or_else(|| {
                    BrainError::Journal(format!("recovery row pk {pk:?} is not a session key"))
                })?
                .to_owned();
            let due_key = string("recovery_due_key")?;
            let due_ms = due_key
                .split_once('#')
                .and_then(|(value, _)| value.parse().ok())
                .ok_or_else(|| BrainError::Journal("invalid recovery due key".into()))?;
            items.push(RecoveryItem {
                session_id,
                due_ms,
                state: string("state")?.parse()?,
                active_phase: item
                    .get("active_phase")
                    .and_then(|value| value.as_s().ok())
                    .filter(|value| !value.is_empty())
                    .map(|value| value.parse())
                    .transpose()?,
                last_seq: number("last_seq")?,
                root_id: string("root_id")?,
                parent_id: item
                    .get("parent_id")
                    .and_then(|value| value.as_s().ok())
                    .filter(|value| !value.is_empty())
                    .cloned(),
                updated_ms: number("updated_ms")?,
            });
        }
        let next_cursor = output.last_evaluated_key().and_then(|key| {
            key.get("recovery_due_key")
                .and_then(|value| value.as_s().ok())
                .cloned()
        });
        Ok(RecoveryPage { items, next_cursor })
    }

    async fn list_sessions(&self, _limit: usize) -> Result<Vec<Head>> {
        Err(BrainError::Invalid(
            "all-session enumeration is local-audit-only; Dynamo requires a tenant-index query"
                .into(),
        ))
    }
}

fn parse_head(
    session_id: &str,
    attrs: &HashMap<String, AttributeValue>,
    config: ConfigDoc,
) -> Result<Head> {
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
    let control = serde_json::from_str(doc_s)
        .map_err(|e| BrainError::Journal(format!("control doc does not parse: {e}")))?;
    Ok(Head {
        session_id: session_id.to_string(),
        doc: HeadDoc::join(control, config),
        fence: n("fence")?,
        last_seq: n("last_seq")?,
        retention: JournalRetention {
            metered_bytes: n("journal_metered_bytes")?,
            effect_reserve_bytes: n("journal_effect_reserve_bytes")?,
            lifecycle_reserve_bytes: n("journal_lifecycle_reserve_bytes")?,
        },
    })
}

fn optional_number(attrs: &HashMap<String, AttributeValue>, name: &str) -> Result<Option<u64>> {
    attrs
        .get(name)
        .map(|value| {
            value
                .as_n()
                .map_err(|_| BrainError::Journal(format!("{name} is not numeric")))?
                .parse::<u64>()
                .map_err(|_| BrainError::Journal(format!("{name} is not an unsigned integer")))
        })
        .transpose()
}

fn parse_config(attrs: &HashMap<String, AttributeValue>) -> Result<ConfigDoc> {
    let content = attrs
        .get("content_base64")
        .and_then(|value| value.as_s().ok())
        .ok_or_else(|| BrainError::Journal("CONFIG missing content".into()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content)
        .map_err(|_| BrainError::Journal("CONFIG content is not base64".into()))?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if attrs
        .get("sha256")
        .and_then(|value| value.as_s().ok())
        .map(String::as_str)
        != Some(digest.as_str())
    {
        return Err(BrainError::Journal("CONFIG digest mismatch".into()));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| BrainError::Journal(format!("CONFIG does not parse: {error}")))
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

fn parse_sandbox(item: &HashMap<String, AttributeValue>) -> Result<SandboxInventoryDoc> {
    let body = item
        .get("body")
        .and_then(|value| value.as_s().ok())
        .ok_or_else(|| BrainError::Journal("sandbox inventory row missing body".into()))?;
    serde_json::from_str(body)
        .map_err(|error| BrainError::Journal(format!("sandbox inventory does not parse: {error}")))
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

fn transaction_condition_failed_at<R>(
    error: &SdkError<aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError, R>,
    index: usize,
) -> bool {
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
    let SdkError::ServiceError(service) = error else {
        return false;
    };
    let TransactWriteItemsError::TransactionCanceledException(cancelled) = service.err() else {
        return false;
    };
    cancelled
        .cancellation_reasons()
        .get(index)
        .and_then(|reason| reason.code())
        == Some("ConditionalCheckFailed")
}

/// A `TransactionCanceledException` whose only meaningful reasons are `TransactionConflict`
/// is a retryable cross-transaction collision — typically two sessions of one tenant
/// updating the shared tenant meter item — not a condition failure. Without this
/// distinction, `conditional_failure` misreports ordinary contention as `Fenced` on commit
/// and as "session already exists" on create (found by the 2026-08-22 kernel-overhead
/// spike, which failed any two concurrent same-tenant sessions).
fn transaction_conflicted<R>(
    error: &SdkError<aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError, R>,
) -> bool {
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
    let SdkError::ServiceError(service) = error else {
        return false;
    };
    let TransactWriteItemsError::TransactionCanceledException(cancelled) = service.err() else {
        return false;
    };
    let mut saw_conflict = false;
    for reason in cancelled.cancellation_reasons() {
        match reason.code() {
            Some("TransactionConflict") => saw_conflict = true,
            Some("None") | None => {}
            // Any other reason (condition failure, throttle, validation) decides the outcome.
            Some(_) => return false,
        }
    }
    saw_conflict
}

/// Stable idempotency token for one logical transaction, so an identical retry after an
/// invisibly-committed attempt settles as success (DynamoDB dedupes for 10 minutes)
/// instead of failing its own conditions.
fn transaction_token(parts: &[&[u8]]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut token = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write;
        let _ = write!(token, "{byte:02x}");
    }
    token
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
mod conflict_tests {
    use super::*;
    use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
    use aws_sdk_dynamodb::types::CancellationReason;
    use aws_sdk_dynamodb::types::error::TransactionCanceledException;

    fn cancelled(codes: &[&str]) -> SdkError<TransactWriteItemsError, ()> {
        let reasons: Vec<CancellationReason> = codes
            .iter()
            .map(|code| CancellationReason::builder().code(*code).build())
            .collect();
        let err = TransactionCanceledException::builder()
            .set_cancellation_reasons(Some(reasons))
            .meta(
                aws_sdk_dynamodb::error::ErrorMetadata::builder()
                    .code("TransactionCanceledException")
                    .build(),
            )
            .build();
        SdkError::service_error(
            TransactWriteItemsError::TransactionCanceledException(err),
            (),
        )
    }

    #[test]
    fn pure_conflict_is_retryable() {
        assert!(transaction_conflicted(&cancelled(&[
            "None",
            "None",
            "None",
            "TransactionConflict"
        ])));
    }

    #[test]
    fn condition_failure_wins_over_conflict() {
        // A real condition failure must reach classification (fence/quota), never retry.
        assert!(!transaction_conflicted(&cancelled(&[
            "ConditionalCheckFailed",
            "TransactionConflict"
        ])));
    }

    #[test]
    fn no_conflict_reason_is_not_a_conflict() {
        assert!(!transaction_conflicted(&cancelled(&["None", "None"])));
        assert!(!transaction_conflicted(&cancelled(&[
            "ConditionalCheckFailed"
        ])));
    }

    #[test]
    fn conflicted_error_is_still_a_conditional_failure_for_legacy_callers() {
        // Guards the ordering contract: the conflict-retry loop must run BEFORE
        // `conditional_failure` maps TransactionCanceledException to Fenced.
        assert!(conditional_failure(&cancelled(&["TransactionConflict"])));
    }

    #[test]
    fn transaction_tokens_are_stable_and_bounded() {
        let a = transaction_token(&[b"ses_x", &1u64.to_be_bytes()]);
        let b = transaction_token(&[b"ses_x", &1u64.to_be_bytes()]);
        let c = transaction_token(&[b"ses_x", &2u64.to_be_bytes()]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // DynamoDB ClientRequestToken allows at most 36 characters.
        assert_eq!(a.len(), 32);
    }
}
