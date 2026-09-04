//! One session's directory on disk, and the index of it in memory.
//!
//! ```text
//! {sessions}/{session_id}/
//!     config.json        the configuration the session was created with, written once
//!     events/*.segment   effect and lifecycle records: what clients page and subscribe to
//!     journal/*.segment  transcript deltas, slot writes and checkpoints: what the session
//!                        folds its own state out of
//! ```
//!
//! Both logs are numbered by the session's one sequence counter, so the order between a
//! delta and the effect that followed it survives a restart. Every read a running session
//! makes is served from the index; the logs are read to rebuild it at open, to page a
//! client back through records it has not seen, and to fold the transcript.

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
        AppendRecord, Feed, Folded, JournalEntry, JournalRecord, JournalStore, SessionRow,
        SessionUpdate, Writer,
        log::{Append, Frame, Location, SegmentLog},
    },
};

const CONFIG_FILE: &str = "config.json";
const EVENTS_DIR: &str = "events";
const JOURNAL_DIR: &str = "journal";

pub struct SessionStore {
    session_id: SessionId,
    directory: PathBuf,
    events: SegmentLog,
    journal: SegmentLog,
    feed: Arc<Feed>,
    state: Mutex<State>,
}

struct State {
    row: SessionRow,
    last_recorded_at_ms: u64,
    /// `events[i]` holds the record at sequence `i + 1` when that sequence went to the
    /// events log; `None` when it went to the journal.
    events: Vec<Option<Location>>,
    /// Journal entries in order, each with the sequence it was appended under.
    journal: Vec<(u64, Location)>,
    /// Index into `journal` of the last checkpoint, if any.
    last_checkpoint: Option<usize>,
    bytes_since_checkpoint: u64,
}

