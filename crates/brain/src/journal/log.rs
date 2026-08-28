//! The on-disk half of the journal: an append-only segment log written behind the
//! caller.
//!
//! Appending assigns a location and hands the bytes to a writer thread; it does not
//! wait for the write, and it never fsyncs. Durability is not Brain's concern — the
//! log exists so a restart can rebuild what was in memory, and so a client can page
//! back through records the session actor no longer holds.
//!
//! Frame integrity lives here and nowhere else. A write-behind log can lose its tail
//! to a crash mid-write, so every frame carries a check value over its own bytes and
//! recovery truncates at the first frame that does not verify. Nothing above this
//! module sees that value or needs to know it exists.
//!
//! Session state does not go in the log. A session's state is the whole conversation so
//! far, it is rewritten at the end of every turn, and only its latest value is ever read
//! — appending it would write the sum of every context the session ever had and read all
//! of them back at startup. It lives in a file per session, rewritten in place by the
//! same writer thread, which drops a state superseded before it reached the disk. The
//! writer handles both in order, so a state that a record depends on is on disk before
//! that record is.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::KernelError;

/// Rotate to a new segment once the open one reaches this size. Reclamation frees
/// whole segments, so this is also the granularity at which disk comes back.
const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

const HEADER_BYTES: usize = 12;

/// How far the writer may fall behind before a caller waits for it.
///
/// Writing behind the caller turns disk latency into memory: with a deliberately slow
/// writer, producers queued 250 MiB of frames in 218 ms and every byte stayed on the
/// heap. Above this the append waits instead, so a stalled disk shows up as a slow turn
/// rather than as a process that grows until it is killed. Large enough that a healthy
/// disk never reaches it.
const MAX_QUEUED_BYTES: u64 = 64 * 1024 * 1024;

/// What the writer has been handed and has not yet put on disk, and whether it has
/// failed. Shared with the writer thread, which is the only thing that subtracts.
#[derive(Default)]
struct Queue {
    bytes: u64,
    /// Set once. A write-behind log has no caller left to tell when a write fails, so
    /// the failure is kept and every later append reports it: losing records silently is
    /// the one outcome worth refusing.
    failure: Option<String>,
}

type Backlog = Arc<(Mutex<Queue>, Condvar)>;

/// Frames handed to the writer but not yet flushed, keyed by segment and offset.
type Staged = Arc<Mutex<HashMap<(u64, u64), Arc<Vec<u8>>>>>;

/// Where a frame lives. Small and `Copy` so the in-memory index can hold one per record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub segment: u64,
    pub offset: u64,
    pub length: u32,
}

/// What a caller writes. `sequence` is zero for a frame that is not part of a
/// session's client-visible record sequence.
pub(crate) struct Append<'a> {
    pub session_id: &'a str,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: &'a str,
    pub payload: &'a serde_json::Value,
}

/// A frame read back out of the log. The payload stays as bytes until asked for, so
/// replay can walk a whole segment without deserialising anything it does not need.
pub(crate) struct Frame<'a> {
    pub session_id: &'a str,
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: &'a str,
    payload: &'a [u8],
}

impl Frame<'_> {
    pub(crate) fn payload(&self) -> Result<serde_json::Value, KernelError> {
        self.decode()
    }

    /// The payload as whatever shape the caller wants. Replay reads frames it will
    /// mostly discard, and building a `Value` for one only to pull two fields out of it
    /// materialises the whole payload twice.
    pub(crate) fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T, KernelError> {
        serde_json::from_slice(self.payload)
            .map_err(|error| KernelError::Journal(error.to_string()))
    }

    pub(crate) fn is_sequenced(&self) -> bool {
        self.sequence > 0
    }
}

enum Message {
    Frame {
        location: Location,
        bytes: Arc<Vec<u8>>,
    },
    /// `None` deletes the session's state file. Later states for one session supersede
    /// earlier ones, so the writer keeps only the last of each batch.
    State {
        session_id: String,
        bytes: Option<Vec<u8>>,
    },
    Reclaim(u64),
}

