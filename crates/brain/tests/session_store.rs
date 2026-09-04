//! One session's store: two logs under one sequence counter, a transcript that folds out
//! of deltas and checkpoints, and slots that keep their last value.

use std::{fs, path::PathBuf, sync::Arc};

use brain::{
    AppendRecord, Feed, Folded, JournalEntry, JournalStore, SessionStore, SessionUpdate, Writer,
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

fn created(directory: &std::path::Path, writer: &Arc<Writer>) -> Arc<SessionStore> {
    let store = SessionStore::create(
        &directory.join("ses_1"),
        SessionId::new("ses_1"),
        &serde_json::json!({"system": "test"}),
        writer.clone(),
        feed(),
    )
    .unwrap();
    store
        .append(
            0,
            &[AppendRecord::new(
                "session_creation_ended",
                serde_json::json!({"configuration": {"system": "test"}}),
            )],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                context: None,
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
fn deltas_keep_a_prefix_and_append_the_rest() {
    let directory = temporary("delta");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    let through = store
        .append_journal(
            1,
            &[JournalEntry::ContextDelta {
                keep: 0,
                append: vec![user("a"), user("b"), user("c")],
            }],
        )
        .unwrap();
    assert_eq!(through, 2);
    // The loop rewrote the tail: keep `a`, replace the rest.
    store
        .append_journal(
            2,
            &[JournalEntry::ContextDelta {
                keep: 1,
                append: vec![user("d")],
            }],
        )
        .unwrap();
    let folded = store.fold().unwrap();
    assert_eq!(folded.transcript, vec![user("a"), user("d")]);
    assert_eq!(folded.through_sequence, 3);
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn a_slot_keeps_its_last_value() {
    let directory = temporary("slot");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_journal(
            1,
            &[
                JournalEntry::Slot {
                    name: "loop".into(),
                    value: serde_json::json!({"summary": null}),
                },
                JournalEntry::Slot {
                    name: "loop".into(),
                    value: serde_json::json!({"summary": "so far"}),
                },
                JournalEntry::Slot {
                    name: "tool".into(),
                    value: serde_json::json!(7),
                },
            ],
        )
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
fn a_checkpoint_resets_the_fold_and_the_bytes_since() {
    let directory = temporary("checkpoint");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append_journal(
            1,
            &[JournalEntry::ContextDelta {
                keep: 0,
                append: vec![user("old"), user("older")],
            }],
        )
        .unwrap();
    assert!(store.journal_bytes_since_checkpoint().unwrap() > 0);
    store
        .append_journal(
            2,
            &[JournalEntry::Checkpoint {
                transcript: vec![user("summary")],
                slots: [("loop".to_string(), serde_json::json!(1))]
                    .into_iter()
                    .collect(),
            }],
        )
        .unwrap();
    assert_eq!(store.journal_bytes_since_checkpoint().unwrap(), 0);
    store
        .append_journal(
            3,
            &[JournalEntry::ContextDelta {
                keep: 1,
                append: vec![user("after")],
            }],
        )
        .unwrap();
    let folded = store.fold().unwrap();
    assert_eq!(folded.transcript, vec![user("summary"), user("after")]);
    assert_eq!(folded.slots["loop"], serde_json::json!(1));
    drop(store);
    drop(writer);
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn both_logs_share_one_sequence_and_survive_reopening() {
    let directory = temporary("reopen");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    store
        .append(
            1,
            &[AppendRecord::new("turn_started", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Running),
                context: None,
                configuration: None,
            },
        )
        .unwrap();
    store
        .append_journal(
            2,
            &[JournalEntry::ContextDelta {
                keep: 0,
                append: vec![user("hi")],
            }],
        )
        .unwrap();
    store
        .append(
            3,
            &[AppendRecord::new("turn_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                context: None,
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

    let reopened = SessionStore::open(&directory.join("ses_1"), writer.clone(), feed()).unwrap();
    let row = reopened.session_row().unwrap();
    assert_eq!(row.through_sequence, 4);
    assert!(matches!(row.status, SessionStatus::Idle));
    assert_eq!(row.configuration, serde_json::json!({"system": "test"}));
    assert_eq!(reopened.fold().unwrap().transcript, vec![user("hi")]);
    assert!(reopened.take_restored());
    assert!(!reopened.take_restored());
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
        .append(
            1,
            &[AppendRecord::new("turn_started", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Running),
                context: None,
                configuration: None,
            },
        )
        .unwrap();
    writer.sync().unwrap();
    drop(store);

    let stores = SessionStore::open_all(&directory, writer.clone(), feed()).unwrap();
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
fn an_append_at_the_wrong_position_is_a_conflict() {
    let directory = temporary("conflict");
    let writer = Writer::spawn();
    let store = created(&directory, &writer);
    let error = store
        .append(
            0,
            &[AppendRecord::new("turn_started", serde_json::json!({}))],
            SessionUpdate::default(),
        )
        .unwrap_err();
    assert!(matches!(error, brain::Error::Conflict(_)), "{error}");
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
        .append(
            1,
            &[AppendRecord::new("session_ended", serde_json::json!({}))],
            SessionUpdate {
                status: Some(SessionStatus::Ended),
                context: None,
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
