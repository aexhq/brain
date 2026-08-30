//! The result schema. Every published table is generated from these records, so a number
//! can never appear in a document without the context that makes it readable: what was
//! measured, on what, how many times, and whether we measured it or cited it.

use serde::{Deserialize, Serialize};

/// What the subject is, for grouping in generated tables. Never a gate: a subject appears
/// in any probe table it can answer, and the `definition` on each datapoint says how.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Class {
    /// Holds a conversation, decides what happens next, dispatches tool calls.
    SessionKernel,
    /// Hands out a machine to run commands in.
    Sandbox,
    /// Runs one unit of untrusted code. Sits underneath the other two.
    IsolationSubstrate,
    /// A library you build an agent *with*, not a server that holds sessions. It cannot
    /// answer a latency probe until someone writes a harness around it — and the results
    /// must then say we wrote that harness, because it affects the number.
    Framework,
}

/// The measurements. A subject declares which of these it can answer; absent probes stay
/// absent in the output rather than becoming a zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Probe {
    /// Request to create the unit of work, until it will accept work.
    Create,
    /// Work submitted, until the first useful output byte reaches the client.
    Ttfb,
    /// One complete unit of work, end to end.
    RoundTrip,
    /// Session kernels only: tool call emitted, until the result is durably recorded.
    ToolDispatch,
    /// Sustained units per second at N concurrent.
    Throughput,
    /// What the process costs at rest, holding nothing.
    ///
    /// The figure every project puts in its README, and almost none of them say how they
    /// got it. Measured here as private memory across the subject's whole process tree,
    /// after it has said it is ready and then been left alone.
    Idle,
    /// Memory attributable to one live unit.
    Resident,
    /// Delete the units, then measure what came back.
    Reclaim,
    /// Money per unit-hour.
    Cost,
    /// Bytes written to durable storage per turn, against turn index.
    ///
    /// The probe that separates an append-only log from a snapshot-per-step: Agno rewrites
    /// its whole runs list per run, CrewAI writes a full snapshot per task, LangGraph a
    /// full checkpoint per super-step. All are O(n²) over a session, so the divergence
    /// widens with conversation length and a single figure would hide it.
    Persistence,
}

impl Probe {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Ttfb => "ttfb",
            Self::RoundTrip => "round_trip",
            Self::ToolDispatch => "tool_dispatch",
            Self::Throughput => "throughput",
            Self::Idle => "idle",
            Self::Resident => "resident",
            Self::Reclaim => "reclaim",
            Self::Cost => "cost",
            Self::Persistence => "persistence",
        }
    }
}

/// How much weight a number carries. Set by the runner, never by hand: `Measured` is only
/// ever written by code that took the sample itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Evidence {
    /// We ran it and took the sample.
    Measured,
    /// The project publishes it. Not independently confirmed.
    Vendor,
    /// A third party published it, method unstated.
    Blog,
}

/// What bounded a throughput number. Both are publishable; conflating them is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitSource {
    /// We own the machine, so the number is the engine's.
    Engine,
    /// A hosted service bounded us. This measures their quota, not their engine.
    Service,
}

impl Probe {
    /// Every probe, in the order a run attempts them.
    ///
    /// Ordered so the cheap probes bank a result before the expensive ones start: on spot
    /// capacity a run can end at any moment, and this order decides what survives.
    ///
    /// It lives here, against the enum, because it used to live in `probes.rs` and a
    /// variant was added to one and not the other. `--probe idle` then matched nothing and
    /// the run recorded neither a number nor a skip, which is the one outcome this
    /// benchmark is built to prevent. `the_run_order_holds_every_probe` fails to compile
    /// if a variant is added without a decision about where it belongs here.
    pub const ALL: [Probe; 9] = [
        Probe::Create,
        Probe::Idle,
        Probe::Ttfb,
        Probe::RoundTrip,
        Probe::ToolDispatch,
        Probe::Persistence,
        Probe::Throughput,
        Probe::Resident,
        Probe::Reclaim,
    ];
}

/// How a `Resident` number was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResidentKind {
    /// Private_Clean + Private_Dirty + Private_Hugetlb, summed over the process tree.
    /// The only kind that may be called "memory per session".
    Private,
    /// An observable stand-in for a service we cannot read: units held before
    /// degradation, or money per idle unit-hour. Never call this memory.
    Proxy,
}

