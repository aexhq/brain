//! The one thread in the process that writes journal bytes to disk.
//!
//! Every session's logs append through it. An append hands the writer a frame and
//! returns; the writer batches what is queued, writes each file's frames in the order
//! they were handed over, flushes, and only then lets the log forget the staged copy, so
//! a reader that misses the staged copy finds the bytes on disk and never neither.
//!
//! Sessions do not wait on each other's disk writes. What they share is the queue: each
//! owner has its own allowance, so a session that appends faster than the disk drains
//! waits on its own bytes and nobody else's, and one process-wide cap turns a stalled
//! disk into slow appends rather than a process that grows until it is killed.
//!
//! Nothing here fsyncs. The log exists so a restart can rebuild what was in memory; a
//! crash may lose its tail, and recovery truncates at the first frame that does not
//! verify.

use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread::{self, JoinHandle},
};

use crate::Error;

/// Bytes one owner may have queued before its next append waits.
pub const OWNER_QUEUE_BYTES: u64 = 8 * 1024 * 1024;

/// Bytes the whole process may have queued before any append waits.
pub const MAX_QUEUED_BYTES: u64 = 64 * 1024 * 1024;

/// Files the writer keeps open between batches. Above this the least recently written
/// is closed; a session that appends again simply reopens its segment.
const OPEN_FILES: usize = 256;

/// Frames handed to the writer but not yet flushed, keyed by segment and offset. One per
/// log; the writer removes an entry after the bytes are on disk.
pub(crate) type Staged = Arc<Mutex<HashMap<(u64, u64), Arc<Vec<u8>>>>>;

pub(crate) struct Frame {
    pub path: Arc<PathBuf>,
    pub owner: Arc<str>,
    pub key: (u64, u64),
    pub bytes: Arc<Vec<u8>>,
    pub staged: Staged,
}

enum Message {
    Frame(Frame),
    /// Answered once everything queued before it is on disk.
    Sync(mpsc::Sender<()>),
    Reclaim {
        directory: PathBuf,
        keep_from: u64,
        extension: &'static str,
    },
}

#[derive(Default)]
struct Queue {
    total: u64,
    per_owner: HashMap<Arc<str>, u64>,
    /// Set once. A write-behind log has no caller left to tell when a write fails, so
    /// the failure is kept and every later append reports it: losing records silently is
    /// the one outcome worth refusing.
    failure: Option<String>,
}

type Backlog = Arc<(Mutex<Queue>, Condvar)>;

