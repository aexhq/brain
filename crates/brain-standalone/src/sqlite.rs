//! SQLite implementation of Brain's journal contract.
//!
//! One Brain decision is one `BEGIN IMMEDIATE` transaction containing the head update and every
//! record in that decision. The `(session_id, seq)` primary key remains the idempotency barrier;
//! the owner/fence predicate remains the stale-writer barrier.

use async_trait::async_trait;
use brain::journal::{
    ChildListQuery, ChildPage, ConfigDoc, ControlDoc, DeletionStatusDoc, EndFence, Entry, Head,
    HeadDoc, JournalRetention, JournalRetentionLimits, JournalStore, LEASE_MS, Record, RecordPage,
    RecordPageQuery, RecoveryItem, RecoveryPage, RecoveryQuery, STEAL_GRACE_MS,
    SandboxInventoryDoc, SandboxListQuery, SandboxPage, SandboxReserveRequest,
    SandboxUpdateRequest, SessionListQuery, SessionPage, SessionSummary, child_admission_open,
    initial_retention, project_end_fence, project_retention, recovery_due_key, recovery_shard,
    requires_ancestor_admission, retention_delta, session_id_from_list_cursor,
    tenant_session_sort_key, validate_ancestor_path, validate_config_doc,
    validate_record_page_query,
};
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
                    control_json TEXT NOT NULL,
                    config_json TEXT NOT NULL,
                    summary_json TEXT NOT NULL,
                    fence INTEGER NOT NULL,
                    last_seq INTEGER NOT NULL,
                    owner TEXT,
                    lease_expires_ms INTEGER NOT NULL,
                    created_ms INTEGER NOT NULL,
                    tenant_id TEXT NOT NULL,
                    state TEXT NOT NULL,
                    list_key TEXT NOT NULL,
                    direct_children INTEGER NOT NULL DEFAULT 0,
                    descendants INTEGER NOT NULL DEFAULT 0,
                    live_sandboxes INTEGER NOT NULL DEFAULT 0,
                    journal_metered_bytes INTEGER NOT NULL DEFAULT 0,
                    journal_effect_reserve_bytes INTEGER NOT NULL DEFAULT 0,
                    journal_lifecycle_reserve_bytes INTEGER NOT NULL DEFAULT 0
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS records (
                    session_id TEXT NOT NULL,
                    seq INTEGER NOT NULL,
                    ts_ms INTEGER NOT NULL,
                    record_json TEXT NOT NULL,
                    PRIMARY KEY (session_id, seq),
                    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS child_links (
                    parent_id TEXT NOT NULL,
                    child_id TEXT NOT NULL,
                    summary_json TEXT NOT NULL,
                    PRIMARY KEY (parent_id, child_id),
                    FOREIGN KEY (parent_id) REFERENCES sessions(session_id) ON DELETE CASCADE
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS deletion_jobs (
                    session_id TEXT PRIMARY KEY NOT NULL,
                    state TEXT NOT NULL,
                    status_json TEXT NOT NULL,
                    expires_at_ms INTEGER NOT NULL
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS tenant_storage (
                    tenant_id TEXT PRIMARY KEY NOT NULL,
                    total_bytes INTEGER NOT NULL CHECK(total_bytes >= 0)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS tenant_retention (
                    tenant_id TEXT PRIMARY KEY NOT NULL,
                    total_bytes INTEGER NOT NULL CHECK(total_bytes >= 0),
                    session_count INTEGER NOT NULL CHECK(session_count >= 0)
                 ) STRICT;
                 CREATE TABLE IF NOT EXISTS sandbox_inventory (
                    root_id TEXT NOT NULL,
                    sandbox_id TEXT NOT NULL,
                    version INTEGER NOT NULL,
                    doc_json TEXT NOT NULL,
                    PRIMARY KEY (root_id, sandbox_id),
                    FOREIGN KEY (root_id) REFERENCES sessions(session_id) ON DELETE CASCADE
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS sessions_created
                    ON sessions(created_ms DESC, session_id ASC);
                 CREATE INDEX IF NOT EXISTS sessions_tenant_list
                    ON sessions(tenant_id, list_key);
                 CREATE INDEX IF NOT EXISTS sessions_tenant_state_list
                    ON sessions(tenant_id, state, list_key);
                 PRAGMA user_version=1;",
            )
            .map_err(|error| db_error("initialise SQLite journal", error))?;
        ensure_session_column(&connection, "direct_children", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_session_column(&connection, "descendants", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_session_column(&connection, "live_sandboxes", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_session_column(
            &connection,
            "journal_metered_bytes",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_session_column(
            &connection,
            "journal_effect_reserve_bytes",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_session_column(
            &connection,
            "journal_lifecycle_reserve_bytes",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
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
        _owner: &str,
        now_ms: u64,
        tenant_storage_limit: u64,
        retention: JournalRetention,
        retention_limits: JournalRetentionLimits,
    ) -> Result<()> {
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
        let doc_value = doc;
        let (control, config) = doc.split();
        let control = encode(&control, "journal control")?;
        let config = encode(&config, "journal config")?;
        let summary = encode(
            &SessionSummary::from_head(session_id, doc_value),
            "session summary",
        )?;
        let first = encode(first, "journal record")?;
        let now = integer(now_ms, "timestamp")?;
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
        if doc_value.tenant_metered_storage_bytes != 0 {
            let used = transaction
                .query_row(
                    "SELECT total_bytes FROM tenant_storage WHERE tenant_id=?1",
                    [&doc_value.tenant_id],
                    |row| row.get::<_, u64>(0),
                )
                .optional()
                .map_err(|error| db_error("read create tenant storage meter", error))?
                .unwrap_or(0);
            let next = used
                .checked_add(doc_value.tenant_metered_storage_bytes)
                .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
            if next > tenant_storage_limit {
                return Err(BrainError::TenantStorageQuotaExceeded {
                    requested: doc_value.tenant_metered_storage_bytes,
                    limit: tenant_storage_limit,
                });
            }
            transaction
                .execute(
                    "INSERT INTO tenant_storage(tenant_id,total_bytes) VALUES (?1,?2)
                     ON CONFLICT(tenant_id) DO UPDATE SET total_bytes=excluded.total_bytes",
                    params![
                        doc_value.tenant_id,
                        integer(next, "create tenant storage meter")?
                    ],
                )
                .map_err(|error| db_error("reserve create tenant storage meter", error))?;
        }
        let (retained_bytes, retained_sessions) = transaction
            .query_row(
                "SELECT total_bytes,session_count FROM tenant_retention WHERE tenant_id=?1",
                [&doc_value.tenant_id],
                |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
            )
            .optional()
            .map_err(|error| db_error("read create tenant retention meter", error))?
            .unwrap_or((0, 0));
        let next_retained_bytes = retained_bytes
            .checked_add(retention.metered_bytes)
            .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
        if next_retained_bytes > retention_limits.tenant_bytes {
            return Err(BrainError::TenantJournalQuotaExceeded {
                requested: retention.metered_bytes,
                limit: retention_limits.tenant_bytes,
            });
        }
        let next_retained_sessions = retained_sessions.checked_add(1).ok_or_else(|| {
            BrainError::Journal("tenant retained-session meter overflowed".into())
        })?;
        if next_retained_sessions > retention_limits.tenant_sessions {
            return Err(BrainError::TenantRetainedSessionQuotaExceeded {
                limit: retention_limits.tenant_sessions,
            });
        }
        transaction
            .execute(
                "INSERT INTO tenant_retention(tenant_id,total_bytes,session_count) VALUES (?1,?2,?3)
                 ON CONFLICT(tenant_id) DO UPDATE SET
                 total_bytes=excluded.total_bytes,session_count=excluded.session_count",
                params![
                    doc_value.tenant_id,
                    integer(next_retained_bytes, "create tenant journal meter")?,
                    integer(next_retained_sessions, "create tenant retained-session meter")?,
                ],
            )
            .map_err(|error| db_error("reserve create tenant retention meter", error))?;
        if let Some(parent_id) = &doc_value.parent_id {
            let read_admission = |session: &str| -> Result<(HeadDoc, u32, u32)> {
                let row: Option<(String, String, u32, u32)> = transaction
                    .query_row(
                        "SELECT control_json,config_json,direct_children,descendants
                         FROM sessions WHERE session_id=?1",
                        [session],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()
                    .map_err(|error| db_error("read child admission anchor", error))?;
                let Some((control, config, direct, descendants)) = row else {
                    return Err(BrainError::NoSuchSession(session.into()));
                };
                Ok((decode_head(&control, &config)?, direct, descendants))
            };
            let (parent, direct_children, parent_descendants) = read_admission(parent_id)?;
            let mut expected_ancestors = parent.ancestor_ids.clone();
            expected_ancestors.push(parent_id.clone());
            if doc_value.ancestor_ids != expected_ancestors {
                return Err(BrainError::Invalid(
                    "child ancestor path does not extend its direct parent".into(),
                ));
            }
            for ancestor_id in &doc_value.ancestor_ids {
                let (ancestor, _, _) = read_admission(ancestor_id)?;
                if ancestor.root_id != doc_value.root_id || !child_admission_open(&ancestor) {
                    return Err(BrainError::Invalid(
                        "child admission is closed by an ancestor fence".into(),
                    ));
                }
            }
            let (root, _, descendants) = if parent_id == &doc_value.root_id {
                (parent.clone(), direct_children, parent_descendants)
            } else {
                read_admission(&doc_value.root_id)?
            };
            if !child_admission_open(&parent)
                || !child_admission_open(&root)
                || parent.root_id != doc_value.root_id
                || doc_value.depth != parent.depth.saturating_add(1)
                || parent.depth >= root.prefix.max_child_depth
            {
                return Err(BrainError::Invalid(
                    "child admission is closed or its rooted scope is stale".into(),
                ));
            }
            if direct_children >= root.prefix.max_direct_children
                || descendants >= root.prefix.max_descendants
            {
                return Err(BrainError::Overloaded);
            }
            if parent_id == &doc_value.root_id {
                transaction
                    .execute(
                        "UPDATE sessions SET direct_children=direct_children+1,
                         descendants=descendants+1 WHERE session_id=?1",
                        [parent_id],
                    )
                    .map_err(|error| db_error("reserve root child admission", error))?;
            } else {
                transaction
                    .execute(
                        "UPDATE sessions SET direct_children=direct_children+1
                         WHERE session_id=?1",
                        [parent_id],
                    )
                    .map_err(|error| db_error("reserve direct child admission", error))?;
                transaction
                    .execute(
                        "UPDATE sessions SET descendants=descendants+1 WHERE session_id=?1",
                        [&doc_value.root_id],
                    )
                    .map_err(|error| db_error("reserve root descendant admission", error))?;
            }
        }
        transaction
            .execute(
                "INSERT INTO sessions
                 (session_id, control_json, config_json, summary_json, fence, last_seq, owner,
                  lease_expires_ms, created_ms, tenant_id, state, list_key,
                  journal_metered_bytes,journal_effect_reserve_bytes,
                  journal_lifecycle_reserve_bytes)
                 VALUES (?1, ?2, ?3, ?4, 0, 1, NULL, 0, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session_id,
                    control,
                    config,
                    summary,
                    now,
                    doc_value.tenant_id,
                    doc_value.state,
                    tenant_session_sort_key(doc_value.updated_ms, session_id),
                    integer(retention.metered_bytes, "session journal meter")?,
                    integer(retention.effect_reserve_bytes, "session effect reserve")?,
                    integer(
                        retention.lifecycle_reserve_bytes,
                        "session lifecycle reserve"
                    )?,
                ],
            )
            .map_err(|error| db_error("insert journal head", error))?;
        transaction
            .execute(
                "INSERT INTO records(session_id, seq, ts_ms, record_json)
                 VALUES (?1, 1, ?2, ?3)",
                params![session_id, now, first],
            )
            .map_err(|error| db_error("insert first journal record", error))?;
        if let Some(parent_id) = &doc_value.parent_id {
            transaction
                .execute(
                    "INSERT INTO child_links(parent_id, child_id, summary_json)
                     VALUES (?1, ?2, ?3)",
                    params![parent_id, session_id, summary],
                )
                .map_err(|error| db_error("insert direct-child adjacency", error))?;
        }
        transaction
            .commit()
            .map_err(|error| db_error("commit journal create", error))
    }

    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal claim", error))?;
        let row: Option<(String, String, u64, u64, Option<String>, u64, u64, u64, u64)> =
            transaction
                .query_row(
                    "SELECT control_json, config_json, fence, last_seq, owner, lease_expires_ms,
                 journal_metered_bytes,journal_effect_reserve_bytes,
                 journal_lifecycle_reserve_bytes
                 FROM sessions WHERE session_id=?1",
                    [session_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                            row.get(8)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| db_error("read journal claim", error))?;
        let Some((
            control,
            config,
            fence,
            last_seq,
            current_owner,
            expires,
            journal_metered_bytes,
            journal_effect_reserve_bytes,
            journal_lifecycle_reserve_bytes,
        )) = row
        else {
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
            doc: decode_head(&control, &config)?,
            fence: next_fence,
            last_seq,
            retention: JournalRetention {
                metered_bytes: journal_metered_bytes,
                effect_reserve_bytes: journal_effect_reserve_bytes,
                lifecycle_reserve_bytes: journal_lifecycle_reserve_bytes,
            },
        })
    }

    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<EndFence> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin end fence", error))?;
        let row: Option<(String, String, u64, u64, u64, u64, u64)> = transaction
            .query_row(
                "SELECT control_json, config_json, fence, last_seq,
                 journal_metered_bytes,journal_effect_reserve_bytes,
                 journal_lifecycle_reserve_bytes
                 FROM sessions WHERE session_id=?1",
                [session_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| db_error("read end fence head", error))?;
        let Some((
            control,
            config,
            fence,
            last_seq,
            journal_metered_bytes,
            journal_effect_reserve_bytes,
            journal_lifecycle_reserve_bytes,
        )) = row
        else {
            return Err(BrainError::NoSuchSession(session_id.into()));
        };
        let head = Head {
            session_id: session_id.into(),
            doc: decode_head(&control, &config)?,
            fence,
            last_seq,
            retention: JournalRetention {
                metered_bytes: journal_metered_bytes,
                effect_reserve_bytes: journal_effect_reserve_bytes,
                lifecycle_reserve_bytes: journal_lifecycle_reserve_bytes,
            },
        };
        let Some((doc, sequence, record)) = project_end_fence(&head, now_ms)? else {
            return Ok(EndFence {
                head,
                newly_fenced: false,
            });
        };
        let next_fence = fence
            .checked_add(1)
            .ok_or_else(|| BrainError::Journal("journal fence exhausted".into()))?;
        let control = encode(&doc.control_doc(), "end fence control")?;
        let summary = encode(
            &SessionSummary::from_head(session_id, &doc),
            "end fence summary",
        )?;
        let record_json = encode(&record, "end fence record")?;
        let next_retention = project_retention(
            head.retention,
            &[(sequence, record.clone())],
            retention_limits.session_bytes,
        )?;
        let delta = retention_delta(head.retention, next_retention)?;
        apply_tenant_retention_delta(
            &transaction,
            &doc.tenant_id,
            delta,
            retention_limits.tenant_bytes,
            "end fence",
        )?;
        let changed = transaction
            .execute(
                "UPDATE sessions
                 SET control_json=?2, summary_json=?3, fence=?4, last_seq=?5,
                     owner=NULL, lease_expires_ms=0, state=?6, list_key=?7,
                     journal_metered_bytes=?8,journal_effect_reserve_bytes=?9,
                     journal_lifecycle_reserve_bytes=?10
                 WHERE session_id=?1 AND fence=?11 AND last_seq=?12",
                params![
                    session_id,
                    control,
                    summary,
                    integer(next_fence, "fence")?,
                    integer(sequence, "sequence")?,
                    doc.state,
                    tenant_session_sort_key(doc.updated_ms, session_id),
                    integer(next_retention.metered_bytes, "end journal meter")?,
                    integer(next_retention.effect_reserve_bytes, "end effect reserve")?,
                    integer(
                        next_retention.lifecycle_reserve_bytes,
                        "end lifecycle reserve"
                    )?,
                    integer(fence, "fence")?,
                    integer(last_seq, "sequence")?
                ],
            )
            .map_err(|error| db_error("update end fence", error))?;
        if changed != 1 {
            return Err(BrainError::Fenced);
        }
        transaction
            .execute(
                "INSERT INTO records(session_id,seq,ts_ms,record_json) VALUES (?1,?2,?3,?4)",
                params![
                    session_id,
                    integer(sequence, "sequence")?,
                    integer(now_ms, "timestamp")?,
                    record_json
                ],
            )
            .map_err(|error| db_error("insert end fence record", error))?;
        if let Some(parent_id) = &doc.parent_id {
            transaction
                .execute(
                    "UPDATE child_links SET summary_json=?3
                     WHERE parent_id=?1 AND child_id=?2",
                    params![
                        parent_id,
                        session_id,
                        encode(
                            &SessionSummary::from_head(session_id, &doc),
                            "end fence child summary"
                        )?
                    ],
                )
                .map_err(|error| db_error("update end fence child link", error))?;
        }
        transaction
            .commit()
            .map_err(|error| db_error("commit end fence", error))?;
        Ok(EndFence {
            head: Head {
                session_id: session_id.into(),
                doc,
                fence: next_fence,
                last_seq: sequence,
                retention: next_retention,
            },
            newly_fenced: true,
        })
    }

    async fn get_head(&self, session_id: &str) -> Result<Head> {
        let connection = self.connection()?;
        let row: Option<(String, String, u64, u64, u64, u64, u64)> = connection
            .query_row(
                "SELECT control_json, config_json, fence, last_seq,
                 journal_metered_bytes,journal_effect_reserve_bytes,
                 journal_lifecycle_reserve_bytes FROM sessions WHERE session_id=?1",
                [session_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| db_error("read journal head", error))?;
        let Some((control, config, fence, last_seq, metered, effect, lifecycle)) = row else {
            return Err(BrainError::NoSuchSession(session_id.into()));
        };
        Ok(Head {
            session_id: session_id.into(),
            doc: decode_head(&control, &config)?,
            fence,
            last_seq,
            retention: JournalRetention {
                metered_bytes: metered,
                effect_reserve_bytes: effect,
                lifecycle_reserve_bytes: lifecycle,
            },
        })
    }

    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> Result<RecordPage> {
        let (limit, max_bytes) = validate_record_page_query(query)?;
        let connection = self.connection()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM sessions WHERE session_id=?1",
                [query.session_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|error| db_error("check journal session", error))?
            .is_some();
        if !exists {
            return Err(BrainError::NoSuchSession(query.session_id.into()));
        }
        if query.after >= query.through_seq {
            return Ok(RecordPage {
                entries: Vec::new(),
                next_after: None,
            });
        }
        let mut statement = connection
            .prepare(
                "SELECT seq, ts_ms, record_json FROM records
                 WHERE session_id=?1 AND seq>?2 AND seq<=?3 ORDER BY seq ASC LIMIT ?4",
            )
            .map_err(|error| db_error("prepare journal read", error))?;
        let rows = statement
            .query_map(
                params![
                    query.session_id,
                    integer(query.after, "sequence")?,
                    integer(query.through_seq, "sequence")?,
                    integer((limit + 1) as u64, "record page limit")?,
                ],
                |row| {
                    Ok((
                        row.get::<_, u64>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|error| db_error("query journal records", error))?;
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        let mut more = false;
        for row in rows {
            let (seq, ts_ms, record) =
                row.map_err(|error| db_error("decode journal row", error))?;
            if entries.len() >= limit || bytes.saturating_add(record.len()) > max_bytes {
                more = true;
                break;
            }
            bytes = bytes.saturating_add(record.len());
            entries.push(Entry {
                seq,
                ts_ms,
                record: decode(&record, "journal record")?,
            });
        }
        let next_after = more.then(|| entries.last().expect("page limit admits one record").seq);
        Ok(RecordPage {
            entries,
            next_after,
        })
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
        tenant_storage_delta: i64,
        tenant_storage_limit: u64,
        retention: JournalRetention,
        tenant_retention_delta: i64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<()> {
        let doc_value = doc;
        let control = doc.control_doc();
        let control = encode(&control, "journal control")?;
        let summary = encode(
            &SessionSummary::from_head(session_id, doc_value),
            "session summary",
        )?;
        let encoded = records
            .iter()
            .map(|(seq, record)| Ok((*seq, encode(record, "journal record")?)))
            .collect::<Result<Vec<_>>>()?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal commit", error))?;
        let current: Option<(u64, Option<String>, u64, u64, u64)> = transaction
            .query_row(
                "SELECT fence, owner, journal_metered_bytes,journal_effect_reserve_bytes,
                 journal_lifecycle_reserve_bytes FROM sessions WHERE session_id=?1",
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
            .map_err(|error| db_error("read journal ownership", error))?;
        let Some((current_fence, current_owner, metered, effect, lifecycle)) = current else {
            return Err(BrainError::NoSuchSession(session_id.into()));
        };
        if current_fence != fence || current_owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        let current_retention = JournalRetention {
            metered_bytes: metered,
            effect_reserve_bytes: effect,
            lifecycle_reserve_bytes: lifecycle,
        };
        if retention
            != project_retention(current_retention, records, retention_limits.session_bytes)?
            || tenant_retention_delta != retention_delta(current_retention, retention)?
        {
            return Err(BrainError::Journal(
                "journal retention transition does not match the canonical charge".into(),
            ));
        }
        if tenant_storage_delta != 0 {
            let used = transaction
                .query_row(
                    "SELECT total_bytes FROM tenant_storage WHERE tenant_id=?1",
                    [&doc_value.tenant_id],
                    |row| row.get::<_, u64>(0),
                )
                .optional()
                .map_err(|error| db_error("read tenant storage meter", error))?
                .unwrap_or(0);
            let next = if tenant_storage_delta > 0 {
                let requested = tenant_storage_delta as u64;
                let next = used
                    .checked_add(requested)
                    .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
                if next > tenant_storage_limit {
                    return Err(BrainError::TenantStorageQuotaExceeded {
                        requested,
                        limit: tenant_storage_limit,
                    });
                }
                next
            } else {
                used.checked_sub(tenant_storage_delta.unsigned_abs())
                    .ok_or_else(|| {
                        BrainError::Journal("tenant storage meter would become negative".into())
                    })?
            };
            transaction
                .execute(
                    "INSERT INTO tenant_storage(tenant_id,total_bytes) VALUES (?1,?2)
                     ON CONFLICT(tenant_id) DO UPDATE SET total_bytes=excluded.total_bytes",
                    params![doc_value.tenant_id, integer(next, "tenant storage meter")?],
                )
                .map_err(|error| db_error("update tenant storage meter", error))?;
        }
        apply_tenant_retention_delta(
            &transaction,
            &doc_value.tenant_id,
            tenant_retention_delta,
            retention_limits.tenant_bytes,
            "journal commit",
        )?;
        if requires_ancestor_admission(records) {
            for ancestor_id in &doc_value.ancestor_ids {
                let row: Option<(String, String)> = transaction
                    .query_row(
                        "SELECT control_json, config_json FROM sessions WHERE session_id=?1",
                        [ancestor_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
                    .map_err(|error| db_error("read ancestor admission fence", error))?;
                let Some((control, config)) = row else {
                    return Err(BrainError::NoSuchSession(ancestor_id.clone()));
                };
                let ancestor = decode_head(&control, &config)?;
                if ancestor.root_id != doc_value.root_id || !child_admission_open(&ancestor) {
                    return Err(BrainError::Fenced);
                }
            }
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
                 SET control_json=?4, summary_json=?5, last_seq=?6, lease_expires_ms=?7,
                     state=?8, list_key=?9, journal_metered_bytes=?10,
                     journal_effect_reserve_bytes=?11,journal_lifecycle_reserve_bytes=?12
                 WHERE session_id=?1 AND owner=?2 AND fence=?3",
                params![
                    session_id,
                    owner,
                    integer(fence, "fence")?,
                    control,
                    summary,
                    integer(high_water, "sequence")?,
                    integer(now_ms.saturating_add(LEASE_MS), "lease expiry")?,
                    doc_value.state,
                    tenant_session_sort_key(doc_value.updated_ms, session_id),
                    integer(retention.metered_bytes, "session journal meter")?,
                    integer(retention.effect_reserve_bytes, "session effect reserve")?,
                    integer(
                        retention.lifecycle_reserve_bytes,
                        "session lifecycle reserve"
                    )?,
                ],
            )
            .map_err(|error| db_error("update journal head", error))?;
        if updated != 1 {
            return Err(BrainError::Fenced);
        }
        if let Some(parent_id) = &doc_value.parent_id {
            transaction
                .execute(
                    "UPDATE child_links SET summary_json=?3
                     WHERE parent_id=?1 AND child_id=?2",
                    params![parent_id, session_id, summary],
                )
                .map_err(|error| db_error("update direct-child adjacency", error))?;
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

    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> Result<()> {
        let connection = self.connection()?;
        let mut control = doc.control_doc();
        control.recovery_due_ms = Some(due_ms);
        let updated = connection
            .execute(
                "UPDATE sessions SET control_json=?4, owner=NULL, lease_expires_ms=0
                 WHERE session_id=?1 AND owner=?2 AND fence=?3",
                params![
                    session_id,
                    owner,
                    integer(fence, "fence")?,
                    encode(&control, "journal control")?
                ],
            )
            .map_err(|error| db_error("release and schedule recovery", error))?;
        if updated != 1 {
            return Err(BrainError::Fenced);
        }
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
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin lease heartbeat", error))?;
        let updated = if let Some(recovery_due_ms) = recovery_due_ms {
            let control_json: Option<String> = transaction
                .query_row(
                    "SELECT control_json FROM sessions
                     WHERE session_id=?1 AND owner=?2 AND fence=?3",
                    params![session_id, owner, integer(fence, "fence")?],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| db_error("read lease heartbeat", error))?;
            let Some(control_json) = control_json else {
                return Err(BrainError::Fenced);
            };
            let mut control: ControlDoc = decode(&control_json, "journal control")?;
            control.recovery_due_ms = Some(recovery_due_ms);
            transaction
                .execute(
                    "UPDATE sessions SET control_json=?4, lease_expires_ms=?5
                     WHERE session_id=?1 AND owner=?2 AND fence=?3",
                    params![
                        session_id,
                        owner,
                        integer(fence, "fence")?,
                        encode(&control, "journal control")?,
                        integer(now_ms.saturating_add(LEASE_MS), "lease expiry")?
                    ],
                )
                .map_err(|error| db_error("renew journal lease", error))?
        } else {
            transaction
                .execute(
                    "UPDATE sessions SET lease_expires_ms=?4
                     WHERE session_id=?1 AND owner=?2 AND fence=?3",
                    params![
                        session_id,
                        owner,
                        integer(fence, "fence")?,
                        integer(now_ms.saturating_add(LEASE_MS), "lease expiry")?
                    ],
                )
                .map_err(|error| db_error("renew journal lease", error))?
        };
        if updated != 1 {
            return Err(BrainError::Fenced);
        }
        transaction
            .commit()
            .map_err(|error| db_error("commit lease heartbeat", error))
    }

    async fn purge_history(&self, session_id: &str) -> Result<u64> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin journal history purge", error))?;
        let removed = transaction
            .execute("DELETE FROM records WHERE session_id=?1", [session_id])
            .map_err(|error| db_error("delete journal history", error))?;
        let sandboxes = transaction
            .execute(
                "DELETE FROM sandbox_inventory WHERE root_id=?1",
                [session_id],
            )
            .map_err(|error| db_error("delete sandbox inventory", error))?;
        transaction
            .commit()
            .map_err(|error| db_error("commit journal history purge", error))?;
        Ok((removed + sandboxes) as u64)
    }

    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO deletion_jobs(session_id,state,status_json,expires_at_ms)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(session_id) DO UPDATE SET
                    state=excluded.state,
                    status_json=excluded.status_json,
                    expires_at_ms=excluded.expires_at_ms
                 WHERE deletion_jobs.state <> 'succeeded' OR excluded.state = 'succeeded'",
                params![
                    status.session_id,
                    status.state,
                    encode(status, "deletion status")?,
                    integer(status.expires_at_ms, "deletion expiry")?,
                ],
            )
            .map_err(|error| db_error("put deletion status", error))?;
        Ok(())
    }

    async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>> {
        let connection = self.connection()?;
        let encoded: Option<String> = connection
            .query_row(
                "SELECT status_json FROM deletion_jobs WHERE session_id=?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| db_error("get deletion status", error))?;
        encoded
            .map(|value| decode(&value, "deletion status"))
            .transpose()
    }

    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin final deletion", error))?;
        let already_succeeded = transaction
            .query_row(
                "SELECT state FROM deletion_jobs WHERE session_id=?1",
                [&status.session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| db_error("check deletion tombstone", error))?
            .is_some_and(|state| state == "succeeded");
        if already_succeeded {
            transaction
                .commit()
                .map_err(|error| db_error("finish idempotent final deletion", error))?;
            return Ok(());
        }
        let retention_anchor: Option<(String, u64)> = transaction
            .query_row(
                "SELECT tenant_id,journal_metered_bytes FROM sessions WHERE session_id=?1",
                [&status.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| db_error("read final retention anchor", error))?;
        let Some((anchor_tenant, anchor_bytes)) = retention_anchor else {
            return Err(BrainError::Journal(
                "deletion lost its retained-session anchor before final release".into(),
            ));
        };
        if anchor_tenant != status.tenant_id || anchor_bytes != status.metered_journal_bytes {
            return Err(BrainError::Journal(
                "deletion status tenant retention anchor does not match HEAD".into(),
            ));
        }
        if status.metered_storage_bytes > 0 {
            let used = transaction
                .query_row(
                    "SELECT total_bytes FROM tenant_storage WHERE tenant_id=?1",
                    [&status.tenant_id],
                    |row| row.get::<_, u64>(0),
                )
                .optional()
                .map_err(|error| db_error("read final tenant storage meter", error))?
                .unwrap_or(0);
            let next = used
                .checked_sub(status.metered_storage_bytes)
                .ok_or_else(|| {
                    BrainError::Journal("tenant storage meter would become negative".into())
                })?;
            transaction
                .execute(
                    "INSERT INTO tenant_storage(tenant_id,total_bytes) VALUES (?1,?2)
                     ON CONFLICT(tenant_id) DO UPDATE SET total_bytes=excluded.total_bytes",
                    params![status.tenant_id, integer(next, "tenant storage meter")?],
                )
                .map_err(|error| db_error("release final tenant storage meter", error))?;
        }
        let (retained_bytes, retained_sessions) = transaction
            .query_row(
                "SELECT total_bytes,session_count FROM tenant_retention WHERE tenant_id=?1",
                [&status.tenant_id],
                |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
            )
            .optional()
            .map_err(|error| db_error("read final tenant retention meter", error))?
            .unwrap_or((0, 0));
        let next_retained_bytes = retained_bytes
            .checked_sub(status.metered_journal_bytes)
            .ok_or_else(|| {
                BrainError::Journal("tenant journal meter would become negative".into())
            })?;
        let next_retained_sessions = retained_sessions.checked_sub(1).ok_or_else(|| {
            BrainError::Journal("tenant retained-session meter would become negative".into())
        })?;
        transaction
            .execute(
                "INSERT INTO tenant_retention(tenant_id,total_bytes,session_count) VALUES (?1,?2,?3)
                 ON CONFLICT(tenant_id) DO UPDATE SET
                 total_bytes=excluded.total_bytes,session_count=excluded.session_count",
                params![
                    status.tenant_id,
                    integer(next_retained_bytes, "tenant journal meter")?,
                    integer(next_retained_sessions, "tenant retained-session meter")?,
                ],
            )
            .map_err(|error| db_error("release final tenant retention meter", error))?;
        transaction
            .execute(
                "INSERT INTO deletion_jobs(session_id,state,status_json,expires_at_ms)
                 VALUES (?1,'succeeded',?2,?3)
                 ON CONFLICT(session_id) DO UPDATE SET
                    state='succeeded',status_json=excluded.status_json,expires_at_ms=excluded.expires_at_ms",
                params![
                    status.session_id,
                    encode(status, "deletion tombstone")?,
                    integer(status.expires_at_ms, "deletion expiry")?,
                ],
            )
            .map_err(|error| db_error("install deletion tombstone", error))?;
        transaction
            .execute(
                "DELETE FROM child_links WHERE parent_id=?1 AND child_id=?2",
                params![status.parent_id, status.session_id],
            )
            .map_err(|error| db_error("delete parent child adjacency", error))?;
        if let Some(parent_id) = &status.parent_id {
            if parent_id == &status.root_id {
                transaction
                    .execute(
                        "UPDATE sessions SET
                         direct_children=MAX(direct_children-1,0),
                         descendants=MAX(descendants-1,0)
                         WHERE session_id=?1",
                        [parent_id],
                    )
                    .map_err(|error| db_error("release root child admission", error))?;
            } else {
                transaction
                    .execute(
                        "UPDATE sessions SET direct_children=MAX(direct_children-1,0)
                         WHERE session_id=?1",
                        [parent_id],
                    )
                    .map_err(|error| db_error("release direct child admission", error))?;
                transaction
                    .execute(
                        "UPDATE sessions SET descendants=MAX(descendants-1,0)
                         WHERE session_id=?1",
                        [&status.root_id],
                    )
                    .map_err(|error| db_error("release root descendant admission", error))?;
            }
        }
        transaction
            .execute(
                "DELETE FROM sessions WHERE session_id=?1",
                [&status.session_id],
            )
            .map_err(|error| db_error("delete final session anchor", error))?;
        transaction
            .commit()
            .map_err(|error| db_error("commit final deletion", error))
    }

    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> Result<SessionPage> {
        if let Some(cursor) = query.cursor {
            session_id_from_list_cursor(cursor)?;
        }
        let connection = self.connection()?;
        let sql = match (query.state, query.cursor) {
            (Some(_), Some(_)) => {
                "SELECT summary_json FROM sessions
                 WHERE tenant_id=?1 AND state=?2 AND list_key>?3
                 ORDER BY list_key ASC LIMIT ?4"
            }
            (Some(_), None) => {
                "SELECT summary_json FROM sessions
                 WHERE tenant_id=?1 AND state=?2
                 ORDER BY list_key ASC LIMIT ?4"
            }
            (None, Some(_)) => {
                "SELECT summary_json FROM sessions
                 WHERE tenant_id=?1 AND list_key>?3
                 ORDER BY list_key ASC LIMIT ?4"
            }
            (None, None) => {
                "SELECT summary_json FROM sessions
                 WHERE tenant_id=?1
                 ORDER BY list_key ASC LIMIT ?4"
            }
        };
        let state = query.state.unwrap_or("");
        let cursor = query.cursor.unwrap_or("");
        let fetch = query.limit.saturating_add(1) as u64;
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| db_error("prepare tenant session list", error))?;
        let rows = statement
            .query_map(
                params![
                    query.tenant_id,
                    state,
                    cursor,
                    integer(fetch, "session list limit")?
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| db_error("query tenant session list", error))?;
        let mut sessions = Vec::new();
        for row in rows {
            let summary = row.map_err(|error| db_error("decode tenant session list row", error))?;
            sessions.push(decode(&summary, "session summary")?);
        }
        let has_more = sessions.len() > query.limit;
        sessions.truncate(query.limit);
        let next_cursor = has_more.then(|| {
            let last: &SessionSummary =
                sessions.last().expect("a page with more rows is non-empty");
            tenant_session_sort_key(last.updated_ms, &last.session_id)
        });
        Ok(SessionPage {
            sessions,
            next_cursor,
        })
    }

    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> Result<ChildPage> {
        let connection = self.connection()?;
        let limit = query.limit.clamp(1, 100);
        let mut rows = if let Some(cursor) = query.cursor {
            let mut statement = connection
                .prepare(
                    "SELECT summary_json FROM child_links
                     WHERE parent_id=?1 AND child_id>?2
                     ORDER BY child_id ASC LIMIT ?3",
                )
                .map_err(|error| db_error("prepare child list", error))?;
            statement
                .query_map(
                    params![
                        query.parent_id,
                        cursor,
                        integer((limit + 1) as u64, "limit")?
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| db_error("query child list", error))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| db_error("read child list", error))?
        } else {
            let mut statement = connection
                .prepare(
                    "SELECT summary_json FROM child_links
                     WHERE parent_id=?1 ORDER BY child_id ASC LIMIT ?2",
                )
                .map_err(|error| db_error("prepare child list", error))?;
            statement
                .query_map(
                    params![query.parent_id, integer((limit + 1) as u64, "limit")?],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| db_error("query child list", error))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| db_error("read child list", error))?
        }
        .into_iter()
        .map(|json| decode::<SessionSummary>(&json, "child summary"))
        .collect::<Result<Vec<_>>>()?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = has_more.then(|| {
            rows.last()
                .expect("non-empty child page")
                .session_id
                .clone()
        });
        Ok(ChildPage {
            sessions: rows,
            next_cursor,
        })
    }

    async fn reserve_sandbox(
        &self,
        request: &SandboxReserveRequest,
    ) -> Result<SandboxInventoryDoc> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin sandbox reservation", error))?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT doc_json FROM sandbox_inventory WHERE root_id=?1 AND sandbox_id=?2",
                params![request.root_id, request.sandbox_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| db_error("read sandbox reservation", error))?;
        if let Some(existing) = existing {
            let existing: SandboxInventoryDoc = decode(&existing, "sandbox inventory")?;
            if existing.operation_id == request.operation_id
                && existing.request_digest == request.request_digest
                && existing.owner_session_id == request.owner_session_id
            {
                transaction
                    .commit()
                    .map_err(|error| db_error("finish sandbox reservation replay", error))?;
                return Ok(existing);
            }
            return Err(BrainError::IdempotencyConflict);
        }
        let root: Option<(String, String, u32)> = transaction
            .query_row(
                "SELECT control_json,config_json,live_sandboxes FROM sessions WHERE session_id=?1",
                [&request.root_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| db_error("read sandbox root admission", error))?;
        let Some((control, config, live_sandboxes)) = root else {
            return Err(BrainError::NoSuchSession(request.root_id.clone()));
        };
        let root = decode_head(&control, &config)?;
        if root.root_id != request.root_id || !child_admission_open(&root) {
            return Err(BrainError::Invalid(
                "additional sandbox admission is closed for this root".into(),
            ));
        }
        if live_sandboxes >= root.prefix.max_additional_sandboxes_per_root {
            return Err(BrainError::SandboxResourceExhausted);
        }
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
        transaction
            .execute(
                "UPDATE sessions SET live_sandboxes=live_sandboxes+1 WHERE session_id=?1",
                [&request.root_id],
            )
            .map_err(|error| db_error("reserve sandbox slot", error))?;
        transaction
            .execute(
                "INSERT INTO sandbox_inventory(root_id,sandbox_id,version,doc_json)
                 VALUES (?1,?2,1,?3)",
                params![
                    request.root_id,
                    request.sandbox_id,
                    encode(&doc, "sandbox inventory")?
                ],
            )
            .map_err(|error| db_error("insert sandbox inventory", error))?;
        transaction
            .commit()
            .map_err(|error| db_error("commit sandbox reservation", error))?;
        Ok(doc)
    }

    async fn get_sandbox(&self, root_id: &str, sandbox_id: &str) -> Result<SandboxInventoryDoc> {
        let connection = self.connection()?;
        let value: Option<String> = connection
            .query_row(
                "SELECT doc_json FROM sandbox_inventory WHERE root_id=?1 AND sandbox_id=?2",
                params![root_id, sandbox_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| db_error("get sandbox inventory", error))?;
        value
            .map(|value| decode(&value, "sandbox inventory"))
            .transpose()?
            .ok_or_else(|| BrainError::FileNotFound(format!("sandbox {sandbox_id}")))
    }

    async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> Result<SandboxPage> {
        let connection = self.connection()?;
        let limit = query.limit.clamp(1, 100);
        let mut statement = connection
            .prepare(
                "SELECT doc_json FROM sandbox_inventory
                 WHERE root_id=?1 AND sandbox_id>?2 ORDER BY sandbox_id ASC LIMIT ?3",
            )
            .map_err(|error| db_error("prepare sandbox inventory list", error))?;
        let rows = statement
            .query_map(
                params![
                    query.root_id,
                    query.cursor.unwrap_or(""),
                    integer((limit + 1) as u64, "sandbox list limit")?
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| db_error("query sandbox inventory", error))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| db_error("read sandbox inventory", error))?
            .into_iter()
            .map(|value| decode(&value, "sandbox inventory"))
            .collect::<Result<Vec<SandboxInventoryDoc>>>()?;
        let mut sandboxes = rows;
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
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| db_error("begin sandbox lifecycle update", error))?;
        let row: Option<(u64, String)> = transaction
            .query_row(
                "SELECT version,doc_json FROM sandbox_inventory WHERE root_id=?1 AND sandbox_id=?2",
                params![request.root_id, request.sandbox_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| db_error("read sandbox lifecycle", error))?;
        let Some((version, body)) = row else {
            return Err(BrainError::FileNotFound(format!(
                "sandbox {}",
                request.sandbox_id
            )));
        };
        let mut current: SandboxInventoryDoc = decode(&body, "sandbox inventory")?;
        if version != request.expected_version || current.version != request.expected_version {
            if serde_json::to_value(&current.status)? == serde_json::to_value(&request.status)? {
                transaction
                    .commit()
                    .map_err(|error| db_error("finish sandbox lifecycle replay", error))?;
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
        if request.release_slot && !current.slot_released {
            transaction
                .execute(
                    "UPDATE sessions SET live_sandboxes=MAX(live_sandboxes-1,0)
                     WHERE session_id=?1",
                    [&request.root_id],
                )
                .map_err(|error| db_error("release sandbox slot", error))?;
            current.slot_released = true;
        }
        current.status = request.status.clone();
        current.updated_at_ms = request.now_ms;
        current.version = current.version.saturating_add(1);
        let updated = transaction
            .execute(
                "UPDATE sandbox_inventory SET version=?3,doc_json=?4
                 WHERE root_id=?1 AND sandbox_id=?2 AND version=?5",
                params![
                    request.root_id,
                    request.sandbox_id,
                    integer(current.version, "sandbox version")?,
                    encode(&current, "sandbox inventory")?,
                    integer(request.expected_version, "sandbox expected version")?
                ],
            )
            .map_err(|error| db_error("update sandbox lifecycle", error))?;
        if updated != 1 {
            return Err(BrainError::Fenced);
        }
        transaction
            .commit()
            .map_err(|error| db_error("commit sandbox lifecycle update", error))?;
        Ok(current)
    }

    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> Result<RecoveryPage> {
        // Local mode has one durable SQLite node. A bounded scan avoids a second migration-only
        // index while preserving the exact hosted ordering/cursor contract.
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, control_json, config_json, last_seq FROM sessions
                 ORDER BY session_id ASC",
            )
            .map_err(|error| db_error("prepare recovery list", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })
            .map_err(|error| db_error("query recovery list", error))?;
        let mut candidates = Vec::new();
        for row in rows {
            let (session_id, control, config, last_seq) =
                row.map_err(|error| db_error("decode recovery row", error))?;
            let doc = decode_head(&control, &config)?;
            let Some(due_ms) = doc.recovery_due_ms else {
                continue;
            };
            let key = recovery_due_key(due_ms, &session_id);
            if recovery_shard(&session_id) != query.shard
                || due_ms > query.due_before_ms
                || query.cursor.is_some_and(|cursor| key.as_str() <= cursor)
            {
                continue;
            }
            candidates.push((
                key,
                RecoveryItem {
                    session_id,
                    due_ms,
                    state: doc.state,
                    active_phase: doc.active_phase,
                    last_seq,
                    root_id: doc.root_id,
                    parent_id: doc.parent_id,
                    updated_ms: doc.updated_ms,
                },
            ));
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        let limit = query.limit.clamp(1, 100);
        let has_more = candidates.len() > limit;
        candidates.truncate(limit);
        let next_cursor = has_more.then(|| candidates.last().expect("non-empty page").0.clone());
        Ok(RecoveryPage {
            items: candidates.into_iter().map(|(_, item)| item).collect(),
            next_cursor,
        })
    }

    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, control_json, config_json, fence, last_seq,
                 journal_metered_bytes,journal_effect_reserve_bytes,
                 journal_lifecycle_reserve_bytes FROM sessions
                 ORDER BY created_ms DESC, session_id ASC LIMIT ?1",
            )
            .map_err(|error| db_error("prepare session list", error))?;
        let rows = statement
            .query_map([integer(limit as u64, "session list limit")?], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, u64>(7)?,
                ))
            })
            .map_err(|error| db_error("query session list", error))?;
        let mut heads = Vec::new();
        for row in rows {
            let (session_id, control, config, fence, last_seq, metered, effect, lifecycle) =
                row.map_err(|error| db_error("decode session list row", error))?;
            heads.push(Head {
                session_id,
                doc: decode_head(&control, &config)?,
                fence,
                last_seq,
                retention: JournalRetention {
                    metered_bytes: metered,
                    effect_reserve_bytes: effect,
                    lifecycle_reserve_bytes: lifecycle,
                },
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

fn decode_head(control: &str, config: &str) -> Result<HeadDoc> {
    Ok(HeadDoc::join(
        decode::<ControlDoc>(control, "journal control")?,
        decode::<ConfigDoc>(config, "journal config")?,
    ))
}

fn apply_tenant_retention_delta(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    delta: i64,
    limit: u64,
    context: &str,
) -> Result<()> {
    if delta == 0 {
        return Ok(());
    }
    let (used, sessions) = transaction
        .query_row(
            "SELECT total_bytes,session_count FROM tenant_retention WHERE tenant_id=?1",
            [tenant_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )
        .optional()
        .map_err(|error| db_error(&format!("read {context} tenant retention meter"), error))?
        .unwrap_or((0, 0));
    let next = if delta >= 0 {
        let requested = delta as u64;
        let next = used
            .checked_add(requested)
            .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
        if next > limit {
            return Err(BrainError::TenantJournalQuotaExceeded { requested, limit });
        }
        next
    } else {
        used.checked_sub(delta.unsigned_abs()).ok_or_else(|| {
            BrainError::Journal("tenant journal meter would become negative".into())
        })?
    };
    transaction
        .execute(
            "INSERT INTO tenant_retention(tenant_id,total_bytes,session_count) VALUES (?1,?2,?3)
             ON CONFLICT(tenant_id) DO UPDATE SET total_bytes=excluded.total_bytes",
            params![
                tenant_id,
                integer(next, "tenant journal meter")?,
                integer(sessions, "tenant retained-session meter")?,
            ],
        )
        .map_err(|error| db_error(&format!("update {context} tenant retention meter"), error))?;
    Ok(())
}

fn integer(value: u64, what: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| BrainError::Journal(format!("{what} exceeds SQLite integer range")))
}

fn db_error(context: &str, error: rusqlite::Error) -> BrainError {
    BrainError::Journal(format!("{context}: {error}"))
}

fn ensure_session_column(connection: &Connection, name: &str, declaration: &str) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(|error| db_error("inspect SQLite journal schema", error))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| db_error("read SQLite journal schema", error))?;
    for column in columns {
        if column.map_err(|error| db_error("decode SQLite journal schema", error))? == name {
            return Ok(());
        }
    }
    connection
        .execute(
            &format!("ALTER TABLE sessions ADD COLUMN {name} {declaration}"),
            [],
        )
        .map_err(|error| db_error("migrate SQLite journal schema", error))?;
    Ok(())
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
    use brain::journal::{
        Journal, Lease, PrefixDoc, SandboxListQuery, SandboxReserveRequest, SandboxUpdateRequest,
    };
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
            loop_state: None,
            tenant_id: "local".into(),
            root_id: "ses_test".into(),
            parent_id: None,
            ancestor_ids: Vec::new(),
            child_name: None,
            context_fork: None,
            depth: 0,
            last_seq: 1,
            state: "open".into(),
            failure: None,
            turn: None,
            active_phase: None,
            provider_attempt: None,
            active_context: HashMap::new(),
            active_rounds: 0,
            active_tool_calls: 0,
            message_replays: vec![],
            context: None,
            turns: 0,
            created_ms: 1,
            updated_ms: 1,
            recovery_due_ms: None,
            recovery_attempt: 0,
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
                context_window_tokens: 32 * 1024,
                context_soft_tokens: 18 * 1024,
                context_hard_tokens: 22 * 1024,
                context_tail_tokens: 4 * 1024,
                context_summary_tokens: 4 * 1024,
                temperature: Some(0.0),
                reasoning_effort: None,
                provider_recovery_retries: 1,
                storage_max_object_bytes: brain::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
                storage_max_session_bytes: brain::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
                storage_transfer_ttl_ms: brain::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
                max_additional_sandboxes_per_root: 2,
                max_child_depth: 4,
                max_direct_children: 32,
                max_descendants: 256,
                customer_client_id: None,
                customer_submit_retries: 1,
                rendered_base: serde_json::json!({}),
                rendered_base_digest: String::new(),
                prompt_cache_key: String::new(),
                tools: vec![],
                managed_bundles: vec![],
                official_capabilities: HashMap::new(),
                hand_enabled: false,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                hand_env_keys: vec![],
                network: serde_json::json!({"outbound": "none"}),
                metadata: HashMap::new(),
            },
            key_b64: String::new(),
            hand_secrets_b64: String::new(),
            session_storage_bytes: 0,
            storage_reserved_bytes: 0,
            tenant_metered_storage_bytes: 0,
            storage_upload: None,
            storage_delete: None,
            pending_customer_acks: vec![],
            pending_managed_acks: vec![],
            default_sandbox: None,
        }
    }

    fn user(text: &str) -> Record {
        Record::UserMessage {
            turn: "trn_test".into(),
            content: vec![ContentBlock::text(text)],
            starts_turn: false,
            metadata: HashMap::new(),
            idempotency_key_hash: None,
            request_hash: None,
        }
    }

    async fn create_direct(
        store: &SqliteStore,
        session_id: &str,
        doc: &HeadDoc,
        first: &Record,
        owner: &str,
        now_ms: u64,
    ) -> Result<()> {
        let limits = JournalRetentionLimits::default();
        let retention = initial_retention(first, limits.session_bytes)?;
        store
            .create(
                session_id,
                doc,
                first,
                owner,
                now_ms,
                u64::MAX,
                retention,
                limits,
            )
            .await
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
            retention: head.retention,
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
            retention: head.retention,
        };
        assert_eq!(reopened.read_records("ses_test", 0).await.unwrap().len(), 2);
        let duplicate = reopened
            .commit("ses_test", &mut lease, &[(2, user("duplicate"))], &doc(), 2)
            .await;
        assert!(matches!(duplicate, Err(BrainError::Fenced)));
    }

    #[tokio::test]
    async fn active_owner_cannot_be_stolen_and_final_deletion_is_metered() {
        let dir = TestDir::new();
        let store = Arc::new(SqliteStore::open(dir.0.join("journal.sqlite3")).unwrap());
        let a = Journal::new(store.clone(), "owner-a");
        let b = Journal::new(store, "owner-b");
        a.create("ses_test", &doc(), &user("one")).await.unwrap();
        a.claim("ses_test").await.unwrap();
        assert!(matches!(b.claim("ses_test").await, Err(BrainError::Fenced)));
        let head = a.get_head("ses_test").await.unwrap();
        assert_eq!(a.purge_history("ses_test").await.unwrap(), 1);
        a.finalize_deletion(&DeletionStatusDoc {
            session_id: "ses_test".into(),
            tenant_id: head.doc.tenant_id.clone(),
            root_id: head.doc.root_id.clone(),
            parent_id: head.doc.parent_id.clone(),
            metered_storage_bytes: head.doc.tenant_metered_storage_bytes,
            metered_journal_bytes: head.retention.metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: i64::MAX as u64,
            attempts: 1,
            last_error: None,
        })
        .await
        .unwrap();
        assert!(matches!(
            a.get_head("ses_test").await,
            Err(BrainError::NoSuchSession(_))
        ));
    }

    #[tokio::test]
    async fn child_admission_and_exact_release_survive_reopen() {
        let dir = TestDir::new();
        let path = dir.0.join("journal.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        let mut root = doc();
        root.root_id = "ses_root".into();
        root.prefix.max_direct_children = 1;
        root.prefix.max_descendants = 1;
        create_direct(&store, "ses_root", &root, &user("root"), "owner-a", 0)
            .await
            .unwrap();
        let mut child = root.clone();
        child.parent_id = Some("ses_root".into());
        child.ancestor_ids = vec!["ses_root".into()];
        child.depth = 1;
        create_direct(&store, "ses_child", &child, &user("child"), "owner-a", 1)
            .await
            .unwrap();
        drop(store);

        let store = SqliteStore::open(&path).unwrap();
        assert!(matches!(
            create_direct(
                &store,
                "ses_over_limit",
                &child,
                &user("over limit"),
                "owner-b",
                2,
            )
            .await,
            Err(BrainError::Overloaded)
        ));
        let terminal = DeletionStatusDoc {
            session_id: "ses_child".into(),
            tenant_id: "local".into(),
            root_id: "ses_root".into(),
            parent_id: Some("ses_root".into()),
            metered_storage_bytes: 0,
            metered_journal_bytes: store
                .get_head("ses_child")
                .await
                .unwrap()
                .retention
                .metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 3,
            updated_at_ms: 4,
            completed_at_ms: Some(4),
            expires_at_ms: 86_400_000,
            attempts: 1,
            last_error: None,
        };
        store.finalize_deletion(&terminal).await.unwrap();
        store
            .finalize_deletion(&terminal)
            .await
            .expect("a lost final response cannot release capacity twice");
        create_direct(
            &store,
            "ses_replacement",
            &child,
            &user("replacement"),
            "owner-b",
            5,
        )
        .await
        .unwrap();
        assert_eq!(
            store
                .list_child_page(&ChildListQuery {
                    parent_id: "ses_root",
                    limit: 100,
                    cursor: None,
                })
                .await
                .unwrap()
                .sessions
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["ses_replacement"]
        );
    }

    #[tokio::test]
    async fn tenant_retention_meter_survives_reopen_and_final_release_is_exact() {
        let dir = TestDir::new();
        let path = dir.0.join("journal.sqlite3");
        let limits = JournalRetentionLimits {
            session_bytes: brain::journal::DEFAULT_MAX_SESSION_JOURNAL_BYTES,
            tenant_bytes: brain::journal::DEFAULT_MAX_TENANT_JOURNAL_BYTES,
            tenant_sessions: 1,
        };
        let mut first = doc();
        first.tenant_id = "tenant-sqlite-retention".into();
        first.root_id = "ses_sqlite_retained".into();
        let journal = Journal::new(
            Arc::new(SqliteStore::open(&path).unwrap()),
            "owner-retention-a",
        )
        .with_retention_limits(limits);
        journal
            .create("ses_sqlite_retained", &first, &user("retained"))
            .await
            .unwrap();
        let retained = journal
            .get_head("ses_sqlite_retained")
            .await
            .unwrap()
            .retention;
        drop(journal);

        let reopened_store = Arc::new(SqliteStore::open(&path).unwrap());
        let reopened =
            Journal::new(reopened_store.clone(), "owner-retention-b").with_retention_limits(limits);
        assert_eq!(
            reopened
                .get_head("ses_sqlite_retained")
                .await
                .unwrap()
                .retention,
            retained
        );
        let mut rejected = doc();
        rejected.tenant_id = first.tenant_id.clone();
        rejected.root_id = "ses_sqlite_rejected".into();
        assert!(matches!(
            reopened
                .create("ses_sqlite_rejected", &rejected, &user("rejected"))
                .await,
            Err(BrainError::TenantRetainedSessionQuotaExceeded { limit: 1 })
        ));

        let terminal = DeletionStatusDoc {
            session_id: "ses_sqlite_retained".into(),
            tenant_id: first.tenant_id,
            root_id: first.root_id,
            parent_id: None,
            metered_storage_bytes: 0,
            metered_journal_bytes: retained.metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: 86_400_000,
            attempts: 1,
            last_error: None,
        };
        reopened_store.finalize_deletion(&terminal).await.unwrap();
        reopened_store.finalize_deletion(&terminal).await.unwrap();
        reopened
            .create("ses_sqlite_rejected", &rejected, &user("replacement"))
            .await
            .expect("one physical final deletion releases one durable identity");
    }

    fn sandbox_reservation(index: usize) -> SandboxReserveRequest {
        let sandbox_id = format!("sbx_{index:02}");
        SandboxReserveRequest {
            root_id: "ses_test".into(),
            owner_session_id: "ses_test".into(),
            sandbox_id: sandbox_id.clone(),
            operation_id: format!("op_{index:02}"),
            request_digest: format!("{index:064x}"),
            generation_intent: format!("gen_{index:02}"),
            initial_status: serde_json::from_value(serde_json::json!({
                "state": "creating",
                "target": {
                    "kind": "additional",
                    "session_id": "ses_test",
                    "root_id": "ses_test",
                    "binding_ref": format!("bnd_{index:02}"),
                    "sandbox_id": sandbox_id,
                },
                "generation": format!("gen_{index:02}"),
                "changed_at_ms": index as u64 + 1,
                "expires_at_ms": null,
            }))
            .unwrap(),
            now_ms: index as u64 + 1,
        }
    }

    #[tokio::test]
    async fn sandbox_inventory_cap_and_terminal_tombstone_survive_reopen() {
        let dir = TestDir::new();
        let path = dir.0.join("journal.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        create_direct(&store, "ses_test", &doc(), &user("root"), "owner-a", 0)
            .await
            .unwrap();
        let first = store
            .reserve_sandbox(&sandbox_reservation(0))
            .await
            .unwrap();
        store
            .reserve_sandbox(&sandbox_reservation(1))
            .await
            .unwrap();
        assert!(matches!(
            store.reserve_sandbox(&sandbox_reservation(2)).await,
            Err(BrainError::SandboxResourceExhausted)
        ));
        drop(store);

        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(
            store
                .reserve_sandbox(&sandbox_reservation(0))
                .await
                .unwrap()
                .sandbox_id,
            first.sandbox_id
        );
        let mut terminal = first.status.clone();
        terminal.state = brain_protocol::hand::SandboxState::Terminated;
        let tombstone = store
            .update_sandbox(&SandboxUpdateRequest {
                root_id: first.root_id,
                sandbox_id: first.sandbox_id,
                expected_version: first.version,
                status: terminal,
                release_slot: true,
                now_ms: 10,
            })
            .await
            .unwrap();
        assert!(tombstone.slot_released);
        store
            .reserve_sandbox(&sandbox_reservation(2))
            .await
            .expect("terminal target releases one live slot");
        assert_eq!(
            store
                .list_sandbox_page(&SandboxListQuery {
                    root_id: "ses_test",
                    limit: 10,
                    cursor: None,
                })
                .await
                .unwrap()
                .sandboxes
                .len(),
            3
        );
        assert!(matches!(
            store
                .update_sandbox(&SandboxUpdateRequest {
                    root_id: tombstone.root_id.clone(),
                    sandbox_id: tombstone.sandbox_id.clone(),
                    expected_version: tombstone.version,
                    status: sandbox_reservation(0).initial_status,
                    release_slot: false,
                    now_ms: 11,
                })
                .await,
            Err(BrainError::SandboxGone)
        ));
    }
}