pub(crate) struct SegmentLog {
    directory: PathBuf,
    tail: Mutex<Tail>,
    /// A read consults this first, so a record is readable the instant it is appended.
    /// The writer flushes before it removes an entry, so a miss here means the bytes
    /// are on disk.
    pending: Staged,
    backlog: Backlog,
    sender: Option<mpsc::Sender<Message>>,
    writer: Option<JoinHandle<()>>,
}

struct Tail {
    segment: u64,
    offset: u64,
}

impl SegmentLog {
    /// Open the log at `directory`. Every session state it holds goes to `visit_state`
    /// first, then every frame goes to `visit_frame` in write order — states first
    /// because a frame belongs to a session and says nothing about which. A torn tail —
    /// the last frame of a crashed process — is truncated away.
    pub(crate) fn open(
        directory: &Path,
        visit_state: impl FnMut(&str, &[u8]) -> Result<(), KernelError>,
        visit: impl FnMut(Frame<'_>, Location) -> Result<(), KernelError>,
    ) -> Result<Self, KernelError> {
        Self::open_inner(directory, visit_state, visit, Duration::ZERO)
    }

    /// A log whose writer sleeps before each frame, so a test can hold it behind and
    /// watch what the queue does. Never used outside tests: a real writer is as fast as
    /// the disk it is writing to.
    #[cfg(test)]
    pub(crate) fn open_behind(
        directory: &Path,
        writer_delay: Duration,
    ) -> Result<Self, KernelError> {
        Self::open_inner(directory, |_, _| Ok(()), |_, _| Ok(()), writer_delay)
    }

    fn open_inner(
        directory: &Path,
        mut visit_state: impl FnMut(&str, &[u8]) -> Result<(), KernelError>,
        mut visit: impl FnMut(Frame<'_>, Location) -> Result<(), KernelError>,
        writer_delay: Duration,
    ) -> Result<Self, KernelError> {
        fs::create_dir_all(directory).map_err(log_error)?;
        for (session_id, bytes) in read_states(directory)? {
            visit_state(&session_id, &bytes)?;
        }
        let mut segments = Vec::new();
        for entry in fs::read_dir(directory).map_err(log_error)? {
            let path = entry.map_err(log_error)?.path();
            if let Some(id) = segment_id(&path) {
                segments.push(id);
            }
        }
        segments.sort_unstable();

        let mut tail = Tail {
            segment: segments.last().copied().unwrap_or(0),
            offset: 0,
        };
        for id in &segments {
            let path = segment_path(directory, *id);
            let mut bytes = Vec::new();
            File::open(&path)
                .map_err(log_error)?
                .read_to_end(&mut bytes)
                .map_err(log_error)?;
            let mut offset = 0usize;
            while let Some((frame, length)) = decode(&bytes[offset..]) {
                visit(
                    frame,
                    Location {
                        segment: *id,
                        offset: offset as u64,
                        length: length as u32,
                    },
                )?;
                offset += length;
            }
            if offset < bytes.len() {
                // Torn or corrupt tail: everything after the last good frame is unusable.
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .map_err(log_error)?
                    .set_len(offset as u64)
                    .map_err(log_error)?;
            }
            if *id == tail.segment {
                tail.offset = offset as u64;
            }
        }

        let pending: Staged = Arc::default();
        let backlog: Backlog = Arc::default();
        let (sender, receiver) = mpsc::channel::<Message>();
        let writer = spawn_writer(
            directory.to_path_buf(),
            receiver,
            pending.clone(),
            backlog.clone(),
            writer_delay,
        );

        Ok(Self {
            directory: directory.to_path_buf(),
            tail: Mutex::new(tail),
            pending,
            backlog,
            sender: Some(sender),
            writer: Some(writer),
        })
    }

    /// Wait until the writer is far enough ahead to accept `bytes`, and refuse outright
    /// if it has already failed. A frame larger than the whole allowance goes through
    /// once the queue is empty rather than waiting for room that can never appear.
    fn reserve(&self, bytes: u64) -> Result<(), KernelError> {
        let (queue, room) = &*self.backlog;
        let mut queue = queue.lock().map_err(|_| poisoned())?;
        loop {
            if let Some(failure) = &queue.failure {
                return Err(KernelError::Journal(format!(
                    "journal writer failed and the log is no longer being written: {failure}"
                )));
            }
            if queue.bytes == 0 || queue.bytes + bytes <= MAX_QUEUED_BYTES {
                queue.bytes += bytes;
                return Ok(());
            }
            queue = room.wait(queue).map_err(|_| poisoned())?;
        }
    }

    /// Assign a location, hand the bytes to the writer, and return. Does not block on
    /// the disk.
    pub(crate) fn append(&self, append: Append<'_>) -> Result<Location, KernelError> {
        // `Arc<Vec<u8>>` rather than `Arc<[u8]>`: converting a `Vec` into `Arc<[u8]>`
        // allocates a second buffer and copies the frame into it.
        let frame = Arc::new(encode(&append)?);
        let length = frame.len() as u64;
        self.reserve(length)?;

        let mut tail = self.tail.lock().map_err(|_| poisoned())?;
        if tail.offset > 0 && tail.offset + length > SEGMENT_BYTES {
            tail.segment += 1;
            tail.offset = 0;
        }
        let location = Location {
            segment: tail.segment,
            offset: tail.offset,
            length: length as u32,
        };
        tail.offset += length;

        self.pending
            .lock()
            .map_err(|_| poisoned())?
            .insert((location.segment, location.offset), frame.clone());
        self.send(Message::Frame {
            location,
            bytes: frame,
        })?;
        Ok(location)
    }

    /// Replace a session's state on disk. Queued behind whatever is already waiting, so
    /// it lands before any record appended after this call, and superseded by any later
    /// state for the same session that is still queued.
    pub(crate) fn write_state(
        &self,
        session_id: &str,
        payload: &serde_json::Value,
    ) -> Result<(), KernelError> {
        check_session_id(session_id)?;
        let bytes =
            serde_json::to_vec(payload).map_err(|error| KernelError::Journal(error.to_string()))?;
        self.reserve(bytes.len() as u64)?;
        self.send(Message::State {
            session_id: session_id.to_owned(),
            bytes: Some(bytes),
        })
    }

    /// Forget a session's state. Its records stay in the log until their segments are
    /// reclaimed, exactly as they did when its state was a frame among them.
    pub(crate) fn remove_state(&self, session_id: &str) -> Result<(), KernelError> {
        check_session_id(session_id)?;
        self.send(Message::State {
            session_id: session_id.to_owned(),
            bytes: None,
        })
    }

    /// Read a run of frames, in order, handing each to `read`. A page of a session's
    /// history is a contiguous stretch of one segment, so the whole stretch comes back
    /// in one read rather than one per record.
    pub(crate) fn read_many<T>(
        &self,
        locations: &[Location],
        mut read: impl FnMut(Frame<'_>) -> Result<T, KernelError>,
    ) -> Result<Vec<T>, KernelError> {
        // Only the frames this call asks for, so the lock is held for a bounded moment
        // and never across the reads below.
        let staged = {
            let pending = self.pending.lock().map_err(|_| poisoned())?;
            locations
                .iter()
                .filter_map(|location| {
                    pending
                        .get(&(location.segment, location.offset))
                        .map(|bytes| ((location.segment, location.offset), bytes.clone()))
                })
                .collect::<HashMap<_, _>>()
        };

        let mut frames = Vec::with_capacity(locations.len());
        let mut index = 0;
        while index < locations.len() {
            let location = locations[index];
            if let Some(bytes) = staged.get(&(location.segment, location.offset)) {
                frames.push(read(decoded(bytes)?)?);
                index += 1;
                continue;
            }
            // The longest run that shares a segment and has reached the disk.
            let mut end = index;
            while end < locations.len()
                && locations[end].segment == location.segment
                && !staged.contains_key(&(location.segment, locations[end].offset))
            {
                end += 1;
            }
            let last = locations[end - 1];
            let span = (last.offset + u64::from(last.length) - location.offset) as usize;
            let mut file =
                File::open(segment_path(&self.directory, location.segment)).map_err(log_error)?;
            file.seek(SeekFrom::Start(location.offset))
                .map_err(log_error)?;
            let mut buffer = vec![0_u8; span];
            file.read_exact(&mut buffer).map_err(log_error)?;
            for entry in &locations[index..end] {
                let at = (entry.offset - location.offset) as usize;
                frames.push(read(decoded(&buffer[at..])?)?);
            }
            index = end;
        }
        Ok(frames)
    }

    /// The segment currently being appended to. Never reclaimed.
    pub(crate) fn current_segment(&self) -> Result<u64, KernelError> {
        Ok(self.tail.lock().map_err(|_| poisoned())?.segment)
    }

    /// Delete every segment older than `keep_from`. Reclamation is a file unlink: the
    /// log never rewrites live data to free dead data.
    pub(crate) fn reclaim(&self, keep_from: u64) -> Result<(), KernelError> {
        if keep_from == 0 {
            return Ok(());
        }
        self.send(Message::Reclaim(keep_from))
    }

    fn send(&self, message: Message) -> Result<(), KernelError> {
        self.sender
            .as_ref()
            .ok_or_else(|| KernelError::Journal("journal writer is shut down".into()))?
            .send(message)
            .map_err(|_| KernelError::Journal("journal writer stopped".into()))
    }
}

impl Drop for SegmentLog {
    fn drop(&mut self) {
        // Close the channel so the writer drains what is queued, then wait for it. A
        // process that shuts down cleanly loses nothing; one that crashes loses its tail.
        self.sender = None;
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn spawn_writer(
    directory: PathBuf,
    receiver: mpsc::Receiver<Message>,
    pending: Staged,
    backlog: Backlog,
    delay: Duration,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut open: Option<(u64, BufWriter<File>)> = None;
        let mut written: Vec<(u64, u64)> = Vec::new();
        // Bytes this batch took off the queue, released once they are on the disk.
        let mut drained = 0_u64;
        // The last state seen for each session in this batch. A session that finished
        // three turns while the disk was busy is written once, not three times.
        let mut states: HashMap<String, Option<Vec<u8>>> = HashMap::new();
        while let Ok(first) = receiver.recv() {
            let mut next = Some(first);
            while let Some(message) = next.take() {
                match message {
                    Message::State { session_id, bytes } => {
                        drained += bytes.as_ref().map_or(0, |bytes| bytes.len() as u64);
                        // A state superseded before it reached the disk is dropped, and
                        // its allowance goes back with the one that replaced it.
                        states.insert(session_id, bytes);
                    }
                    Message::Frame { location, bytes } => {
                        if !delay.is_zero() {
                            thread::sleep(delay);
                        }
                        drained += bytes.len() as u64;
                        if open.as_ref().is_none_or(|(id, _)| *id != location.segment) {
                            if let Some((_, mut file)) = open.take() {
                                let _ = file.flush();
                            }
                            match OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(segment_path(&directory, location.segment))
                            {
                                Ok(file) => {
                                    open = Some((
                                        location.segment,
                                        BufWriter::with_capacity(1 << 20, file),
                                    ));
                                }
                                Err(error) => {
                                    open = None;
                                    fail(&backlog, format!("cannot open a segment: {error}"));
                                }
                            }
                        }
                        match open.as_mut() {
                            Some((_, file)) => match file.write_all(&bytes) {
                                Ok(()) => written.push((location.segment, location.offset)),
                                Err(error) => {
                                    fail(&backlog, format!("cannot write a frame: {error}"))
                                }
                            },
                            None => fail(&backlog, "the segment is not open".to_owned()),
                        }
                    }
                    Message::Reclaim(keep_from) => reclaim_segments(&directory, keep_from),
                }
                next = receiver.try_recv().ok();
            }
            // State before the segment flush: a record that a state must precede is
            // still in the `BufWriter`, so the state reaches the disk first.
            write_states(&directory, states.drain());
            if let Some((_, file)) = open.as_mut() {
                let _ = file.flush();
            }
            // Flushed first, unstaged second: a reader that misses `pending` finds the
            // bytes on disk, never neither.
            if let Ok(mut pending) = pending.lock() {
                for key in written.drain(..) {
                    pending.remove(&key);
                }
            }
            release(&backlog, std::mem::take(&mut drained));
        }
        write_states(&directory, states.drain());
        release(&backlog, drained);
        if let Some((_, mut file)) = open.take() {
            let _ = file.flush();
        }
    })
}

/// Hand back the queue allowance for bytes that have reached the disk, and wake whoever
/// is waiting for room.
fn release(backlog: &Backlog, bytes: u64) {
    let (queue, room) = &**backlog;
    if let Ok(mut queue) = queue.lock() {
        queue.bytes = queue.bytes.saturating_sub(bytes);
    }
    room.notify_all();
}

/// Record the first failure and wake everyone waiting: a log that cannot be written is
/// not going to drain, so waiting for room in it is waiting forever.
fn fail(backlog: &Backlog, reason: String) {
    let (queue, room) = &**backlog;
    if let Ok(mut queue) = queue.lock() {
        queue.failure.get_or_insert(reason);
    }
    room.notify_all();
}

/// Replace or delete each session's state file. A state is written to a temporary file
/// and renamed over the old one, so a reader — including the next process to open this
/// directory — sees either the whole previous state or the whole new one.
fn write_states(directory: &Path, states: impl Iterator<Item = (String, Option<Vec<u8>>)>) {
    for (session_id, bytes) in states {
        let Some(path) = state_path(directory, &session_id) else {
            continue;
        };
        match bytes {
            None => {
                let _ = fs::remove_file(&path);
            }
            Some(bytes) => {
                let staging = path.with_extension("state-writing");
                if fs::write(&staging, &bytes).is_ok() {
                    let _ = fs::rename(&staging, &path);
                }
            }
        }
    }
}

/// Every session state in `directory`, as raw bytes. A file left behind by a crash
/// between write and rename carries the `.state-writing` extension and is removed rather
/// than read: the state it was replacing is still whole under its own name.
fn read_states(directory: &Path) -> Result<Vec<(String, Vec<u8>)>, KernelError> {
    let mut states = Vec::new();
    for entry in fs::read_dir(directory).map_err(log_error)? {
        let path = entry.map_err(log_error)?.path();
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("state-writing") => {
                let _ = fs::remove_file(&path);
            }
            Some("state") => {
                let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                let session_id = session_id.to_owned();
                states.push((session_id, fs::read(&path).map_err(log_error)?));
            }
            _ => {}
        }
    }
    // Deterministic, so a directory listing's order cannot change what recovery sees.
    states.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(states)
}

fn state_path(directory: &Path, session_id: &str) -> Option<PathBuf> {
    check_session_id(session_id).ok()?;
    Some(directory.join(format!("{session_id}.state")))
}

/// A session id becomes a file name, so it may not be able to name anything but a file
/// in this directory. Ids are generated, not supplied, so this never fires today — it is
/// here so that it fires loudly rather than silently if that ever changes.
fn check_session_id(session_id: &str) -> Result<(), KernelError> {
    let usable = !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if usable {
        return Ok(());
    }
    Err(KernelError::Journal(
        "session id cannot name a state file".into(),
    ))
}

fn reclaim_segments(directory: &Path, keep_from: u64) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if segment_id(&path).is_some_and(|id| id < keep_from) {
            let _ = fs::remove_file(path);
        }
    }
}

