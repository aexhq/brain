//! Reading a session must not copy the conversation it is not returning.
//!
//! A session row holds the whole configuration and the whole context. What a caller
//! outside the session can see is a few small fields. When the store answered a summary
//! with a cloned row, every list of N sessions deep-copied N configurations and N
//! conversations and then dropped them: a page of the session list allocated
//! proportionally to how much the sessions had been used, and startup recovery — which
//! reads only status and sequence — did the same for every session on disk.
//!
//! These tests bound the allocation rather than the wall time, because allocation is the
//! part that is deterministic on any machine.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use brain::{AppendRecord, Feed, JournalStore, SessionStore, SessionUpdate, Writer};
use brain_protocol::{SessionId, SessionStatus};

/// Counts bytes handed out, so a test can measure one call rather than a whole process.
///
/// Per thread, not per process: the tests in this binary run concurrently, and the
/// journal's writer thread allocates in the background, so a shared counter charged one
/// test for another's allocations and failed it — rarely, and only under load.
struct Counting;

std::thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

/// `try_with`, because an allocation during thread teardown must not touch a destroyed
/// thread-local; bytes allocated then are nobody's measurement.
fn count(bytes: usize) {
    let _ = ALLOCATED.try_with(|allocated| allocated.set(allocated.get() + bytes));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size.saturating_sub(layout.size()));
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn measure<T>(call: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCATED.with(Cell::get);
    let value = call();
    (value, ALLOCATED.with(Cell::get) - before)
}

/// Sessions in the listing.
const SESSIONS: usize = 8;

/// Context bytes per session. One turn of a real conversation is already this large.
const CONTEXT_BYTES: usize = 256 * 1024;

/// A listing may allocate for its own vector and its ids; it may not allocate a copy of
/// even one session's context. The bound sits far from both regimes.
const MAX_LISTING_BYTES: usize = CONTEXT_BYTES / 4;

fn stores_with_sessions(directory: &Path) -> (Vec<Arc<SessionStore>>, Arc<Writer>) {
    let writer = Writer::spawn();
    let (publisher, _worker) = brain_telemetry::telemetry_channel();
    let feed = Arc::new(Feed::new(publisher));
    let filler = serde_json::Value::String("x".repeat(CONTEXT_BYTES));
    let mut stores = Vec::with_capacity(SESSIONS);
    for index in 0..SESSIONS {
        let session_id = SessionId::new(format!("ses_{index:04}"));
        let store = SessionStore::create(
            &directory.join(session_id.as_str()),
            session_id,
            &serde_json::json!({ "configuration": filler }),
            writer.clone(),
            feed.clone(),
        )
        .unwrap();
        store
            .append(
                0,
                &[AppendRecord::new(
                    "session_creation_ended",
                    serde_json::json!({}),
                )],
                SessionUpdate {
                    status: Some(SessionStatus::Idle),
                    configuration: None,
                },
            )
            .unwrap();
        stores.push(store);
    }
    (stores, writer)
}

fn temporary_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "brain-listing-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn listing_sessions_does_not_copy_their_contexts() {
    let directory = temporary_directory("list");
    let (stores, writer) = stores_with_sessions(&directory);

    let (sessions, allocated) = measure(|| {
        stores
            .iter()
            .map(|store| store.session_summary().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(sessions.len(), SESSIONS);
    assert_eq!(sessions[0].session_id.as_str(), "ses_0000");
    assert!(
        allocated <= MAX_LISTING_BYTES,
        "listing {SESSIONS} sessions allocated {allocated} bytes for a \
         {CONTEXT_BYTES}-byte context each (bound {MAX_LISTING_BYTES}): the store is \
         cloning whole rows to answer a summary"
    );

    drop(stores);
    drop(writer);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reading_one_session_does_not_copy_its_context() {
    let directory = temporary_directory("get");
    let (stores, writer) = stores_with_sessions(&directory);

    let (session, allocated) = measure(|| stores[3].session_summary().unwrap());

    assert_eq!(session.session_id.as_str(), "ses_0003");
    assert!(
        allocated <= MAX_LISTING_BYTES,
        "reading one session allocated {allocated} bytes for a {CONTEXT_BYTES}-byte \
         context (bound {MAX_LISTING_BYTES})"
    );

    drop(stores);
    drop(writer);
    fs::remove_dir_all(directory).unwrap();
}

/// The full row is still available where it is genuinely needed — rehydrating a session
/// actor — and it still carries what the actor rebuilds itself from.
#[test]
fn the_full_row_is_still_reachable_for_rehydration() {
    let directory = temporary_directory("row");
    let (stores, writer) = stores_with_sessions(&directory);

    let row = stores[5].session_row().unwrap();

    assert_eq!(
        row.configuration["configuration"].as_str().unwrap().len(),
        CONTEXT_BYTES
    );

    drop(stores);
    drop(writer);
    fs::remove_dir_all(directory).unwrap();
}
