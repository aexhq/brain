//! Running one probe against one subject.
//!
//! Memory is not a probe of its own. A sampler runs in the background for whichever probe
//! is executing, so the same mechanism answers every memory question the benchmark has:
//! ramp sessions and the readings give memory *per session* as the slope of a fit; run it
//! during throughput and it is memory *under load*; keep sampling past the deletes and the
//! tail is *reclaim*. No forced allocator trim anywhere — no competitor exposes one, and a
//! number only Brain can produce is not a comparison.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::driver::Driver;
use crate::schema::{Datapoint, Evidence, Percentiles, Probe, ResidentKind};
use crate::{floor, host, mem, stats, subject};

/// How often the background sampler reads the process tree. Fast enough to show the shape
/// of a ramp, slow enough that reading `/proc` is not itself the load.
const MEMORY_INTERVAL: Duration = Duration::from_millis(250);
/// Steps in a session ramp. A slope wants several points; three is the minimum a fit will
/// How long the tail after the deletes is watched, and how the reclaim curve is sampled.
/// Left alone this long before the idle figure is taken, so what is measured is a
/// settled process and not the tail of its own start-up.
const IDLE_SETTLE: Duration = Duration::from_secs(10);
/// Long enough for a ramp's allocations to stop moving before the machine is asked.
const SETTLE: Duration = Duration::from_secs(5);

const RECLAIM_TAIL: Duration = Duration::from_secs(60);

pub struct Context_ {
    /// Machine memory available before this subject was started, in KiB. Every memory
    /// figure here is a difference from it.
    pub machine_baseline_kib: u64,
    pub samples: usize,
    pub units: usize,
    pub floor_ms: f64,
    pub concurrency: usize,
    /// The directory the subject was told to keep its state in, when the runner started
    /// it. `None` for a subject someone else is running, which is why a hosted service
    /// cannot answer the persistence probe.
    pub data_dir: Option<std::path::PathBuf>,
    /// Turns in one conversation for the persistence probe.
    pub turns: usize,
    /// The probe is abandoned past this instant, so one hung subject cannot eat a run's
    /// whole budget. Without it a stuck request costs `samples` × the client timeout.
    pub deadline: Instant,
    /// Launch access for the probes that restart the subject themselves; `None` for a
    /// subject someone else runs.
    pub relaunch: Option<Relaunch>,
}

pub fn probe_list(entry: &subject::Subject, wanted: &[String]) -> Vec<Probe> {
    // Every probe the subject declares, including ones with no runner yet: `measure`
    // refuses those with a reason, and a recorded refusal is worth more than a probe that
    // quietly vanishes. The order, and the guarantee that nothing is missing from it, live
    // on `Probe::ALL` beside the enum.
    Probe::ALL
        .into_iter()
        .filter(|probe| entry.probe(*probe).is_some())
        .filter(|probe| wanted.is_empty() || wanted.iter().any(|name| name == probe.as_str()))
        .collect()
}

