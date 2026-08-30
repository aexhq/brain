//! Per-turn cost against turn index, with the fixture's own cost separated out.
//!
//! The `round_trip` probe reports a p50 over a whole run, which cannot tell a constant
//! per-turn cost from one that grows: a longer run simply moves the median. This walks one
//! conversation turn by turn and records, for every turn, what the client saw and what the
//! scripted provider spent serving that turn's model call. The provider is handed the whole
//! transcript every turn, so its own parse and serialise cost grows with the conversation
//! too — subtracting it is the only way to say what part of the growth is the subject's.
//!
//! It also records what the subject wrote for that turn: the size of the session state file
//! it rewrites, the bytes on disk under its data directory, and the write counters of its
//! own process. A store that appends costs the same on turn 500 as on turn 1; a store that
//! rewrites the conversation does not, and these columns say which this is.
//!
//! Nothing here changes the subject. Every column is read from outside it — the client's
//! clock, the benchmark's own fixture, the filesystem, and `/proc`.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{drivers, fixtures, launch, mem, subject};

pub struct Options {
    pub subjects: PathBuf,
    pub subject: String,
    /// Turns in one conversation. The curve is the point, so this wants to be long.
    pub turns: usize,
    /// Independent conversations, each on a freshly started subject with an empty data
    /// directory. One run is an anecdote; the shape has to reproduce.
    pub repeats: usize,
    pub agentloop: PathBuf,
    pub out: PathBuf,
}

/// One turn, as seen from every side at once.
struct Row {
    repeat: usize,
    turn: usize,
    /// What the client timed: send until the turn came back.
    round_trip_ms: f64,
    /// What the scripted provider spent inside itself on this turn's model calls.
    fixture_ms: f64,
    /// Of that, the part spent reading the request body off the socket, which can be
    /// waiting on the subject rather than the fixture working.
    fixture_read_ms: f64,
    provider_calls: usize,
    /// Request body the subject sent the provider on this turn's last call.
    request_bytes: u64,
    messages: usize,
    /// The session state file the subject rewrites, after this turn.
    state_bytes: u64,
    /// Everything under the subject's journal directory, after this turn. Held apart from
    /// the data directory below, which is dominated by the admitted agentloop package and
    /// would bury a few kilobytes of journal in twenty megabytes of constant.
    journal_bytes: u64,
    /// Everything under the subject's data directory, after this turn.
    dir_bytes: u64,
    /// Bytes this turn passed to `write(2)` in the subject's own process, and of those,
    /// the bytes that reached the storage layer.
    wchar_delta: u64,
    write_bytes_delta: u64,
    /// The same, across the whole process tree, so a loop worker's writes are visible.
    tree_wchar_delta: u64,
}

pub async fn run(options: Options) -> Result<()> {
    let subjects = subject::load_all(&options.subjects)?;
    let entry = subjects
        .iter()
        .find(|entry| entry.name == options.subject)
        .with_context(|| format!("no subject named {}", options.subject))?;
    let launch_block = entry
        .launch
        .as_ref()
        .context("this experiment starts the subject itself, so it needs a launch block")?;

    if let Some(parent) = options.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(&options.out)
        .with_context(|| format!("creating {}", options.out.display()))?;
    writeln!(
        out,
        "repeat,turn,round_trip_ms,fixture_ms,fixture_read_ms,provider_calls,request_bytes,messages,state_bytes,journal_bytes,dir_bytes,wchar_delta,write_bytes_delta,tree_wchar_delta"
    )?;

    for repeat in 1..=options.repeats {
        let rows = one_conversation(entry, launch_block, &options, repeat).await?;
        for row in &rows {
            writeln!(
                out,
                "{},{},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{}",
                row.repeat,
                row.turn,
                row.round_trip_ms,
                row.fixture_ms,
                row.fixture_read_ms,
                row.provider_calls,
                row.request_bytes,
                row.messages,
                row.state_bytes,
                row.journal_bytes,
                row.dir_bytes,
                row.wchar_delta,
                row.write_bytes_delta,
                row.tree_wchar_delta,
            )?;
        }
        out.flush()?;
        summarize(&rows, repeat);
    }
    eprintln!("wrote {}", options.out.display());
    Ok(())
}

