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
    time::{Duration, SystemTime, UNIX_EPOCH},
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

/// How long a recorded answer is kept, and so how long a redelivery of the same request
/// is answered from the record rather than executed again.
///
/// A window is unavoidable: a record that never expires is a record that pins the
/// segment it was written in forever, and Brain measured exactly that — one entry from
/// a deleted session held back every reclaimable segment, and a million entries held
/// 2 GiB of memory across a restart. What the window buys is that the cost is bounded
/// by traffic within it rather than by traffic since the process was first started.
///
/// A day is far longer than any retry a client makes and far shorter than forever.
pub const DEFAULT_IDEMPOTENCY_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// Below this an eviction sweep costs more than the entries it would find.
const MIN_IDEMPOTENCY_SWEEP: usize = 64;

pub struct SegmentJournal {
    log: SegmentLog,
    state: Mutex<State>,
    idempotency_retention: Duration,
}

#[derive(Default)]
struct State {
    /// Keyed by the raw session id so replay and lookups never allocate one.
    sessions: HashMap<String, Tracked>,
    idempotency: HashMap<(String, String), Stored>,
    /// Sweep expired records once the table has doubled since the last sweep, so the
    /// total cost of sweeping stays linear in the number of records written.
    idempotency_sweep_at: usize,
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
    expires_at_ms: u64,
}