pub async fn measure(
    subject_driver: &dyn Driver,
    entry: &subject::Subject,
    probe: Probe,
    context: &Context_,
) -> Result<Datapoint> {
    let spec = entry.probe(probe).context("probe not declared")?;
    let steal_window = host::StealWindow::open();
    // Sampling runs for every probe against a process we can read, so memory under load
    // comes free with the throughput arm rather than needing its own.
    let sampler = subject_driver
        .pid()
        .filter(|_| cfg!(target_os = "linux"))
        .map(|pid| mem::Sampler::start(pid, MEMORY_INTERVAL));

    let mut notes = Vec::new();
    let outcome = match probe {
        Probe::Resident => machine_ramp(subject_driver, context, &mut notes).await?,
        Probe::Idle => idle(context, &mut notes).await?,
        Probe::Throughput => throughput(subject_driver, context, &mut notes).await?,
        Probe::Persistence => persistence(subject_driver, context, &mut notes).await?,
        Probe::Reclaim => {
            host::require_private_memory_support()?;
            let sampler = sampler
                .as_ref()
                .context("no readable process for this subject; a hosted service needs a proxy")?;
            reclaim(subject_driver, sampler, context, &mut notes).await?
        }
        Probe::ToolDispatch | Probe::Create | Probe::Ttfb | Probe::RoundTrip => {
            latency(subject_driver, probe, context, &mut notes).await?
        }
        Probe::ColdStart => cold_start(context, &mut notes).await?,
        Probe::Recovery => recovery(context, &mut notes).await?,
        other => anyhow::bail!("{} has no runner yet", other.as_str()),
    };

    let steal = steal_window.close();
    if steal.exceeded {
        notes.push(format!(
            "hypervisor took {:.2}% of CPU during this probe; excluded from comparison tables",
            steal.fraction * 100.0
        ));
    }
    let memory_kib = sampler.map(mem::Sampler::finish).unwrap_or_default();
    let fit = matches!(probe, Probe::Resident)
        .then(|| mem::fit_against_units(&memory_kib))
        .flatten();

    // No fit at all means the ramp never moved: the readings were identical from the first
    // session to the last, so the slope is not "zero cost per session", it is the probe
    // failing to resolve one. Only a substrate whose sessions cost megabytes -- a MicroVM,
    // say -- rises clear of a host process's own drift here.
    anyhow::ensure!(
        !matches!(probe, Probe::Resident) || fit.is_some(),
        "resident memory did not vary with session count at all, so no per-session cost could be fitted; this probe cannot resolve sessions that cost less than the host process drifts"
    );

    if let Some(fit) = fit {
        // A ramp that does not fit a line has no single per-session cost, and quoting one
        // anyway is the error this check exists to catch. It used to be caught in a note
        // beside a number that stayed publishable, which is not catching it: the value
        // still reached a table, and for the in-process subjects it was noise -- several
        // measured a *negative* cost per session, memory falling as sessions were added.
        // A slope this weak is not a small number, it is not a number.
        anyhow::ensure!(
            fit.r2 >= 0.95,
            "memory against session count fits a line at only r²={:.3} (slope {:.3} KiB/session): per-session cost is not constant here, so there is no figure to quote. Either the subject's sessions cost too little to separate from this process's own drift, or the ramp was not clean.",
            fit.r2,
            fit.slope,
        );
    }

    Ok(Datapoint {
        subject: entry.name.clone(),
        subject_version: entry.version.clone(),
        class: entry.class,
        probe,
        definition: spec.definition.clone(),
        variant: None,
        evidence: Evidence::Measured,
        value: outcome.value,
        unit: outcome.unit,
        n: outcome.n,
        percentiles: outcome.percentiles,
        model_included: spec.model_included,
        limit_source: spec.limit_source,
        resident_kind: outcome.resident_kind.or(spec.resident_kind),
        // Only latency rows are judged against the generator's cost; a memory or
        // throughput figure is not the client's to distort in that way.
        generator_floor_ms: outcome.latency.then_some(context.floor_ms),
        memory_kib,
        fit,
        steal,
        notes,
    })
}

struct Outcome {
    value: f64,
    unit: String,
    n: usize,
    percentiles: Percentiles,
    resident_kind: Option<ResidentKind>,
    latency: bool,
}

