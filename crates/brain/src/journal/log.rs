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

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
};

use crate::KernelError;

/// Rotate to a new segment once the open one reaches this size. Reclamation frees
/// whole segments, so this is also the granularity at which disk comes back.
const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

const HEADER_BYTES: usize = 12;

/// Frames handed to the writer but not yet flushed, keyed by segment and offset.
type Staged = Arc<Mutex<HashMap<(u64, u64), Arc<[u8]>>>>;

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
        bytes: Arc<[u8]>,
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
    sender: Option<mpsc::Sender<Message>>,
    writer: Option<JoinHandle<()>>,
}

struct Tail {
    segment: u64,
    offset: u64,
}

impl SegmentLog {
    /// Open the log at `directory`, handing every frame it holds to `visit` in write
    /// order. A torn tail — the last frame of a crashed process — is truncated away.
    pub(crate) fn open(
        directory: &Path,
        mut visit: impl FnMut(Frame<'_>, Location) -> Result<(), KernelError>,
    ) -> Result<Self, KernelError> {
        fs::create_dir_all(directory).map_err(log_error)?;
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
        let (sender, receiver) = mpsc::channel::<Message>();
        let writer = spawn_writer(directory.to_path_buf(), receiver, pending.clone());

        Ok(Self {
            directory: directory.to_path_buf(),
            tail: Mutex::new(tail),
            pending,
            sender: Some(sender),
            writer: Some(writer),
        })
    }

    /// Assign a location, hand the bytes to the writer, and return. Does not block on
    /// the disk.
    pub(crate) fn append(&self, append: Append<'_>) -> Result<Location, KernelError> {
        let frame: Arc<[u8]> = encode(&append)?.into();
        let length = frame.len() as u64;

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
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut open: Option<(u64, BufWriter<File>)> = None;
        let mut written: Vec<(u64, u64)> = Vec::new();
        while let Ok(first) = receiver.recv() {
            let mut next = Some(first);
            while let Some(message) = next.take() {
                match message {
                    Message::Frame { location, bytes } => {
                        if open.as_ref().is_none_or(|(id, _)| *id != location.segment) {
                            if let Some((_, mut file)) = open.take() {
                                let _ = file.flush();
                            }
                            open = OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(segment_path(&directory, location.segment))
                                .ok()
                                .map(|file| {
                                    (location.segment, BufWriter::with_capacity(1 << 20, file))
                                });
                        }
                        // If the segment could not be opened the frame stays staged in
                        // `pending` and stays readable; there is no caller left to tell.
                        if let Some((_, file)) = open.as_mut()
                            && file.write_all(&bytes).is_ok()
                        {
                            written.push((location.segment, location.offset));
                        }
                    }
                    Message::Reclaim(keep_from) => reclaim_segments(&directory, keep_from),
                }
                next = receiver.try_recv().ok();
            }
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
        }
        if let Some((_, mut file)) = open.take() {
            let _ = file.flush();
        }
    })
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

fn encode(append: &Append<'_>) -> Result<Vec<u8>, KernelError> {
    let payload = serde_json::to_vec(append.payload)
        .map_err(|error| KernelError::Journal(error.to_string()))?;
    let session = append.session_id.as_bytes();
    let kind = append.kind.as_bytes();

    let mut body = Vec::with_capacity(payload.len() + session.len() + kind.len() + 24);
    body.extend_from_slice(&append.sequence.to_le_bytes());
    body.extend_from_slice(&append.recorded_at_ms.to_le_bytes());
    body.extend_from_slice(&(session.len() as u16).to_le_bytes());
    body.extend_from_slice(session);
    body.extend_from_slice(&(kind.len() as u16).to_le_bytes());
    body.extend_from_slice(kind);
    body.extend_from_slice(&payload);

    let mut frame = Vec::with_capacity(body.len() + HEADER_BYTES);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&xxhash_rust::xxh3::xxh3_64(&body).to_le_bytes());
    frame.extend_from_slice(&body);
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
        let mut seen = Vec::new();
        let log = SegmentLog::open(directory, |frame, location| {
            seen.push((frame.sequence, frame.kind.to_string(), location));
            Ok(())
        })
        .unwrap();
        (log, seen)
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
