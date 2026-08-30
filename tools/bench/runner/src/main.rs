//! Comparative benchmark runner.
//!
//! Measures Brain and its rivals on the same probes, on the same instance, through each
//! subject's own public surface. Three things it will not do, because each is how a
//! benchmark stops being evidence: publish a latency it cannot separate from its own
//! jitter, publish a percentile too few samples support, or substitute RSS for private
//! memory.
//!
//! Built for spot capacity. Results are appended and flushed per record, a termination
//! notice ends the run cleanly, and `--resume` picks up the probes that never ran.

mod coldstart;
mod driver;
mod drivers;
mod fixtures;
mod floor;
mod growth;
mod host;
mod launch;
mod mem;
mod probes;
mod report;
mod results;
mod schema;
mod stats;
mod subject;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use schema::{Class, Outcome, Probe, Skipped};

#[derive(Parser)]
#[command(
    name = "brain-bench",
    about = "Comparative benchmark runner for Brain and its rivals"
)]
struct Cli {
    /// Directory of subject manifests.
    #[arg(long, default_value = "tools/bench/subjects", global = true)]
    subjects: PathBuf,
    /// Where results are appended, one JSON Lines file per run.
    #[arg(long, default_value = "tools/bench/results", global = true)]
    results: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show every subject, what it can answer, and anything blocking it here.
    List,
    /// Render a results file as the markdown that gets published.
    Report {
        /// The JSON Lines file a run appended to.
        file: PathBuf,
    },
    /// Measure the load generator's own floor and stop. Run this first on a new instance:
    /// it is the number every latency result is judged against.
    Floor {
        #[arg(long, default_value_t = 2_000)]
        samples: usize,
    },
    /// Per-turn cost against turn index, with the fixture's own service time separated
    /// out. Answers what a p50 over a whole run cannot: whether a turn costs more late in
    /// a conversation than early in it, and how much of any growth is the benchmark's own
    /// scripted provider rather than the subject.
    Growth {
        /// The subject to walk a conversation with. It must have a launch block: the
        /// experiment starts it itself so it knows where the subject writes.
        #[arg(long, default_value = "brain")]
        subject: String,
        /// Turns in one conversation. The curve is the point, so this wants to be long.
        #[arg(long, default_value_t = 1_000)]
        turns: usize,
        /// Independent conversations, each on a freshly started subject with an empty data
        /// directory. One run is an anecdote; the shape has to reproduce.
        #[arg(long, default_value_t = 2)]
        repeats: usize,
        /// Compiled agentloop package for the Brain driver.
        #[arg(long, default_value = "examples/dist/example.brain.json")]
        agentloop: PathBuf,
        /// Where the per-turn rows are written, as CSV.
        #[arg(long, default_value = "tools/bench/results/growth.csv")]
        out: PathBuf,
    },
    /// Cold and warm session start, separated. `create` publishes the steady state and
    /// discards its warm-up, so the first session a process ever holds -- the one a person
    /// actually meets -- is the sample it throws away.
    Coldstart {
        #[arg(long, default_value = "brain")]
        subject: String,
        /// Process starts. One boot and one cold sample each; a cold start cannot be taken
        /// twice on the same process.
        #[arg(long, default_value_t = 30)]
        repeats: usize,
        /// Sessions created after the cold one, on each process.
        #[arg(long, default_value_t = 20)]
        warm_per_repeat: usize,
        #[arg(long, default_value = "examples/dist/example.brain.json")]
        agentloop: PathBuf,
    },
    /// Run probes.
    Run {
        /// Subjects to measure. Defaults to every subject that is not blocked.
        #[arg(long)]
        subject: Vec<String>,
        /// Probes to run. Defaults to every probe the subject declares.
        #[arg(long)]
        probe: Vec<String>,
        /// Samples per latency probe. The default supports a p99; fewer will not print one.
        #[arg(long, default_value_t = 1_000)]
        samples: usize,
        /// Live sessions held at the top of the memory ramp.
        #[arg(long, default_value_t = 512)]
        units: usize,
        /// Sessions in flight for the throughput probe. A serial loop measures latency and
        /// calls it throughput; this is what puts the subject under concurrent load.
        #[arg(long, default_value_t = 64)]
        concurrency: usize,
        /// Turns in the one conversation the persistence probe measures. The divergence
        /// between appending and rewriting only shows up over a long conversation.
        #[arg(long, default_value_t = 100)]
        turns: usize,
        /// Base URL of an already-running subject, for the common case of driving a
        /// server the operator started.
        #[arg(long)]
        base_url: Option<String>,
        /// Compiled agentloop package for the Brain driver.
        #[arg(long, default_value = "examples/dist/example.brain.json")]
        agentloop: PathBuf,
        /// Pid whose process tree holds the subject's memory, when the runner did not
        /// start it.
        #[arg(long)]
        pid: Option<u32>,
        /// Continue a run that spot capacity interrupted.
        #[arg(long)]
        resume: Option<String>,
        /// Permit subjects that cost money. Never set in CI.
        #[arg(long)]
        allow_paid: bool,
        /// Stop and write a footer after this many minutes, so an instance is never left
        /// billing on a probe that will not finish.
        #[arg(long, default_value_t = 120)]
        budget_minutes: u64,
        /// A name for this host, recorded on every datapoint.
        #[arg(long, default_value = "unlabelled")]
        host_label: String,
        /// Shell command run after every record to copy the results file somewhere that
        /// outlives the instance, with `{file}` replaced by its path. Per-record flushing
        /// survives the process dying; it does not survive spot reclaiming the machine,
        /// which is the likelier ending. Example:
        /// `--sync-command 'aws s3 cp {file} s3://bench-results/'`
        #[arg(long)]
        sync_command: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // Pin the load generator before it does anything, so every sample it takes is taken
    // from the CPUs the run says it was taken from. Recorded but never applied before,
    // which left the generator free to contend with the subject it was measuring.
    if let Ok(cpus) = std::env::var("BENCH_GENERATOR_CPUS") {
        host::pin(std::process::id(), &cpus)
            .with_context(|| format!("pinning the load generator to CPUs {cpus}"))?;
    }
    match cli.command {
        Command::List => list(&cli.subjects),
        Command::Floor { samples } => {
            let floor_ms = floor::measure(samples).await?;
            println!("load generator floor: {floor_ms:.3} ms (p50 over {samples} samples)");
            println!(
                "any subject latency below {:.3} ms is mostly this process and will not be published",
                floor_ms * floor::MARGIN
            );
            Ok(())
        }
        Command::Report { ref file } => {
            let run = results::load(file)?;
            print!("{}", report::markdown(&run)?);
            Ok(())
        }
        Command::Growth {
            ref subject,
            turns,
            repeats,
            ref agentloop,
            ref out,
        } => {
            growth::run(growth::Options {
                subjects: cli.subjects.clone(),
                subject: subject.clone(),
                turns,
                repeats,
                agentloop: agentloop.clone(),
                out: out.clone(),
            })
            .await
        }
        Command::Coldstart {
            subject,
            repeats,
            warm_per_repeat,
            agentloop,
        } => {
            coldstart::run(coldstart::Options {
                subject,
                subjects: cli.subjects.clone(),
                repeats,
                warm_per_repeat,
                agentloop,
            })
            .await
        }
        Command::Run { .. } => run(cli).await,
    }
}

fn list(dir: &std::path::Path) -> Result<()> {
    let subjects = subject::load_all(dir)?;
    for entry in &subjects {
        let blocked = entry.blocked(false, false);
        println!(
            "{:<28} {:<20} {}",
            entry.name,
            format!("{:?}", entry.class),
            blocked.unwrap_or_else(|| "ready".to_owned())
        );
        for (probe, spec) in &entry.probes {
            println!(" {probe:<14} {}", spec.definition);
        }
    }
    println!("\n{} subjects", subjects.len());
    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
    let Command::Run {
        subject: wanted,
        probe: wanted_probes,
        turns,
        samples,
        units,
        concurrency,
        base_url,
        agentloop,
        pid,
        resume,
        allow_paid,
        budget_minutes,
        host_label,
        sync_command,
    } = cli.command
    else {
        unreachable!()
    };

    let host = host::detect(&host_label).await;
    if let Some(ec2) = &host.ec2 {
        eprintln!(
            "host: {} {} in {} ({} capacity){}",
            ec2.instance_type,
            ec2.instance_id,
            ec2.availability_zone,
            ec2.lifecycle,
            if ec2.metal { ", metal" } else { "" }
        );
    }
    let metal = host.ec2.as_ref().is_some_and(|ec2| ec2.metal);

    let subjects = subject::load_all(&cli.subjects)?;
    let selected: Vec<_> = subjects
        .iter()
        .filter(|entry| wanted.is_empty() || wanted.contains(&entry.name))
        .collect();
    anyhow::ensure!(!selected.is_empty(), "no subject matched");

    let run_id = resume.clone().unwrap_or_else(results::new_run_id);
    let already = if resume.is_some() {
        results::completed(&cli.results, &run_id)?
    } else {
        Default::default()
    };
    if sync_command.is_none() {
        eprintln!(
            "warning: no --sync-command, so results live only on this instance. If spot reclaims it, the run is lost."
        );
    }
    let mut writer = results::Writer::open(&cli.results, &run_id, &host, sync_command)?;
    eprintln!("run {} -> {}", writer.run_id(), writer.path().display());

    // Established once per run, on this instance, under whatever pinning is in effect.
    let floor_ms = floor::measure(samples.min(2_000)).await?;
    eprintln!("load generator floor: {floor_ms:.3} ms");

    let deadline = Instant::now() + Duration::from_secs(budget_minutes * 60);
    let mut interruption = Box::pin(host::spot_interruption());
    let mut outcome = Outcome::Complete;

    'subjects: for entry in selected {
        if let Some(reason) = entry.blocked(metal, allow_paid) {
            writer.skipped(Skipped {
                subject: entry.name.clone(),
                probe: Probe::Create,
                reason,
            })?;
            continue;
        }

        // Fixtures are per-subject so one subject's provider state never leaks into
        // another's numbers.
        let provider = fixtures::scripted_provider(
            "ok",
            vec![serde_json::json!({"id": "call_bench", "name": "echo", "arguments": "{}"})],
        )
        .await?;
        let environment = std::sync::Arc::new(fixtures::echo_environment().await?);
        eprintln!(
            "{}: provider {} environment {}",
            entry.name, provider.base_url, environment.base_url
        );

        // Either the runner starts the subject from its manifest — the normal path, and
        // the only one that yields a pid to sample memory from — or the operator points it
        // at something already running, in which case memory needs an explicit --pid.
        // Taken before the subject exists, so every memory figure for it is a difference
        // from a machine that was not running it. Zero when the kernel will not say (this
        // is a Linux measurement); the probes that need it refuse rather than guess.
        let machine_baseline_kib = host::machine_available_kib().unwrap_or(0);
        let mut running = match (&base_url, &entry.launch) {
            (Some(_), _) => None,
            (None, Some(launch)) => {
                match launch::start(
                    launch,
                    &entry.name,
                    writer.run_id(),
                    &provider.base_url,
                    &environment.base_url,
                )
                .await
                {
                    Ok(started) => Some(started),
                    // One subject that will not start is that subject's result, not the
                    // end of the run: the others still have numbers to give, and on spot
                    // capacity there may not be a second chance to collect them.
                    Err(error) => {
                        eprintln!("{}: {error:#}", entry.name);
                        writer.skipped(Skipped {
                            subject: entry.name.clone(),
                            probe: Probe::Create,
                            reason: format!("{error:#}"),
                        })?;
                        provider.shutdown();
                        environment.shutdown();
                        continue;
                    }
                }
            }
            // A hosted service whose driver already knows its own endpoint needs neither.
            // Daytona and E2B publish fixed API hosts; asking an operator to pass one in
            // would invite a run against the wrong region without saying so.
            (None, None) if drivers::carries_its_own_endpoint(&entry.name) => None,
            (None, None) => {
                writer.skipped(Skipped {
                    subject: entry.name.clone(),
                    probe: Probe::Create,
                    reason: "hosted subject with no launch block; pass --base-url to drive it"
                        .to_owned(),
                })?;
                provider.shutdown();
                environment.shutdown();
                continue;
            }
        };

        let base = running
            .as_ref()
            .map(|subject| subject.base_url.clone())
            .or_else(|| base_url.clone())
            // Empty for a hosted subject whose driver carries its own endpoint; the
            // driver ignores it, and the alternative is a placeholder URL that would
            // look like somewhere the benchmark actually talked to.
            .unwrap_or_default();
        let subject_pid = running.as_ref().map(|subject| subject.pid).or(pid);
        eprintln!("{}: driving {base} (pid {subject_pid:?})", entry.name);

        // The probe loop is a labelled block rather than a plain loop so that stopping —
        // for a spot notice or the budget — still falls through to teardown below. A
        // subject left running would hold ports and memory into the next one's numbers.
        let halt: Option<Outcome> = 'probes: {
            let bench = drivers::Bench {
                base_url: base.clone(),
                agentloop_package: agentloop.clone(),
                pid: subject_pid,
                environment: std::sync::Arc::clone(&environment),
                model_base_url: provider.base_url.clone(),
            };
            let mut subject_driver = match drivers::for_subject(&entry.name, &bench) {
                Ok(Some(driver)) => driver,
                Ok(None) => {
                    let _ = writer.skipped(Skipped {
                        subject: entry.name.clone(),
                        probe: Probe::Create,
                        reason: "no driver implemented for this subject yet".to_owned(),
                    });
                    break 'probes None;
                }
                Err(error) => {
                    let _ = writer.skipped(Skipped {
                        subject: entry.name.clone(),
                        probe: Probe::Create,
                        reason: format!("the driver for this subject could not start: {error:#}"),
                    });
                    break 'probes None;
                }
            };
            if let Err(error) = subject_driver.prepare().await {
                let _ = writer.skipped(Skipped {
                    subject: entry.name.clone(),
                    probe: Probe::Create,
                    reason: format!("preparation failed: {error:#}"),
                });
                break 'probes None;
            }

            for probe in probes::probe_list(entry, &wanted_probes) {
                if already.contains(&(entry.name.clone(), probe, None)) {
                    eprintln!(
                        "{}: {} already recorded, skipping",
                        entry.name,
                        probe.as_str()
                    );
                    continue;
                }
                if Instant::now() >= deadline {
                    break 'probes Some(Outcome::BudgetExhausted);
                }
                let context = probes::Context_ {
                    machine_baseline_kib,
                    samples,
                    units,
                    concurrency,
                    floor_ms,
                    // Only for a subject the runner started: it is the directory we told
                    // that subject to write to, so nobody else's state is in it.
                    data_dir: running.as_ref().map(|subject| subject.data_dir.clone()),
                    turns,
                    // Each probe gets what is left of the run budget, so one hung subject
                    // cannot eat the whole thing waiting on a client timeout per sample.
                    deadline,
                };
                let step = probes::measure(subject_driver.as_ref(), entry, probe, &context);
                let point = tokio::select! {
                    biased;
                    () = &mut interruption => {
                        eprintln!("spot termination notice; writing what is complete");
                        break 'probes Some(Outcome::Interrupted);
                    }
                    result = step => result,
                };
                let recorded = match point {
                    Ok(point) => writer.datapoint(point),
                    Err(error) => {
                        // A probe that failed says so in the results with its reason, and
                        // the subject's own log usually carries the cause.
                        if let Some(log) = running.as_ref().and_then(|subject| subject.log()) {
                            eprintln!("--- {} log ---\n{log}", entry.name);
                        }
                        writer.skipped(Skipped {
                            subject: entry.name.clone(),
                            probe,
                            reason: format!("{error:#}"),
                        })
                    }
                };
                if recorded.is_err() {
                    break 'probes Some(Outcome::Failed);
                }
            }

            // The fixture counts were printed and nothing was done with them. A turn that
            // never reaches the model still answers HTTP 200, so a run could report
            // latencies and a throughput figure for work that did not happen: at
            // concurrency 64, 12 of 256 requests reached the model and the runner said
            // 1,926 turns/s. Every timed turn must have a model call behind it.
            // Only for subjects wired to the scripted provider. A sandbox runs a command
            // and never calls a model, so comparing its turns against model calls
            // withheld perfectly good numbers for failing a test that did not apply.
            let asked = if matches!(entry.class, Class::SessionKernel) {
                subject_driver.turns_requested()
            } else {
                0
            };
            let served = provider.calls();
            // This verdict arrives too late to stop the datapoints: they were appended as
            // each probe finished, and JSONL cannot take a line back. The skip below is
            // therefore a *withdrawal* of records already in the file, and every consumer
            // has to apply it -- a reader that honours only the datapoints will publish
            // work that did not happen. One run left a create, a round_trip, a throughput
            // and a resident figure behind this gate.
            if served < asked {
                let _ = writer.skipped(Skipped {
                    subject: entry.name.clone(),
                    probe: Probe::RoundTrip,
                    reason: format!(
                        "the scripted provider served {served} model calls for {asked} timed turns; the missing turns never reached a model and this subject's numbers are withheld"
                    ),
                });
                break 'probes Some(Outcome::Failed);
            }
            None
        };

        eprintln!(
            "{}: fixtures served {} model calls and {} tool calls",
            entry.name,
            provider.calls(),
            environment.calls()
        );
        provider.shutdown();
        environment.shutdown();
        if let Some(subject) = &mut running {
            subject.stop().await;
        }
        if let Some(reason) = halt {
            outcome = reason;
            break 'subjects;
        }
    }

    writer.finish(outcome)?;
    eprintln!("run finished: {outcome:?}");
    Ok(())
}
