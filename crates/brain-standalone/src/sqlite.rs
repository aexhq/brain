//! SQLite implementation of Brain's journal contract.
//!
//! One Brain decision is one `BEGIN IMMEDIATE` transaction containing the head update and every
//! record in that decision. The `(session_id, seq)` primary key remains the idempotency barrier;
//! the owner/fence predicate remains the stale-writer barrier.

use async_trait::async_trait;
use brain::journal::{Entry, Head, HeadDoc, JournalStore, LEASE_MS, Record, STEAL_GRACE_MS};
use brain::{BrainError, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub struct SqliteStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let parent = path
            .parent()
            .ok_or_else(|| BrainError::Journal("SQLite path has no parent".into()))?;
        std::fs::create_dir_all(parent)
            .map_err(|error| BrainError::Journal(format!("create SQLite directory: {error}")))?;
        let connection =
            Connection::open(&path).map_err(|error| db_error("open SQLite journal", error))?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(|error| db_error("configure SQLite busy timeout", error))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=FULL;
                 PRAGMA foreign_keys=ON;
                 PRAGMA trusted_schema=OFF;
                 CREATE TABLE IF NOT EXISTS sessions (
                    session_id TEXT PRIMARY KEY NOT NULL,
                    doc_json TEXT NOT NULL,
                    fence INTEGER NOT NULL,
                    last_seq INTEGER NOT NULL,
                    owner TEXT,
                    lease_expires_ms INTEGER NOT NULL,
                    created_ms INTEGER NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS records (
                    session_id TEXT NOT NULL,
                    seq INTEGER NOT NULL,
                    ts_ms INTEGER NOT NULL,
                    record_json TEXT NOT NULL,
                    PRIMARY KEY (session_id, seq),
                    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS sessions_created
                    ON sessions(created_ms DESC, session_id ASC);
                 PRAGMA user_version=1;",
            )
            .map_err(|error| db_error("initialise SQLite journal", error))?;
        secure_file(&path)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| BrainError::Journal("SQLite connection lock was poisoned".into()))
    }
}

#[async_trait]
impl JournalStore for SqliteStore {
    async fn create(
        &self,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        owner: &str,
        now_ms: u64,
    ) -> Result<()> {
        let doc = encode(doc, "journal head")?;
        let first = encode(first, "journal record")?;
        let now = integer(now_ms, "timestamp")?;
        let lease = integer(now_ms.saturating_add(LEASE_MS), "lease expiry")?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal create", error))?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM sessions WHERE session_id=?1",
                [session_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|error| db_error("check journal session", error))?
            .is_some();
        if exists {
            return Err(BrainError::Invalid(format!(
                "session {session_id} already exists"
            )));
        }
        transaction
            .execute(
                "INSERT INTO sessions
                 (session_id, doc_json, fence, last_seq, owner, lease_expires_ms, created_ms)
                 VALUES (?1, ?2, 1, 1, ?3, ?4, ?5)",
                params![session_id, doc, owner, lease, now],
            )
            .map_err(|error| db_error("insert journal head", error))?;
        transaction
            .execute(
                "INSERT INTO records(session_id, seq, ts_ms, record_json)
                 VALUES (?1, 1, ?2, ?3)",
                params![session_id, now, first],
            )
            .map_err(|error| db_error("insert first journal record", error))?;
        transaction
            .commit()
            .map_err(|error| db_error("commit journal create", error))
    }

    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal claim", error))?;
        let row: Option<(String, u64, u64, Option<String>, u64)> = transaction
            .query_row(
                "SELECT doc_json, fence, last_seq, owner, lease_expires_ms
                 FROM sessions WHERE session_id=?1",
                [session_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| db_error("read journal claim", error))?;
        let Some((doc, fence, last_seq, current_owner, expires)) = row else {
            return Err(BrainError::NoSuchSession(session_id.into()));
        };
        let claimable = match current_owner.as_deref() {
            None => true,
            Some(current) if current == owner => true,
            Some(_) => expires < now_ms.saturating_sub(STEAL_GRACE_MS),
        };
        if !claimable {
            return Err(BrainError::Fenced);
        }
        let next_fence = fence
            .checked_add(1)
            .ok_or_else(|| BrainError::Journal("journal fence exhausted".into()))?;
        transaction
            .execute(
                "UPDATE sessions
                 SET owner=?2, lease_expires_ms=?3, fence=?4
                 WHERE session_id=?1 AND fence=?5",
                params![
                    session_id,
                    owner,
                    integer(now_ms.saturating_add(LEASE_MS), "lease expiry")?,
                    integer(next_fence, "fence")?,
                    integer(fence, "fence")?
                ],
            )
            .map_err(|error| db_error("update journal claim", error))?;
        transaction
            .commit()
            .map_err(|error| db_error("commit journal claim", error))?;
        Ok(Head {
            session_id: session_id.into(),
            doc: decode(&doc, "journal head")?,
            fence: next_fence,
            last_seq,
        })
    }

