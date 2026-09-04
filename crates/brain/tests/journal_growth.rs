//! The journal must not grow with the *square* of a turn's model-call count or of a
//! session's turn count.
//!
//! The transcript grows monotonically, so anything the runtime writes whole per model
//! call or per turn costs the sum of every intermediate size, not the final one. The
//! journal records deltas, and a checkpoint only once the deltas since the last one
//! outweigh the transcript, so the log stays a constant multiple of the transcript.

mod common;

use std::{fs, sync::Arc, time::Duration};

use brain::{Error, JournalStore};
use brain_protocol::{ContentBlock, Message, MessageRequest, ModelRequest, TurnOutput};
use brain_telemetry::telemetry_channel;
use common::{NoTools, Runtime, ScriptedModel, config, dir_bytes, scripted, temporary_directory};

/// Bytes each model call adds to the transcript. Large enough that the quadratic term
/// dominates fixed per-record overhead, small enough to keep the test quick.
const ITEM_BYTES: usize = 16 * 1024;

/// Model calls in the measured turn.
const CALLS: usize = 64;

/// The journal may hold a constant number of copies of the final transcript plus
/// per-record overhead, not one copy per call or per turn.
const MAX_JOURNAL_TO_TRANSCRIPT_RATIO: u64 = 8;

fn filler(index: usize) -> Message {
    Message::user_text(format!("{index:04}{}", "x".repeat(ITEM_BYTES)))
}

/// A loop that makes `calls` model calls, growing the transcript by one filler message
/// before each, and keeps every answer.
fn growing_loop(calls: usize) -> Arc<common::ScriptedLoop> {
    scripted(move |input, services| async move {
        let mut transcript = input.transcript;
        let base = transcript.len();
        for index in 0..calls {
            transcript.push(filler(base + index));
            let result = services
                .model(ModelRequest {
                    system: None,
                    tools: None,
                    messages: transcript.clone(),
                    response_format: None,
                    max_output_tokens: Some(16),
                })
                .await?;
            transcript.push(result.message);
        }
        Ok(TurnOutput {
            transcript,
            slots: Default::default(),
            result: Some(serde_json::json!({"ok": true})),
        })
    })
}

fn runtime(data_dir: &std::path::Path, calls: usize) -> Runtime {
    let (publisher, _worker) = telemetry_channel();
    Runtime::open(
        data_dir,
        publisher,
        calls.max(1),
        brain::DEFAULT_TOOL_DEADLINE_MS,
        growing_loop(calls),
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    )
}

/// Runs one turn of `CALLS` model calls and returns the closed journal's size on disk
/// alongside the session it wrote.
async fn measure_one_turn(data_dir: &std::path::Path) -> (u64, brain_protocol::SessionId) {
    let runtime = runtime(data_dir, CALLS);
    let handle = runtime.create(&config(), &[]).unwrap();
    let session_id = handle.id().clone();
    let finished = handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    assert!(
        matches!(finished.status, brain_protocol::SessionStatus::Idle),
        "the turn must finish, not hit its budget"
    );
    drop(handle);
    runtime.drain();
    drop(runtime);
    tokio::time::sleep(Duration::from_millis(20)).await;
    (dir_bytes(&data_dir.join("sessions")), session_id)
}

#[tokio::test]
async fn a_turn_does_not_journal_one_transcript_copy_per_model_call() {
    let data_dir = temporary_directory("growth-calls");
    let (journal_bytes, _) = measure_one_turn(&data_dir).await;
    let transcript_bytes = (CALLS * ITEM_BYTES) as u64;
    let ratio = journal_bytes as f64 / transcript_bytes as f64;
    let _ = fs::remove_dir_all(&data_dir);
    assert!(
        journal_bytes <= transcript_bytes * MAX_JOURNAL_TO_TRANSCRIPT_RATIO,
        "journal grew to {journal_bytes} bytes for a {transcript_bytes}-byte transcript \
         ({ratio:.1}x, bound {MAX_JOURNAL_TO_TRANSCRIPT_RATIO}x): the runtime is writing the \
         whole transcript once per model call"
    );
}

#[tokio::test]
async fn the_journal_folds_to_the_final_transcript_after_the_turn() {
    let data_dir = temporary_directory("growth-fold");
    let (_, session_id) = measure_one_turn(&data_dir).await;
    let (publisher, _worker) = telemetry_channel();
    let store = brain::SessionStore::open(
        &data_dir.join("sessions").join(session_id.as_str()),
        brain::Writer::spawn(),
        Arc::new(brain::Feed::new(publisher)),
    )
    .unwrap();
    let folded = store.fold().unwrap();
    assert_eq!(
        folded.transcript.len(),
        CALLS * 2,
        "every filler message and every answer folds back out of the journal"
    );
    assert!(folded.slots.contains_key(brain::LAST_ACTIVATION_SLOT));
    drop(store);
    let _ = fs::remove_dir_all(data_dir);
}