/// Where a run happened. Two runs are only comparable when this matches — and on spot
/// capacity it frequently will not, because you get whatever instance type was available.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Host {
    pub label: String,
    pub os: String,
    pub arch: String,
    pub kernel: Option<String>,
    pub cpus: usize,
    /// EC2 identity from IMDSv2. Absent off EC2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec2: Option<Ec2>,
    /// Machine settings that move latency and memory numbers. Recorded so a run that
    /// forgot to normalize them is visible afterwards rather than silently wrong.
    pub tuning: Tuning,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ec2 {
    pub instance_type: String,
    pub instance_id: String,
    pub availability_zone: String,
    pub region: String,
    /// `spot` or `on-demand`. Spot is fine for throughput and memory work; it is the
    /// interruption and the capacity lottery that need handling, not the silicon.
    pub lifecycle: String,
    /// True for a `.metal` instance type. Nested KVM — Firecracker, forkd, self-hosted
    /// E2B — is only available here.
    pub metal: bool,
}

/// Host settings normalized before a run. Each is `None` when it could not be read.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tuning {
    pub transparent_hugepages: Option<String>,
    pub cpu_governor: Option<String>,
    /// Cores the subject was pinned to, if any. On a shared box the generator and the
    /// subject fight for CPU; pinning them apart is what keeps a sub-ms p50 honest.
    pub subject_cpus: Option<String>,
    pub generator_cpus: Option<String>,
}

/// Fraction of CPU time stolen by the hypervisor across a probe, sampled from
/// `/proc/stat`. The single biggest threat to a benchmark measuring milliseconds on
/// shared EC2 capacity: a p99 measured under steal is the neighbour's p99.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Steal {
    pub fraction: f64,
    /// True once `fraction` crosses the run's tolerance. Datapoints taken while set are
    /// still recorded, and are excluded from generated comparison tables.
    pub exceeded: bool,
}

/// How a run ended. Spot capacity means `Interrupted` is a normal outcome, not a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Complete,
    /// A spot termination notice arrived. Everything already written stands; the
    /// remaining probes are resumable against the same `run_id`.
    Interrupted,
    /// Stopped on the wall-clock budget before finishing.
    BudgetExhausted,
    Failed,
}

/// One reading in a time series, milliseconds from the start of the probe.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Reading {
    pub at_ms: u64,
    pub value: f64,
    /// Live units at this moment, where the probe was ramping them. Absent for a plain
    /// time series.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<usize>,
}

/// A straight line fitted through readings.
///
/// Memory per session is the *slope* of memory against live session count, not a
/// difference between two samples. The slope drops the runtime's fixed floor
/// automatically, and `r2` says whether treating the cost as per-session was even
/// legitimate: a curve that does not fit a line has no single per-session number, and
/// reporting one anyway is the mistake this field exists to catch.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Fit {
    pub slope: f64,
    pub intercept: f64,
    pub r2: f64,
    pub points: usize,
}

impl Fit {
    /// Least squares through `(x, y)`.
    pub fn least_squares(points: &[(f64, f64)]) -> Option<Self> {
        if points.len() < 3 {
            return None;
        }
        let n = points.len() as f64;
        let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
        let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        for (x, y) in points {
            sxx += (x - mean_x) * (x - mean_x);
            sxy += (x - mean_x) * (y - mean_y);
        }
        if sxx == 0.0 {
            return None;
        }
        let slope = sxy / sxx;
        let intercept = mean_y - slope * mean_x;
        let mut residual = 0.0;
        let mut total = 0.0;
        for (x, y) in points {
            let predicted = slope * x + intercept;
            residual += (y - predicted) * (y - predicted);
            total += (y - mean_y) * (y - mean_y);
        }
        Some(Self {
            slope,
            intercept,
            r2: if total == 0.0 {
                1.0
            } else {
                1.0 - residual / total
            },
            points: points.len(),
        })
    }
}

/// Latency percentiles. Only the ones `n` supports are populated; see `stats::summarize`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Percentiles {
    pub p50_ms: Option<f64>,
    pub p90_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