    async fn get_head(&self, session_id: &str) -> Result<Head> {
        let connection = self.connection()?;
        let row: Option<(String, u64, u64)> = connection
            .query_row(
                "SELECT doc_json, fence, last_seq FROM sessions WHERE session_id=?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| db_error("read journal head", error))?;
        let Some((doc, fence, last_seq)) = row else {
            return Err(BrainError::NoSuchSession(session_id.into()));
        };
        Ok(Head {
            session_id: session_id.into(),
            doc: decode(&doc, "journal head")?,
            fence,
            last_seq,
        })
    }

    async fn read_records(&self, session_id: &str, after: u64) -> Result<Vec<Entry>> {
        let connection = self.connection()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM sessions WHERE session_id=?1",
                [session_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|error| db_error("check journal session", error))?
            .is_some();
        if !exists {
            return Err(BrainError::NoSuchSession(session_id.into()));
        }
        let mut statement = connection
            .prepare(
                "SELECT seq, ts_ms, record_json FROM records
                 WHERE session_id=?1 AND seq>?2 ORDER BY seq ASC",
            )
            .map_err(|error| db_error("prepare journal read", error))?;
        let rows = statement
            .query_map(params![session_id, integer(after, "sequence")?], |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| db_error("query journal records", error))?;
        let mut entries = Vec::new();
        for row in rows {
            let (seq, ts_ms, record) =
                row.map_err(|error| db_error("decode journal row", error))?;
            entries.push(Entry {
                seq,
                ts_ms,
                record: decode(&record, "journal record")?,
            });
        }
        Ok(entries)
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
        let doc = encode(doc, "journal head")?;
        let encoded = records
            .iter()
            .map(|(seq, record)| Ok((*seq, encode(record, "journal record")?)))
            .collect::<Result<Vec<_>>>()?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal commit", error))?;
        let current: Option<(u64, Option<String>)> = transaction
            .query_row(
                "SELECT fence, owner FROM sessions WHERE session_id=?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| db_error("read journal ownership", error))?;
        let Some((current_fence, current_owner)) = current else {
            return Err(BrainError::NoSuchSession(session_id.into()));
        };
        if current_fence != fence || current_owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        for (seq, record) in encoded {
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO records(session_id, seq, ts_ms, record_json)
                 VALUES (?1, ?2, ?3, ?4)",
                    params![
                        session_id,
                        integer(seq, "sequence")?,
                        integer(now_ms, "timestamp")?,
                        record
                    ],
                )
                .map_err(|error| db_error("insert journal record", error))?;
            if inserted != 1 {
                return Err(BrainError::Fenced);
            }
        }
        let updated = transaction
            .execute(
                "UPDATE sessions
                 SET doc_json=?4, last_seq=?5, lease_expires_ms=?6
                 WHERE session_id=?1 AND owner=?2 AND fence=?3",
                params![
                    session_id,
                    owner,
                    integer(fence, "fence")?,
                    doc,
                    integer(high_water, "sequence")?,
                    integer(now_ms.saturating_add(LEASE_MS), "lease expiry")?
                ],
            )
            .map_err(|error| db_error("update journal head", error))?;
        if updated != 1 {
            return Err(BrainError::Fenced);
        }
        transaction
            .commit()
            .map_err(|error| db_error("commit journal decision", error))
    }

    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE sessions SET owner=NULL, lease_expires_ms=0
                 WHERE session_id=?1 AND owner=?2 AND fence=?3",
                params![session_id, owner, integer(fence, "fence")?],
            )
            .map_err(|error| db_error("release journal lease", error))?;
        Ok(())
    }

    async fn purge(&self, session_id: &str) -> Result<u64> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal purge", error))?;
        let count: Option<u64> = transaction
            .query_row(
                "SELECT (SELECT COUNT(*) FROM records WHERE session_id=?1) + 1
                 FROM sessions WHERE session_id=?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| db_error("count journal purge", error))?;
        transaction
            .execute("DELETE FROM sessions WHERE session_id=?1", [session_id])
            .map_err(|error| db_error("delete journal session", error))?;
        transaction
            .commit()
            .map_err(|error| db_error("commit journal purge", error))?;
        Ok(count.unwrap_or(0))
    }

    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, doc_json, fence, last_seq FROM sessions
                 ORDER BY created_ms DESC, session_id ASC LIMIT ?1",
            )
            .map_err(|error| db_error("prepare session list", error))?;
        let rows = statement
            .query_map([integer(limit as u64, "session list limit")?], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })
            .map_err(|error| db_error("query session list", error))?;
        let mut heads = Vec::new();
        for row in rows {
            let (session_id, doc, fence, last_seq) =
                row.map_err(|error| db_error("decode session list row", error))?;
            heads.push(Head {
                session_id,
                doc: decode(&doc, "journal head")?,
                fence,
                last_seq,
            });
        }
        Ok(heads)
    }
}

