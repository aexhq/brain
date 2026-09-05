//! One session's canonical journal on disk, and its in-memory projections.
//!
//! ```text
//! {sessions}/{session_id}/
//!     journal/*.segment
//! ```

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use brain_protocol::{SessionId, SessionStatus, SessionSummary, codes};

use crate::{
    Error,
    journal::{
        AppendRecord, CommitHandle, Feed, Folded, JournalEntry, SessionRecord, SessionRow,
        SessionStore, SessionUpdate, Writer,
        log::{Frame, Location, SegmentLog, encode_unsequenced},
        writer::Prepared,
    },
};

const JOURNAL_DIR: &str = "journal";
const MAX_EVENT_PAGE_BYTES: u64 = 8 * 1024 * 1024;

pub struct LocalSessionStore {
    session_id: SessionId,
    directory: PathBuf,
    journal: Arc<SegmentLog>,
    feed: Arc<Feed>,
    state: Arc<Mutex<State>>,
}

struct State {
    row: SessionRow,
    last_recorded_at_ms: u64,
    next_sequence: u64,
    next_recorded_at_ms: u64,
    /// `events[i]` holds the public Event at sequence `i + 1`; pure transcript appends
    /// and private state mutations leave `None` at their sequence.
    events: Vec<Option<Location>>,
    /// Transcript and Agentloop-state entries in order.
    journal: Vec<(u64, Location)>,
    transcript_len: usize,
}

impl LocalSessionStore {
    /// Creates the session directory. The caller's first append is its configuration.
    pub fn create(
        directory: &Path,
        session_id: SessionId,
        configuration: &serde_json::Value,
        writer: Arc<Writer>,
        feed: Arc<Feed>,
    ) -> Result<Arc<Self>, Error> {
        if directory.exists() {
            return Err(Error::Conflict("session already exists".into()));
        }
        fs::create_dir_all(directory).map_err(io_error)?;
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
        let owner: Arc<str> = Arc::from(session_id.as_str());
        let journal = Arc::new(SegmentLog::open(
            &directory.join(JOURNAL_DIR),
            owner,
            writer,
            |_, _| Ok(()),
        )?);
        sync_directory(directory)?;
        Ok(Arc::new(Self {
            session_id: session_id.clone(),
            directory: directory.to_path_buf(),
            journal,
            feed,
            state: Arc::new(Mutex::new(State {
                row: SessionRow {
                    session_id,
                    status: SessionStatus::Creating,
                    through_sequence: 0,
                    configuration: configuration.clone(),
                },
                last_recorded_at_ms: 0,
                next_sequence: 0,
                next_recorded_at_ms: 0,
                events: Vec::new(),
                journal: Vec::new(),
                transcript_len: 0,
            })),
        }))
    }

