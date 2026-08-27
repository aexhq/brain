use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use brain_protocol::{JournalId, SessionId, SessionStatus, request_digest};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    KernelError,
    journal::{AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate},
};

const SCHEMA_VERSION: u32 = 2;

pub struct SqliteJournal {
    journal_id: JournalId,
    connection: Mutex<Connection>,
}

impl SqliteJournal {
    pub fn open(path: &Path) -> Result<Self, KernelError> {
        let mut connection = Connection::open(path).map_err(journal_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(journal_error)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(journal_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(journal_error)?;
        let transaction = connection.transaction().map_err(journal_error)?;
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (
               session_id TEXT PRIMARY KEY,
               journal_id TEXT NOT NULL,
               status TEXT NOT NULL,
               through_sequence INTEGER NOT NULL,
               configuration_json TEXT NOT NULL,
               context_json TEXT NOT NULL,
               presentation_digest TEXT NOT NULL,
               metadata_json TEXT NOT NULL DEFAULT 'null'
             );
             CREATE TABLE IF NOT EXISTS records (
               session_id TEXT NOT NULL,
               sequence INTEGER NOT NULL,
               schema_version INTEGER NOT NULL,
               recorded_at_ms INTEGER NOT NULL,
               kind TEXT NOT NULL,
               payload_json TEXT NOT NULL,
               checksum TEXT NOT NULL,
               PRIMARY KEY (session_id, sequence),
               FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS idempotency (
               scope TEXT NOT NULL,
               key TEXT NOT NULL,
               request_digest TEXT NOT NULL,
               response_json TEXT NOT NULL,
               PRIMARY KEY (scope, key)
             );",
            )
            .map_err(journal_error)?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key = 'journal_id'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(journal_error)?;
        let journal_id = match existing {
            Some(value) => JournalId::new(value),
            None => {
                let mut random = [0_u8; 16];
                rand::rng().fill_bytes(&mut random);
                let value = format!("jrn_{}", hex::encode(random));
                transaction
                    .execute(
                        "INSERT INTO metadata(key, value) VALUES('journal_id', ?1)",
                        [&value],
                    )
                    .map_err(journal_error)?;
                JournalId::new(value)
            }
        };
        transaction.commit().map_err(journal_error)?;
        let journal = Self {
            journal_id,
            connection: Mutex::new(connection),
        };
        journal.verify()?;
        Ok(journal)
    }

    fn verify(&self) -> Result<(), KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let mut statement = connection.prepare(
            "SELECT session_id, sequence, schema_version, recorded_at_ms, kind, payload_json, checksum FROM records ORDER BY session_id, sequence",
        ).map_err(journal_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(journal_error)?;
        let mut positions = std::collections::HashMap::new();
        for row in rows {
            let (
                session_id,
                sequence,
                schema_version,
                recorded_at_ms,
                kind,
                payload_json,
                stored_checksum,
            ) = row.map_err(journal_error)?;
            if schema_version != SCHEMA_VERSION {
                return Err(KernelError::Journal(format!(
                    "unsupported journal schema {schema_version} at {session_id}:{sequence}"
                )));
            }
            let payload: serde_json::Value = serde_json::from_str(&payload_json)
                .map_err(|error| KernelError::Journal(error.to_string()))?;
            let expected = checksum(&session_id, sequence, recorded_at_ms, &kind, &payload)?;
            if expected != stored_checksum {
                return Err(KernelError::Journal(format!(
                    "journal checksum mismatch at {session_id}:{sequence}"
                )));
            }
            let previous = positions.insert(session_id.clone(), sequence).unwrap_or(0);
            if sequence != previous + 1 {
                return Err(KernelError::Journal(format!(
                    "journal sequence gap at {session_id}:{sequence}"
                )));
            }
        }
        let mut sessions = connection
            .prepare("SELECT session_id,through_sequence FROM sessions")
            .map_err(journal_error)?;
        let sessions = sessions
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(journal_error)?;
        for session in sessions {
            let (session_id, through_sequence) = session.map_err(journal_error)?;
            if positions.get(&session_id).copied() != Some(through_sequence) {
                return Err(KernelError::Journal(format!(
                    "session position does not match its journal at {session_id}:{through_sequence}"
                )));
            }
        }
        Ok(())
    }
}

impl JournalStore for SqliteJournal {
    fn journal_id(&self) -> &JournalId {
        &self.journal_id
    }

    fn create_session(
        &self,
        row: &SessionRow,
        record: AppendRecord,
    ) -> Result<JournalRecord, KernelError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(journal_error)?;
        transaction.execute(
            "INSERT INTO sessions(session_id,journal_id,status,through_sequence,configuration_json,context_json,presentation_digest) VALUES(?1,?2,?3,1,?4,?5,?6)",
            params![row.session_id.as_str(), row.journal_id.as_str(), status_name(&row.status), encode(&row.configuration)?, encode(&row.context)?, row.presentation_digest],
        ).map_err(journal_error)?;
        let saved = insert_record(&transaction, &row.session_id, 1, record)?;
        transaction.commit().map_err(journal_error)?;
        Ok(saved)
    }

    fn append(
        &self,
        session_id: &SessionId,
        expected_through: u64,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, KernelError> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let transaction = connection.transaction().map_err(journal_error)?;
        let through: u64 = transaction
            .query_row(
                "SELECT through_sequence FROM sessions WHERE session_id=?1",
                [session_id.as_str()],
                |row| row.get(0),
            )
            .map_err(journal_error)?;
        if through != expected_through {
            return Err(KernelError::InvalidState(format!(
                "journal position changed: expected {expected_through}, found {through}"
            )));
        }
        let mut saved = Vec::with_capacity(records.len());
        for (offset, record) in records.iter().cloned().enumerate() {
            saved.push(insert_record(
                &transaction,
                session_id,
                through + offset as u64 + 1,
                record,
            )?);
        }
        let new_through = through + records.len() as u64;
        transaction
            .execute(
                "UPDATE sessions SET through_sequence=?2 WHERE session_id=?1",
                params![session_id.as_str(), new_through],
            )
            .map_err(journal_error)?;
        if let Some(status) = update.status {
            transaction
                .execute(
                    "UPDATE sessions SET status=?2 WHERE session_id=?1",
                    params![session_id.as_str(), status_name(&status)],
                )
                .map_err(journal_error)?;
        }
        if let Some(context) = update.context {
            transaction
                .execute(
                    "UPDATE sessions SET context_json=?2 WHERE session_id=?1",
                    params![session_id.as_str(), encode(context)?],
                )
                .map_err(journal_error)?;
        }
        if let Some(configuration) = update.configuration {
            transaction
                .execute(
                    "UPDATE sessions SET configuration_json=?2 WHERE session_id=?1",
                    params![session_id.as_str(), encode(configuration)?],
                )
                .map_err(journal_error)?;
        }
        transaction.commit().map_err(journal_error)?;
        Ok(saved)
    }

    fn session(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        connection.query_row(
            "SELECT session_id,journal_id,status,through_sequence,configuration_json,context_json,presentation_digest FROM sessions WHERE session_id=?1",
            [session_id.as_str()], read_session,
        ).optional().map_err(journal_error)
    }

    fn sessions(&self) -> Result<Vec<SessionRow>, KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let mut statement = connection.prepare("SELECT session_id,journal_id,status,through_sequence,configuration_json,context_json,presentation_digest FROM sessions ORDER BY session_id").map_err(journal_error)?;
        statement
            .query_map([], read_session)
            .map_err(journal_error)?
            .map(|row| row.map_err(journal_error))
            .collect()
    }

    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let mut statement = connection.prepare("SELECT sequence,schema_version,recorded_at_ms,kind,payload_json,checksum FROM records WHERE session_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(journal_error)?;
        let rows = statement
            .query_map(params![session_id.as_str(), after, limit as u64], |row| {
                let payload_json: String = row.get(4)?;
                let payload = serde_json::from_str(&payload_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        payload_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(JournalRecord {
                    session_id: session_id.clone(),
                    sequence: row.get(0)?,
                    schema_version: row.get(1)?,
                    recorded_at_ms: row.get(2)?,
                    kind: row.get(3)?,
                    payload,
                    checksum: row.get(5)?,
                })
            })
            .map_err(journal_error)?;
        rows.map(|row| row.map_err(journal_error)).collect()
    }

    fn delete_ended(&self, session_id: &SessionId) -> Result<(), KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let changed = connection
            .execute(
                "DELETE FROM sessions WHERE session_id=?1 AND status='ended'",
                [session_id.as_str()],
            )
            .map_err(journal_error)?;
        if changed == 0 {
            return Err(KernelError::InvalidState(
                "session must exist and be ended before deletion".into(),
            ));
        }
        Ok(())
    }

