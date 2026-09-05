use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use brain_protocol::{Message, SessionId, SessionStatus, SessionSummary};
use serde::{Deserialize, Serialize};

use crate::{
    Error,
    journal::{AppendRecord, SessionRecord, writer::Ticket},
};

#[derive(Clone, Debug)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub status: SessionStatus,
    pub through_sequence: u64,
    pub configuration: serde_json::Value,
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
        }
    }
}

#[derive(Default)]
pub struct SessionUpdate<'a> {
    pub status: Option<SessionStatus>,
    pub configuration: Option<&'a serde_json::Value>,
}

/// A background append that has been admitted to the bounded writer queue. Its records
/// have no sequence and are not visible until [`wait`](Self::wait) succeeds.
pub struct CommitHandle {
    ticket: Ticket,
    records: Arc<Mutex<Option<Vec<SessionRecord>>>>,
}

impl CommitHandle {
    pub(crate) fn new(ticket: Ticket, records: Arc<Mutex<Option<Vec<SessionRecord>>>>) -> Self {
        Self { ticket, records }
    }

    pub(crate) fn ready(records: Vec<SessionRecord>) -> Self {
        Self {
            ticket: Ticket::ready(),
            records: Arc::new(Mutex::new(Some(records))),
        }
    }

    pub fn wait(self) -> Result<Vec<SessionRecord>, Error> {
        self.ticket.wait()?;
        self.records
            .lock()
            .map_err(|_| Error::Journal("journal commit result poisoned".into()))?
            .take()
            .ok_or_else(|| Error::Journal("journal commit produced no result".into()))
    }
}

/// A compact session-state mutation stored in the same journal as public Events.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEntry {
    TranscriptDelta {
        keep: u64,
        append: Vec<Message>,
    },
    StateSet {
        name: String,
        value: serde_json::Value,
    },
}

impl JournalEntry {
    pub(crate) fn is_kind(kind: &str) -> bool {
        matches!(kind, "transcript_delta" | "state_set")
    }
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
            JournalEntry::TranscriptDelta { keep, append } => {
                self.transcript.truncate(keep as usize);
                self.transcript.extend(append);
            }
            JournalEntry::StateSet { name, value } => {
                self.slots.insert(name, value);
            }
        }
    }
}

/// One session's canonical journal and its disposable projections.
pub trait SessionStore: Send + Sync + 'static {
    fn session_id(&self) -> &SessionId;
    /// Appends effect and lifecycle records and waits for durable commit.
    fn append_sync(
        &self,
        records: &[AppendRecord],
        update: SessionUpdate<'_>,
    ) -> Result<Vec<SessionRecord>, Error>;
    /// Admits background records. The writer assigns their sequence only when selected.
    fn append_async(&self, records: Vec<AppendRecord>) -> Result<CommitHandle, Error>;
    /// Appends transcript and Agentloop-state mutations and waits for durable commit.
    fn append_journal_sync(&self, entries: &[JournalEntry]) -> Result<u64, Error>;
    /// The transcript and Agentloop state folded from the journal.
    fn fold(&self) -> Result<Folded, Error>;
    /// The whole row, including configuration and context. Only rehydrating a session
    /// actor needs this; everything else wants a summary.
    fn session_row(&self) -> Result<SessionRow, Error>;
    fn session_summary(&self) -> Result<SessionSummary, Error>;
    fn records_after(&self, after: u64, limit: usize) -> Result<Vec<SessionRecord>, Error>;
    /// Returns once everything appended so far is on disk.
    fn sync(&self) -> Result<(), Error>;
}