    /// Rebuilds every session projection from its one journal.
    ///
    /// Best effort: a torn tail is truncated, and a log whose records are not dense is
    /// refused rather than indexed with a gap that would answer for the wrong record.
    ///
    /// [`fold`]: SessionStore::fold
    pub fn open(
        directory: &Path,
        writer: Arc<Writer>,
        feed: Arc<Feed>,
    ) -> Result<Arc<Self>, Error> {
        let session_id = SessionId::new(
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::Journal("session directory has no name".into()))?,
        );
        let owner: Arc<str> = Arc::from(session_id.as_str());
        let mut state = State {
            row: SessionRow {
                session_id,
                status: SessionStatus::Creating,
                through_sequence: 0,
                configuration: serde_json::Value::Null,
            },
            last_recorded_at_ms: 0,
            next_sequence: 0,
            next_recorded_at_ms: 0,
            events: Vec::new(),
            journal: Vec::new(),
            transcript_len: 0,
        };
        let journal = SegmentLog::open(
            &directory.join(JOURNAL_DIR),
            owner,
            writer,
            |frame, location| {
                if frame.sequence != state.row.through_sequence + 1 {
                    return Err(Error::Journal(format!(
                        "session {} has a gap at sequence {}",
                        state.row.session_id, frame.sequence
                    )));
                }
                state.row.through_sequence = frame.sequence;
                state.last_recorded_at_ms = frame.recorded_at_ms;
                if journal_kind(frame.kind) {
                    let entry = frame.decode::<JournalEntry>()?;
                    let visible = transcript_replaced(&mut state.transcript_len, &entry);
                    state.events.push(visible.then_some(location));
                    state.journal.push((frame.sequence, location));
                } else {
                    state.events.push(Some(location));
                    if let Some(lifecycle) = lifecycle_of(&frame)? {
                        apply_lifecycle(&mut state.row, lifecycle);
                    }
                }
                Ok(())
            },
        )?;
        if state.row.configuration.is_null() {
            return Err(Error::Journal(format!(
                "session {} has no creation record",
                state.row.session_id
            )));
        }
        state.next_sequence = state.row.through_sequence;
        state.next_recorded_at_ms = state.last_recorded_at_ms;
        Ok(Arc::new(Self {
            session_id: state.row.session_id.clone(),
            directory: directory.to_path_buf(),
            journal: Arc::new(journal),
            feed,
            state: Arc::new(Mutex::new(state)),
        }))
    }

    /// Opens every session under `sessions`, closing any turn the previous process left
    /// running. A directory that will not open is skipped with a warning: one damaged
    /// session must not stop a process starting.
    pub fn open_all(
        sessions: &Path,
        writer: Arc<Writer>,
        feed: Arc<Feed>,
    ) -> Result<Vec<Arc<Self>>, Error> {
        fs::create_dir_all(sessions).map_err(io_error)?;
        let mut stores = Vec::new();
        let mut entries: Vec<PathBuf> = fs::read_dir(sessions)
            .map_err(io_error)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .collect();
        entries.sort();
        for directory in entries {
            match Self::open(&directory, writer.clone(), feed.clone()) {
                Ok(store) => {
                    store.interrupt_unfinished_turn()?;
                    stores.push(store);
                }
                Err(error) => {
                    tracing::warn!(
                        directory = %directory.display(),
                        %error,
                        "session directory could not be opened and is skipped"
                    );
                }
            }
        }
        Ok(stores)
    }

    /// Closes a turn the previous process did not finish.
    ///
    /// A session still `Running` after its log has been read was mid-turn when that
    /// process stopped. Whether the model call or the tool call actually happened is not
    /// knowable from here, so Brain says exactly that and returns the session to Idle
    /// rather than deciding on the client's behalf. Returns whether a turn was closed.
    pub fn interrupt_unfinished_turn(&self) -> Result<bool, Error> {
        {
            let state = self.lock()?;
            if !matches!(state.row.status, SessionStatus::Running) {
                return Ok(false);
            }
        }
        self.append_sync(
            &[AppendRecord::new(
                codes::event::TURN_FAILED,
                serde_json::to_value(
                    codes::Failure::new(
                        codes::failure::INTERRUPTED,
                        "Brain restarted while this turn was in flight; whether its effects reached the model or a tool is not recorded",
                    )
                    .ambiguous(true),
                )
                .map_err(json_error)?,
            )],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                configuration: None,
            },
        )?;
        Ok(true)
    }

    /// Removes the session from disk. Only an ended or failed session can go.
    pub fn delete(&self) -> Result<(), Error> {
        self.ensure_deletable()?;
        self.sync()?;
        fs::remove_dir_all(&self.directory).map_err(io_error)
    }

    pub fn ensure_deletable(&self) -> Result<(), Error> {
        {
            let state = self.lock()?;
            if !matches!(
                state.row.status,
                SessionStatus::Ended | SessionStatus::Failed
            ) {
                return Err(Error::InvalidState(
                    "session must be ended before deletion".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, Error> {
        self.state
            .lock()
            .map_err(|_| Error::Journal("session store mutex poisoned".into()))
    }

    fn submit_records(
        &self,
        records: Vec<AppendRecord>,
        status: Option<SessionStatus>,
        configuration: Option<serde_json::Value>,
        synchronous: bool,
    ) -> Result<CommitHandle, Error> {
        if records.is_empty() {
            return Ok(CommitHandle::ready(Vec::new()));
        }
        let encoded = records
            .iter()
            .map(|record| encode_unsequenced(&record.kind, &record.payload))
            .collect::<Result<Vec<_>, Error>>()?;
        let bytes = encoded.iter().try_fold(0_u64, |total, frame| {
            total
                .checked_add(frame.len() as u64)
                .ok_or_else(|| Error::Journal("journal append is too large".into()))
        })?;
        let state = self.state.clone();
        let journal = self.journal.clone();
        let feed = self.feed.clone();
        let session_id = self.session_id.clone();
        let result = Arc::new(Mutex::new(None));
        let committed = result.clone();
        let prepare = Box::new(move || {
            let (first_sequence, recorded_at_ms) = {
                let mut state = state
                    .lock()
                    .map_err(|_| "session store mutex poisoned".to_owned())?;
                let first_sequence = state
                    .next_sequence
                    .checked_add(1)
                    .ok_or_else(|| "journal sequence is exhausted".to_owned())?;
                state.next_sequence = state
                    .next_sequence
                    .checked_add(records.len() as u64)
                    .ok_or_else(|| "journal sequence is exhausted".to_owned())?;
                let recorded_at_ms = wall_clock_ms()
                    .map_err(|error| error.to_string())?
                    .max(state.next_recorded_at_ms);
                state.next_recorded_at_ms = recorded_at_ms;
                (first_sequence, recorded_at_ms)
            };
            let (locations, frames) = journal
                .prepare(encoded, first_sequence, recorded_at_ms)
                .map_err(|error| error.to_string())?;
            let saved = records
                .into_iter()
                .enumerate()
                .map(|(offset, record)| SessionRecord {
                    session_id: session_id.clone(),
                    sequence: first_sequence + offset as u64,
                    recorded_at_ms,
                    kind: record.kind,
                    payload: record.payload,
                })
                .collect::<Vec<_>>();
            Ok(Prepared {
                frames,
                complete: Box::new(move || {
                    let mut state = state
                        .lock()
                        .map_err(|_| "session store mutex poisoned".to_owned())?;
                    if state.row.through_sequence + 1 != first_sequence {
                        return Err(format!(
                            "journal commit order changed: expected {}, found {first_sequence}",
                            state.row.through_sequence + 1
                        ));
                    }
                    for location in locations {
                        state.events.push(Some(location));
                    }
                    state.row.through_sequence += saved.len() as u64;
                    state.last_recorded_at_ms = recorded_at_ms;
                    if let Some(status) = status {
                        state.row.status = status;
                    }
                    if let Some(configuration) = configuration {
                        state.row.configuration = configuration;
                    }
                    drop(state);
                    for record in &saved {
                        feed.publish(record);
                    }
                    *committed
                        .lock()
                        .map_err(|_| "journal commit result poisoned".to_owned())? = Some(saved);
                    Ok(())
                }),
            })
        });
        let ticket = if synchronous {
            self.journal
                .writer()
                .submit_sync(self.journal.owner(), bytes, prepare)?
        } else {
            self.journal
                .writer()
                .submit_async(self.journal.owner(), bytes, prepare)?
        };
        Ok(CommitHandle::new(ticket, result))
    }

    fn append_journal_records(&self, entries: Vec<JournalEntry>) -> Result<u64, Error> {
        if entries.is_empty() {
            return Ok(self.lock()?.row.through_sequence);
        }
        let encoded = entries
            .iter()
            .map(|entry| {
                let kind = match entry {
                    JournalEntry::TranscriptDelta { .. } => "transcript_delta",
                    JournalEntry::StateSet { .. } => "state_set",
                };
                let payload = serde_json::to_value(entry).map_err(json_error)?;
                encode_unsequenced(kind, &payload)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let bytes = encoded.iter().try_fold(0_u64, |total, frame| {
            total
                .checked_add(frame.len() as u64)
                .ok_or_else(|| Error::Journal("journal append is too large".into()))
        })?;
        let state = self.state.clone();
        let journal = self.journal.clone();
        let feed = self.feed.clone();
        let session_id = self.session_id.clone();
        let result = Arc::new(Mutex::new(None));
        let committed = result.clone();
        let count = entries.len() as u64;
        let prepare = Box::new(move || {
            let (first_sequence, recorded_at_ms) = {
                let mut state = state
                    .lock()
                    .map_err(|_| "session store mutex poisoned".to_owned())?;
                let first_sequence = state
                    .next_sequence
                    .checked_add(1)
                    .ok_or_else(|| "journal sequence is exhausted".to_owned())?;
                state.next_sequence = state
                    .next_sequence
                    .checked_add(count)
                    .ok_or_else(|| "journal sequence is exhausted".to_owned())?;
                let recorded_at_ms = wall_clock_ms()
                    .map_err(|error| error.to_string())?
                    .max(state.next_recorded_at_ms);
                state.next_recorded_at_ms = recorded_at_ms;
                (first_sequence, recorded_at_ms)
            };
            let (locations, frames) = journal
                .prepare(encoded, first_sequence, recorded_at_ms)
                .map_err(|error| error.to_string())?;
            Ok(Prepared {
                frames,
                complete: Box::new(move || {
                    let mut state = state
                        .lock()
                        .map_err(|_| "session store mutex poisoned".to_owned())?;
                    if state.row.through_sequence + 1 != first_sequence {
                        return Err(format!(
                            "journal commit order changed: expected {}, found {first_sequence}",
                            state.row.through_sequence + 1
                        ));
                    }
                    let mut projected = Vec::new();
                    for (offset, (location, entry)) in
                        locations.into_iter().zip(entries).enumerate()
                    {
                        let sequence = first_sequence + offset as u64;
                        let visible = transcript_replaced(&mut state.transcript_len, &entry);
                        state.events.push(visible.then_some(location));
                        state.journal.push((sequence, location));
                        if visible {
                            projected.push(project_transcript_replacement(
                                session_id.clone(),
                                sequence,
                                recorded_at_ms,
                                entry,
                            ));
                        }
                    }
                    state.row.through_sequence += count;
                    state.last_recorded_at_ms = recorded_at_ms;
                    let through_sequence = state.row.through_sequence;
                    drop(state);
                    for record in &projected {
                        feed.publish(record);
                    }
                    *committed
                        .lock()
                        .map_err(|_| "journal commit result poisoned".to_owned())? =
                        Some(through_sequence);
                    Ok(())
                }),
            })
        });
        let ticket = self
            .journal
            .writer()
            .submit_sync(self.journal.owner(), bytes, prepare)?;
        ticket.wait()?;
        result
            .lock()
            .map_err(|_| Error::Journal("journal commit result poisoned".into()))?
            .take()
            .ok_or_else(|| Error::Journal("journal commit produced no result".into()))
    }
}

/// What a lifecycle record does to the row when folded back.
enum Lifecycle {
    Creating(serde_json::Value),
    Created(serde_json::Value),
    Status(SessionStatus),
}

fn lifecycle_of(frame: &Frame<'_>) -> Result<Option<Lifecycle>, Error> {
    Ok(match frame.kind {
        kind if kind == codes::event::SESSION_CREATION_STARTED => {
            Some(Lifecycle::Creating(frame.payload()?))
        }
        kind if kind == codes::event::SESSION_CREATION_ENDED => {
            #[derive(serde::Deserialize)]
            struct Created {
                configuration: serde_json::Value,
            }
            frame
                .decode::<Created>()
                .ok()
                .map(|created| Lifecycle::Created(created.configuration))
        }
        kind if kind == codes::event::SESSION_CREATION_FAILED => {
            Some(Lifecycle::Status(SessionStatus::Failed))
        }
        kind if kind == codes::event::TURN_STARTED => {
            Some(Lifecycle::Status(SessionStatus::Running))
        }
        kind if kind == codes::event::TURN_ENDED || kind == codes::event::TURN_FAILED => {
            Some(Lifecycle::Status(SessionStatus::Idle))
        }
        kind if kind == codes::event::SESSION_ENDED => {
            Some(Lifecycle::Status(SessionStatus::Ended))
        }
        _ => None,
    })
}

/// Last write wins, which is what the records mean: both `turn_ended` and `turn_failed`
/// leave a session Idle, so neither implies a failed session — only a failed creation
/// does that.
fn apply_lifecycle(row: &mut SessionRow, lifecycle: Lifecycle) {
    match lifecycle {
        Lifecycle::Creating(configuration) => {
            row.configuration = configuration;
            row.status = SessionStatus::Creating;
        }
        Lifecycle::Created(configuration) => {
            row.configuration = configuration;
            row.status = SessionStatus::Idle;
        }
        Lifecycle::Status(status) => row.status = status,
    }
}

fn journal_kind(kind: &str) -> bool {
    matches!(kind, "transcript_delta" | "state_set")
}

impl SessionStore for LocalSessionStore {
    fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    fn append_sync(
        &self,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<SessionRecord>, Error> {
        self.submit_records(
            records.to_vec(),
            update.status,
            update.configuration.cloned(),
            true,
        )?
        .wait()
    }

    fn append_async(&self, records: Vec<AppendRecord>) -> Result<CommitHandle, Error> {
        self.submit_records(records, None, None, false)
    }

    fn append_journal_sync(&self, entries: &[JournalEntry]) -> Result<u64, Error> {
        self.append_journal_records(entries.to_vec())
    }

    fn fold(&self) -> Result<Folded, Error> {
        let (locations, through) = {
            let state = self.lock()?;
            (
                state
                    .journal
                    .iter()
                    .map(|(_, location)| *location)
                    .collect::<Vec<_>>(),
                state.row.through_sequence,
            )
        };
        let mut folded = Folded {
            transcript: Vec::new(),
            slots: BTreeMap::new(),
            through_sequence: through,
        };
        for entry in self
            .journal
            .read_many(&locations, |frame| frame.decode::<JournalEntry>())?
        {
            folded.apply(entry);
        }
        Ok(folded)
    }

    fn session_row(&self) -> Result<SessionRow, Error> {
        Ok(self.lock()?.row.clone())
    }

    fn session_summary(&self) -> Result<SessionSummary, Error> {
        Ok(SessionSummary::from(&self.lock()?.row))
    }

    fn records_after(&self, after: u64, limit: usize) -> Result<Vec<SessionRecord>, Error> {
        // Resolve locations under the lock, read outside it: a client paging through
        // history must never hold up a session that is appending.
        let (session_id, wanted) = {
            let state = self.lock()?;
            let mut wanted = Vec::with_capacity(limit.min(state.events.len()));
            let mut bytes = 0_u64;
            let mut sequence = after + 1;
            while wanted.len() < limit {
                let Some(slot) = state.events.get(sequence as usize - 1) else {
                    break;
                };
                if let Some(location) = slot {
                    let next = bytes.saturating_add(u64::from(location.length));
                    if !wanted.is_empty() && next > MAX_EVENT_PAGE_BYTES {
                        break;
                    }
                    bytes = next;
                    wanted.push((sequence, *location));
                }
                sequence += 1;
            }
            (state.row.session_id.clone(), wanted)
        };
        let locations: Vec<Location> = wanted.iter().map(|(_, location)| *location).collect();
        let mut sequences = wanted.into_iter().map(|(sequence, _)| sequence);
        self.journal.read_many(&locations, |frame| {
            let (kind, payload) = if frame.kind == "transcript_delta" {
                let entry = frame.decode::<JournalEntry>()?;
                let JournalEntry::TranscriptDelta { keep, append } = entry else {
                    return Err(Error::Journal(
                        "transcript_delta frame has the wrong payload".into(),
                    ));
                };
                (
                    codes::event::TRANSCRIPT_REPLACED.to_owned(),
                    serde_json::json!({"keep": keep, "append": append}),
                )
            } else {
                (frame.kind.to_owned(), frame.payload()?)
            };
            Ok(SessionRecord {
                session_id: session_id.clone(),
                sequence: sequences.next().unwrap_or(frame.sequence),
                recorded_at_ms: frame.recorded_at_ms,
                kind,
                payload,
            })
        })
    }

    fn sync(&self) -> Result<(), Error> {
        self.journal.writer().sync()
    }
}

fn transcript_replaced(transcript_len: &mut usize, entry: &JournalEntry) -> bool {
    let JournalEntry::TranscriptDelta { keep, append } = entry else {
        return false;
    };
    let keep = usize::try_from(*keep)
        .unwrap_or(usize::MAX)
        .min(*transcript_len);
    let replaced = keep < *transcript_len;
    *transcript_len = keep.saturating_add(append.len());
    replaced
}

fn project_transcript_replacement(
    session_id: SessionId,
    sequence: u64,
    recorded_at_ms: u64,
    entry: JournalEntry,
) -> SessionRecord {
    let JournalEntry::TranscriptDelta { keep, append } = entry else {
        unreachable!("only transcript replacements are projected")
    };
    SessionRecord {
        session_id,
        sequence,
        recorded_at_ms,
        kind: codes::event::TRANSCRIPT_REPLACED.into(),
        payload: serde_json::json!({"keep": keep, "append": append}),
    }
}

fn wall_clock_ms() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Journal("system time is before the Unix epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Journal("system time exceeds the journal range".into()))
}

fn io_error(error: std::io::Error) -> Error {
    Error::Journal(error.to_string())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), Error> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), Error> {
    Ok(())
}

fn json_error(error: serde_json::Error) -> Error {
    Error::Journal(error.to_string())
}