    fn idempotency_get(
        &self,
        scope: &str,
        key: &str,
        digest: &str,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        let row: Option<(String, String)> = connection
            .query_row(
                "SELECT request_digest,response_json FROM idempotency WHERE scope=?1 AND key=?2",
                params![scope, key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(journal_error)?;
        match row {
            None => Ok(None),
            Some((saved_digest, _)) if saved_digest != digest => Err(KernelError::InvalidState(
                "idempotency key reused with different request".into(),
            )),
            Some((_, response)) => serde_json::from_str(&response)
                .map(Some)
                .map_err(|error| KernelError::Journal(error.to_string())),
        }
    }

    fn idempotency_put(
        &self,
        scope: &str,
        key: &str,
        digest: &str,
        response: &serde_json::Value,
    ) -> Result<(), KernelError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| KernelError::Journal("journal mutex poisoned".into()))?;
        connection.execute("INSERT INTO idempotency(scope,key,request_digest,response_json) VALUES(?1,?2,?3,?4)", params![scope, key, digest, encode(response)?]).map_err(journal_error)?;
        Ok(())
    }
}

fn insert_record(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &SessionId,
    sequence: u64,
    record: AppendRecord,
) -> Result<JournalRecord, KernelError> {
    let wall_time_ms: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| KernelError::Journal("system time is before the Unix epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| KernelError::Journal("system time exceeds the journal range".into()))?;
    let previous_recorded_at_ms = transaction
        .query_row(
            "SELECT COALESCE(MAX(recorded_at_ms), 0) FROM records WHERE session_id=?1",
            [session_id.as_str()],
            |row| row.get::<_, u64>(0),
        )
        .map_err(journal_error)?;
    let recorded_at_ms = wall_time_ms.max(previous_recorded_at_ms);
    let checksum = checksum(
        session_id.as_str(),
        sequence,
        recorded_at_ms,
        &record.kind,
        &record.payload,
    )?;
    transaction.execute("INSERT INTO records(session_id,sequence,schema_version,recorded_at_ms,kind,payload_json,checksum) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![session_id.as_str(), sequence, SCHEMA_VERSION, recorded_at_ms, record.kind, encode(&record.payload)?, checksum]).map_err(journal_error)?;
    Ok(JournalRecord {
        schema_version: SCHEMA_VERSION,
        session_id: session_id.clone(),
        sequence,
        recorded_at_ms,
        kind: record.kind,
        payload: record.payload,
        checksum,
    })
}

fn checksum(
    session_id: &str,
    sequence: u64,
    recorded_at_ms: u64,
    kind: &str,
    payload: &serde_json::Value,
) -> Result<String, KernelError> {
    request_digest(&(
        SCHEMA_VERSION,
        session_id,
        sequence,
        recorded_at_ms,
        kind,
        payload,
    ))
    .map_err(|error| KernelError::Journal(error.to_string()))
}

fn read_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    let status: String = row.get(2)?;
    let configuration: String = row.get(4)?;
    let context: String = row.get(5)?;
    Ok(SessionRow {
        session_id: SessionId::new(row.get::<_, String>(0)?),
        journal_id: JournalId::new(row.get::<_, String>(1)?),
        status: parse_status(&status)?,
        through_sequence: row.get(3)?,
        configuration: decode(configuration)?,
        context: decode(context)?,
        presentation_digest: row.get(6)?,
    })
}

fn parse_status(status: &str) -> rusqlite::Result<SessionStatus> {
    match status {
        "creating" => Ok(SessionStatus::Creating),
        "idle" => Ok(SessionStatus::Idle),
        "running" => Ok(SessionStatus::Running),
        "ended" => Ok(SessionStatus::Ended),
        "failed" => Ok(SessionStatus::Failed),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn status_name(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Creating => "creating",
        SessionStatus::Idle => "idle",
        SessionStatus::Running => "running",
        SessionStatus::Ended => "ended",
        SessionStatus::Failed => "failed",
    }
}

fn encode(value: &serde_json::Value) -> Result<String, KernelError> {
    serde_json::to_string(value).map_err(|error| KernelError::Journal(error.to_string()))
}
fn decode(value: String) -> rusqlite::Result<serde_json::Value> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
fn journal_error(error: rusqlite::Error) -> KernelError {
    KernelError::Journal(error.to_string())
}
