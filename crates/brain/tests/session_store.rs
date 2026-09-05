//! One session's store: one journal, a transcript that folds from deltas, and
//! Agentloop state with last-write-wins values.

use std::{fs, path::PathBuf, sync::Arc};

use brain::{
    AppendRecord, Feed, Folded, JournalEntry, LocalSessionStore, SessionStore, SessionUpdate,
    Writer,
};
use brain_protocol::{Message, SessionId, SessionStatus};

fn temporary(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "brain-session-store-{name}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn feed() -> Arc<Feed> {
    let (publisher, worker) = brain_telemetry::telemetry_channel();
    // The worker is dropped here; publishing to a closed channel is a non-blocking miss.
    drop(worker);
    Arc::new(Feed::new(publisher))
}

fn user(text: &str) -> Message {
    Message::user_text(text)
}

fn created(directory: &std::path::Path, writer: &Arc<Writer>) -> Arc<LocalSessionStore> {
    let store = LocalSessionStore::create(
        &directory.join("ses_1"),
        SessionId::new("ses_1"),
        &serde_json::json!({"system": "test"}),
        writer.clone(),
        feed(),
    )
    .unwrap();
    store
        .append_sync(
            &[AppendRecord::new(
                "session_creation_ended",
                serde_json::json!({"configuration": {"system": "test"}}),
            )],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                configuration: None,
            },
        )
        .unwrap();
    store
}

#[test]
fn a_new_session_folds_to_nothing() {
    let directory = temporary("empty");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    assert_eq!(
        store.fold().unwrap(),
        Folded {
            transcript: Vec::new(),
            slots: Default::default(),
            through_sequence: 1,
        }
    );
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn checkpoints_resume_and_rebuild_when_stale_or_damaged() {
    let directory = temporary("checkpoint");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 0,
            append: vec![user("one")],
        }])
        .unwrap();
    store.checkpoint().unwrap();
    let snapshot = store.fold().unwrap();
    drop(store);
    let path = directory.join("ses_1");
    let store = LocalSessionStore::open(&path, writer.clone(), feed()).unwrap();
    assert_eq!(store.fold().unwrap(), snapshot);
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 1,
            append: vec![user("two")],
        }])
        .unwrap();
    let expected = store.fold().unwrap();
    drop(store);
    let store = LocalSessionStore::open(&path, writer.clone(), feed()).unwrap();
    assert_eq!(store.fold().unwrap(), expected);
    store.checkpoint().unwrap();
    drop(store);
    fs::write(path.join("checkpoint"), b"damaged").unwrap();
    let store = LocalSessionStore::open(&path, writer, feed()).unwrap();
    assert_eq!(store.fold().unwrap(), expected);
    assert_eq!(store.records_after(0, 100).unwrap().len(), 1);
    drop(store);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn independent_storage_can_construct_a_commit_completion() {
    let handle = brain::CommitHandle::from_completion(|| Ok(Vec::new()));
    assert!(handle.wait().unwrap().is_empty());
    assert!(
        brain::CommitHandle::from_completion(|| Err(brain::Error::Journal("commit failed".into())))
            .wait()
            .is_err()
    );
}