impl SegmentJournal {
    /// `idempotency_retention` is how long a recorded answer stays answerable. See
    /// [`DEFAULT_IDEMPOTENCY_RETENTION`] for why it is finite.
    pub fn open(directory: &Path, idempotency_retention: Duration) -> Result<Self, KernelError> {
        // Both visitors need the same state and neither ever runs while the other is
        // borrowed, which the borrow checker cannot see across two closures.
        let state = RefCell::new(State::default());
        let log = SegmentLog::open(
            directory,
            |session_id, bytes| restore_session(&mut state.borrow_mut(), session_id, bytes),
            |frame, location| replay(&mut state.borrow_mut(), &frame, location),
        )?;
        let mut state = state.into_inner();
        state.idempotency_sweep_at = state.idempotency.len().max(MIN_IDEMPOTENCY_SWEEP);
        Ok(Self {
            log,
            state: Mutex::new(state),
            idempotency_retention,
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

impl SegmentJournal {
    /// Free every segment nothing live still needs.
    ///
    /// Sessions pin the segment their oldest record is in, and that is inherent: those
    /// records are the history a client can still page through. Idempotency records are
    /// not — they are small, they are not addressable, and one of them sitting in an old
    /// segment used to hold back every reclaimable segment behind it. Expired ones are
    /// dropped, and live ones below the floor are appended again at the head so the
    /// segment they were in can go.
    fn reclaim(&self) -> Result<(), KernelError> {
        let now = wall_clock_ms()?;
        let current = self.log.current_segment()?;
        let stale = {
            let mut state = self.lock()?;
            state
                .idempotency
                .retain(|_, stored| stored.expires_at_ms > now);
            let floor = session_floor(&state, current);
            state
                .idempotency
                .iter()
                .filter(|(_, stored)| stored.segment < floor)
                .map(|((scope, key), stored)| {
                    (
                        scope.clone(),
                        key.clone(),
                        stored.request,
                        stored.response.clone(),
                        stored.expires_at_ms,
                    )
                })
                .collect::<Vec<_>>()
        };

        for (scope, key, request, response, expires_at_ms) in stale {
            let location = self.write(
                "",
                0,
                now,
                IDEMPOTENCY,
                &serde_json::json!({
                    "scope": scope,
                    "key": key,
                    "request": request,
                    "response": response,
                    "expires_at_ms": expires_at_ms,
                }),
            )?;
            if let Some(stored) = self.lock()?.idempotency.get_mut(&(scope, key)) {
                stored.segment = location.segment;
            }
        }

        let floor = {
            let state = self.lock()?;
            reclaim_floor(&state, current)
        };
        self.log.reclaim(floor)
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
        drop(state);

        // The session's records were the only thing keeping its segments alive.
        self.reclaim()
    }

    fn idempotency_get(
        &self,
        scope: &str,
        key: &str,
        request: &Identity,
    ) -> Result<Option<serde_json::Value>, KernelError> {
        let now = wall_clock_ms()?;
        let mut state = self.lock()?;
        let entry = (scope.to_string(), key.to_string());
        match state.idempotency.get(&entry) {
            None => Ok(None),
            // Past its retention it is not a record any more. Dropped here rather than
            // returned, so the caller executes the request instead of replaying an
            // answer Brain has stopped promising to keep.
            Some(stored) if stored.expires_at_ms <= now => {
                state.idempotency.remove(&entry);
                Ok(None)
            }
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
        let recorded_at_ms = wall_clock_ms()?;
        let expires_at_ms = recorded_at_ms.saturating_add(
            u64::try_from(self.idempotency_retention.as_millis()).unwrap_or(u64::MAX),
        );
        let mut state = self.lock()?;
        let entry = (scope.to_string(), key.to_string());
        if state
            .idempotency
            .get(&entry)
            .is_some_and(|stored| stored.expires_at_ms > recorded_at_ms)
        {
            return Err(KernelError::InvalidState(
                "idempotency key already recorded".into(),
            ));
        }
        if state.idempotency.len() >= state.idempotency_sweep_at {
            state
                .idempotency
                .retain(|_, stored| stored.expires_at_ms > recorded_at_ms);
            state.idempotency_sweep_at = state
                .idempotency
                .len()
                .saturating_mul(2)
                .max(MIN_IDEMPOTENCY_SWEEP);
        }
        let location = self.write(
            "",
            0,
            recorded_at_ms,
            IDEMPOTENCY,
            &serde_json::json!({
                "scope": scope,
                "key": key,
                "request": request,
                "response": response,
                "expires_at_ms": expires_at_ms,
            }),
        )?;
        state.idempotency.insert(
            entry,
            Stored {
                request: *request,
                response: response.clone(),
                segment: location.segment,
                expires_at_ms,
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
                // A frame written before expiry was recorded describes a record from a
                // journal that kept them forever. Treating it as already expired is what
                // the retention now says, and it is what lets its segment be reclaimed.
                expires_at_ms: payload.expires_at_ms.unwrap_or(0),
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
    expires_at_ms: Option<u64>,
}

/// The oldest segment any surviving state still lives in. Everything below it is dead
/// and can be unlinked whole. With nothing left alive, every closed segment goes.
fn reclaim_floor(state: &State, current_segment: u64) -> u64 {
    let idempotency = state.idempotency.values().map(|stored| stored.segment);
    session_floor(state, current_segment).min(idempotency.min().unwrap_or(u64::MAX))
}

/// The oldest segment a live session's history still lives in.
fn session_floor(state: &State, current_segment: u64) -> u64 {
    state
        .sessions
        .values()
        .filter_map(|tracked| tracked.locations.first().map(|location| location.segment))
        .min()
        .unwrap_or(current_segment)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::AppendRecord;

    fn temporary() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "brain-segment-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn row() -> SessionRow {
        SessionRow {
            session_id: SessionId::new("ses_0000".to_owned()),
            journal_id: JournalId::new("jrn_0000".to_owned()),
            status: SessionStatus::Idle,
            through_sequence: 1,
            configuration: serde_json::json!({}),
            context: serde_json::json!({}),
            presentation_identity: Identity::of_bytes(b"presentation"),
        }
    }

    /// The floor is what decides which segments can be unlinked. A record that is not a
    /// session's history has no business holding it down.
    #[test]
    fn an_expired_record_leaves_the_floor_to_the_sessions() {
        let mut state = State::default();
        state.idempotency.insert(
            ("scope".to_owned(), "key".to_owned()),
            Stored {
                request: Identity::of_bytes(b"request"),
                response: serde_json::json!({}),
                segment: 0,
                expires_at_ms: 0,
            },
        );
        assert_eq!(
            reclaim_floor(&state, 9),
            0,
            "a record still in the table is still at the floor"
        );

        state
            .idempotency
            .retain(|_, stored| stored.expires_at_ms > 1);
        assert_eq!(
            reclaim_floor(&state, 9),
            9,
            "once swept, nothing holds the floor below the open segment"
        );
    }

    /// A live record cannot be dropped, so reclamation writes it again at the head and
    /// leaves the old copy behind to be unlinked with its segment. Without this it holds
    /// that segment for its whole retention.
    ///
    /// The move itself is only visible once the log has rotated, which costs 64 MiB to
    /// reach, so what is asserted here is that the record was written a second time and
    /// is still answerable.
    #[test]
    fn a_live_record_below_the_floor_is_written_again_at_the_head() {
        let directory = temporary();
        let journal = SegmentJournal::open(&directory, Duration::from_secs(3_600)).unwrap();
        let request = Identity::of_bytes(b"request");
        journal
            .idempotency_put("scope", "key", &request, &serde_json::json!({ "ok": true }))
            .unwrap();

        // A session whose history starts above the record, which is what an old segment
        // means once the log has rotated past it.
        journal.lock().unwrap().sessions.insert(
            "ses_pin".to_owned(),
            Tracked {
                row: row(),
                locations: vec![Location {
                    segment: 1,
                    offset: 0,
                    length: 1,
                }],
                last_recorded_at_ms: 0,
            },
        );

        journal.reclaim().unwrap();
        assert_eq!(
            journal.idempotency_get("scope", "key", &request).unwrap(),
            Some(serde_json::json!({ "ok": true }))
        );
        drop(journal);

        let mut records = 0;
        let log = SegmentLog::open(
            &directory,
            |_, _| Ok(()),
            |frame, _| {
                if frame.kind == IDEMPOTENCY {
                    records += 1;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            records, 2,
            "the record below the floor must be written again rather than pinning its segment"
        );
        drop(log);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Session state is the one thing that is never a record, so a session that has
    /// written nothing since it was created must still come back.
    #[test]
    fn a_session_comes_back_from_its_state_file() {
        let directory = temporary();
        let journal = SegmentJournal::open(&directory, DEFAULT_IDEMPOTENCY_RETENTION).unwrap();
        let mut created = row();
        created.context = serde_json::json!({ "items": ["one", "two"] });
        journal
            .create_session(
                &created,
                AppendRecord::new("session_created", serde_json::json!({})),
            )
            .unwrap();
        drop(journal);

        let journal = SegmentJournal::open(&directory, DEFAULT_IDEMPOTENCY_RETENTION).unwrap();
        let restored = journal
            .session_row(&created.session_id)
            .unwrap()
            .expect("the session was created above");
        assert_eq!(restored.context, created.context);
        assert_eq!(restored.journal_id, created.journal_id);
        assert_eq!(
            restored.through_sequence, 1,
            "the sequence comes from the records on disk, not from the state file"
        );
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