/// One measurement of one probe against one subject.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Datapoint {
    pub subject: String,
    pub subject_version: String,
    pub class: Class,
    pub probe: Probe,
    /// Exactly what the number measures, in this subject's own terms. Required, always
    /// rendered next to the value, and the reason two rows can sit in one table.
    pub definition: String,
    /// `cold`, `warm`, or a subject-specific variant.
    pub variant: Option<String>,
    pub evidence: Evidence,

    pub value: f64,
    pub unit: String,
    pub n: usize,
    #[serde(default)]
    pub percentiles: Percentiles,

    /// True when the subject could not be given the scripted provider, so the number
    /// carries a real model's latency. Excludes the row from engine comparisons.
    #[serde(default)]
    pub model_included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_source: Option<LimitSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident_kind: Option<ResidentKind>,

    /// The load generator's own floor at the time, in ms. A latency value within
    /// `floor::MARGIN` of this is the client's jitter, not the subject's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator_floor_ms: Option<f64>,
    /// Private memory sampled throughout the probe, in KiB. Present on every probe that
    /// ran against a process the runner can read, so memory under load is answered by the
    /// throughput probe and memory over time by the probe's own tail — rather than needing
    /// a separate arm for each question.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_kib: Vec<Reading>,
    /// Memory against live session count, where the probe ramped them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit: Option<Fit>,
    /// Hypervisor steal observed while this probe ran.
    #[serde(default)]
    pub steal: Steal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Datapoint {
    /// Whether this row may appear in a generated comparison table. A row that fails
    /// here is kept in the raw results and rendered only with its reason attached.
    pub fn comparable(&self) -> Result<(), &'static str> {
        if self.definition.trim().is_empty() {
            return Err("no definition recorded");
        }
        if self.steal.exceeded {
            return Err("hypervisor steal exceeded tolerance");
        }
        if self.model_included {
            return Err("a real model's latency is inside this number");
        }
        Ok(())
    }
}

/// A subject the runner declined to measure, and why. Recorded so a missing row in a
/// generated table is explained rather than silently absent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skipped {
    pub subject: String,
    pub probe: Probe,
    pub reason: String,
}

/// Everything one invocation produced.
///
/// Written incrementally as JSON Lines, never assembled in memory and flushed at the end:
/// on spot capacity a run can be killed two minutes after a notice arrives, and a probe
/// that finished must survive that. `load` reassembles a run from its lines, which is
/// also how `--resume` knows what not to repeat.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub run_id: String,
    pub started_at_ms: u64,
    pub host: Host,
    pub outcome: Outcome,
    pub datapoints: Vec<Datapoint>,
    pub skipped: Vec<Skipped>,
}

/// One line of the incremental results file.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "kebab-case")]
pub enum Record {
    /// First line: identity of the run. Repeated on resume so a results file carries
    /// every host it touched — spot may hand back a different instance type.
    Header {
        run_id: String,
        started_at_ms: u64,
        host: Box<Host>,
    },
    Datapoint(Box<Datapoint>),
    Skipped(Skipped),
    /// Last line, when the runner gets to write one.
    Footer {
        outcome: Outcome,
    },
}

#[cfg(test)]
mod probe_order_tests {
    use super::Probe;

    /// A probe that exists but is never attempted is worse than one that fails: the run
    /// says nothing about it at all. The match below is the guard -- adding a variant
    /// stops this compiling until someone places it in `Probe::ALL`.
    #[test]
    fn the_run_order_holds_every_probe() {
        fn placed(probe: Probe) -> bool {
            let expected = match probe {
                Probe::Create => "create",
                Probe::Idle => "idle",
                Probe::Ttfb => "ttfb",
                Probe::RoundTrip => "round_trip",
                Probe::ToolDispatch => "tool_dispatch",
                Probe::Persistence => "persistence",
                Probe::Throughput => "throughput",
                Probe::Resident => "resident",
                Probe::Reclaim => "reclaim",
                Probe::Cost => "cost",
            };
            assert_eq!(probe.as_str(), expected);
            Probe::ALL.contains(&probe)
        }

        for probe in Probe::ALL {
            assert!(placed(probe), "{} is in the run order", probe.as_str());
        }
    }
}
