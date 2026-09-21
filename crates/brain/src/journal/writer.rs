//! The one process thread that sequences and durably commits session records.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread::{self, JoinHandle},
};

use crate::Error;

const MAX_BATCH_REQUESTS: usize = 128;

pub(crate) struct Frame {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

pub(crate) struct Prepared {
    pub frames: Vec<Frame>,
    pub complete: Box<dyn FnOnce() -> Result<(), String> + Send>,
}

type Prepare = Box<dyn FnOnce() -> Result<Prepared, String> + Send>;

enum Request {
    Write {
        owner: Arc<str>,
        bytes: u64,
        prepare: Option<Prepare>,
        done: mpsc::Sender<Result<(), String>>,
    },
    Barrier(mpsc::Sender<Result<(), String>>),
    Delete {
        directory: PathBuf,
        done: mpsc::Sender<Result<(), String>>,
    },
}

#[derive(Default)]
struct Queue {
    synchronous: VecDeque<Request>,
    asynchronous: VecDeque<Request>,
    total: u64,
    per_owner: HashMap<Arc<str>, u64>,
    failure: Option<String>,
    closed: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

pub(crate) struct Ticket {
    wait: mpsc::Receiver<Result<(), String>>,
}

impl Ticket {
    pub(crate) fn wait(self) -> Result<(), Error> {
        self.wait
            .recv()
            .map_err(|_| Error::Journal("journal writer stopped".into()))?
            .map_err(Error::Journal)
    }
}

pub struct Writer {
    shared: Shared,
    queue_bytes: u64,
    owner_bytes: u64,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Writer {
    /// A writer under the default limits.
    pub fn spawn() -> Arc<Self> {
        Self::spawn_with(&crate::Limits::default())
    }

    pub fn spawn_with(limits: &crate::Limits) -> Arc<Self> {
        Self::spawn_inner(limits, None)
    }

    #[cfg(test)]
    pub(crate) fn spawn_held(release: Arc<std::sync::atomic::AtomicBool>) -> Arc<Self> {
        Self::spawn_inner(&crate::Limits::default(), Some(release))
    }

    fn spawn_inner(
        limits: &crate::Limits,
        hold: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> Arc<Self> {
        let shared: Shared = Arc::default();
        let handle = run(
            shared.clone(),
            crate::limits::ceiling(limits.max_journal_open_files),
            hold,
        );
        Arc::new(Self {
            shared,
            queue_bytes: crate::limits::ceiling_u64(limits.max_journal_queue_bytes),
            owner_bytes: crate::limits::ceiling_u64(limits.max_session_queue_bytes),
            handle: Mutex::new(Some(handle)),
        })
    }

    pub(crate) fn submit_sync(
        &self,
        owner: Arc<str>,
        bytes: u64,
        prepare: Prepare,
    ) -> Result<Ticket, Error> {
        self.submit(owner, bytes, prepare, true)
    }

    pub(crate) fn submit_async(
        &self,
        owner: Arc<str>,
        bytes: u64,
        prepare: Prepare,
    ) -> Result<Ticket, Error> {
        self.submit(owner, bytes, prepare, false)
    }

    fn submit(
        &self,
        owner: Arc<str>,
        bytes: u64,
        prepare: Prepare,
        synchronous: bool,
    ) -> Result<Ticket, Error> {
        self.reserve(&owner, bytes)?;
        let (done, wait) = mpsc::channel();
        self.enqueue(
            Request::Write {
                owner,
                bytes,
                prepare: Some(prepare),
                done,
            },
            synchronous,
        )?;
        Ok(Ticket { wait })
    }

    /// Waits for every request admitted before this call.
    pub fn sync(&self) -> Result<(), Error> {
        let (done, wait) = mpsc::channel();
        self.enqueue(Request::Barrier(done), false)?;
        Ticket { wait }.wait()
    }

    pub(crate) fn delete(&self, directory: PathBuf) -> Result<(), Error> {
        let (done, wait) = mpsc::channel();
        self.enqueue(Request::Delete { directory, done }, false)?;
        Ticket { wait }.wait()
    }

    pub fn queued_bytes(&self) -> u64 {
        self.shared.0.lock().map(|queue| queue.total).unwrap_or(0)
    }

    fn reserve(&self, owner: &Arc<str>, bytes: u64) -> Result<(), Error> {
        let (queue, room) = &*self.shared;
        let mut queue = queue.lock().map_err(|_| poisoned())?;
        loop {
            if let Some(failure) = &queue.failure {
                return Err(failed(failure));
            }
            if queue.closed {
                return Err(Error::Journal("journal writer is shut down".into()));
            }
            let owned = queue.per_owner.get(owner).copied().unwrap_or(0);
            let owner_fits = owned == 0 || owned.saturating_add(bytes) <= self.owner_bytes;
            let total_fits =
                queue.total == 0 || queue.total.saturating_add(bytes) <= self.queue_bytes;
            if owner_fits && total_fits {
                queue.total += bytes;
                *queue.per_owner.entry(owner.clone()).or_default() += bytes;
                return Ok(());
            }
            queue = room.wait(queue).map_err(|_| poisoned())?;
        }
    }

    fn enqueue(&self, request: Request, synchronous: bool) -> Result<(), Error> {
        let (queue, ready) = &*self.shared;
        let mut queue = queue.lock().map_err(|_| poisoned())?;
        if let Some(failure) = &queue.failure {
            return Err(failed(failure));
        }
        if queue.closed {
            return Err(Error::Journal("journal writer is shut down".into()));
        }
        if synchronous {
            queue.synchronous.push_back(request);
        } else {
            queue.asynchronous.push_back(request);
        }
        ready.notify_one();
        Ok(())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let (_, ready) = &*self.shared;
        if let Ok(mut queue) = self.shared.0.lock() {
            queue.closed = true;
            ready.notify_one();
        }
        if let Some(handle) = self.handle.lock().ok().and_then(|mut handle| handle.take()) {
            let _ = handle.join();
        }
    }
}

struct Open {
    file: BufWriter<File>,
    last_batch: u64,
}

fn run(
    shared: Shared,
    open_files: usize,
    hold: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut held = hold;
        let mut open: HashMap<PathBuf, Open> = HashMap::new();
        let mut batch_number = 0_u64;
        loop {
            let mut requests = match take_batch(&shared) {
                Ok(Some(requests)) => requests,
                Ok(None) => break,
                Err(()) => return,
            };
            if let Some(release) = held.take() {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while !release.load(std::sync::atomic::Ordering::Acquire)
                    && std::time::Instant::now() < until
                {
                    thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            if let [Request::Delete { directory, done }] = requests.as_slice() {
                // Earlier batches are durable; closing their cached handles also
                // prevents a recreated session from writing into the deleted files.
                open.retain(|path, _| !path.starts_with(directory));
                let result = std::fs::remove_dir_all(directory)
                    .map_err(|error| format!("cannot delete session storage: {error}"))
                    .and_then(|()| directory.parent().map_or(Ok(()), sync_directory));
                let _ = done.send(result);
                continue;
            }
            batch_number += 1;
            let result = write_batch(&mut requests, &mut open, open_files, batch_number);
            finish_batch(&shared, requests, result.as_ref().err().cloned());
            if let Err(error) = result {
                fail_pending(&shared, error);
                break;
            }
        }
    })
}

fn take_batch(shared: &Shared) -> Result<Option<Vec<Request>>, ()> {
    let (queue, ready) = &**shared;
    let mut queue = queue.lock().map_err(|_| ())?;
    while queue.synchronous.is_empty() && queue.asynchronous.is_empty() && !queue.closed {
        queue = ready.wait(queue).map_err(|_| ())?;
    }
    if queue.synchronous.is_empty() && queue.asynchronous.is_empty() {
        return Ok(None);
    }
    let source = if queue.synchronous.is_empty() {
        &mut queue.asynchronous
    } else {
        &mut queue.synchronous
    };
    let mut requests = Vec::with_capacity(source.len().min(MAX_BATCH_REQUESTS));
    while requests.len() < MAX_BATCH_REQUESTS {
        if !requests.is_empty() && matches!(source.front(), Some(Request::Delete { .. })) {
            break;
        }
        let Some(request) = source.pop_front() else {
            break;
        };
        requests.push(request);
        if matches!(requests.last(), Some(Request::Delete { .. })) {
            break;
        }
    }
    Ok(Some(requests))
}

fn write_batch(
    requests: &mut [Request],
    open: &mut HashMap<PathBuf, Open>,
    open_files: usize,
    batch: u64,
) -> Result<(), String> {
    let mut prepared = Vec::new();
    for request in requests {
        if let Request::Write { prepare, .. } = request {
            let prepare = prepare
                .take()
                .ok_or_else(|| "journal request was already prepared".to_owned())?;
            prepared.push(prepare()?);
        }
    }

    let mut touched = HashSet::new();
    let mut created_directories = HashSet::new();
    for item in &prepared {
        for frame in &item.frames {
            if !open.contains_key(&frame.path) {
                if open.len() >= open_files
                    && let Some(path) = evict_oldest(open)?
                {
                    touched.remove(&path);
                }
                let created = !frame.path.exists();
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&frame.path)
                    .map_err(|error| format!("cannot open a journal segment: {error}"))?;
                if created && let Some(parent) = frame.path.parent() {
                    created_directories.insert(parent.to_path_buf());
                }
                open.insert(
                    frame.path.clone(),
                    Open {
                        file: BufWriter::with_capacity(1 << 20, file),
                        last_batch: batch,
                    },
                );
            }
            let entry = open
                .get_mut(&frame.path)
                .ok_or_else(|| "the journal segment is not open".to_owned())?;
            entry.last_batch = batch;
            entry
                .file
                .write_all(&frame.bytes)
                .map_err(|error| format!("cannot write a journal frame: {error}"))?;
            touched.insert(frame.path.clone());
        }
    }
    for path in touched {
        let entry = open
            .get_mut(&path)
            .ok_or_else(|| "the journal segment is not open".to_owned())?;
        entry
            .file
            .flush()
            .and_then(|()| entry.file.get_ref().sync_data())
            .map_err(|error| format!("cannot make a journal segment durable: {error}"))?;
    }
    for directory in created_directories {
        sync_directory(&directory)?;
    }
    for item in prepared {
        (item.complete)()?;
    }
    Ok(())
}

fn finish_batch(shared: &Shared, requests: Vec<Request>, failure: Option<String>) {
    let mut drained = Vec::new();
    let mut completed = Vec::new();
    for request in requests {
        match request {
            Request::Write {
                owner, bytes, done, ..
            } => {
                drained.push((owner, bytes));
                completed.push(done);
            }
            Request::Barrier(done) | Request::Delete { done, .. } => completed.push(done),
        }
    }
    release(shared, drained, failure.clone());
    for done in completed {
        let _ = done.send(match &failure {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        });
    }
}

fn fail_pending(shared: &Shared, error: String) {
    let pending = {
        let (queue, room) = &**shared;
        let Ok(mut queue) = queue.lock() else {
            return;
        };
        queue.failure.get_or_insert_with(|| error.clone());
        let mut pending = queue.synchronous.drain(..).collect::<Vec<_>>();
        pending.extend(queue.asynchronous.drain(..));
        room.notify_all();
        pending
    };
    finish_batch(shared, pending, Some(error));
}

fn release(shared: &Shared, drained: Vec<(Arc<str>, u64)>, failure: Option<String>) {
    let (queue, room) = &**shared;
    if let Ok(mut queue) = queue.lock() {
        if let Some(failure) = failure {
            queue.failure.get_or_insert(failure);
        }
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

fn evict_oldest(open: &mut HashMap<PathBuf, Open>) -> Result<Option<PathBuf>, String> {
    let Some(path) = open
        .iter()
        .min_by_key(|(_, entry)| entry.last_batch)
        .map(|(path, _)| path.clone())
    else {
        return Ok(None);
    };
    if let Some(mut entry) = open.remove(&path) {
        entry
            .file
            .flush()
            .and_then(|()| entry.file.get_ref().sync_data())
            .map_err(|error| format!("cannot close a journal segment durably: {error}"))?;
    }
    Ok(Some(path))
}

#[cfg(unix)]
fn sync_directory(path: &std::path::Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot make a journal directory durable: {error}"))
}

#[cfg(not(unix))]
fn sync_directory(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}

fn failed(failure: &str) -> Error {
    Error::Journal(format!(
        "journal writer failed and is no longer accepting records: {failure}"
    ))
}

fn poisoned() -> Error {
    Error::Journal("journal writer queue poisoned".into())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use super::*;

    fn job(order: Arc<Mutex<Vec<&'static str>>>, name: &'static str) -> Prepare {
        Box::new(move || {
            Ok(Prepared {
                frames: Vec::new(),
                complete: Box::new(move || {
                    order
                        .lock()
                        .map_err(|_| "test order poisoned".to_owned())?
                        .push(name);
                    Ok(())
                }),
            })
        })
    }

    #[test]
    fn a_batch_can_flush_more_files_than_the_handle_limit() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        let frames = [
            (first.clone(), b"one".to_vec()),
            (second.clone(), b"two".to_vec()),
            (first.clone(), b"three".to_vec()),
        ];
        let (done, _) = mpsc::channel();
        let mut requests = [Request::Write {
            owner: Arc::from("test"),
            bytes: 11,
            prepare: Some(Box::new(move || {
                Ok(Prepared {
                    frames: frames
                        .into_iter()
                        .map(|(path, bytes)| Frame { path, bytes })
                        .collect(),
                    complete: Box::new(|| Ok(())),
                })
            })),
            done,
        }];
        let mut open = HashMap::new();
        write_batch(&mut requests, &mut open, 1, 1).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(std::fs::read(first).unwrap(), b"onethree");
        assert_eq!(std::fs::read(second).unwrap(), b"two");
    }

    #[test]
    fn synchronous_work_passes_queued_background_work() {
        let writer = Writer::spawn();
        let (started, first_started) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let order = Arc::new(Mutex::new(Vec::new()));
        let prepare = job(order.clone(), "first");
        let first = writer
            .submit_async(
                Arc::from("first"),
                1,
                Box::new(move || {
                    started.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(30)).unwrap();
                    prepare()
                }),
            )
            .unwrap();
        first_started.recv_timeout(Duration::from_secs(30)).unwrap();
        let background = writer
            .submit_async(Arc::from("background"), 1, job(order.clone(), "background"))
            .unwrap();
        let synchronous = writer
            .submit_sync(Arc::from("sync"), 1, job(order.clone(), "sync"))
            .unwrap();
        release.send(()).unwrap();
        first.wait().unwrap();
        synchronous.wait().unwrap();
        background.wait().unwrap();
        assert_eq!(*order.lock().unwrap(), ["first", "sync", "background"]);
    }
}