// ---- frame encoding -------------------------------------------------------------
//
// [ body length: u32 ][ check: u64 ][ body ]
//
// body = sequence u64 | recorded_at_ms u64 | session u16-prefixed | kind u16-prefixed | payload

/// Builds the whole frame in one buffer: header space first, then the body written
/// straight into it, then the header patched once the body's length and check value are
/// known. A record's payload is the largest thing the kernel handles, so serialising it
/// into a payload buffer, copying that into a body buffer, and copying that into a frame
/// buffer meant three copies of it before the frame existed.
fn encode(append: &Append<'_>) -> Result<Vec<u8>, KernelError> {
    let session = append.session_id.as_bytes();
    let kind = append.kind.as_bytes();

    let mut frame = Vec::with_capacity(HEADER_BYTES + session.len() + kind.len() + 24);
    frame.resize(HEADER_BYTES, 0);
    frame.extend_from_slice(&append.sequence.to_le_bytes());
    frame.extend_from_slice(&append.recorded_at_ms.to_le_bytes());
    frame.extend_from_slice(&(session.len() as u16).to_le_bytes());
    frame.extend_from_slice(session);
    frame.extend_from_slice(&(kind.len() as u16).to_le_bytes());
    frame.extend_from_slice(kind);
    serde_json::to_writer(&mut frame, append.payload)
        .map_err(|error| KernelError::Journal(error.to_string()))?;

    let body = &frame[HEADER_BYTES..];
    let length = (body.len() as u32).to_le_bytes();
    let check = xxhash_rust::xxh3::xxh3_64(body).to_le_bytes();
    frame[0..4].copy_from_slice(&length);
    frame[4..HEADER_BYTES].copy_from_slice(&check);
    Ok(frame)
}

