//! One append-only segment log: the on-disk half of a session's journal or events.
//!
//! Appending assigns a location and hands the frame to the process's [`Writer`]; it does
//! not wait for the disk. Frame integrity lives here and nowhere else: a write-behind log
//! can lose its tail to a crash mid-write, so every frame carries a check value over its
//! own bytes and opening the log truncates at the first frame that does not verify.
//!
//! The log knows nothing about what its frames mean. The session store folds them back
//! into an index on open, and reads them back by location afterwards.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::Error;

use super::writer::{self, Staged, Writer};

/// Rotate to a new segment once the open one reaches this size. Reclamation frees
/// whole segments, so this is also the granularity at which disk comes back.
const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

const HEADER_BYTES: usize = 12;

pub(crate) const SEGMENT_EXTENSION: &str = "segment";

/// Where a frame lives. Small and `Copy` so the in-memory index can hold one per record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub segment: u64,
    pub offset: u64,
    pub length: u32,
}

/// What a caller writes.
pub(crate) struct Append<'a> {
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: &'a str,
    pub payload: &'a serde_json::Value,
}

/// A frame read back out of the log. The payload stays as bytes until asked for, so
/// replay can walk a whole segment without deserialising anything it does not need.
pub(crate) struct Frame<'a> {
    pub sequence: u64,
    pub recorded_at_ms: u64,
    pub kind: &'a str,
    payload: &'a [u8],
}

impl Frame<'_> {
    pub(crate) fn payload(&self) -> Result<serde_json::Value, Error> {
        self.decode()
    }

    /// The payload as whatever shape the caller wants. Replay reads frames it will
    /// mostly discard, and building a `Value` for one only to pull two fields out of it
    /// materialises the whole payload twice.
    pub(crate) fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_slice(self.payload).map_err(|error| Error::Journal(error.to_string()))
    }

    pub(crate) fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

pub(crate) struct SegmentLog {
    directory: Arc<PathBuf>,
    owner: Arc<str>,
    tail: Mutex<Tail>,
    /// A read consults this first, so a record is readable the instant it is appended.
    /// The writer flushes before it removes an entry, so a miss here means the bytes
    /// are on disk.
    pending: Staged,
    writer: Arc<Writer>,
}

struct Tail {
    segment: u64,
    offset: u64,
}

