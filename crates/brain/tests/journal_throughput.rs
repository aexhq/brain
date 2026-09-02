//! What the journal costs a turn.
//!
//! Ignored by default because it reports rather than asserts: the numbers move with
//! the disk and the machine, and a threshold that passes on a laptop says nothing
//! about a server. The shape is what matters and does not move — an append is a
//! serialise, a hash of the bytes just serialised, and a channel send, with no
//! syscall on the turn's path, and a restart pays for the log it kept rather than
//! for every record ever written.
//!
//!     cargo test --release -p brain --test journal_throughput -- --ignored --nocapture

use std::time::Instant;

use brain::{AppendRecord, JournalStore, SegmentJournal, SessionUpdate, journal::SessionRow};
use brain_protocol::{SessionId, SessionStatus};

const RECORDS: u64 = 20_000;
const PAYLOAD_BYTES: usize = 1024;
const PAGE: usize = 1_000;

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    sorted[((sorted.len() as f64 * fraction) as usize).min(sorted.len() - 1)]
}

#[test]
#[ignore = "reports timings; run it deliberately"]
fn reports_what_the_journal_costs() {
    let directory = std::env::temp_dir().join(format!(
        "brain-journal-throughput-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SegmentJournal::open(&directory).unwrap();

    let session_id = SessionId::new("ses_throughput");
    store
        .create_session(
            &SessionRow {
                session_id: session_id.clone(),
                status: SessionStatus::Idle,
                through_sequence: 1,
                configuration: serde_json::json!({}),
                context: serde_json::json!({}),
            },
            AppendRecord::new("session_creation_ended", serde_json::json!({})),
        )
        .unwrap();

    let payload = serde_json::json!({ "text": "x".repeat(PAYLOAD_BYTES) });
    let mut latencies = Vec::with_capacity(RECORDS as usize);
    let started = Instant::now();
    for sequence in 0..RECORDS {
        let at = Instant::now();
        store
            .append(
                &session_id,
                sequence + 1,
                &[AppendRecord::new("model_result", payload.clone())],
                SessionUpdate::default(),
            )
            .unwrap();
        latencies.push(at.elapsed().as_secs_f64() * 1e6);
    }
    let appending = started.elapsed().as_secs_f64();
    latencies.sort_by(|left, right| left.partial_cmp(right).unwrap());

    let at = Instant::now();
    let page = store.records_after(&session_id, 0, PAGE).unwrap();
    let paging = at.elapsed().as_secs_f64() * 1e3;
    assert_eq!(page.len(), PAGE);
    drop(store);

    let at = Instant::now();
    let reopened = SegmentJournal::open(&directory).unwrap();
    let replay = at.elapsed().as_secs_f64() * 1e3;
    assert_eq!(
        reopened
            .session_row(&session_id)
            .unwrap()
            .unwrap()
            .through_sequence,
        RECORDS + 1
    );
    drop(reopened);

    println!(
        "append {RECORDS} x {PAYLOAD_BYTES} B   {:.0} records/s   p50 {:.2} us   p99 {:.2} us\n\
         page {PAGE} records          {paging:.1} ms\n\
         restart replay              {replay:.0} ms for {} records",
        RECORDS as f64 / appending,
        percentile(&latencies, 0.5),
        percentile(&latencies, 0.99),
        RECORDS + 1,
    );

    std::fs::remove_dir_all(directory).unwrap();
}