/// A restart closes a turn the last process left running with `turn_failed` and code
/// `interrupted`, and the session is idle and usable afterwards.
#[tokio::test]
async fn an_interrupted_turn_is_closed_and_recorded() {
    let data_dir = temporary_directory("growth-interrupt");
    let (_, session_id) = measure_one_turn(&data_dir).await;
    {
        let (publisher, _worker) = telemetry_channel();
        let writer = brain::Writer::spawn();
        let store = brain::SessionStore::open(
            &data_dir.join("sessions").join(session_id.as_str()),
            writer.clone(),
            Arc::new(brain::Feed::new(publisher)),
        )
        .unwrap();
        let row = store.session_row().unwrap();
        store
            .append(
                row.through_sequence,
                &[brain::AppendRecord::new(
                    "turn_started",
                    serde_json::json!({"content": "and then the lights went out"}),
                )],
                brain::SessionUpdate {
                    status: Some(brain_protocol::SessionStatus::Running),
                    configuration: None,
                },
            )
            .unwrap();
        writer.sync().unwrap();
        drop(store);
    }
    let reopened = runtime(&data_dir, CALLS);
    let session = reopened.session(&session_id);
    assert!(
        matches!(session.status, brain_protocol::SessionStatus::Idle),
        "an interrupted turn must leave the session able to take another: {:?}",
        session.status
    );
    let events = reopened.events(&session_id, 0, 1_000).events;
    let last = events.last().expect("the session has records");
    assert_eq!(last.event_type, "turn_failed");
    assert_eq!(last.data["code"], "interrupted");
    drop(reopened);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn a_session_comes_back_from_its_journal() {
    let data_dir = temporary_directory("growth-reopen");
    let (_, session_id) = measure_one_turn(&data_dir).await;
    let (publisher, _worker) = telemetry_channel();
    let store = brain::SessionStore::open(
        &data_dir.join("sessions").join(session_id.as_str()),
        brain::Writer::spawn(),
        Arc::new(brain::Feed::new(publisher)),
    )
    .expect("a session must be rebuilt from the records it left behind");
    let row = store.session_row().unwrap();
    assert!(matches!(row.status, brain_protocol::SessionStatus::Idle));
    let records = store.records_after(0, 1_000).unwrap();
    assert_eq!(
        records.first().map(|record| record.kind.as_str()),
        Some("session_creation_started")
    );
    assert!(
        records
            .windows(2)
            .all(|pair| pair[1].sequence > pair[0].sequence)
    );
    drop(store);
    let _ = fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn a_session_does_not_journal_one_transcript_copy_per_turn() {
    const TURNS: usize = 64;
    let data_dir = temporary_directory("growth-turns");
    let runtime = runtime(&data_dir, 1);
    let handle = runtime.create(&config(), &[]).unwrap();
    for _ in 0..TURNS {
        handle
            .message(MessageRequest { input: "go".into() })
            .await
            .unwrap();
    }
    let session_id = handle.id().clone();
    drop(handle);
    runtime.drain();
    let folded = runtime.store(&session_id).fold().unwrap();
    assert_eq!(folded.transcript.len(), TURNS * 2);
    drop(runtime);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let journal_bytes = dir_bytes(&data_dir.join("sessions"));
    let transcript_bytes = (TURNS * ITEM_BYTES) as u64;
    let ratio = journal_bytes as f64 / transcript_bytes as f64;
    let _ = fs::remove_dir_all(data_dir);
    assert!(
        journal_bytes <= transcript_bytes * MAX_JOURNAL_TO_TRANSCRIPT_RATIO,
        "a {TURNS}-turn session journalled {journal_bytes} bytes for a {transcript_bytes}-byte \
         transcript ({ratio:.1}x, bound {MAX_JOURNAL_TO_TRANSCRIPT_RATIO}x): the runtime is \
         writing the whole transcript once per turn"
    );
}

/// The assembled answer is the durable truth: `model_call_ended` carries it once, and
/// the deltas it arrived in are never written.
#[tokio::test]
async fn a_turn_does_not_journal_the_pieces_its_answer_arrived_in() {
    let data_dir = temporary_directory("growth-pieces");
    let runtime = runtime(&data_dir, 1);
    let handle = runtime.create(&config(), &[]).unwrap();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let events = runtime.events(handle.id(), 0, 1_000).events;
    let ended: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "model_call_ended")
        .collect();
    assert_eq!(ended.len(), 1);
    assert!(ended[0].data["result"]["message"]["content"].is_array());
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "assistant_delta"),
        "deltas are not records"
    );
    drop(handle);
    runtime.drain();
    drop(runtime);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// Rewriting the transcript deep inside it is allowed and journalled as a delta from
/// the point of change, never as a whole copy.
#[tokio::test]
async fn a_rewritten_transcript_is_journalled_from_where_it_differs() {
    let data_dir = temporary_directory("growth-rewrite");
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        4,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        scripted(|input, _services| async move {
            let mut transcript = input.transcript;
            if transcript.len() >= 3 {
                // Compact: replace everything with one summary.
                transcript = vec![Message::assistant(vec![ContentBlock::text("summary")])];
            }
            transcript.push(Message::user_text(input.input.message));
            Ok::<_, Error>(TurnOutput {
                transcript,
                slots: Default::default(),
                result: None,
            })
        }),
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    for _ in 0..5 {
        handle
            .message(MessageRequest { input: "go".into() })
            .await
            .unwrap();
    }
    let folded = runtime.store(handle.id()).fold().unwrap();
    assert_eq!(
        folded.transcript.len(),
        3,
        "compaction happened and the fold agrees"
    );
    assert_eq!(
        folded.transcript[0],
        Message::assistant(vec![ContentBlock::text("summary")])
    );
    drop(handle);
    runtime.drain();
    drop(runtime);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = fs::remove_dir_all(data_dir);
}
