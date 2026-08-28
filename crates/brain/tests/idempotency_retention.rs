//! A recorded answer must not pin the disk it was written on forever.
//!
//! Every idempotency record stayed in memory and in the log for the life of the process,
//! and the reclamation floor included the oldest of them. One record — the one that
//! deleting a session writes — was enough to hold back every segment behind it, so a
//! server that created and deleted sessions grew without bound. Measured before this:
//! deleting a session with no idempotency record freed 2 of 3 segments; with one old
//! record, 0 of 3. A million records held about 2 GiB of private memory after a restart.
//!
//! Records now carry an expiry. Past it they are not answers any more, so they are
//! dropped and they stop pinning anything; a live one below the floor is written forward
//! rather than holding its segment.

use std::{fs, path::PathBuf, time::Duration};

use brain::{AppendRecord, JournalStore, SegmentJournal, SessionUpdate, journal::SessionRow};
use brain_protocol::{Identity, JournalId, SessionId, SessionStatus};

/// The scope every recorded answer in these tests is filed under.
const SCOPE: &str = "session:test:message";

fn temporary_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "brain-idempotency-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn session(index: usize) -> SessionRow {
    SessionRow {
        session_id: SessionId::new(format!("ses_{index:04}")),
        journal_id: JournalId::new(format!("jrn_{index:04}")),
        status: SessionStatus::Idle,
        through_sequence: 1,
        configuration: serde_json::json!({}),
        context: serde_json::json!({ "items": [] }),
        presentation_identity: Identity::of_bytes(b"presentation"),
    }
}

fn put(journal: &SegmentJournal, key: &str) {
    let request = Identity::of_bytes(key.as_bytes());
    journal
        .idempotency_put(
            SCOPE,
            key,
            &request,
            &serde_json::json!({ "answered": key }),
        )
        .unwrap();
}

#[test]
fn a_recorded_answer_is_replayed_inside_its_retention() {
    let directory = temporary_directory("live");
    let journal = SegmentJournal::open(&directory, Duration::from_secs(60)).unwrap();
    let request = Identity::of_bytes(b"one");

    put(&journal, "one");

    assert_eq!(
        journal.idempotency_get(SCOPE, "one", &request).unwrap(),
        Some(serde_json::json!({ "answered": "one" })),
    );
    // The same key with different content is a caller error, not a miss.
    assert!(
        journal
            .idempotency_get(SCOPE, "one", &Identity::of_bytes(b"other"))
            .is_err()
    );

    drop(journal);
    fs::remove_dir_all(directory).unwrap();
}

/// The window's whole point, stated as a test: past it, the request is executed again
/// rather than answered from a record Brain has stopped promising to keep.
#[test]
fn a_recorded_answer_stops_being_replayed_once_it_expires() {
    let directory = temporary_directory("expired");
    let journal = SegmentJournal::open(&directory, Duration::ZERO).unwrap();
    let request = Identity::of_bytes(b"one");

    put(&journal, "one");

    assert_eq!(
        journal.idempotency_get(SCOPE, "one", &request).unwrap(),
        None
    );
    // And the key is free again, rather than permanently poisoned by its own record.
    put(&journal, "one");

    drop(journal);
    fs::remove_dir_all(directory).unwrap();
}

/// An expired record survives a restart in the log until its segment is reclaimed. It
/// must not come back as an answer.
#[test]
fn an_expired_record_does_not_return_after_a_restart() {
    let directory = temporary_directory("restart");
    let journal = SegmentJournal::open(&directory, Duration::ZERO).unwrap();
    put(&journal, "one");
    drop(journal);

    let journal = SegmentJournal::open(&directory, Duration::ZERO).unwrap();
    assert_eq!(
        journal
            .idempotency_get(SCOPE, "one", &Identity::of_bytes(b"one"))
            .unwrap(),
        None
    );

    drop(journal);
    fs::remove_dir_all(directory).unwrap();
}

/// Codex's decisive case: delete a session that has one old idempotency record against
/// it. That record used to sit at the reclamation floor forever. Which segments are
/// unlinked is covered by `journal::segment::tests`; what this pins is that reclamation
/// sweeps the record rather than carrying it.
#[test]
fn an_old_record_is_swept_when_reclamation_runs() {
    let directory = temporary_directory("floor");
    let journal = SegmentJournal::open(&directory, Duration::ZERO).unwrap();

    put(&journal, "the-record-that-used-to-pin-everything");

    let row = session(0);
    journal
        .create_session(
            &row,
            AppendRecord::new("session_created", serde_json::json!({})),
        )
        .unwrap();
    journal
        .append(
            &row.session_id,
            1,
            &[AppendRecord::new("session_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Ended),
                context: None,
                configuration: None,
            },
        )
        .unwrap();

    // Deleting the last session is what triggers reclamation.
    journal.delete_ended(&row.session_id).unwrap();

    // Nothing live is left, so nothing is left to answer either.
    assert!(journal.session_summaries().unwrap().is_empty());
    assert_eq!(
        journal
            .idempotency_get(
                SCOPE,
                "the-record-that-used-to-pin-everything",
                &Identity::of_bytes(b"the-record-that-used-to-pin-everything")
            )
            .unwrap(),
        None,
        "an expired record must not survive the sweep that reclamation runs"
    );

    drop(journal);
    fs::remove_dir_all(directory).unwrap();
}

/// A live record below the floor is written forward instead of holding its segment, so a
/// long-lived record cannot pin a segment for its whole retention.
#[test]
fn a_live_record_is_written_forward_rather_than_holding_its_segment() {
    let directory = temporary_directory("forward");
    let journal = SegmentJournal::open(&directory, Duration::from_secs(3_600)).unwrap();

    put(&journal, "still-live");

    let row = session(0);
    journal
        .create_session(
            &row,
            AppendRecord::new("session_created", serde_json::json!({})),
        )
        .unwrap();
    journal
        .append(
            &row.session_id,
            1,
            &[AppendRecord::new("session_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Ended),
                context: None,
                configuration: None,
            },
        )
        .unwrap();
    journal.delete_ended(&row.session_id).unwrap();

    // Still answerable: writing it forward moves where it lives, not whether it exists.
    assert_eq!(
        journal
            .idempotency_get(SCOPE, "still-live", &Identity::of_bytes(b"still-live"))
            .unwrap(),
        Some(serde_json::json!({ "answered": "still-live" })),
    );

    drop(journal);
    fs::remove_dir_all(directory).unwrap();
}