/// Decode the frame at the start of `bytes`, returning it and its total length.
/// `None` means the frame is incomplete or does not verify — for the last frame in a
/// segment that is a torn write, and for any other it is corruption.
fn decode(bytes: &[u8]) -> Option<(Frame<'_>, usize)> {
    if bytes.len() < HEADER_BYTES {
        return None;
    }
    let length = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
    let check = u64::from_le_bytes(bytes[4..12].try_into().ok()?);
    let end = HEADER_BYTES.checked_add(length)?;
    let body = bytes.get(HEADER_BYTES..end)?;
    if xxhash_rust::xxh3::xxh3_64(body) != check {
        return None;
    }

    let mut cursor = 0usize;
    let sequence = u64::from_le_bytes(take(body, &mut cursor, 8)?.try_into().ok()?);
    let recorded_at_ms = u64::from_le_bytes(take(body, &mut cursor, 8)?.try_into().ok()?);
    let session_length = u16::from_le_bytes(take(body, &mut cursor, 2)?.try_into().ok()?) as usize;
    let session_id = std::str::from_utf8(take(body, &mut cursor, session_length)?).ok()?;
    let kind_length = u16::from_le_bytes(take(body, &mut cursor, 2)?.try_into().ok()?) as usize;
    let kind = std::str::from_utf8(take(body, &mut cursor, kind_length)?).ok()?;

    Some((
        Frame {
            session_id,
            sequence,
            recorded_at_ms,
            kind,
            payload: body.get(cursor..)?,
        },
        end,
    ))
}