impl SegmentLog {
    /// Open or create the log at `directory`, handing every frame already on disk to
    /// `visit` in write order and stopping at a torn tail. `owner` is the queue the
    /// writer charges this log's bytes to.
    pub(crate) fn open(
        directory: &Path,
        owner: Arc<str>,
        writer: Arc<Writer>,
        mut visit: impl FnMut(Frame<'_>, Location) -> Result<(), Error>,
    ) -> Result<Self, Error> {
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

        Ok(Self {
            directory: Arc::new(directory.to_path_buf()),
            owner,
            tail: Mutex::new(tail),
            pending: Arc::default(),
            writer,
        })
    }

    /// Assign a location, hand the bytes to the writer, and return. Does not block on
    /// the disk.
    pub(crate) fn append(&self, append: Append<'_>) -> Result<Location, Error> {
        // `Arc<Vec<u8>>` rather than `Arc<[u8]>`: converting a `Vec` into `Arc<[u8]>`
        // allocates a second buffer and copies the frame into it.
        let frame = Arc::new(encode(&append)?);
        let length = frame.len() as u64;
        self.writer.reserve(&self.owner, length)?;

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
        self.writer.write(writer::Frame {
            path: Arc::new(segment_path(&self.directory, location.segment)),
            owner: self.owner.clone(),
            key: (location.segment, location.offset),
            bytes: frame,
            staged: self.pending.clone(),
        })?;
        Ok(location)
    }

    pub(crate) fn read_many<T>(
        &self,
        locations: &[Location],
        mut read: impl FnMut(Frame<'_>) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
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

    /// Delete every segment older than `keep_from`. Reclamation is a file unlink: the
    /// log never rewrites live data to free dead data.
    pub(crate) fn reclaim(&self, keep_from: u64) -> Result<(), Error> {
        if keep_from == 0 {
            return Ok(());
        }
        self.writer.reclaim(
            self.directory.as_ref().clone(),
            keep_from,
            SEGMENT_EXTENSION,
        )
    }

    pub(crate) fn writer(&self) -> &Arc<Writer> {
        &self.writer
    }
}

// ---- frame encoding -------------------------------------------------------------
//
// [ body length: u32 ][ check: u64 ][ body ]
//
// body = sequence u64 | recorded_at_ms u64 | kind u16-prefixed | payload

/// Builds the whole frame in one buffer: header space first, then the body written
/// straight into it, then the header patched once the body's length and check value are
/// known. A record's payload is the largest thing the journal handles, so serialising it
/// into a payload buffer, copying that into a body buffer, and copying that into a frame
/// buffer meant three copies of it before the frame existed.
fn encode(append: &Append<'_>) -> Result<Vec<u8>, Error> {
    let kind = append.kind.as_bytes();

    let mut frame = Vec::with_capacity(HEADER_BYTES + kind.len() + 24);
    frame.resize(HEADER_BYTES, 0);
    frame.extend_from_slice(&append.sequence.to_le_bytes());
    frame.extend_from_slice(&append.recorded_at_ms.to_le_bytes());
    frame.extend_from_slice(&(kind.len() as u16).to_le_bytes());
    frame.extend_from_slice(kind);
    serde_json::to_writer(&mut frame, append.payload)
        .map_err(|error| Error::Journal(error.to_string()))?;

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
    let kind_length = u16::from_le_bytes(take(body, &mut cursor, 2)?.try_into().ok()?) as usize;
    let kind = std::str::from_utf8(take(body, &mut cursor, kind_length)?).ok()?;

    Some((
        Frame {
            sequence,
            recorded_at_ms,
            kind,
            payload: body.get(cursor..)?,
        },
        end,
    ))
}

fn decoded(bytes: &[u8]) -> Result<Frame<'_>, Error> {
    decode(bytes)
        .map(|(frame, _)| frame)
        .ok_or_else(|| Error::Journal("journal frame failed its check".into()))
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, length: usize) -> Option<&'a [u8]> {
    let end = cursor.checked_add(length)?;
    let slice = bytes.get(*cursor..end)?;
    *cursor = end;
    Some(slice)
}

fn segment_path(directory: &Path, id: u64) -> PathBuf {
    directory.join(format!("{id:020}.{SEGMENT_EXTENSION}"))
}

fn segment_id(path: &Path) -> Option<u64> {
    if path.extension()? != SEGMENT_EXTENSION {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

fn poisoned() -> Error {
    Error::Journal("journal tail mutex poisoned".into())
}

fn log_error(error: std::io::Error) -> Error {
    Error::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };

    use super::*;

    fn payload(kind: &str) -> serde_json::Value {
        serde_json::json!({ "value": kind })
    }

    fn append<'a>(sequence: u64, kind: &'a str, body: &'a serde_json::Value) -> Append<'a> {
        Append {
            sequence,
            recorded_at_ms: 1_700_000_000_000,
            kind,
            payload: body,
        }
    }

    fn temporary() -> PathBuf {
        std::env::temp_dir().join(format!(
            "brain-segment-log-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    fn collect(
        directory: &Path,
        writer: Arc<Writer>,
    ) -> (SegmentLog, Vec<(u64, String, Location)>) {
        let mut seen = Vec::new();
        let log = SegmentLog::open(directory, Arc::from("test"), writer, |frame, location| {
            seen.push((frame.sequence, frame.kind.to_string(), location));
            Ok(())
        })
        .unwrap();
        (log, seen)
    }

    #[test]
    fn a_frame_round_trips_through_encoding() {
        let body = payload("model_call_ended");
        let bytes = encode(&append(7, "model_call_ended", &body)).unwrap();
        let (frame, length) = decode(&bytes).unwrap();
        assert_eq!(length, bytes.len());
        assert_eq!(frame.sequence, 7);
        assert_eq!(frame.kind, "model_call_ended");
        assert_eq!(frame.payload().unwrap(), body);
    }

    #[test]
    fn a_record_is_readable_before_the_writer_has_flushed_it() {
        let directory = temporary();
        let writer = Writer::spawn();
        let (log, _) = collect(&directory, writer);
        let body = payload("turn_started");
        let location = log.append(append(1, "turn_started", &body)).unwrap();
        let read = log
            .read_many(&[location], |frame| Ok(frame.payload().unwrap()))
            .unwrap();
        assert_eq!(read, vec![body]);
        drop(log);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn reopening_replays_every_frame_in_write_order() {
        let directory = temporary();
        let writer = Writer::spawn();
        {
            let (log, _) = collect(&directory, writer.clone());
            for sequence in 1..=5 {
                let body = payload("k");
                log.append(append(sequence, "k", &body)).unwrap();
            }
            writer.sync().unwrap();
        }
        let (_, seen) = collect(&directory, writer);
        let sequences: Vec<u64> = seen.iter().map(|(sequence, _, _)| *sequence).collect();
        assert_eq!(sequences, vec![1, 2, 3, 4, 5]);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_stalled_writer_makes_appends_wait_instead_of_growing_the_queue() {
        let directory = temporary();
        let release = Arc::new(AtomicBool::new(false));
        let writer = Writer::spawn_held(release.clone());
        let (log, _) = collect(&directory, writer.clone());
        let body = serde_json::json!({ "text": "x".repeat(1024 * 1024) });
        let log = Arc::new(log);
        let producer = {
            let log = log.clone();
            let body = body.clone();
            thread::spawn(move || {
                for sequence in 1..=64 {
                    log.append(append(sequence, "big", &body)).unwrap();
                }
            })
        };
        let mut peak = 0;
        let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < until {
            peak = peak.max(writer.queued_bytes());
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            peak <= writer::OWNER_QUEUE_BYTES + 2 * 1024 * 1024,
            "queue reached {peak} bytes with a stalled writer"
        );
        release.store(true, Ordering::Release);
        producer.join().unwrap();
        drop(log);
        drop(writer);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_torn_tail_is_dropped_and_its_space_reused() {
        let directory = temporary();
        let writer = Writer::spawn();
        {
            let (log, _) = collect(&directory, writer.clone());
            let body = payload("k");
            log.append(append(1, "k", &body)).unwrap();
            log.append(append(2, "k", &body)).unwrap();
            writer.sync().unwrap();
        }
        let path = segment_path(&directory, 0);
        let mut bytes = fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 3);
        bytes.extend_from_slice(&[0xff; 5]);
        fs::write(&path, &bytes).unwrap();
        let (log, seen) = collect(&directory, writer.clone());
        assert_eq!(seen.len(), 1, "the torn second frame is dropped");
        let body = payload("k");
        let location = log.append(append(2, "k", &body)).unwrap();
        assert_eq!(
            location.offset,
            seen[0].2.offset + u64::from(seen[0].2.length)
        );
        writer.sync().unwrap();
        drop(log);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_corrupt_frame_body_does_not_decode() {
        let body = payload("k");
        let mut bytes = encode(&append(1, "k", &body)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        assert!(decode(&bytes).is_none());
    }

    #[test]
    fn reclaim_removes_only_segments_below_the_floor() {
        let directory = temporary();
        fs::create_dir_all(&directory).unwrap();
        for id in 0..3 {
            fs::write(segment_path(&directory, id), b"").unwrap();
        }
        let writer = Writer::spawn();
        let (log, _) = collect(&directory, writer.clone());
        log.reclaim(2).unwrap();
        writer.sync().unwrap();
        assert!(!segment_path(&directory, 0).exists());
        assert!(!segment_path(&directory, 1).exists());
        assert!(segment_path(&directory, 2).exists());
        drop(log);
        let _ = fs::remove_dir_all(directory);
    }
}
