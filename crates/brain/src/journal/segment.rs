//! The journal: session state in memory, records appended to a segment log.
//!
//! Every read a running session makes is served from memory. The log is written
//! behind the caller and is only read to rebuild this state at startup, or to page a
//! client back through records it has not seen. Appending costs a serialise and a
//! channel send; no journal call waits on the disk.

use std::{
    collections::{HashMap, hash_map::Entry},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use brain_protocol::{Identity, SessionId, SessionStatus};

use crate::{
    KernelError,
    journal::{
        AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate,
        log::{Append, Frame, Location, SegmentLog},
    },
};

/// Frame kinds the journal writes for itself. They carry no sequence number, so they
/// never reach a client's event stream, and the `$` prefix keeps them out of the
/// namespace a session's record kinds are drawn from.
const SESSION_STATE: &str = "$session";
const SESSION_DELETED: &str = "$deleted";
const IDEMPOTENCY: &str = "$idempotency";

pub struct SegmentJournal {
    log: SegmentLog,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Keyed by the raw session id so replay and lookups never allocate one.
    sessions: HashMap<String, Tracked>,
    idempotency: HashMap<(String, String), Stored>,
}

struct Tracked {
    row: SessionRow,
    /// `locations[i]` holds the record at sequence `i + 1`.
    locations: Vec<Location>,
    /// Where the session's own state was last written, so reclamation keeps it.
    state_segment: u64,
    last_recorded_at_ms: u64,
}

struct Stored {
    request: Identity,
    response: serde_json::Value,
    segment: u64,
}

impl SegmentJournal {
    pub fn open(directory: &Path) -> Result<Self, KernelError> {
        let mut state = State::default();
        let log = SegmentLog::open(directory, |frame, location| {
            replay(&mut state, &frame, location)
        })?;
        Ok(Self {
            log,
            state: Mutex::new(state),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, KernelError> {
        self.state
            .lock()
            .map_err(|_| KernelError::Journal("journal state mutex poisoned".into()))
    }

    /// Append one frame and return where it landed. Held separate from `write_state`
    /// so the state lock is never held across an encode of a large payload more than
    /// it must be.
    fn write(
        &self,
        session_id: &str,
        sequence: u64,
        recorded_at_ms: u64,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<Location, KernelError> {
        self.log.append(Append {
            session_id,
            sequence,
            recorded_at_ms,
            kind,
            payload,
        })
    }
}

impl JournalStore for SegmentJournal {
    fn create_session(
        &self,
        row: &SessionRow,
        record: AppendRecord,
    ) -> Result<JournalRecord, KernelError> {
        let mut state = self.lock()?;
        if state.sessions.contains_key(row.session_id.as_str()) {
            return Err(KernelError::InvalidState("session already exists".into()));
        }
        let recorded_at_ms = wall_clock_ms()?;

        // State before the record: a replay that stops between the two still knows the
        // session, and the record it is missing is one nothing has acted on yet.
        let state_location = self.write(
            row.session_id.as_str(),
            0,
            recorded_at_ms,
            SESSION_STATE,
            &serde_json::json!({
                "journal_id": row.journal_id,
                "status": row.status,
                "configuration": row.configuration,
                "context": row.context,
                "presentation_digest": row.presentation_digest,
            }),
        )?;
        let location = self.write(
            row.session_id.as_str(),
            1,
            recorded_at_ms,
            &record.kind,
            &record.payload,
        )?;

        let mut row = row.clone();
        row.through_sequence = 1;
        state.sessions.insert(
            row.session_id.as_str().to_string(),
            Tracked {
                row: row.clone(),
                locations: vec![location],
                state_segment: state_location.segment,
                last_recorded_at_ms: recorded_at_ms,
            },
        );

        Ok(JournalRecord {
            session_id: row.session_id,
            journal_id: row.journal_id,
            sequence: 1,
            recorded_at_ms,
            kind: record.kind,
            payload: record.payload,
        })
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
        let mut state = self.lock()?;
        let tracked = state
            .sessions
            .get(session_id.as_str())
            .ok_or_else(not_found)?;
        if tracked.row.through_sequence != expected_through {
            return Err(KernelError::InvalidState(format!(
                "journal position changed: expected {expected_through}, found {}",
                tracked.row.through_sequence
            )));
        }
        let journal_id = tracked.row.journal_id.clone();
        // Recorded time never goes backwards within a session, whatever the wall clock does.
        let recorded_at_ms = wall_clock_ms()?.max(tracked.last_recorded_at_ms);

        let mut saved = Vec::with_capacity(records.len());
        let mut locations = Vec::with_capacity(records.len());
        for (offset, record) in records.iter().enumerate() {
            let sequence = expected_through + offset as u64 + 1;
            locations.push(self.write(
                session_id.as_str(),
                sequence,
                recorded_at_ms,
                &record.kind,
                &record.payload,
            )?);
            saved.push(JournalRecord {
                session_id: session_id.clone(),
                journal_id: journal_id.clone(),
                sequence,
                recorded_at_ms,
                kind: record.kind.clone(),
                payload: record.payload.clone(),
            });
        }

        let state_location = match state_payload(&update) {
            Some(payload) => Some(self.write(
                session_id.as_str(),
                0,
                recorded_at_ms,
                SESSION_STATE,
                &payload,
            )?),
            None => None,
        };

        let tracked = state
            .sessions
            .get_mut(session_id.as_str())
            .ok_or_else(not_found)?;
        tracked.locations.extend(locations);
        tracked.row.through_sequence = expected_through + records.len() as u64;
        tracked.last_recorded_at_ms = recorded_at_ms;
        if let Some(status) = update.status {
            tracked.row.status = status;
        }
        if let Some(context) = update.context {
            tracked.row.context = context.clone();
        }
        if let Some(configuration) = update.configuration {
            tracked.row.configuration = configuration.clone();
        }
        if let Some(location) = state_location {
            tracked.state_segment = location.segment;
        }

        Ok(saved)
    }

    fn session(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError> {
        Ok(self
            .lock()?
            .sessions
            .get(session_id.as_str())
            .map(|tracked| tracked.row.clone()))
    }

    fn sessions(&self) -> Result<Vec<SessionRow>, KernelError> {
        let mut rows: Vec<SessionRow> = self
            .lock()?
            .sessions
            .values()
            .map(|tracked| tracked.row.clone())
            .collect();
        rows.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Ok(rows)
    }

    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, KernelError> {
        // Resolve locations under the lock, read outside it: a client paging through
        // history must never hold up a session that is appending.
        let (journal_id, wanted) = {
            let state = self.lock()?;
            let tracked = state
                .sessions
                .get(session_id.as_str())
                .ok_or_else(not_found)?;
            let first = after + 1;
            let mut wanted = Vec::with_capacity(limit.min(tracked.locations.len()));
            for sequence in first..first.saturating_add(limit as u64) {
                let Some(location) = tracked.locations.get(sequence as usize - 1) else {
                    break;
                };
                wanted.push((sequence, *location));
            }
            (tracked.row.journal_id.clone(), wanted)
        };

        let locations: Vec<Location> = wanted.iter().map(|(_, location)| *location).collect();
        let mut sequences = wanted.into_iter().map(|(sequence, _)| sequence);
        self.log.read_many(&locations, |frame| {
            Ok(JournalRecord {
                session_id: session_id.clone(),
                journal_id: journal_id.clone(),
                sequence: sequences.next().unwrap_or(frame.sequence),
                recorded_at_ms: frame.recorded_at_ms,
                kind: frame.kind.to_string(),
                payload: frame.payload()?,
            })
        })
    }

    fn delete_ended(&self, session_id: &SessionId) -> Result<(), KernelError> {
        let mut state = self.lock()?;
        let tracked = state
            .sessions
            .get(session_id.as_str())
            .ok_or_else(ended_first)?;
        if !matches!(tracked.row.status, SessionStatus::Ended) {
            return Err(ended_first());
        }
        self.write(
            session_id.as_str(),
            0,
            wall_clock_ms()?,
            SESSION_DELETED,
            &serde_json::json!({}),
        )?;
        state.sessions.remove(session_id.as_str());

        // The session's records were the only thing keeping its segments alive.
        let floor = reclaim_floor(&state, self.log.current_segment()?);
        drop(state);
        self.log.reclaim(floor)
    }

    fn idempotency_get(
        &self,
        scope: &str,
        key: &str,
        request: &Identity,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        match self.lock()?.idempotency.get(&(scope.into(), key.into())) {
            None => Ok(None),
            Some(stored) if stored.request != *request => Err(KernelError::InvalidState(
                "idempotency key reused with different request".into(),
            )),
            Some(stored) => Ok(Some(stored.response.clone())),
        }
    }

    fn idempotency_put(
        &self,
        scope: &str,
        key: &str,
        request: &Identity,
        response: &serde_json::Value,
    ) -> Result<(), KernelError> {
        let mut state = self.lock()?;
        let entry = (scope.to_string(), key.to_string());
        if state.idempotency.contains_key(&entry) {
            return Err(KernelError::InvalidState(
                "idempotency key already recorded".into(),
            ));
        }
        let location = self.write(
            "",
            0,
            wall_clock_ms()?,
            IDEMPOTENCY,
            &serde_json::json!({
                "scope": scope,
                "key": key,
                "request": request,
                "response": response,
            }),
        )?;
        state.idempotency.insert(
            entry,
            Stored {
                request: *request,
                response: response.clone(),
                segment: location.segment,
            },
        );
        Ok(())
    }
}

/// Only the fields an update actually changes, so a turn that touches nothing but its
/// status does not rewrite the context envelope.
fn state_payload(update: &SessionUpdate<'_>) -> Option<serde_json::Value> {
    let mut payload = serde_json::Map::new();
    if let Some(status) = &update.status {
        payload.insert("status".into(), serde_json::json!(status));
    }
    if let Some(context) = update.context {
        payload.insert("context".into(), context.clone());
    }
    if let Some(configuration) = update.configuration {
        payload.insert("configuration".into(), configuration.clone());
    }
    (!payload.is_empty()).then_some(serde_json::Value::Object(payload))
}

fn replay(state: &mut State, frame: &Frame<'_>, location: Location) -> Result<(), KernelError> {
    if frame.is_sequenced() {
        if let Some(tracked) = state.sessions.get_mut(frame.session_id) {
            tracked.locations.push(location);
            tracked.row.through_sequence = frame.sequence;
            tracked.last_recorded_at_ms = frame.recorded_at_ms;
        }
        return Ok(());
    }

    match frame.kind {
        SESSION_STATE => {
            let payload = frame.payload()?;
            let tracked = match state.sessions.entry(frame.session_id.to_string()) {
                Entry::Occupied(occupied) => occupied.into_mut(),
                Entry::Vacant(vacant) => vacant.insert(Tracked {
                    row: SessionRow {
                        session_id: SessionId::new(frame.session_id.to_string()),
                        journal_id: serde_json::from_value(payload["journal_id"].clone())
                            .map_err(json_error)?,
                        // A frame that introduces a session always carries both; a
                        // later one that only moves its status hits the arm above.
                        presentation_digest: serde_json::from_value(
                            payload["presentation_digest"].clone(),
                        )
                        .map_err(json_error)?,
                        status: SessionStatus::Idle,
                        through_sequence: 0,
                        configuration: serde_json::Value::Null,
                        context: serde_json::Value::Null,
                    },
                    locations: Vec::new(),
                    state_segment: location.segment,
                    last_recorded_at_ms: frame.recorded_at_ms,
                }),
            };
            if let Some(status) = payload.get("status") {
                tracked.row.status = serde_json::from_value(status.clone()).map_err(json_error)?;
            }
            if let Some(context) = payload.get("context") {
                tracked.row.context = context.clone();
            }
            if let Some(configuration) = payload.get("configuration") {
                tracked.row.configuration = configuration.clone();
            }
            if let Some(digest) = payload.get("presentation_digest") {
                tracked.row.presentation_digest =
                    serde_json::from_value(digest.clone()).map_err(json_error)?;
            }
            tracked.state_segment = location.segment;
        }
        SESSION_DELETED => {
            state.sessions.remove(frame.session_id);
        }
        IDEMPOTENCY => {
            let payload = frame.payload()?;
            let (Some(scope), Some(key)) = (payload["scope"].as_str(), payload["key"].as_str())
            else {
                return Err(KernelError::Journal(
                    "idempotency frame is malformed".into(),
                ));
            };
            state.idempotency.insert(
                (scope.to_string(), key.to_string()),
                Stored {
                    request: serde_json::from_value(payload["request"].clone())
                        .map_err(json_error)?,
                    response: payload["response"].clone(),
                    segment: location.segment,
                },
            );
        }
        _ => {}
    }
    Ok(())
}

/// The oldest segment any surviving state still lives in. Everything below it is dead
/// and can be unlinked whole. With nothing left alive, every closed segment goes.
fn reclaim_floor(state: &State, current_segment: u64) -> u64 {
    let sessions = state.sessions.values().map(|tracked| {
        tracked
            .locations
            .first()
            .map_or(tracked.state_segment, |location| {
                location.segment.min(tracked.state_segment)
            })
    });
    let idempotency = state.idempotency.values().map(|stored| stored.segment);
    sessions.chain(idempotency).min().unwrap_or(current_segment)
}

fn wall_clock_ms() -> Result<u64, KernelError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| KernelError::Journal("system time is before the Unix epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| KernelError::Journal("system time exceeds the journal range".into()))
}

fn not_found() -> KernelError {
    KernelError::InvalidState("session not found".into())
}

fn ended_first() -> KernelError {
    KernelError::InvalidState("session must exist and be ended before deletion".into())
}

fn json_error(error: serde_json::Error) -> KernelError {
    KernelError::Journal(error.to_string())
}