impl SessionStore {
    /// Creates the session's directory and writes its configuration. The caller appends
    /// the genesis record.
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
        let bytes = serde_json::to_vec(configuration).map_err(json_error)?;
        fs::write(directory.join(CONFIG_FILE), bytes).map_err(io_error)?;
        let owner: Arc<str> = Arc::from(session_id.as_str());
        let events = SegmentLog::open(
            &directory.join(EVENTS_DIR),
            owner.clone(),
            writer.clone(),
            |_, _| Ok(()),
        )?;
        let journal = SegmentLog::open(&directory.join(JOURNAL_DIR), owner, writer, |_, _| Ok(()))?;
        Ok(Arc::new(Self {
            session_id: session_id.clone(),
            directory: directory.to_path_buf(),
            events,
            journal,
            feed,
            state: Mutex::new(State {
                row: SessionRow {
                    session_id,
                    status: SessionStatus::Creating,
                    through_sequence: 0,
                    configuration: configuration.clone(),
                },
                last_recorded_at_ms: 0,
                events: Vec::new(),
                journal: Vec::new(),
                last_checkpoint: None,
                bytes_since_checkpoint: 0,
            }),
        }))
    }

    /// Rebuilds a session from its directory. Its status and configuration fold out of
    /// the events log; its transcript and slots are read on demand with [`fold`].
    ///
    /// Best effort: a torn tail is truncated, and a log whose records are not dense is
    /// refused rather than indexed with a gap that would answer for the wrong record.
    ///
    /// [`fold`]: JournalStore::fold
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
        let configuration: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join(CONFIG_FILE)).map_err(io_error)?)
                .map_err(json_error)?;
        let owner: Arc<str> = Arc::from(session_id.as_str());
        let mut state = State {
            row: SessionRow {
                session_id,
                status: SessionStatus::Creating,
                through_sequence: 0,
                configuration,
            },
            last_recorded_at_ms: 0,
            events: Vec::new(),
            journal: Vec::new(),
            last_checkpoint: None,
            bytes_since_checkpoint: 0,
        };
        // Both logs share the counter, so both are read before either is indexed: a
        // sequence is dense across the two together, not within each.
        let mut frames: Vec<(u64, u64, Slot)> = Vec::new();
        let events = SegmentLog::open(
            &directory.join(EVENTS_DIR),
            owner.clone(),
            writer.clone(),
            |frame, location| {
                frames.push((
                    frame.sequence,
                    frame.recorded_at_ms,
                    Slot::Event {
                        location,
                        lifecycle: lifecycle_of(&frame)?,
                    },
                ));
                Ok(())
            },
        )?;
        let journal = SegmentLog::open(
            &directory.join(JOURNAL_DIR),
            owner,
            writer,
            |frame, location| {
                frames.push((
                    frame.sequence,
                    frame.recorded_at_ms,
                    Slot::Journal {
                        location,
                        checkpoint: frame.kind == "checkpoint",
                        bytes: frame.payload_len() as u64,
                    },
                ));
                Ok(())
            },
        )?;
        frames.sort_by_key(|(sequence, _, _)| *sequence);
        for (sequence, recorded_at_ms, slot) in frames {
            if sequence != state.row.through_sequence + 1 {
                return Err(Error::Journal(format!(
                    "session {} has a gap at sequence {sequence}",
                    state.row.session_id
                )));
            }
            state.row.through_sequence = sequence;
            state.last_recorded_at_ms = recorded_at_ms;
            match slot {
                Slot::Event {
                    location,
                    lifecycle,
                } => {
                    state.events.push(Some(location));
                    if let Some(lifecycle) = lifecycle {
                        apply_lifecycle(&mut state.row, lifecycle);
                    }
                }
                Slot::Journal {
                    location,
                    checkpoint,
                    bytes,
                } => {
                    state.events.push(None);
                    state.journal.push((sequence, location));
                    if checkpoint {
                        state.last_checkpoint = Some(state.journal.len() - 1);
                        state.bytes_since_checkpoint = 0;
                    } else {
                        state.bytes_since_checkpoint += bytes;
                    }
                }
            }
        }
        Ok(Arc::new(Self {
            session_id: state.row.session_id.clone(),
            directory: directory.to_path_buf(),
            events,
            journal,
            feed,
            state: Mutex::new(state),
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
        let through = {
            let state = self.lock()?;
            if !matches!(state.row.status, SessionStatus::Running) {
                return Ok(false);
            }
            state.row.through_sequence
        };
        self.append(
            through,
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
        self.sync()?;
        fs::remove_dir_all(&self.directory).map_err(io_error)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, Error> {
        self.state
            .lock()
            .map_err(|_| Error::Journal("session store mutex poisoned".into()))
    }
}

enum Slot {
    Event {
        location: Location,
        lifecycle: Option<Lifecycle>,
    },
    Journal {
        location: Location,
        checkpoint: bool,
        bytes: u64,
    },
}

/// What a lifecycle record does to the row when folded back.
enum Lifecycle {
    Created(serde_json::Value),
    Status(SessionStatus),
}

fn lifecycle_of(frame: &Frame<'_>) -> Result<Option<Lifecycle>, Error> {
    Ok(match frame.kind {
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
        Lifecycle::Created(configuration) => {
            row.configuration = configuration;
            row.status = SessionStatus::Idle;
        }
        Lifecycle::Status(status) => row.status = status,
    }
}

impl JournalStore for SessionStore {
    fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    fn append(
        &self,
        expected_through: u64,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, Error> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let mut state = self.lock()?;
        if state.row.through_sequence != expected_through {
            return Err(Error::Conflict(format!(
                "journal position changed: expected {expected_through}, found {}",
                state.row.through_sequence
            )));
        }
        // Recorded time never goes backwards within a session, whatever the wall clock does.
        let recorded_at_ms = wall_clock_ms()?.max(state.last_recorded_at_ms);
        let session_id = state.row.session_id.clone();

        let mut saved = Vec::with_capacity(records.len());
        for (offset, record) in records.iter().enumerate() {
            let sequence = expected_through + offset as u64 + 1;
            let location = self.events.append(Append {
                sequence,
                recorded_at_ms,
                kind: &record.kind,
                payload: &record.payload,
            })?;
            state.events.push(Some(location));
            saved.push(JournalRecord {
                session_id: session_id.clone(),
                sequence,
                recorded_at_ms,
                kind: record.kind.clone(),
                payload: record.payload.clone(),
            });
        }
        state.row.through_sequence = expected_through + records.len() as u64;
        state.last_recorded_at_ms = recorded_at_ms;
        if let Some(status) = update.status {
            state.row.status = status;
        }
        if let Some(configuration) = update.configuration {
            state.row.configuration = configuration.clone();
        }
        drop(state);
        for record in &saved {
            self.feed.publish(record);
        }
        Ok(saved)
    }

    fn append_journal(
        &self,
        expected_through: u64,
        entries: &[JournalEntry],
    ) -> Result<u64, Error> {
        if entries.is_empty() {
            return Ok(expected_through);
        }
        let mut state = self.lock()?;
        if state.row.through_sequence != expected_through {
            return Err(Error::Conflict(format!(
                "journal position changed: expected {expected_through}, found {}",
                state.row.through_sequence
            )));
        }
        let recorded_at_ms = wall_clock_ms()?.max(state.last_recorded_at_ms);
        for (offset, entry) in entries.iter().enumerate() {
            let sequence = expected_through + offset as u64 + 1;
            let payload = serde_json::to_value(entry).map_err(json_error)?;
            let kind = match entry {
                JournalEntry::ContextDelta { .. } => "context_delta",
                JournalEntry::Slot { .. } => "slot",
                JournalEntry::Checkpoint { .. } => "checkpoint",
            };
            let location = self.journal.append(Append {
                sequence,
                recorded_at_ms,
                kind,
                payload: &payload,
            })?;
            state.events.push(None);
            state.journal.push((sequence, location));
            if matches!(entry, JournalEntry::Checkpoint { .. }) {
                state.last_checkpoint = Some(state.journal.len() - 1);
                state.bytes_since_checkpoint = 0;
                // Segments wholly before the checkpoint hold nothing a fold will read.
                self.journal.reclaim(location.segment)?;
            } else {
                state.bytes_since_checkpoint += u64::from(location.length);
            }
        }
        state.row.through_sequence = expected_through + entries.len() as u64;
        state.last_recorded_at_ms = recorded_at_ms;
        Ok(state.row.through_sequence)
    }

    fn fold(&self) -> Result<Folded, Error> {
        let (locations, through) = {
            let state = self.lock()?;
            let from = state.last_checkpoint.unwrap_or(0);
            (
                state.journal[from..]
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

    fn journal_bytes_since_checkpoint(&self) -> Result<u64, Error> {
        Ok(self.lock()?.bytes_since_checkpoint)
    }

    fn session_row(&self) -> Result<SessionRow, Error> {
        Ok(self.lock()?.row.clone())
    }

    fn session_summary(&self) -> Result<SessionSummary, Error> {
        Ok(SessionSummary::from(&self.lock()?.row))
    }

    fn records_after(&self, after: u64, limit: usize) -> Result<Vec<JournalRecord>, Error> {
        // Resolve locations under the lock, read outside it: a client paging through
        // history must never hold up a session that is appending.
        let (session_id, wanted) = {
            let state = self.lock()?;
            let mut wanted = Vec::with_capacity(limit.min(state.events.len()));
            let mut sequence = after + 1;
            while wanted.len() < limit {
                let Some(slot) = state.events.get(sequence as usize - 1) else {
                    break;
                };
                if let Some(location) = slot {
                    wanted.push((sequence, *location));
                }
                sequence += 1;
            }
            (state.row.session_id.clone(), wanted)
        };
        let locations: Vec<Location> = wanted.iter().map(|(_, location)| *location).collect();
        let mut sequences = wanted.into_iter().map(|(sequence, _)| sequence);
        self.events.read_many(&locations, |frame| {
            Ok(JournalRecord {
                session_id: session_id.clone(),
                sequence: sequences.next().unwrap_or(frame.sequence),
                recorded_at_ms: frame.recorded_at_ms,
                kind: frame.kind.to_string(),
                payload: frame.payload()?,
            })
        })
    }

    fn sync(&self) -> Result<(), Error> {
        self.events.writer().sync()
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

fn json_error(error: serde_json::Error) -> Error {
    Error::Journal(error.to_string())
}