#[test]
fn an_extension_event_cannot_disguise_an_unfinished_effect() {
    let directory = temporary("pending-kind");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    let records = store
        .append_sync(
            &[AppendRecord::new(
                "tool_call_started",
                serde_json::json!({}),
            )],
            SessionUpdate::default(),
        )
        .unwrap();
    store
        .append_sync(
            &[AppendRecord::new(
                "custom_ended",
                serde_json::json!({"sequence": records[0].sequence}),
            )],
            SessionUpdate::default(),
        )
        .unwrap();
    assert!(store.interrupt_unfinished_turn().unwrap());
    assert_eq!(
        store
            .records_after(records[0].sequence, 100)
            .unwrap()
            .last()
            .unwrap()
            .kind,
        "tool_call_failed"
    );
    drop(store);
    drop(writer);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn deltas_keep_a_prefix_and_append_the_rest() {
    let directory = temporary("delta");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    let through = store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 0,
            append: vec![user("a"), user("b"), user("c")],
        }])
        .unwrap();
    assert_eq!(through, 2);
    // The loop rewrote the tail: keep `a`, replace the rest.
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 1,
            append: vec![user("d")],
        }])
        .unwrap();
    let folded = store.fold().unwrap();
    assert_eq!(folded.transcript, vec![user("a"), user("d")]);
    assert_eq!(folded.through_sequence, 3);
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn transcript_replacement_is_an_event_projection_after_reopen() {
    let directory = temporary("transcript-replaced");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 0,
            append: vec![user("a"), user("b"), user("c")],
        }])
        .unwrap();
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 1,
            append: vec![user("summary")],
        }])
        .unwrap();
    writer.sync().unwrap();
    drop(store);

    let reopened =
        LocalSessionStore::open(&directory.join("ses_1"), writer.clone(), feed()).unwrap();
    let replacement = reopened
        .records_after(0, 100)
        .unwrap()
        .into_iter()
        .find(|record| record.kind == "transcript_replaced")
        .expect("the canonical transcript mutation is projected as an Event");
    assert_eq!(replacement.sequence, 3);
    assert_eq!(replacement.payload["keep"], 1);
    assert_eq!(
        replacement.payload["append"][0],
        serde_json::json!(user("summary"))
    );
    assert_eq!(
        reopened.fold().unwrap().transcript,
        vec![user("a"), user("summary")]
    );
    drop(reopened);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn a_slot_keeps_its_last_value() {
    let directory = temporary("slot");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_journal_sync(&[
            JournalEntry::StateSet {
                name: "loop".into(),
                value: serde_json::json!({"summary": null}),
            },
            JournalEntry::StateSet {
                name: "loop".into(),
                value: serde_json::json!({"summary": "so far"}),
            },
            JournalEntry::StateSet {
                name: "tool".into(),
                value: serde_json::json!(7),
            },
        ])
        .unwrap();
    let folded = store.fold().unwrap();
    assert_eq!(
        folded.slots["loop"],
        serde_json::json!({"summary": "so far"})
    );
    assert_eq!(folded.slots["tool"], serde_json::json!(7));
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn one_journal_keeps_sequence_and_survives_reopening() {
    let directory = temporary("reopen");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_sync(
            &[AppendRecord::new("turn_started", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Running),
                configuration: None,
            },
        )
        .unwrap();
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 0,
            append: vec![user("hi")],
        }])
        .unwrap();
    store
        .append_sync(
            &[AppendRecord::new("turn_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                configuration: None,
            },
        )
        .unwrap();
    // The events page skips the journal's sequence but keeps every event's own number.
    let events = store.records_after(0, 100).unwrap();
    let sequences: Vec<u64> = events.iter().map(|record| record.sequence).collect();
    assert_eq!(sequences, vec![1, 2, 4]);
    writer.sync().unwrap();
    drop(store);

    let reopened =
        LocalSessionStore::open(&directory.join("ses_1"), writer.clone(), feed()).unwrap();
    let row = reopened.session_row().unwrap();
    assert_eq!(row.through_sequence, 4);
    assert!(matches!(row.status, SessionStatus::Idle));
    assert_eq!(row.configuration, serde_json::json!({"system": "test"}));
    assert_eq!(reopened.fold().unwrap().transcript, vec![user("hi")]);
    drop(reopened);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn a_running_session_is_interrupted_when_reopened() {
    let directory = temporary("interrupt");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_sync(
            &[AppendRecord::new("turn_started", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Running),
                configuration: None,
            },
        )
        .unwrap();
    writer.sync().unwrap();
    drop(store);

    let stores = LocalSessionStore::open_all(&directory, writer.clone(), feed()).unwrap();
    assert_eq!(stores.len(), 1);
    let row = stores[0].session_row().unwrap();
    assert!(matches!(row.status, SessionStatus::Idle));
    let last = stores[0].records_after(0, 100).unwrap().pop().unwrap();
    assert_eq!(last.kind, "turn_failed");
    assert_eq!(last.payload["code"], "interrupted");
    assert_eq!(last.payload["ambiguous"], true);
    drop(stores);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn a_background_append_gets_its_sequence_when_it_commits() {
    let directory = temporary("background");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    let commit = store
        .append_async(vec![AppendRecord::new("queued", serde_json::json!({}))])
        .unwrap();
    let saved = commit.wait().unwrap();
    assert_eq!(saved[0].sequence, 2);
    assert_eq!(store.records_after(0, 10).unwrap().len(), 2);
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn an_event_page_is_bounded_by_bytes_and_always_makes_progress() {
    let directory = temporary("bounded-page");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    let payload = serde_json::json!({"data": "x".repeat(5 * 1024 * 1024)});
    store
        .append_sync(
            &[
                AppendRecord::new("large_1", payload.clone()),
                AppendRecord::new("large_2", payload),
            ],
            SessionUpdate::default(),
        )
        .unwrap();
    let first = store.records_after(1, 1_000).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].sequence, 2);
    let second = store.records_after(first[0].sequence, 1_000).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].sequence, 3);
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn only_an_ended_session_can_be_deleted() {
    let directory = temporary("delete");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    assert!(store.delete().is_err());
    store
        .append_sync(
            &[AppendRecord::new("session_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Ended),
                configuration: None,
            },
        )
        .unwrap();
    store.delete().unwrap();
    assert!(!directory.join("ses_1").exists());
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}
