//! Cold and warm session start.
//!
//! The `create` probe answers one of these and hides the other. It creates sessions in a
//! loop against a process that is already running and already warm, then discards the
//! first tenth of its samples — so what it publishes is deliberately the steady state, and
//! the number a person actually meets first is the one it threw away.
//!
//! Three things are separated here, because they are paid at different times and by
//! different people:
//!
//! - **boot**: the process from `exec` until it says it is ready. Paid on deploy, and
//!   again on every scale-from-zero.
//! - **cold**: the first session on a process that has never held one. Whatever the
//!   subject defers to first use is in here — lazily built caches, a connection pool that
//!   opens on demand, a compile that has not happened yet.
//! - **warm**: sessions after that, which is what `create` publishes.
//!
//! Admitting the agentloop is done before the cold sample and is not in it: uploading a
//! package is something an operator does once, not something a session pays for.

use std::{path::PathBuf, time::Instant};

use anyhow::{Context, Result};

use crate::{drivers, fixtures, launch, stats, subject};

pub struct Options {
    pub subject: String,
    /// Directory of subject manifests.
    pub subjects: PathBuf,
    /// Process starts. Each one yields one boot sample and one cold sample, so this is the
    /// sample count for both — a cold start cannot be measured twice on the same process.
    pub repeats: usize,
    /// Sessions created after the cold one, on each process.
    pub warm_per_repeat: usize,
    pub agentloop: PathBuf,
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
        .with_context(|| format!("{} has no launch block, so it cannot be started cold", entry.name))?;

    let mut boot = Vec::with_capacity(options.repeats);
    let mut cold = Vec::with_capacity(options.repeats);
    let mut warm = Vec::with_capacity(options.repeats * options.warm_per_repeat);

    for repeat in 0..options.repeats {
        let (boot_ms, cold_ms, warm_ms) = one_process(entry, launch_block, &options, repeat)
            .await
            .with_context(|| format!("cold start {repeat}"))?;
        boot.push(boot_ms);
        cold.push(cold_ms);
        warm.extend(warm_ms);
        eprintln!("{}: boot {boot_ms:.1} ms, cold {cold_ms:.3} ms", entry.name);
    }

    report("boot (exec until ready)", &mut boot);
    let cold_p50 = report("cold (first session on a new process)", &mut cold);
    let warm_p50 = report("warm (every session after)", &mut warm);
    // The whole point of separating them. A ratio near 1 says the subject defers nothing
    // to first use; a large one says the first caller pays for everyone.
    if let (Some(cold_p50), Some(warm_p50)) = (cold_p50, warm_p50)
        && warm_p50 > 0.0
    {
        println!();
        println!("cold is {:.1}x warm", cold_p50 / warm_p50);
    }
    Ok(())
}

/// Prints one line and hands back the median, when there are enough samples for one.
fn report(label: &str, samples: &mut [f64]) -> Option<f64> {
    let count = samples.len();
    let summary = stats::summarize(samples);
    let show = |value: Option<f64>| {
        value.map_or_else(|| "     n/a".to_owned(), |value| format!("{value:8.3}"))
    };
    println!(
        "{label:38} p50 {} ms  p90 {}  min {}  max {}  n={count}",
        show(summary.p50_ms),
        show(summary.p90_ms),
        show(summary.min_ms),
        show(summary.max_ms),
    );
    summary.p50_ms
}

async fn one_process(
    entry: &subject::Subject,
    launch_block: &subject::Launch,
    options: &Options,
    repeat: usize,
) -> Result<(f64, f64, Vec<f64>)> {
    let provider = fixtures::scripted_provider(
        "ok",
        vec![serde_json::json!({"id": "call_bench", "name": "echo", "arguments": "{}"})],
    )
    .await?;
    let environment = std::sync::Arc::new(fixtures::echo_environment().await?);
    let run_id = format!("cold-{repeat}-{}", std::process::id());

    // Timed around the launch itself, which returns once the subject's readiness endpoint
    // answers. An empty data directory every time: a process that starts onto a journal it
    // has already written is not starting cold.
    let started = Instant::now();
    let mut running = launch::start(
        launch_block,
        &entry.name,
        &run_id,
        &provider.base_url,
        &environment.base_url,
    )
    .await?;
    let boot_ms = started.elapsed().as_secs_f64() * 1_000.0;

    let result = measure(entry, options, &provider, &environment, &running).await;

    provider.shutdown();
    environment.shutdown();
    running.stop().await;
    let (cold_ms, warm_ms) = result?;
    Ok((boot_ms, cold_ms, warm_ms))
}

async fn measure(
    entry: &subject::Subject,
    options: &Options,
    provider: &fixtures::Fixture,
    environment: &std::sync::Arc<fixtures::Fixture>,
    running: &launch::Running,
) -> Result<(f64, Vec<f64>)> {
    let bench = drivers::Bench {
        base_url: running.base_url.clone(),
        agentloop_package: options.agentloop.clone(),
        pid: Some(running.pid),
        environment: std::sync::Arc::clone(environment),
        model_base_url: provider.base_url.clone(),
    };
    let mut driver = drivers::for_subject(&entry.name, &bench)?
        .with_context(|| format!("no driver for {}", entry.name))?;
    // Outside the measurement: admitting an agentloop is an operator's one-off, and
    // charging a session for it would say something untrue about both numbers.
    driver.prepare().await?;

    let started = Instant::now();
    let unit = driver.create().await?;
    let cold_ms = started.elapsed().as_secs_f64() * 1_000.0;
    driver.destroy(&unit).await?;

    let mut warm = Vec::with_capacity(options.warm_per_repeat);
    for _ in 0..options.warm_per_repeat {
        let started = Instant::now();
        let unit = driver.create().await?;
        warm.push(started.elapsed().as_secs_f64() * 1_000.0);
        driver.destroy(&unit).await?;
    }
    Ok((cold_ms, warm))
}
