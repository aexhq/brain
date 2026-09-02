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

use brain_protocol::{SessionId, SessionStatus, SessionSummary};

use crate::{
    Error,
    journal::{
        AppendRecord, JournalRecord, JournalStore, SessionRow, SessionUpdate,
        log::{Append, Frame, Location, SegmentLog},
    },
};

pub struct SegmentJournal {
    log: SegmentLog,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Keyed by the raw session id so replay and lookups never allocate one.
    sessions: HashMap<String, Tracked>,
}

struct Tracked {
    row: SessionRow,
    /// `locations[i]` holds the record at sequence `i + 1`.
    locations: Vec<Location>,
    last_recorded_at_ms: u64,
    /// Rebuilt from the journal rather than created by this process. Its context is empty
    /// until the agentloop is handed its own records back.
    restored: bool,
}

impl SegmentJournal {
    pub fn open(directory: &Path) -> Result<Self, Error> {
        // Both visitors need the same state and neither ever runs while the other is
        // borrowed, which the borrow checker cannot see across two closures.
        let state = RefCell::new(State::default());
        let log = SegmentLog::open(directory, |frame, location| {
            replay(&mut state.borrow_mut(), &frame, location)
        })?;
        Ok(Self {
            log,
            state: Mutex::new(state.into_inner()),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, Error> {
        self.state
            .lock()
            .map_err(|_| Error::Journal("journal state mutex poisoned".into()))
    }

    /// Append one frame and return where it landed. Held separate so the state lock is
    /// never held across an encode of a large payload more than it must be.
    fn write(
        &self,
        session_id: &str,
        sequence: u64,
        recorded_at_ms: u64,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<Location, Error> {
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
    /// Free every segment no live session's history still lives in.
    fn reclaim(&self) -> Result<(), Error> {
        let current = self.log.current_segment()?;
        let floor = session_floor(&*self.lock()?, current);
        self.log.reclaim(floor)
    }
}

impl JournalStore for SegmentJournal {
    fn create_session(
        &self,
        row: &SessionRow,
        record: AppendRecord,
    ) -> Result<JournalRecord, Error> {
        let mut state = self.lock()?;
        if state.sessions.contains_key(row.session_id.as_str()) {
            return Err(Error::InvalidState("session already exists".into()));
        }
        let recorded_at_ms = wall_clock_ms()?;

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
                restored: false,
            },
        );

        Ok(JournalRecord {
            session_id: row.session_id,
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
    ) -> Result<Vec<JournalRecord>, Error> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let mut state = self.lock()?;
        let tracked = state
            .sessions
            .get(session_id.as_str())
            .ok_or_else(not_found)?;
        if tracked.row.through_sequence != expected_through {
            return Err(Error::InvalidState(format!(
                "journal position changed: expected {expected_through}, found {}",
                tracked.row.through_sequence
            )));
        }
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
        if let Some(status) = update.status {
            tracked.row.status = status;
        }
        if let Some(context) = update.context {
            tracked.row.context = context.clone();
        }
        if let Some(configuration) = update.configuration {
            tracked.row.configuration = configuration.clone();
        }

        Ok(saved)
    }

    fn session_row(&self, session_id: &SessionId) -> Result<Option<SessionRow>, Error> {
        Ok(self
            .lock()?
            .sessions
            .get(session_id.as_str())
            .map(|tracked| tracked.row.clone()))
    }

    fn session_summary(&self, session_id: &SessionId) -> Result<Option<SessionSummary>, Error> {
        Ok(self
            .lock()?
            .sessions
            .get(session_id.as_str())
            .map(|tracked| SessionSummary::from(&tracked.row)))
    }

    fn session_summaries(&self) -> Result<Vec<SessionSummary>, Error> {
        // Summaries are built under the lock but copy only the fields a caller can see.
        // Cloning whole rows here copied every live configuration and conversation on
        // every request that listed sessions.
        let mut sessions: Vec<SessionSummary> = self
            .lock()?
            .sessions
            .values()
            .map(|tracked| SessionSummary::from(&tracked.row))
            .collect();
        sessions.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Ok(sessions)
    }

    fn records_after(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalRecord>, Error> {
        // Resolve locations under the lock, read outside it: a client paging through
        // history must never hold up a session that is appending.
        let wanted = {
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
            wanted
        };

        let locations: Vec<Location> = wanted.iter().map(|(_, location)| *location).collect();
        let mut sequences = wanted.into_iter().map(|(sequence, _)| sequence);
        self.log.read_many(&locations, |frame| {
            Ok(JournalRecord {
                session_id: session_id.clone(),
                sequence: sequences.next().unwrap_or(frame.sequence),
                recorded_at_ms: frame.recorded_at_ms,
                kind: frame.kind.to_string(),
                payload: frame.payload()?,
            })
        })
    }

    fn take_restored(&self, session_id: &SessionId) -> Result<bool, Error> {
        let mut state = self.lock()?;
        let Some(tracked) = state.sessions.get_mut(session_id.as_str()) else {
            return Ok(false);
        };
        Ok(std::mem::take(&mut tracked.restored))
    }

    fn delete_ended(&self, session_id: &SessionId) -> Result<(), Error> {
        let mut state = self.lock()?;
        let tracked = state
            .sessions
            .get(session_id.as_str())
            .ok_or_else(ended_first)?;
        if !matches!(tracked.row.status, SessionStatus::Ended) {
            return Err(ended_first());
        }
        state.sessions.remove(session_id.as_str());
        drop(state);

        // The session's records were the only thing keeping its segments alive.
        self.reclaim()
    }
}

/// Rebuilds a session from its own records.
///
/// A session survives a restart because the journal is the record of it, folded back in
/// write order. Everything a `SessionRow` needs is in here except the context, which is
/// the agentloop's and is rebuilt by handing the records back to it.
///
/// Best effort throughout. A session whose records will not parse, or whose beginning is no
/// longer on disk, is left out rather than propagated — one damaged session must not stop a
/// process starting.
fn replay(state: &mut State, frame: &Frame<'_>, location: Location) -> Result<(), Error> {
    if frame.is_sequenced() {
        // A session is admitted only at its genesis record, and only at sequence 1.
        //
        // `records_after` indexes `locations[sequence - 1]` and reports the sequence from
        // the index rather than the frame, so a session whose first surviving frame is
        // sequence 40 would answer for sequence 1 with the wrong record and say nothing.
        // If an earlier process reclaimed the segments this session began in, its history
        // is no longer whole and it does not come back.
        if frame.kind == "session_creation_started" && frame.sequence == 1 {
            let Ok(row) = restored_row(frame) else {
                return Ok(());
            };
            state.sessions.insert(
                frame.session_id.to_owned(),
                Tracked {
                    row,
                    locations: vec![location],
                    last_recorded_at_ms: frame.recorded_at_ms,
                    restored: true,
                },
            );
            return Ok(());
        }

        let Some(tracked) = state.sessions.get_mut(frame.session_id) else {
            return Ok(());
        };
        // Dense or not at all: a gap means the records between are gone, and an index that
        // silently slides every later record down by one is worse than no session.
        if frame.sequence as usize != tracked.locations.len() + 1 {
            state.sessions.remove(frame.session_id);
            return Ok(());
        }
        tracked.locations.push(location);
        tracked.row.through_sequence = frame.sequence;
        tracked.last_recorded_at_ms = frame.recorded_at_ms;
        apply_lifecycle(&mut tracked.row, frame);
    }
    Ok(())
}

/// The row a session begins with, from its genesis record.
fn restored_row(frame: &Frame<'_>) -> Result<SessionRow, Error> {
    let configuration: serde_json::Value = frame.decode()?;
    Ok(SessionRow {
        session_id: SessionId::new(frame.session_id.to_owned()),
        status: SessionStatus::Creating,
        through_sequence: frame.sequence,
        configuration,
        context: serde_json::to_value(crate::session::empty_context())
            .map_err(|error| Error::Journal(error.to_string()))?,
    })
}

/// Folds one record into the session's status and, at `session_creation_ended`, its
/// configuration.
///
/// Last write wins, which is what the records mean: both `turn_ended` and `turn_failed`
/// go through `finish_turn` and leave a session Idle, so neither implies a failed session —
/// only a failed creation does that.
fn apply_lifecycle(row: &mut SessionRow, frame: &Frame<'_>) {
    match frame.kind {
        "session_creation_ended" => {
            #[derive(serde::Deserialize)]
            struct Created {
                configuration: serde_json::Value,
            }
            if let Ok(created) = frame.decode::<Created>() {
                row.configuration = created.configuration;
                row.status = SessionStatus::Idle;
            }
        }
        "session_creation_failed" => row.status = SessionStatus::Failed,
        "turn_started" => row.status = SessionStatus::Running,
        "turn_ended" | "turn_failed" => row.status = SessionStatus::Idle,
        "session_ended" => row.status = SessionStatus::Ended,
        _ => {}
    }
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

fn wall_clock_ms() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Journal("system time is before the Unix epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Journal("system time exceeds the journal range".into()))
}

fn not_found() -> Error {
    Error::InvalidState("session not found".into())
}

fn ended_first() -> Error {
    Error::InvalidState("session must exist and be ended before deletion".into())
}