fn encode<T: serde::Serialize>(value: &T, what: &str) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|error| BrainError::Journal(format!("serialize {what}: {error}")))
}

fn decode<T: serde::de::DeserializeOwned>(value: &str, what: &str) -> Result<T> {
    serde_json::from_str(value)
        .map_err(|error| BrainError::Journal(format!("deserialize {what}: {error}")))
}

fn integer(value: u64, what: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| BrainError::Journal(format!("{what} exceeds SQLite integer range")))
}

fn db_error(context: &str, error: rusqlite::Error) -> BrainError {
    BrainError::Journal(format!("{context}: {error}"))
}

fn secure_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| BrainError::Journal(format!("secure SQLite journal: {error}")))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain::journal::{Journal, Lease, PrefixDoc};
    use brain::message::ContentBlock;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(brain::mint_id("brain-sqlite-test", 12));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn doc() -> HeadDoc {
        HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: 1,
            updated_ms: 1,
            create_key_hash: None,
            create_request_hash: None,
            last_message_ms: None,
            ended: false,
            prefix: PrefixDoc {
                system_prompt: Some("test".into()),
                provider: "openai".into(),
                model: "test".into(),
                base_url: None,
                max_output_tokens: Some(128),
                temperature: Some(0.0),
                reasoning_effort: None,
                tools: vec![],
                mcp: vec![],
                mcp_tools: vec![],
                hand_enabled: false,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                hand_env_keys: vec![],
                metadata: HashMap::new(),
            },
            key_b64: String::new(),
            mcp_secrets_b64: String::new(),
            hand_secrets_b64: String::new(),
            hand_manifest: brain::tools::hand_manifest(&[]).unwrap(),
            manifest_digest: String::new(),
            hand_info: HeadDoc::initial_hand_info("1gb"),
            workspace_bytes: 0,
            hand_state: serde_json::json!({"v":1}),
            artifacts: vec![],
        }
    }

    fn user(text: &str) -> Record {
        Record::UserMessage {
            turn: "trn_test".into(),
            content: vec![ContentBlock::text(text)],
            metadata: HashMap::new(),
            idempotency_key_hash: None,
            request_hash: None,
        }
    }

    #[tokio::test]
    async fn survives_reopen_and_preserves_fencing_and_idempotency() {
        let dir = TestDir::new();
        let path = dir.0.join("journal.sqlite3");
        let store = Arc::new(SqliteStore::open(&path).unwrap());
        let journal = Journal::new(store, "owner-a");
        journal
            .create("ses_test", &doc(), &user("one"))
            .await
            .unwrap();
        let head = journal.claim("ses_test").await.unwrap();
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
        };
        journal
            .commit("ses_test", &mut lease, &[(2, user("two"))], &doc(), 2)
            .await
            .unwrap();
        journal.release("ses_test", &lease).await.unwrap();
        drop(journal);

        let reopened = Journal::new(Arc::new(SqliteStore::open(&path).unwrap()), "owner-b");
        let head = reopened.claim("ses_test").await.unwrap();
        let mut lease = Lease {
            fence: head.fence,
            last_seq: head.last_seq,
        };
        assert_eq!(reopened.read_records("ses_test", 0).await.unwrap().len(), 2);
        let duplicate = reopened
            .commit("ses_test", &mut lease, &[(2, user("duplicate"))], &doc(), 2)
            .await;
        assert!(matches!(duplicate, Err(BrainError::Fenced)));
    }

    #[tokio::test]
    async fn active_owner_cannot_be_stolen_and_purge_is_exact() {
        let dir = TestDir::new();
        let store = Arc::new(SqliteStore::open(dir.0.join("journal.sqlite3")).unwrap());
        let a = Journal::new(store.clone(), "owner-a");
        let b = Journal::new(store, "owner-b");
        a.create("ses_test", &doc(), &user("one")).await.unwrap();
        assert!(matches!(b.claim("ses_test").await, Err(BrainError::Fenced)));
        assert_eq!(a.purge("ses_test").await.unwrap(), 2);
        assert!(matches!(
            a.get_head("ses_test").await,
            Err(BrainError::NoSuchSession(_))
        ));
    }
}