async fn latency(
    subject_driver: &dyn Driver,
    probe: Probe,
    context: &Context_,
    notes: &mut Vec<String>,
) -> Result<Outcome> {
    let mut measured = Vec::with_capacity(context.samples);
    let mut abandoned = false;

    match probe {
        Probe::Create => {
            for _ in 0..context.samples {
                if Instant::now() >= context.deadline {
                    abandoned = true;
                    break;
                }
                let started = Instant::now();
                let unit = subject_driver.create().await?;
                measured.push(started.elapsed().as_secs_f64() * 1_000.0);
                subject_driver.destroy(&unit).await?;
            }
        }
        Probe::Ttfb | Probe::RoundTrip | Probe::ToolDispatch => {
            // One session, many turns: this measures the turn, and folding session
            // creation into it would measure something else.
            let unit = subject_driver.create().await?;
            for _ in 0..context.samples {
                if Instant::now() >= context.deadline {
                    abandoned = true;
                    break;
                }
                let sample = match probe {
                    Probe::Ttfb => subject_driver.ttfb_ms(&unit).await,
                    Probe::RoundTrip => subject_driver.round_trip_ms(&unit).await,
                    _ => subject_driver.tool_dispatch_ms(&unit).await,
                };
                measured.push(sample?);
            }
            subject_driver.destroy(&unit).await?;
        }
        other => anyhow::bail!("{} is not a latency probe", other.as_str()),
    }

    if abandoned {
        notes.push(format!(
            "probe abandoned at the wall-clock deadline after {} of {} samples",
            measured.len(),
            context.samples
        ));
    }
    // With the scripted provider's first token deliberately delayed, first-byte latency
    // is separable from turn completion; the delay is the fixture's, not the subject's,
    // so it comes back out of every sample.
    if probe == Probe::Ttfb {
        if let Some(delay_ms) = std::env::var("BENCH_FIRST_TOKEN_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| *value > 0.0)
        {
            for sample in &mut measured {
                *sample = (*sample - delay_ms).max(0.0);
            }
            notes.push(format!(
                "scripted first token delayed {delay_ms} ms; subtracted from every sample"
            ));
        }
    }
    let mut measured = stats::drop_warmup(measured, 0.1);
    let percentiles = stats::summarize(&mut measured);
    let value = percentiles
        .p50_ms
        .context("too few samples to report a median")?;
    if !floor::clears(value, context.floor_ms) {
        notes.push(floor::note(value, context.floor_ms));
    }
    Ok(Outcome {
        value,
        unit: "ms".to_owned(),
        n: measured.len(),
        percentiles,
        resident_kind: None,
        latency: true,
    })
}

/// Complete turns per second with `concurrency` sessions in flight.
///
/// Unpaced and concurrent, because a serial loop measures latency and calls it throughput.
/// Sessions are created before the clock starts: session creation is its own probe and
/// must not be charged to this one.
async fn throughput(
    subject_driver: &dyn Driver,
    context: &Context_,
    notes: &mut Vec<String>,
) -> Result<Outcome> {
    let concurrency = context.concurrency.max(1);
    let per_session = (context.samples / concurrency).max(1);

    let mut sessions = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        sessions.push(subject_driver.create().await?);
    }

    let started = Instant::now();
    let mut turns = 0_usize;
    let mut latencies = Vec::with_capacity(concurrency * per_session);
    for _ in 0..per_session {
        if Instant::now() >= context.deadline {
            notes.push("throughput probe stopped at the wall-clock deadline".to_owned());
            break;
        }
        // Every session takes a turn at once. Awaiting them together is the whole point:
        // it is what puts the subject under concurrent load rather than sequential load.
        let round = futures_util::future::join_all(
            sessions
                .iter()
                .map(|session| subject_driver.round_trip_ms(session)),
        )
        .await;
        for result in round {
            latencies.push(result?);
            turns += 1;
        }
    }
    let elapsed = started.elapsed().as_secs_f64();

    for session in &sessions {
        subject_driver.destroy(session).await?;
    }

    let percentiles = stats::summarize(&mut latencies);
    notes.push(format!(
        "{turns} turns across {concurrency} concurrent sessions in {elapsed:.2}s"
    ));
    Ok(Outcome {
        value: if elapsed > 0.0 {
            turns as f64 / elapsed
        } else {
            0.0
        },
        unit: "turns/s".to_owned(),
        n: turns,
        percentiles,
        resident_kind: None,
        latency: false,
    })
}

