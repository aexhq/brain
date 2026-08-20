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
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};
use brain::journal::{
    Entry, Head, HeadDoc, JournalStore, LEASE_MS, Record, STEAL_GRACE_MS, record_sk, session_pk,
};
use brain::{BrainError, Result};
use std::collections::HashMap;

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
}

#[async_trait::async_trait]
impl JournalStore for DynamoJournal {
    async fn create(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        owner: &str,
        now_ms: u64,
    ) -> Result<()> {
        let head = Put::builder()
            .table_name(&self.table)
            .item("pk", AttributeValue::S(session_pk(session_id)))
            .item("sk", AttributeValue::S("HEAD".into()))
            .item("owner_id", AttributeValue::S(owner.to_string()))
            .item("fence", AttributeValue::N("1".into()))
            .item(
                "lease_expires_ms",
                AttributeValue::N((now_ms + LEASE_MS).to_string()),
            )
            .item("last_seq", AttributeValue::N("1".into()))
            .item("doc", AttributeValue::S(serde_json::to_string(doc)?))
            .condition_expression("attribute_not_exists(sk)")
            .build()
            .map_err(|e| BrainError::Journal(format!("head put: {e}")))?;
        let rec = self.record_put(session_id, 1, now_ms, first)?;
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
        parse_head(session_id, attrs)
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
        parse_head(session_id, attrs)
    }

    async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>> {
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
        let mut tx = self.db.transact_write_items();
        for (seq, record) in records {
            let put = self.record_put(session_id, *seq, now_ms, record)?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());
        }
        let update = Update::builder()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(session_pk(session_id)))
            .key("sk", AttributeValue::S("HEAD".into()))
            .condition_expression("fence = :fence AND owner_id = :me")
            // Deliberately no `ADD fence`: renewing must not fence out the renewer.
            .update_expression("SET last_seq = :hw, doc = :doc, lease_expires_ms = :expires")
            .expression_attribute_values(":fence", AttributeValue::N(fence.to_string()))
            .expression_attribute_values(":me", AttributeValue::S(owner.to_string()))
            .expression_attribute_values(":hw", AttributeValue::N(high_water.to_string()))
            .expression_attribute_values(":doc", AttributeValue::S(serde_json::to_string(doc)?))
            .expression_attribute_values(
                ":expires",
                AttributeValue::N((now_ms + LEASE_MS).to_string()),
            )
            .build()
            .map_err(|e| BrainError::Journal(format!("head update: {e}")))?;
        tx = tx.transact_items(TransactWriteItem::builder().update(update).build());
        tx.send().await.map_err(|e| match conditional_failure(&e) {
            true => BrainError::Fenced,
            false => BrainError::Journal(format!("commit: {}", describe(&e))),
        })?;
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

    async fn purge(&self, session_id: &str) -> Result<u64> {
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

    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
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
