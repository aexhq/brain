//! What the journal costs a turn, alone and beside other sessions.
//!
//! Ignored by default because it reports rather than asserts: the numbers move with
//! the disk and the machine, and a threshold that passes on a laptop says nothing
//! about a server. A foreground append includes the writer's group commit and disk
//! sync. The second measurement shows whether one writer thread is enough: if the p90
//! at many sessions is far above the p90 at one, the writer is the bottleneck.
//!
//!     cargo test --release -p brain --test journal_throughput -- --ignored --nocapture

use std::{sync::Arc, time::Instant};

use brain::{AppendRecord, Feed, LocalSessionStore, SessionStore, SessionUpdate, Writer};
use brain_protocol::{SessionId, SessionStatus};

const RECORDS: u64 = 20_000;
const PAYLOAD_BYTES: usize = 1024;
const PAGE: usize = 1_000;

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    sorted[((sorted.len() as f64 * fraction) as usize).min(sorted.len() - 1)]
}

fn temporary(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "brain-journal-throughput-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn store(
    directory: &std::path::Path,
    id: &str,
    writer: &Arc<Writer>,
    feed: &Arc<Feed>,
) -> Arc<LocalSessionStore> {
    let store = LocalSessionStore::create(
        &directory.join(id),
        SessionId::new(id),
        &serde_json::json!({}),
        writer.clone(),
        feed.clone(),
    )
    .unwrap();
    store
        .append_sync(
            &[AppendRecord::new(
                "session_creation_ended",
                serde_json::json!({"configuration": {}}),
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
#[ignore = "reports timings; run it deliberately"]
fn reports_what_the_journal_costs() {
    let directory = temporary("one");
    let writer = Writer::spawn();
    let (publisher, _worker) = brain_telemetry::telemetry_channel();
    let feed = Arc::new(Feed::new(publisher));
    let store = store(&directory, "ses_throughput", &writer, &feed);

    let payload = serde_json::json!({ "text": "x".repeat(PAYLOAD_BYTES) });
    let mut latencies = Vec::with_capacity(RECORDS as usize);
    let started = Instant::now();
    for _ in 0..RECORDS {
        let at = Instant::now();
        store
            .append_sync(
                &[AppendRecord::new("model_call_ended", payload.clone())],
                SessionUpdate::default(),
            )
            .unwrap();
        latencies.push(at.elapsed().as_secs_f64() * 1e6);
    }
    let appending = started.elapsed().as_secs_f64();
    latencies.sort_by(|left, right| left.partial_cmp(right).unwrap());

    let at = Instant::now();
    let page = store.records_after(0, PAGE).unwrap();
    let paging = at.elapsed().as_secs_f64() * 1e3;
    assert_eq!(page.len(), PAGE);
    writer.sync().unwrap();
    drop(store);

    let at = Instant::now();
    let reopened =
        LocalSessionStore::open(&directory.join("ses_throughput"), writer.clone(), feed).unwrap();
    let replay = at.elapsed().as_secs_f64() * 1e3;
    assert_eq!(
        reopened.session_row().unwrap().through_sequence,
        RECORDS + 1
    );
    drop(reopened);

    println!(
        "append {RECORDS} x {PAYLOAD_BYTES} B   {:.0} records/s   p50 {:.2} us   p99 {:.2} us\n\
         page {PAGE} records   {paging:.2} ms\n\
         reopen {RECORDS} records   {replay:.2} ms",
        RECORDS as f64 / appending,
        percentile(&latencies, 0.5),
        percentile(&latencies, 0.99),
    );
    drop(writer);
    let _ = std::fs::remove_dir_all(directory);
}

/// Many sessions appending at once through the one writer. Reported at 1, 16 and 128
/// sessions so the append latency's dependence on the session count is visible.
#[test]
#[ignore = "reports timings; run it deliberately"]
fn reports_what_the_journal_costs_beside_other_sessions() {
    const PER_SESSION: u64 = 2_000;
    for sessions in [1_usize, 16, 128] {
        let directory = temporary(&format!("many-{sessions}"));
        let writer = Writer::spawn();
        let (publisher, _worker) = brain_telemetry::telemetry_channel();
        let feed = Arc::new(Feed::new(publisher));
        let stores: Vec<Arc<LocalSessionStore>> = (0..sessions)
            .map(|index| store(&directory, &format!("ses_{index:04}"), &writer, &feed))
            .collect();
        let payload = Arc::new(serde_json::json!({ "text": "x".repeat(PAYLOAD_BYTES) }));
        let started = Instant::now();
        let threads: Vec<_> = stores
            .iter()
            .cloned()
            .map(|store| {
                let payload = payload.clone();
                std::thread::spawn(move || {
                    let mut latencies = Vec::with_capacity(PER_SESSION as usize);
                    for _ in 0..PER_SESSION {
                        let at = Instant::now();
                        store
                            .append_sync(
                                &[AppendRecord::new("model_call_ended", (*payload).clone())],
                                SessionUpdate::default(),
                            )
                            .unwrap();
                        latencies.push(at.elapsed().as_secs_f64() * 1e6);
                    }
                    latencies
                })
            })
            .collect();
        let mut latencies: Vec<f64> = threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect();
        let elapsed = started.elapsed().as_secs_f64();
        writer.sync().unwrap();
        let drained = started.elapsed().as_secs_f64();
        latencies.sort_by(|left, right| left.partial_cmp(right).unwrap());
        println!(
            "{sessions:>4} sessions x {PER_SESSION} records   append p50 {:.2} us   p90 {:.2} us   p99 {:.2} us   {:.0} records/s appended   {:.0} records/s to disk",
            percentile(&latencies, 0.5),
            percentile(&latencies, 0.9),
            percentile(&latencies, 0.99),
            latencies.len() as f64 / elapsed,
            latencies.len() as f64 / drained,
        );
        drop(stores);
        drop(writer);
        let _ = std::fs::remove_dir_all(directory);
    }
}
