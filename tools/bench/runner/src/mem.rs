//! Private memory, summed over a process tree.
//!
//! Two rules this module exists to enforce. It never substitutes RSS for private memory,
//! because RSS double-counts pages shared between processes and would flatter anything
//! that forks. And it always walks the whole tree, because subjects are multi-process in
//! different ways — Brain runs `brain` plus a loop worker, Letta runs a server plus
//! Postgres, OpenFang forks Wasm workers — and measuring one pid rewards whichever
//! subject pushed the most memory into a child.

use std::collections::BTreeSet;

use anyhow::Result;

/// A private-memory sample, in kibibytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub private_kib: u64,
    /// How many processes were summed. Recorded because a tree that changed size between
    /// two samples makes the delta between them meaningless.
    pub processes: usize,
}

/// Sums `Private_Clean + Private_Dirty + Private_Hugetlb` across `root` and every
/// descendant.
pub fn sample_tree(root: u32) -> Result<Sample> {
    let pids = descendants(root);
    let mut private_kib = 0;
    let mut processes = 0;
    for pid in &pids {
        // A process can exit between being listed and being read; that is normal and not
        // an error. `processes` records how many were summed, so a tree that changed size
        // between two readings is visible in the series rather than hidden in it.
        if let Some(kib) = private_of(*pid) {
            private_kib += kib;
            processes += 1;
        }
    }
    anyhow::ensure!(
        processes > 0,
        "no process in the tree rooted at {root} could be sampled; \
         is it still running, and is this Linux?"
    );
    Ok(Sample {
        private_kib,
        processes,
    })
}

fn private_of(pid: u32) -> Option<u64> {
    let rollup = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    let mut total = 0;
    for line in rollup.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if matches!(key, "Private_Clean" | "Private_Dirty" | "Private_Hugetlb") {
            total += value
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse::<u64>()
                .unwrap_or(0);
        }
    }
    Some(total)
}

/// Breadth-first walk of `/proc/<pid>/task/<tid>/children`.
pub fn descendants(root: u32) -> BTreeSet<u32> {
    let mut seen = BTreeSet::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        if !seen.insert(pid) {
            continue;
        }
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            continue;
        };
        for task in tasks.flatten() {
            let path = task.path().join("children");
            let Ok(children) = std::fs::read_to_string(&path) else {
                continue;
            };
            queue.extend(
                children
                    .split_whitespace()
                    .filter_map(|value| value.parse::<u32>().ok()),
            );
        }
    }
    seen
}

/// Samples private memory in the background for as long as a probe runs.
///
/// One mechanism answers every memory question the benchmark has. Run it while sessions
/// are being created and the readings carry a live-unit count, so memory per session is
/// the slope of a ramp rather than a difference between two samples. Run it during the
/// throughput probe and it is memory under load. Keep sampling after the deletes and the
/// tail is reclaim — with no forced allocator trim anywhere, because no competitor exposes
/// one and a number only Brain can produce is not a comparison.
pub struct Sampler {
    readings: std::sync::Arc<std::sync::Mutex<Vec<crate::schema::Reading>>>,
    units: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    handle: tokio::task::JoinHandle<()>,
}

impl Sampler {
    /// Starts sampling `pid`'s process tree every `interval`.
    pub fn start(pid: u32, interval: std::time::Duration) -> Self {
        let readings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let units = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sink = std::sync::Arc::clone(&readings);
        let counter = std::sync::Arc::clone(&units);
        let started = std::time::Instant::now();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                // A failed read means the subject exited. Stop rather than filling the
                // series with zeros that would read as a dramatic drop.
                let Ok(sample) = sample_tree(pid) else {
                    return;
                };
                let reading = crate::schema::Reading {
                    at_ms: started.elapsed().as_millis() as u64,
                    value: sample.private_kib as f64,
                    units: Some(counter.load(std::sync::atomic::Ordering::Relaxed)),
                };
                if let Ok(mut readings) = sink.lock() {
                    readings.push(reading);
                }
            }
        });
        Self {
            readings,
            units,
            handle,
        }
    }

    /// Tells the sampler how many units are live, so the readings it takes from now on
    /// can be fitted against unit count.
    pub fn set_units(&self, units: usize) {
        self.units
            .store(units, std::sync::atomic::Ordering::Relaxed);
    }

    /// Readings so far, without stopping.
    pub fn readings(&self) -> Vec<crate::schema::Reading> {
        self.readings
            .lock()
            .map(|readings| readings.clone())
            .unwrap_or_default()
    }

    pub fn finish(self) -> Vec<crate::schema::Reading> {
        self.handle.abort();
        self.readings()
    }
}

/// Fits memory against live unit count over a ramp.
///
/// Readings taken while the count was changing are dropped: a sample caught mid-create
/// belongs to neither step and would flatten the slope. Only the settled reading at each
/// step counts.
pub fn fit_against_units(readings: &[crate::schema::Reading]) -> Option<crate::schema::Fit> {
    let mut by_step: std::collections::BTreeMap<usize, f64> = Default::default();
    for reading in readings {
        let Some(units) = reading.units else { continue };
        // The last reading at each step is the settled one.
        by_step.insert(units, reading.value);
    }
    let points: Vec<(f64, f64)> = by_step
        .into_iter()
        .map(|(units, kib)| (units as f64, kib))
        .collect();
    crate::schema::Fit::least_squares(&points)
}