pub struct Writer {
    sender: Mutex<Option<mpsc::Sender<Message>>>,
    backlog: Backlog,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Writer {
    pub fn spawn() -> Arc<Self> {
        Self::spawn_inner(None)
    }

    /// A writer that waits, before its first frame, until `release` is set. A test can
    /// then fill the queue behind it and see what happens without racing it.
    #[cfg(test)]
    pub(crate) fn spawn_held(release: Arc<std::sync::atomic::AtomicBool>) -> Arc<Self> {
        Self::spawn_inner(Some(release))
    }

    fn spawn_inner(hold: Option<Arc<std::sync::atomic::AtomicBool>>) -> Arc<Self> {
        let backlog: Backlog = Arc::default();
        let (sender, receiver) = mpsc::channel::<Message>();
        let handle = run(receiver, backlog.clone(), hold);
        Arc::new(Self {
            sender: Mutex::new(Some(sender)),
            backlog,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// Wait until `owner` and the process are far enough below their allowances to
    /// accept `bytes`, and refuse outright if the writer has already failed. A frame
    /// larger than a whole allowance goes through once that queue is empty rather than
    /// waiting for room that can never appear.
    pub(crate) fn reserve(&self, owner: &Arc<str>, bytes: u64) -> Result<(), Error> {
        let (queue, room) = &*self.backlog;
        let mut queue = queue.lock().map_err(|_| poisoned())?;
        loop {
            if let Some(failure) = &queue.failure {
                return Err(Error::Journal(format!(
                    "journal writer failed and the log is no longer being written: {failure}"
                )));
            }
            let owned = queue.per_owner.get(owner).copied().unwrap_or(0);
            let owner_fits = owned == 0 || owned + bytes <= OWNER_QUEUE_BYTES;
            let total_fits = queue.total == 0 || queue.total + bytes <= MAX_QUEUED_BYTES;
            if owner_fits && total_fits {
                queue.total += bytes;
                *queue.per_owner.entry(owner.clone()).or_default() += bytes;
                return Ok(());
            }
            queue = room.wait(queue).map_err(|_| poisoned())?;
        }
    }

    pub(crate) fn write(&self, frame: Frame) -> Result<(), Error> {
        self.send(Message::Frame(frame))
    }

    pub(crate) fn reclaim(
        &self,
        directory: PathBuf,
        keep_from: u64,
        extension: &'static str,
    ) -> Result<(), Error> {
        self.send(Message::Reclaim {
            directory,
            keep_from,
            extension,
        })
    }

    /// Returns once every frame queued before the call is on disk.
    pub fn sync(&self) -> Result<(), Error> {
        let (done, wait) = mpsc::channel();
        self.send(Message::Sync(done))?;
        wait.recv()
            .map_err(|_| Error::Journal("journal writer stopped".into()))
    }

    /// Bytes handed over and not yet on disk, across every log.
    pub fn queued_bytes(&self) -> u64 {
        self.backlog.0.lock().map(|queue| queue.total).unwrap_or(0)
    }

    fn send(&self, message: Message) -> Result<(), Error> {
        let sender = self.sender.lock().map_err(|_| poisoned())?;
        sender
            .as_ref()
            .ok_or_else(|| Error::Journal("journal writer is shut down".into()))?
            .send(message)
            .map_err(|_| Error::Journal("journal writer stopped".into()))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Close the channel so the writer drains what is queued, then wait for it. A
        // process that shuts down cleanly loses nothing; one that crashes loses its tail.
        if let Ok(mut sender) = self.sender.lock() {
            *sender = None;
        }
        if let Some(handle) = self.handle.lock().ok().and_then(|mut handle| handle.take()) {
            let _ = handle.join();
        }
    }
}

struct Open {
    file: BufWriter<File>,
    /// The batch this file was last written in; the least recently written is closed
    /// first when too many are open.
    last_batch: u64,
}

fn run(
    receiver: mpsc::Receiver<Message>,
    backlog: Backlog,
    hold: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut held = hold;
        let mut open: HashMap<PathBuf, Open> = HashMap::new();
        let mut batch = 0_u64;
        // What this batch wrote, released and unstaged once it is flushed.
        let mut written: Vec<(Staged, (u64, u64))> = Vec::new();
        let mut drained: Vec<(Arc<str>, u64)> = Vec::new();
        let mut syncs: Vec<mpsc::Sender<()>> = Vec::new();
        let mut touched: Vec<PathBuf> = Vec::new();
        while let Ok(first) = receiver.recv() {
            batch += 1;
            let mut next = Some(first);
            while let Some(message) = next.take() {
                match message {
                    Message::Frame(frame) => {
                        if let Some(release) = held.take() {
                            // Capped so a test that never releases fails on its own
                            // assertions rather than hanging the job.
                            let until =
                                std::time::Instant::now() + std::time::Duration::from_secs(30);
                            while !release.load(std::sync::atomic::Ordering::Acquire)
                                && std::time::Instant::now() < until
                            {
                                thread::sleep(std::time::Duration::from_millis(1));
                            }
                        }
                        drained.push((frame.owner.clone(), frame.bytes.len() as u64));
                        if !open.contains_key(frame.path.as_path()) {
                            if open.len() >= OPEN_FILES {
                                evict_oldest(&mut open);
                            }
                            match OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(frame.path.as_path())
                            {
                                Ok(file) => {
                                    open.insert(
                                        frame.path.as_ref().clone(),
                                        Open {
                                            file: BufWriter::with_capacity(1 << 20, file),
                                            last_batch: batch,
                                        },
                                    );
                                }
                                Err(error) => {
                                    fail(&backlog, format!("cannot open a segment: {error}"));
                                }
                            }
                        }
                        match open.get_mut(frame.path.as_path()) {
                            Some(entry) => {
                                entry.last_batch = batch;
                                match entry.file.write_all(&frame.bytes) {
                                    Ok(()) => {
                                        written.push((frame.staged, frame.key));
                                        if !touched.contains(frame.path.as_ref()) {
                                            touched.push(frame.path.as_ref().clone());
                                        }
                                    }
                                    Err(error) => {
                                        fail(&backlog, format!("cannot write a frame: {error}"))
                                    }
                                }
                            }
                            None => fail(&backlog, "the segment is not open".to_owned()),
                        }
                    }
                    Message::Sync(done) => syncs.push(done),
                    Message::Reclaim {
                        directory,
                        keep_from,
                        extension,
                    } => reclaim_segments(&directory, keep_from, extension, &mut open),
                }
                next = receiver.try_recv().ok();
            }
            for path in touched.drain(..) {
                if let Some(entry) = open.get_mut(&path) {
                    let _ = entry.file.flush();
                }
            }
            // Flushed first, unstaged second: a reader that misses the staged copy finds
            // the bytes on disk, never neither.
            for (staged, key) in written.drain(..) {
                if let Ok(mut staged) = staged.lock() {
                    staged.remove(&key);
                }
            }
            release(&backlog, std::mem::take(&mut drained));
            for done in syncs.drain(..) {
                let _ = done.send(());
            }
        }
        for entry in open.values_mut() {
            let _ = entry.file.flush();
        }
    })
}

fn evict_oldest(open: &mut HashMap<PathBuf, Open>) {
    let Some(path) = open
        .iter()
        .min_by_key(|(_, entry)| entry.last_batch)
        .map(|(path, _)| path.clone())
    else {
        return;
    };
    if let Some(mut entry) = open.remove(&path) {
        let _ = entry.file.flush();
    }
}

/// Hand back the queue allowance for bytes that have reached the disk, and wake whoever
/// is waiting for room.
fn release(backlog: &Backlog, drained: Vec<(Arc<str>, u64)>) {
    let (queue, room) = &**backlog;
    if let Ok(mut queue) = queue.lock() {
        for (owner, bytes) in drained {
            queue.total = queue.total.saturating_sub(bytes);
            let remaining = match queue.per_owner.get_mut(&owner) {
                Some(owned) => {
                    *owned = owned.saturating_sub(bytes);
                    *owned
                }
                None => 0,
            };
            if remaining == 0 {
                queue.per_owner.remove(&owner);
            }
        }
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

fn reclaim_segments(
    directory: &Path,
    keep_from: u64,
    extension: &str,
    open: &mut HashMap<PathBuf, Open>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let below = path.extension().is_some_and(|found| found == extension)
            && path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u64>().ok())
                .is_some_and(|id| id < keep_from);
        if below {
            open.remove(&path);
            let _ = std::fs::remove_file(path);
        }
    }
}

fn poisoned() -> Error {
    Error::Journal("journal writer queue poisoned".into())
}
