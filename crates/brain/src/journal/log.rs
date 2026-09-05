//! One append-only segment log: the on-disk half of a session's journal.
//!
//! The process [`Writer`] selects a request before this log assigns its locations. Frame
//! integrity lives here and nowhere else: a crash can tear the tail, so every frame
//! carries a check value. Only an incomplete final write is truncated; corruption is refused.
//!
//! The log knows nothing about what its frames mean. The session store folds them back
//! into an index on open, and reads them back by location afterwards.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::Error;

use super::writer::{self, Writer};

/// Rotate to a new segment once the open one reaches this size. Reclamation frees
/// whole segments, so this is also the granularity at which disk comes back.
const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

const HEADER_BYTES: usize = 12;

pub(crate) const SEGMENT_EXTENSION: &str = "segment";

/// Where a frame lives. Small and `Copy` so the in-memory index can hold one per record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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
}

pub(crate) struct SegmentLog {
    directory: Arc<PathBuf>,
    owner: Arc<str>,
    tail: Mutex<Tail>,
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
                let remaining = &bytes[offset..];
                let incomplete = remaining.len() < HEADER_BYTES
                    || HEADER_BYTES
                        + u32::from_le_bytes(remaining[..4].try_into().unwrap()) as usize
                        > remaining.len();
                if *id != tail.segment || !incomplete {
                    return Err(Error::Journal(format!(
                        "corrupt journal segment {id} at offset {offset}"
                    )));
                }
                let file = OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .map_err(log_error)?;
                file.set_len(offset as u64).map_err(log_error)?;
                file.sync_all().map_err(log_error)?;
            }
            if *id == tail.segment {
                tail.offset = offset as u64;
            }
        }

        Ok(Self {
            directory: Arc::new(directory.to_path_buf()),
            owner,
            tail: Mutex::new(tail),
            writer,
        })
    }

    pub(crate) fn from_checkpoint(
        directory: &Path,
        owner: Arc<str>,
        writer: Arc<Writer>,
        segment: u64,
        offset: u64,
    ) -> Self {
        Self {
            directory: Arc::new(directory.to_path_buf()),
            owner,
            tail: Mutex::new(Tail { segment, offset }),
            writer,
        }
    }

    /// Assigns locations to encoded frames after the writer has selected their request.
    pub(crate) fn prepare(
        &self,
        mut encoded: Vec<Vec<u8>>,
        first_sequence: u64,
        recorded_at_ms: u64,
    ) -> Result<(Vec<Location>, Vec<writer::Frame>), Error> {
        let mut tail = self.tail.lock().map_err(|_| poisoned())?;
        let mut locations = Vec::with_capacity(encoded.len());
        let mut frames = Vec::with_capacity(encoded.len());
        for (offset, mut bytes) in encoded.drain(..).enumerate() {
            patch_metadata(&mut bytes, first_sequence + offset as u64, recorded_at_ms)?;
            let length = bytes.len() as u64;
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
            locations.push(location);
            frames.push(writer::Frame {
                path: segment_path(&self.directory, location.segment),
                bytes,
            });
        }
        Ok((locations, frames))
    }

    pub(crate) fn read_many<T>(
        &self,
        locations: &[Location],
        mut read: impl FnMut(Frame<'_>) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let mut frames = Vec::with_capacity(locations.len());
        let mut index = 0;
        while index < locations.len() {
            let location = locations[index];
            // The longest run that shares a segment.
            let mut end = index;
            while end < locations.len() && locations[end].segment == location.segment {
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

    pub(crate) fn writer(&self) -> &Arc<Writer> {
        &self.writer
    }

    pub(crate) fn owner(&self) -> Arc<str> {
        self.owner.clone()
    }

    #[cfg(test)]
    fn append(self: &Arc<Self>, append: Append<'_>) -> Result<Location, Error> {
        let encoded = encode(&append)?;
        let bytes = encoded.len() as u64;
        let sequence = append.sequence;
        let recorded_at_ms = append.recorded_at_ms;
        let log = self.clone();
        let result = Arc::new(Mutex::new(None));
        let committed = result.clone();
        let ticket = self.writer.submit_sync(
            self.owner.clone(),
            bytes,
            Box::new(move || {
                let (mut locations, frames) = log
                    .prepare(vec![encoded], sequence, recorded_at_ms)
                    .map_err(|error| error.to_string())?;
                let location = locations.remove(0);
                Ok(writer::Prepared {
                    frames,
                    complete: Box::new(move || {
                        *committed
                            .lock()
                            .map_err(|_| "journal test result poisoned".to_owned())? =
                            Some(location);
                        Ok(())
                    }),
                })
            }),
        )?;
        ticket.wait()?;
        result
            .lock()
            .map_err(|_| Error::Journal("journal test result poisoned".into()))?
            .take()
            .ok_or_else(|| Error::Journal("journal test append produced no location".into()))
    }

    #[cfg(test)]
    fn append_background(self: &Arc<Self>, append: Append<'_>) -> Result<(), Error> {
        let encoded = encode(&append)?;
        let bytes = encoded.len() as u64;
        let sequence = append.sequence;
        let recorded_at_ms = append.recorded_at_ms;
        let log = self.clone();
        self.writer.submit_async(
            self.owner.clone(),
            bytes,
            Box::new(move || {
                let (_, frames) = log
                    .prepare(vec![encoded], sequence, recorded_at_ms)
                    .map_err(|error| error.to_string())?;
                Ok(writer::Prepared {
                    frames,
                    complete: Box::new(|| Ok(())),
                })
            }),
        )?;
        Ok(())
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

pub(crate) fn encode_unsequenced(
    kind: &str,
    payload: &serde_json::Value,
) -> Result<Vec<u8>, Error> {
    encode(&Append {
        sequence: 0,
        recorded_at_ms: 0,
        kind,
        payload,
    })
}

fn patch_metadata(bytes: &mut [u8], sequence: u64, recorded_at_ms: u64) -> Result<(), Error> {
    if bytes.len() < HEADER_BYTES + 16 {
        return Err(Error::Journal("journal frame is too short".into()));
    }
    bytes[HEADER_BYTES..HEADER_BYTES + 8].copy_from_slice(&sequence.to_le_bytes());
    bytes[HEADER_BYTES + 8..HEADER_BYTES + 16].copy_from_slice(&recorded_at_ms.to_le_bytes());
    let check = xxhash_rust::xxh3::xxh3_64(&bytes[HEADER_BYTES..]).to_le_bytes();
    bytes[4..HEADER_BYTES].copy_from_slice(&check);
    Ok(())
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
    ) -> (Arc<SegmentLog>, Vec<(u64, String, Location)>) {
        let mut seen = Vec::new();
        let log = SegmentLog::open(directory, Arc::from("test"), writer, |frame, location| {
            seen.push((frame.sequence, frame.kind.to_string(), location));
            Ok(())
        })
        .unwrap();
        (Arc::new(log), seen)
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
    fn a_record_is_readable_after_commit() {
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
        let producer = {
            let log = log.clone();
            let body = body.clone();
            thread::spawn(move || {
                for sequence in 1..=64 {
                    log.append_background(append(sequence, "big", &body))
                        .unwrap();
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
    fn complete_corruption_and_torn_nonfinal_segments_are_never_truncated() {
        for nonfinal in [false, true] {
            let directory = temporary();
            fs::create_dir_all(&directory).unwrap();
            let mut bytes = encode(&append(1, "k", &payload("k"))).unwrap();
            if nonfinal {
                bytes.truncate(bytes.len() - 1);
            } else {
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
            }
            let path = segment_path(&directory, 0);
            fs::write(&path, &bytes).unwrap();
            if nonfinal {
                fs::write(segment_path(&directory, 1), []).unwrap();
            }
            assert!(
                SegmentLog::open(
                    &directory,
                    Arc::from("test"),
                    Writer::spawn(),
                    |_, _| Ok(())
                )
                .is_err()
            );
            assert_eq!(fs::read(path).unwrap(), bytes);
            fs::remove_dir_all(directory).unwrap();
        }
    }
}