fn decoded(bytes: &[u8]) -> Result<Frame<'_>, KernelError> {
    decode(bytes)
        .map(|(frame, _)| frame)
        .ok_or_else(|| KernelError::Journal("journal frame failed its check".into()))
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, length: usize) -> Option<&'a [u8]> {
    let end = cursor.checked_add(length)?;
    let slice = bytes.get(*cursor..end)?;
    *cursor = end;
    Some(slice)
}

fn segment_path(directory: &Path, id: u64) -> PathBuf {
    directory.join(format!("{id:020}.journal"))
}

fn segment_id(path: &Path) -> Option<u64> {
    if path.extension()? != "journal" {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

fn poisoned() -> KernelError {
    KernelError::Journal("journal tail mutex poisoned".into())
}

fn log_error(error: std::io::Error) -> KernelError {
    KernelError::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(kind: &str) -> serde_json::Value {
        serde_json::json!({ "value": kind })
    }

    fn append<'a>(
        session_id: &'a str,
        sequence: u64,
        kind: &'a str,
        body: &'a serde_json::Value,
    ) -> Append<'a> {
        Append {
            session_id,
            sequence,
            recorded_at_ms: 1_700_000_000_000,
            kind,
            payload: body,
        }
    }

    fn temporary() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "brain-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn collect(directory: &Path) -> (SegmentLog, Vec<(u64, String, Location)>) {
        let (log, seen, _) = collect_all(directory);
        (log, seen)
    }

    /// Frames and session states, in the order `open` hands them over.
    #[allow(clippy::type_complexity)]
    fn collect_all(
        directory: &Path,
    ) -> (
        SegmentLog,
        Vec<(u64, String, Location)>,
        Vec<(String, serde_json::Value)>,
    ) {
        let mut seen = Vec::new();
        let mut states = Vec::new();
        let log = SegmentLog::open(
            directory,
            |session_id, bytes| {
                states.push((
                    session_id.to_owned(),
                    serde_json::from_slice(bytes).unwrap(),
                ));
                Ok(())
            },
            |frame, location| {
                seen.push((frame.sequence, frame.kind.to_string(), location));
                Ok(())
            },
        )
        .unwrap();
        (log, seen, states)
    }

    #[test]
    fn a_frame_round_trips_through_encoding() {
        let body = payload("turn_started");
        let bytes = encode(&append("ses_test", 7, "turn_started", &body)).unwrap();
        let (frame, length) = decode(&bytes).unwrap();
        assert_eq!(length, bytes.len());
        assert_eq!(frame.sequence, 7);
        assert_eq!(frame.session_id, "ses_test");
        assert_eq!(frame.kind, "turn_started");
        assert_eq!(frame.payload().unwrap(), body);
    }

    #[test]
    fn a_record_is_readable_before_the_writer_has_flushed_it() {
        let directory = temporary();
        let (log, seen) = collect(&directory);
        assert!(seen.is_empty());
        let body = payload("session_created");
        let location = log
            .append(append("ses_test", 1, "session_created", &body))
            .unwrap();
        let kinds = log
            .read_many(&[location], |frame| Ok(frame.kind.to_string()))
            .unwrap();
        assert_eq!(kinds, ["session_created"]);
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn reopening_replays_every_frame_in_write_order() {
        let directory = temporary();
        let (log, _) = collect(&directory);
        let body = payload("turn_finished");
        for sequence in 1..=32 {
            log.append(append("ses_test", sequence, "turn_finished", &body))
                .unwrap();
        }
        drop(log);

        let (log, seen) = collect(&directory);
        assert_eq!(seen.len(), 32);
        assert_eq!(seen[0].0, 1);
        assert_eq!(seen[31].0, 32);
        let sequences = log
            .read_many(&[seen[9].2, seen[10].2], |frame| Ok(frame.sequence))
            .unwrap();
        assert_eq!(sequences, [10, 11]);
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_unsequenced_frame_is_marked_as_such() {
        let body = payload("session_state");
        let bytes = encode(&append("ses_test", 0, "session_state", &body)).unwrap();
        let (frame, _) = decode(&bytes).unwrap();
        assert!(!frame.is_sequenced());
    }

    /// Writing behind the caller turns disk latency into memory. With a slow writer,
    /// producers queued 250 MiB of frames in 218 ms and every byte stayed on the heap.
    /// Past the allowance the append waits, so a stalled disk is a slow turn rather than
    /// a process that grows until it is killed.
    #[test]
    fn a_slow_writer_makes_appends_wait_instead_of_growing_the_queue() {
        let directory = temporary();
        // One millisecond per frame, which is roughly what the original spike used to
        // hold the writer behind its producers.
        let log = SegmentLog::open_behind(&directory, Duration::from_millis(1)).unwrap();
        let body = serde_json::json!({ "filler": "x".repeat(64 * 1024) });

        let mut peak = 0;
        // Enough frames that an unbounded queue would hold several times the allowance.
        for sequence in 1..=2_048 {
            log.append(append("ses_test", sequence, "turn_finished", &body))
                .unwrap();
            peak = peak.max(log.backlog.0.lock().unwrap().bytes);
            if peak > MAX_QUEUED_BYTES {
                break;
            }
        }

        assert!(
            peak <= MAX_QUEUED_BYTES,
            "the writer was allowed to fall {peak} bytes behind, past the              {MAX_QUEUED_BYTES}-byte allowance"
        );
        // And the allowance was actually reached, so the bound above is holding
        // something back rather than being satisfied by a queue that never filled.
        assert!(
            peak > MAX_QUEUED_BYTES / 2,
            "the queue only reached {peak} bytes, so this run never tested the bound"
        );
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    /// A frame bigger than the whole allowance must go through rather than wait for room
    /// that can never appear.
    #[test]
    fn a_frame_larger_than_the_allowance_is_still_written() {
        let directory = temporary();
        let log = SegmentLog::open_behind(&directory, Duration::ZERO).unwrap();
        let body = serde_json::json!({
            "filler": "x".repeat(MAX_QUEUED_BYTES as usize + 1),
        });

        let location = log
            .append(append("ses_test", 1, "turn_finished", &body))
            .unwrap();
        assert!(u64::from(location.length) > MAX_QUEUED_BYTES);

        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    /// A write-behind log has no caller left to tell when a write fails. Keeping the
    /// failure and refusing every later append is the difference between a loud stop and
    /// silently losing records.
    #[test]
    fn a_writer_failure_stops_the_log_rather_than_being_swallowed() {
        let directory = temporary();
        let log = SegmentLog::open_behind(&directory, Duration::ZERO).unwrap();
        fail(&log.backlog, "the disk went away".to_owned());

        let body = payload("turn_finished");
        let error = log
            .append(append("ses_test", 1, "turn_finished", &body))
            .expect_err("an append after a writer failure must not report success");
        assert!(
            error.to_string().contains("the disk went away"),
            "the failure must say what happened: {error}"
        );
        assert!(log.write_state("ses_test", &body).is_err());

        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_session_state_is_replaced_rather_than_accumulated() {
        let directory = temporary();
        let (log, _, states) = collect_all(&directory);
        assert!(states.is_empty());

        for turn in 1..=8 {
            log.write_state("ses_test", &serde_json::json!({ "turn": turn }))
                .unwrap();
        }
        drop(log);

        let (log, _, states) = collect_all(&directory);
        assert_eq!(states.len(), 1, "one file per session, not one per write");
        assert_eq!(states[0].0, "ses_test");
        assert_eq!(states[0].1, serde_json::json!({ "turn": 8 }));
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_removed_session_state_does_not_come_back() {
        let directory = temporary();
        let (log, _, _) = collect_all(&directory);
        log.write_state("ses_test", &serde_json::json!({ "turn": 1 }))
            .unwrap();
        log.remove_state("ses_test").unwrap();
        drop(log);

        let (log, _, states) = collect_all(&directory);
        assert!(states.is_empty());
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    /// A crash between writing the replacement and renaming it over the old one leaves a
    /// staging file. The state under its own name is still whole, so the staging file is
    /// discarded rather than read.
    #[test]
    fn a_half_written_state_is_discarded_and_the_whole_one_kept() {
        let directory = temporary();
        let (log, _, _) = collect_all(&directory);
        log.write_state("ses_test", &serde_json::json!({ "turn": 1 }))
            .unwrap();
        drop(log);
        fs::write(directory.join("ses_test.state-writing"), b"{\"turn\":").unwrap();

        let (log, _, states) = collect_all(&directory);
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].1, serde_json::json!({ "turn": 1 }));
        assert!(!directory.join("ses_test.state-writing").exists());
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    /// A session id becomes a file name. Ids are generated today, so this only fires if
    /// that changes — which is the point.
    #[test]
    fn a_session_id_that_could_escape_the_directory_is_refused() {
        let directory = temporary();
        let (log, _, _) = collect_all(&directory);
        for hostile in ["../escape", "a/b", "", "."] {
            assert!(
                log.write_state(hostile, &serde_json::json!({})).is_err(),
                "{hostile:?} must not name a state file"
            );
        }
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_torn_tail_is_dropped_and_its_space_reused() {
        let directory = temporary();
        let (log, _) = collect(&directory);
        let body = payload("turn_finished");
        for sequence in 1..=4 {
            log.append(append("ses_test", sequence, "turn_finished", &body))
                .unwrap();
        }
        drop(log);

        // Simulate a crash partway through the fifth frame.
        let path = segment_path(&directory, 0);
        let length = fs::metadata(&path).unwrap().len();
        let torn = encode(&append("ses_test", 5, "turn_finished", &body)).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&torn[..torn.len() / 2])
            .unwrap();

        let (log, seen) = collect(&directory);
        assert_eq!(seen.len(), 4);
        assert_eq!(fs::metadata(&path).unwrap().len(), length);
        let location = log
            .append(append("ses_test", 5, "turn_finished", &body))
            .unwrap();
        assert_eq!(location.offset, length);
        drop(log);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_corrupt_frame_body_does_not_decode() {
        let body = payload("turn_started");
        let mut bytes = encode(&append("ses_test", 1, "turn_started", &body)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert!(decode(&bytes).is_none());
    }

    #[test]
    fn reclaim_removes_only_segments_below_the_floor() {
        let directory = temporary();
        let (log, _) = collect(&directory);
        let body = payload("turn_finished");
        log.append(append("ses_test", 1, "turn_finished", &body))
            .unwrap();
        drop(log);
        fs::copy(segment_path(&directory, 0), segment_path(&directory, 1)).unwrap();

        let (log, _) = collect(&directory);
        log.reclaim(1).unwrap();
        drop(log);
        assert!(!segment_path(&directory, 0).exists());
        assert!(segment_path(&directory, 1).exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