/// Memory against live session count, plus the reclaim tail.
///
/// The number worth quoting is the *slope* of a ramp, not a difference between two
/// samples. A slope drops the runtime's fixed floor automatically — tens of megabytes for
/// anything on Python or Node — and it comes with an r² that says whether a single
async fn persistence(
    subject_driver: &dyn Driver,
    context: &Context_,
    notes: &mut Vec<String>,
) -> Result<Outcome> {
    let data_dir = context
        .data_dir
        .as_ref()
        .context("persistence needs a subject the runner started, so it knows where it writes")?;

    let unit = subject_driver.create().await?;
    let settled = |bytes: u64| bytes as f64 / 1_048_576.0;
    // Everything already on disk before the conversation starts: the admitted agentloop
    // package, the binding store, whatever a subject unpacks at boot. Measuring the
    // directory's absolute size charged a 16 MB agentloop to the conversation and made a
    // 1 MB conversation look like 60 MB. What is being asked is what the *conversation*
    // cost, so the baseline comes off.
    let baseline = directory_bytes(data_dir);
    let mut first_turn_bytes = 0_u64;
    let mut last_turn_bytes = 0_u64;
    let mut previous = baseline;
    let mut turns_done = 0_usize;

    for turn in 1..=context.turns {
        if Instant::now() >= context.deadline {
            notes.push(format!(
                "persistence stopped at the deadline after {turns_done} of {} turns",
                context.turns
            ));
            break;
        }
        subject_driver.round_trip_ms(&unit).await?;
        // Written behind the turn by design in at least one subject, so settle before
        // reading or the last turn's bytes land in the next turn's measurement.
        tokio::time::sleep(Duration::from_millis(250)).await;
        let now = directory_bytes(data_dir);
        last_turn_bytes = now.saturating_sub(previous);
        if turn == 1 {
            first_turn_bytes = last_turn_bytes;
        }
        previous = now;
        turns_done = turn;
    }

    let total = previous.saturating_sub(baseline);
    notes.push(format!(
        "{:.2} MiB was already on disk before the conversation and is not counted: {}",
        settled(baseline),
        breakdown(data_dir)
    ));

    // The shape, stated plainly: what the last turn cost against what the first one cost.
    // Appending holds those together however long the conversation runs; rewriting the
    // conversation pulls them apart, and the ratio is how far apart.
    let growth = if first_turn_bytes > 0 {
        last_turn_bytes as f64 / first_turn_bytes as f64
    } else {
        0.0
    };
    notes.push(format!(
        "{turns_done} turns wrote {:.2} MiB; the last turn cost {:.1} KiB against {:.1} KiB for the first ({growth:.1}x). A store that appends holds those together; a store that rewrites the conversation pulls them apart, and the gap widens with every turn",
        settled(total),
        last_turn_bytes as f64 / 1024.0,
        first_turn_bytes as f64 / 1024.0,
    ));

    subject_driver.destroy(&unit).await?;

    Ok(Outcome {
        value: settled(total),
        unit: "MiB".to_owned(),
        n: turns_done,
        percentiles: Percentiles::default(),
        resident_kind: None,
        latency: false,
    })
}

/// What each top-level entry of `directory` holds, largest first. A single total says a
/// conversation was expensive; this says which part of the subject wrote it.
fn breakdown(directory: &std::path::Path) -> String {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return "unreadable".to_owned();
    };
    let mut parts: Vec<(String, u64)> = entries
        .flatten()
        .map(|entry| {
            let bytes = match entry.file_type() {
                Ok(kind) if kind.is_dir() => directory_bytes(&entry.path()),
                _ => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            };
            (entry.file_name().to_string_lossy().into_owned(), bytes)
        })
        .collect();
    parts.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    parts
        .iter()
        .map(|(name, bytes)| format!("{name} {:.2} MiB", *bytes as f64 / 1_048_576.0))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every byte under `directory`, following subdirectories. Unreadable entries count zero
