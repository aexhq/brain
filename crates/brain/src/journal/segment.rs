//! The journal: session state in memory, records appended to a segment log.
//!
//! Every read a running session makes is served from memory. The log is written
//! behind the caller and is only read to rebuild this state at startup, or to page a
//! client back through records it has not seen. Appending costs a serialise and a
//! channel send; no journal call waits on the disk.
//!
//! A session's own state — its status, its sealed configuration, its context — is not a
//! record. It is rewritten at the end of every turn and only its latest value is ever
//! read, so it lives in a file per session that the log rewrites in place. Appending it
//! instead wrote the sum of every context the session ever had: a 64-turn session
//! holding a 1 MiB context left 34 MB of journal, all of it read back at every restart,
//! and nothing bounds the turn count.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use brain_protocol::{Identity, JournalId, Session, SessionId, SessionStatus};

use crate::{
    KernelError,
    journal::{
        AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate,
        log::{Append, Frame, Location, SegmentLog},
    },
};

/// The one frame kind the journal writes for itself. It carries no sequence number, so
/// it never reaches a client's event stream, and the `$` prefix keeps it out of the
/// namespace a session's record kinds are drawn from.
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
    last_recorded_at_ms: u64,
}

struct Stored {
    request: Identity,
    response: serde_json::Value,
    segment: u64,
}

impl SegmentJournal {
    pub fn open(directory: &Path) -> Result<Self, KernelError> {
        // Both visitors need the same state and neither ever runs while the other is
        // borrowed, which the borrow checker cannot see across two closures.
        let state = RefCell::new(State::default());
        let log = SegmentLog::open(
            directory,
            |session_id, bytes| restore_session(&mut state.borrow_mut(), session_id, bytes),
            |frame, location| replay(&mut state.borrow_mut(), &frame, location),
        )?;
        Ok(Self {
            log,
            state: Mutex::new(state.into_inner()),
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
        self.log
            .write_state(row.session_id.as_str(), &state_payload(row))?;
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

        let tracked = state
            .sessions
            .get_mut(session_id.as_str())
            .ok_or_else(not_found)?;
        tracked.locations.extend(locations);
        tracked.row.through_sequence = expected_through + records.len() as u64;
        tracked.last_recorded_at_ms = recorded_at_ms;
        let changed =
            update.status.is_some() || update.context.is_some() || update.configuration.is_some();
        if let Some(status) = update.status {
            tracked.row.status = status;
        }
        if let Some(context) = update.context {
            tracked.row.context = context.clone();
        }
        if let Some(configuration) = update.configuration {
            tracked.row.configuration = configuration.clone();
        }
        // Rewritten only when the update moved something. A turn whose records change
        // nothing about the session — a batch of intents mid-turn — leaves the state
        // file alone, so the context is written once per turn and not once per commit.
        if changed {
            let payload = state_payload(&tracked.row);
            let session = session_id.as_str().to_owned();
            drop(state);
            self.log.write_state(&session, &payload)?;
        }

        Ok(saved)
    }

    fn session_row(&self, session_id: &SessionId) -> Result<Option<SessionRow>, KernelError> {
        Ok(self
            .lock()?
            .sessions
            .get(session_id.as_str())
            .map(|tracked| tracked.row.clone()))
    }

    fn session_summary(&self, session_id: &SessionId) -> Result<Option<Session>, KernelError> {
        Ok(self
            .lock()?
            .sessions
            .get(session_id.as_str())
            .map(|tracked| Session::from(&tracked.row)))
    }

    fn session_summaries(&self) -> Result<Vec<Session>, KernelError> {
        // Summaries are built under the lock but copy only the five fields a caller can
        // see. Cloning whole rows here copied every live configuration and conversation
        // on every request that listed sessions.
        let mut sessions: Vec<Session> = self
            .lock()?
            .sessions
            .values()
            .map(|tracked| Session::from(&tracked.row))
            .collect();
        sessions.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Ok(sessions)
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
        self.log.remove_state(session_id.as_str())?;
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

/// The whole state, because the file it goes to replaces its predecessor rather than
/// following it. `through_sequence` is deliberately absent: it is what the records on
/// disk say it is, and deriving it there keeps the two from disagreeing.
fn state_payload(row: &SessionRow) -> serde_json::Value {
    serde_json::json!({
        "journal_id": row.journal_id,
        "status": row.status,
        "configuration": row.configuration,
        "context": row.context,
        "presentation_identity": row.presentation_identity,
    })
}

/// Rebuild one session from its state file. Called for every session before any record
/// is replayed, so a record always finds the session it belongs to.
fn restore_session(state: &mut State, session_id: &str, bytes: &[u8]) -> Result<(), KernelError> {
    let payload: StateFile = serde_json::from_slice(bytes)
        .map_err(|error| KernelError::Journal(format!("session state is unreadable: {error}")))?;
    state.sessions.insert(
        session_id.to_owned(),
        Tracked {
            row: SessionRow {
                session_id: SessionId::new(session_id.to_owned()),
                journal_id: payload.journal_id,
                presentation_identity: payload.presentation_identity,
                status: payload.status,
                through_sequence: 0,
                configuration: payload.configuration,
                context: payload.context,
            },
            locations: Vec::new(),
            last_recorded_at_ms: 0,
        },
    );
    Ok(())
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

    if frame.kind == IDEMPOTENCY {
        let payload: IdempotencyFrame = frame.decode()?;
        state.idempotency.insert(
            (payload.scope, payload.key),
            Stored {
                request: payload.request,
                response: payload.response,
                segment: location.segment,
            },
        );
    }
    Ok(())
}

/// A session's state file. Every field is required: the file is the whole state, and a
/// file that cannot supply one of them describes no session that can be resumed.
#[derive(serde::Deserialize)]
struct StateFile {
    journal_id: JournalId,
    presentation_identity: Identity,
    status: SessionStatus,
    context: serde_json::Value,
    configuration: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct IdempotencyFrame {
    scope: String,
    key: String,
    request: Identity,
    response: serde_json::Value,
}

/// The oldest segment any surviving state still lives in. Everything below it is dead
/// and can be unlinked whole. With nothing left alive, every closed segment goes.
fn reclaim_floor(state: &State, current_segment: u64) -> u64 {
    let sessions = state
        .sessions
        .values()
        .filter_map(|tracked| tracked.locations.first().map(|location| location.segment));
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