async fn one_conversation(
    entry: &subject::Subject,
    launch_block: &subject::Launch,
    options: &Options,
    repeat: usize,
) -> Result<Vec<Row>> {
    let provider = fixtures::scripted_provider(
        "ok",
        vec![serde_json::json!({"id": "call_bench", "name": "echo", "arguments": "{}"})],
    )
    .await?;
    let environment = std::sync::Arc::new(fixtures::echo_environment().await?);
    let run_id = format!("growth-{repeat}-{}", std::process::id());

    let mut running = launch::start(
        launch_block,
        &entry.name,
        &run_id,
        &provider.base_url,
        &environment.base_url,
    )
    .await?;

    let result = drive(
        entry,
        options,
        repeat,
        &provider,
        &environment,
        &running.base_url,
        running.pid,
        &running.data_dir,
    )
    .await;

    provider.shutdown();
    environment.shutdown();
    running.stop().await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    entry: &subject::Subject,
    options: &Options,
    repeat: usize,
    provider: &fixtures::Fixture,
    environment: &std::sync::Arc<fixtures::Fixture>,
    base_url: &str,
    pid: u32,
    data_dir: &Path,
) -> Result<Vec<Row>> {
    let bench = drivers::Bench {
        base_url: base_url.to_owned(),
        agentloop_package: options.agentloop.clone(),
        pid: Some(pid),
        environment: std::sync::Arc::clone(environment),
        model_base_url: provider.base_url.clone(),
    };
    let mut driver = drivers::for_subject(&entry.name, &bench)?
        .with_context(|| format!("no driver for {}", entry.name))?;
    driver.prepare().await?;
    let unit = driver.create().await?;

    // Found rather than assumed. The state file's directory is the journal's, which is
    // nested inside the data directory the subject was told to use — and guessing that
    // path wrong is silent: `metadata` simply fails and every turn records zero bytes.
    let state_name = format!("{}.state", unit.id);
    let state_file = find(data_dir, &state_name);
    let journal = state_file
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.to_path_buf());
    anyhow::ensure!(
        state_file.is_some(),
        "no {state_name} anywhere under {}; a wrong guess at that path is silent, and every bytes-per-turn reading would be zero",
        data_dir.display(),
    );
    let mut rows = Vec::with_capacity(options.turns);
    // Anything the subject asked the provider while creating the session belongs to
    // neither turn; starting the mark at zero would charge all of it to turn 1.
    let mut served = provider.timings().len();
    let mut io = process_io(pid);
    let mut tree = tree_wchar(pid);

    for turn in 1..=options.turns {
        let round_trip_ms = driver.round_trip_ms(&unit).await?;

        // Everything below happens between turns, never inside the timed region.
        let timings = provider.timings();
        let this_turn = &timings[served.min(timings.len())..];
        let fixture_ms = this_turn
            .iter()
            .map(|call| call.service_ns as f64 / 1_000_000.0)
            .sum();
        let fixture_read_ms = this_turn
            .iter()
            .map(|call| call.read_ns as f64 / 1_000_000.0)
            .sum();
        let last = this_turn.last();
        served = timings.len();

        let next_io = process_io(pid);
        let next_tree = tree_wchar(pid);
        rows.push(Row {
            repeat,
            turn,
            round_trip_ms,
            fixture_ms,
            fixture_read_ms,
            provider_calls: this_turn.len(),
            request_bytes: last.map(|call| call.request_bytes).unwrap_or(0),
            messages: last.map(|call| call.messages).unwrap_or(0),
            state_bytes: state_file
                .as_ref()
                .and_then(|path| std::fs::metadata(path).ok())
                .map(|meta| meta.len())
                .unwrap_or(0),
            journal_bytes: directory_bytes(&journal),
            dir_bytes: directory_bytes(data_dir),
            wchar_delta: next_io.0.saturating_sub(io.0),
            write_bytes_delta: next_io.1.saturating_sub(io.1),
            tree_wchar_delta: next_tree.saturating_sub(tree),
        });
        io = next_io;
        tree = next_tree;
    }

    driver.destroy(&unit).await?;
    Ok(rows)
}

/// Mean of a column over each block of `BUCKET` turns, printed as the run goes so a
/// broken run is visible before it finishes rather than after.
const BUCKET: usize = 50;

fn summarize(rows: &[Row], repeat: usize) {
    eprintln!(
        "repeat {repeat}: turns {:>4} | {:>9} {:>9} {:>9} | {:>10} {:>10} {:>10}",
        "", "round_trip", "fixture", "net", "state KiB", "req KiB", "wchar KiB"
    );
    for chunk in rows.chunks(BUCKET) {
        let n = chunk.len() as f64;
        let mean = |value: fn(&Row) -> f64| chunk.iter().map(value).sum::<f64>() / n;
        eprintln!(
            "repeat {repeat}: turns {:>4} | {:>9.3} {:>9.3} {:>9.3} | {:>10.1} {:>10.1} {:>10.1}",
            format!(
                "{}-{}",
                chunk.first().map(|row| row.turn).unwrap_or(0),
                chunk.last().map(|row| row.turn).unwrap_or(0)
            ),
            mean(|row| row.round_trip_ms),
            mean(|row| row.fixture_ms),
            mean(|row| row.round_trip_ms - row.fixture_ms),
            mean(|row| row.state_bytes as f64 / 1024.0),
            mean(|row| row.request_bytes as f64 / 1024.0),
            mean(|row| row.wchar_delta as f64 / 1024.0),
        );
    }
}

/// `wchar` and `write_bytes` for one process: bytes handed to `write(2)`, and of those,
/// the bytes that reached the storage layer. Zero where `/proc` cannot be read, which is
/// every platform but Linux.
fn process_io(pid: u32) -> (u64, u64) {
    let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/io")) else {
        return (0, 0);
    };
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("wchar:"), field("write_bytes:"))
}

fn tree_wchar(root: u32) -> u64 {
    mem::descendants(root)
        .iter()
        .map(|pid| process_io(*pid).0)
        .sum()
}

/// The first file named `name` anywhere under `directory`.
fn find(directory: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => directories.push(entry.path()),
            Ok(_) if entry.file_name() == name => return Some(entry.path()),
            _ => {}
        }
    }
    directories.iter().find_map(|child| find(child, name))
}

/// Every byte under `directory`, following subdirectories. Unreadable entries count zero:
/// a subject writing while this walks is normal, and an answer that is a hair low beats no
/// answer.
fn directory_bytes(directory: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_bytes(&entry.path()),
            Ok(_) => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}