/// rather than aborting: a subject writing while this walks is normal, and a partial
/// answer that is 0.1% low beats no answer at all.
fn directory_bytes(directory: &std::path::Path) -> u64 {
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

/// The share of memory a subject gives back after its sessions are deleted.
///
/// No allocator trim is forced, for any subject: no competitor exposes one, so a forced
/// number would be a column only Brain could produce. What a runtime returns on its own
/// schedule is its policy, and that is the comparable thing.
async fn reclaim(
    subject_driver: &dyn Driver,
    sampler: &mem::Sampler,
    context: &Context_,
    notes: &mut Vec<String>,
) -> Result<Outcome> {
    sampler.set_units(0);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let settling = sampler.readings();
    let idle = settling
        .last()
        .map(|reading| reading.value)
        .context("the sampler produced no reading before the ramp")?;
    // How much this process's RSS moves when nothing is being asked of it. A ramp that
    // does not clear its own idle jitter has not measurably taken memory, and the ratio
    // below would be dividing by noise.
    let jitter = spread(&settling);

    let mut held = Vec::with_capacity(context.units);
    while held.len() < context.units && Instant::now() < context.deadline {
        held.push(subject_driver.create().await?);
    }
    sampler.set_units(held.len());
    tokio::time::sleep(Duration::from_secs(2)).await;
    let peak = sampler
        .readings()
        .last()
        .map(|reading| reading.value)
        .unwrap_or(idle);

    let units = held.len();
    for unit in &held {
        subject_driver.destroy(unit).await?;
    }
    sampler.set_units(0);
    tokio::time::sleep(RECLAIM_TAIL).await;
    let settled = sampler
        .readings()
        .last()
        .map(|reading| reading.value)
        .unwrap_or(peak);

    let grew = peak - idle;
    // Refused rather than reported. `0.0` here would mean "none of it came back", and a
    // reader -- or a chart -- cannot tell that from "nothing was taken in the first place":
    // opposite findings that this probe used to print identically. Brain holds 256 sessions
    // inside its own idle jitter, which is the good outcome, and publishing that as 0%
    // returned says the reverse. What a session costs is what `resident` answers.
    anyhow::ensure!(
        grew > jitter,
        "{} sessions moved resident memory by {grew:.0} KiB, within the {jitter:.0} KiB this process drifts while idle, so nothing measurable was taken and the share given back is undefined; `resident` is the probe for what a session costs",
        held.len(),
    );
    let returned = ((peak - settled) / grew * 100.0).clamp(0.0, 100.0);
    notes.push(format!(
        "idle {idle:.0} KiB, {peak:.0} KiB at {units} sessions, {settled:.0} KiB {}s after deleting them; no allocator trim was forced, here or for any subject",
        RECLAIM_TAIL.as_secs()
    ));

    Ok(Outcome {
        value: returned,
        unit: "% returned".to_owned(),
        n: units,
        percentiles: Percentiles::default(),
        resident_kind: Some(ResidentKind::Private),
        latency: false,
    })
}

/// What the process costs holding nothing.
///
/// Every project publishes this number and almost none of them say how they got it, which
/// makes the published ones incomparable: an idle figure taken a second after start is not
/// the same as one taken after the allocator has settled, and a single sample is not the
/// same as a median. So: the subject is started and left completely alone, given time to
/// settle, and then sampled across its whole process tree for a further window. No
/// sessions are created, and no allocator trim is forced -- for this subject or any other.
async fn idle(context: &Context_, notes: &mut Vec<String>) -> Result<Outcome> {
    // Whatever the subject did while starting has to finish before this means anything.
    tokio::time::sleep(IDLE_SETTLE).await;
    let available = host::machine_available_kib()?;
    let used = context.machine_baseline_kib.saturating_sub(available) as f64 / 1024.0;
    notes.push(format!(
        "the machine had {:.0} MiB available before this subject was started and {:.0} MiB          after it had been running {}s holding no sessions; the difference is what it costs          to have it switched on",
        context.machine_baseline_kib as f64 / 1024.0,
        available as f64 / 1024.0,
        IDLE_SETTLE.as_secs(),
    ));
    Ok(Outcome {
        value: used,
        unit: "MiB".to_owned(),
        n: 1,
        percentiles: Percentiles::default(),
        resident_kind: Some(ResidentKind::Private),
        latency: false,
    })
}

/// What a session costs, measured from outside the process.
///
/// The machine before, the machine after, divided by the sessions in between. It replaces
/// a least-squares fit of the subject's process-tree private memory against session count,
/// which was the more careful method and could not answer at all: at 8,192 sessions it fit
/// a line at r-squared 0.255 and the probe correctly refused to publish a slope. This
/// counts everything the careful version missed, too -- worker processes, page cache, the
/// kernel's own structures -- because it asks the machine rather than the process.
async fn machine_ramp(
    subject_driver: &dyn Driver,
    context: &Context_,
    notes: &mut Vec<String>,
) -> Result<Outcome> {
    tokio::time::sleep(SETTLE).await;
    let before = host::machine_available_kib()?;

    let mut held = Vec::with_capacity(context.units);
    while held.len() < context.units && Instant::now() < context.deadline {
        held.push(subject_driver.create().await?);
    }
    anyhow::ensure!(
        !held.is_empty(),
        "no sessions were created, so there is nothing to divide by"
    );
    tokio::time::sleep(SETTLE).await;
    let after = host::machine_available_kib()?;

    let units = held.len();
    let consumed_kib = before.saturating_sub(after) as f64;
    let per_session = consumed_kib / units as f64;
    notes.push(format!(
        "machine memory available fell from {:.0} MiB to {:.0} MiB while {units} sessions          were held, so {:.1} MiB went into them; no allocator trim was forced, here or for          any subject",
        before as f64 / 1024.0,
        after as f64 / 1024.0,
        consumed_kib / 1024.0,
    ));
    // A machine measurement picks up anything else happening on the box. If the sessions
    // did not move it, the answer is that they are too cheap to see this way, not zero.
    anyhow::ensure!(
        consumed_kib > 0.0,
        "holding {units} sessions did not reduce the machine's available memory at all, so          there is nothing to divide; either they cost too little to see from outside or          something else on the box gave memory back at the same time"
    );

    for unit in &held {
        subject_driver.destroy(unit).await?;
    }

    Ok(Outcome {
        value: per_session,
        unit: "KiB/session".to_owned(),
        n: units,
        percentiles: Percentiles::default(),
        resident_kind: Some(ResidentKind::Private),
        latency: false,
    })
}

/// The distance between the highest and lowest reading in a window.
fn spread(readings: &[crate::schema::Reading]) -> f64 {
    let values = readings.iter().map(|reading| reading.value);
    let high = values.clone().fold(f64::MIN, f64::max);
    let low = values.fold(f64::MAX, f64::min);
    if high < low { 0.0 } else { high - low }
}

/// Everything the launch-lifecycle probes need to start, kill, and restart the subject
/// themselves. `None` when the operator pointed the runner at something already running,
/// which those probes then refuse — honestly, with a reason.
pub struct Relaunch {
    pub launch: subject::Launch,
    pub subject: String,
    pub run_id: String,
    pub model_base_url: String,
    pub environment_base_url: String,
    /// The scripted provider's per-call record; `messages` on the latest entry is how
    /// many transcript messages the subject actually sent the model — the recovery
    /// probe's proof that a survived session still remembers its conversation.
    pub provider_timings: std::sync::Arc<std::sync::Mutex<Vec<crate::fixtures::CallTiming>>>,
    /// Builds a driver against a freshly launched instance's base URL. Each launch gets
    /// its own: drivers pin their base URL at construction.
    #[allow(clippy::type_complexity)]
    pub make_driver: std::sync::Arc<dyn Fn(String) -> Result<Box<dyn Driver>> + Send + Sync>,
}

/// Launch on a fresh data directory, until the first turn is served.
///
/// The redeploy definition: process artifact caches persist across samples (they live
/// outside the data directory), session data does not. One-time installation — an
/// agentloop upload, a project — is `prepare` and stays untimed, matching the create
/// probe's "already admitted" convention. Kept short on purpose: ten samples, one turn
/// each.
async fn cold_start(context: &Context_, notes: &mut Vec<String>) -> Result<Outcome> {
    let relaunch = context
        .relaunch
        .as_ref()
        .context("cold start needs a subject the runner launches itself; --base-url cannot answer it")?;
    let samples = context.samples.clamp(3, 10);
    let mut measured = Vec::with_capacity(samples);
    for index in 0..samples {
        if Instant::now() >= context.deadline {
            notes.push(format!(
                "probe abandoned at the wall-clock deadline after {} of {samples} samples",
                measured.len()
            ));
            break;
        }
        let booted = Instant::now();
        let mut running = crate::launch::start_in(
            &relaunch.launch,
            &relaunch.subject,
            &format!("{}-cold{index}", relaunch.run_id),
            &relaunch.model_base_url,
            &relaunch.environment_base_url,
            None,
        )
        .await?;
        let ready_ms = booted.elapsed().as_secs_f64() * 1_000.0;
        let mut driver = (relaunch.make_driver)(running.base_url.clone())?;
        // Installation is not a redeploy cost; the clock pauses for it.
        driver.prepare().await?;
        let serving = Instant::now();
        let unit = driver.create().await?;
        driver.round_trip_ms(&unit).await?;
        measured.push(ready_ms + serving.elapsed().as_secs_f64() * 1_000.0);
        running.stop().await;
    }
    let percentiles = stats::summarize(&mut measured);
    let value = percentiles
        .p50_ms
        .context("too few samples to report a median")?;
    notes.push("launch to first turn served on a fresh data directory; installation (prepare) untimed".to_owned());
    Ok(Outcome {
        value,
        unit: "ms".to_owned(),
        n: measured.len(),
        percentiles,
        resident_kind: None,
        latency: true,
    })
}

/// Turns one conversation deep, kill -9, relaunch on the same data, and time until the
/// *same session* serves the next turn — with the model's own transcript as proof the
/// history survived. A subject that comes back without the conversation is a recorded
/// refusal, not a bar: that is the result.
async fn recovery(context: &Context_, notes: &mut Vec<String>) -> Result<Outcome> {
    let relaunch = context
        .relaunch
        .as_ref()
        .context("recovery needs a subject the runner launches itself; --base-url cannot answer it")?;
    /// Deep enough that lost history is unmistakable, short enough that the warmup is
    /// seconds — per the no-long-benchmarks rule.
    const WARM_TURNS: usize = 50;
    let samples = context.samples.clamp(2, 5);
    let mut measured = Vec::with_capacity(samples);
    for index in 0..samples {
        if Instant::now() >= context.deadline {
            notes.push(format!(
                "probe abandoned at the wall-clock deadline after {} of {samples} samples",
                measured.len()
            ));
            break;
        }
        let mut before = crate::launch::start_in(
            &relaunch.launch,
            &relaunch.subject,
            &format!("{}-recovery{index}", relaunch.run_id),
            &relaunch.model_base_url,
            &relaunch.environment_base_url,
            None,
        )
        .await?;
        let data_dir = before.data_dir.clone();
        let mut warm_driver = (relaunch.make_driver)(before.base_url.clone())?;
        warm_driver.prepare().await?;
        let unit = warm_driver.create().await?;
        for _ in 0..WARM_TURNS {
            warm_driver.round_trip_ms(&unit).await?;
        }
        before.kill_hard().await;

        let killed = Instant::now();
        let mut after = crate::launch::start_in(
            &relaunch.launch,
            &relaunch.subject,
            &format!("{}-recovered{index}", relaunch.run_id),
            &relaunch.model_base_url,
            &relaunch.environment_base_url,
            Some(data_dir),
        )
        .await
        .context("the subject did not come back up on its own data")?;
        let ready_ms = killed.elapsed().as_secs_f64() * 1_000.0;
        let mut driver = (relaunch.make_driver)(after.base_url.clone())?;
        driver.prepare().await?;
        let serving = Instant::now();
        let served = driver.round_trip_ms(&unit).await;
        let serve_ms = serving.elapsed().as_secs_f64() * 1_000.0;
        let survived = relaunch
            .provider_timings
            .lock()
            .ok()
            .and_then(|timings| timings.last().map(|timing| timing.messages))
            .unwrap_or(0);
        after.stop().await;
        match served {
            Err(error) => {
                anyhow::bail!(
                    "the subject restarted but the conversation was lost: turn on the surviving session failed: {error:#}"
                );
            }
            Ok(_) if survived < WARM_TURNS => {
                anyhow::bail!(
                    "the subject served the turn without its history: the model saw {survived} messages after {WARM_TURNS} prior turns"
                );
            }
            Ok(_) => measured.push(ready_ms + serve_ms),
        }
    }
    let percentiles = stats::summarize(&mut measured);
    let value = percentiles
        .p50_ms
        .context("too few samples to report a median")?;
    notes.push(format!(
        "kill -9 after {} turns, relaunch on the same data, until the same session served a turn whose model request carried its history",
        50
    ));
    Ok(Outcome {
        value,
        unit: "ms".to_owned(),
        n: measured.len(),
        percentiles,
        resident_kind: None,
        latency: true,
    })
}
