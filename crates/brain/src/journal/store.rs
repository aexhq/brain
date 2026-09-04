use std::collections::BTreeMap;

use brain_protocol::{Message, SessionId, SessionStatus, SessionSummary};
use serde::{Deserialize, Serialize};

use crate::{
    Error,
    journal::{AppendRecord, JournalRecord},
};

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub status: SessionStatus,
    pub through_sequence: u64,
    pub configuration: serde_json::Value,
    pub context: serde_json::Value,
}

/// What a caller outside the session is allowed to see about it. Building one borrows
/// the row rather than cloning it, so listing sessions never copies a conversation or a
/// configuration.
impl From<&SessionRow> for SessionSummary {
    fn from(row: &SessionRow) -> Self {
        Self {
            session_id: row.session_id.clone(),
            status: row.status.clone(),
            last_sequence: row.through_sequence,
            share_key: String::new(),
        }
    }
}

#[derive(Default)]
pub struct SessionUpdate<'a> {
    pub status: Option<SessionStatus>,
    pub context: Option<&'a serde_json::Value>,
    pub configuration: Option<&'a serde_json::Value>,
}

/// One entry in a session's journal: what changed in its transcript or its state.
///
/// The transcript is Brain's message list. A delta keeps the longest prefix the new
/// transcript shares with the last recorded one and appends the rest; a compaction is a
/// delta that keeps nothing. A slot is a named value with last-write-wins semantics for
/// state that is not a transcript item. A checkpoint carries the whole transcript and
/// every slot, so folding never has to start from the first entry.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEntry {
    ContextDelta {
        keep: u64,
        append: Vec<Message>,
    },
    Slot {
        name: String,
        value: serde_json::Value,
    },
    Checkpoint {
        transcript: Vec<Message>,
        slots: BTreeMap<String, serde_json::Value>,
    },
}

/// What folding a session's journal yields: its transcript, its slots, and the sequence
/// the journal is current to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folded {
    pub transcript: Vec<Message>,
    pub slots: BTreeMap<String, serde_json::Value>,
    pub through_sequence: u64,
}

impl Folded {
    pub fn apply(&mut self, entry: JournalEntry) {
        match entry {
            JournalEntry::ContextDelta { keep, append } => {
                self.transcript.truncate(keep as usize);
                self.transcript.extend(append);
            }
            JournalEntry::Slot { name, value } => {
                self.slots.insert(name, value);
            }
            JournalEntry::Checkpoint { transcript, slots } => {
                self.transcript = transcript;
                self.slots = slots;
            }
        }
    }
}

/// One session's durable record: an events log that clients read and a journal the
/// session's own state folds out of, numbered by one sequence counter.
pub trait JournalStore: Send + Sync + 'static {
    fn session_id(&self) -> &SessionId;
    /// Appends effect and lifecycle records to the events log.
    fn append(
        &self,
        expected_through: u64,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<JournalRecord>, Error>;
    /// Appends transcript deltas, slot writes and checkpoints to the journal. Returns the
    /// sequence the session is now through.
    fn append_journal(&self, expected_through: u64, entries: &[JournalEntry])
    -> Result<u64, Error>;
    /// The transcript and slots as the journal has them, folded from the last checkpoint.
    fn fold(&self) -> Result<Folded, Error>;
    /// Journal bytes appended since the last checkpoint; the session decides when that
    /// has earned another one.
    fn journal_bytes_since_checkpoint(&self) -> Result<u64, Error>;
    /// The whole row, including configuration and context. Only rehydrating a session
    /// actor needs this; everything else wants a summary.
    fn session_row(&self) -> Result<SessionRow, Error>;
    fn session_summary(&self) -> Result<SessionSummary, Error>;
    fn records_after(&self, after: u64, limit: usize) -> Result<Vec<JournalRecord>, Error>;
    /// Whether this session was rebuilt from disk and has not yet been handed its own
    /// records back. Answers true once and then false: the agentloop is told what it is
    /// continuing exactly once, and telling it twice would replay a conversation it is
    /// already holding.
    fn take_restored(&self) -> bool;
    /// Returns once everything appended so far is on disk.
    fn sync(&self) -> Result<(), Error>;
}
