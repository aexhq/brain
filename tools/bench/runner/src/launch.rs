//! Starting a subject from its manifest, and stopping it again.
//!
//! Doing this in the runner rather than by hand is what makes the memory arm possible: the
//! runner knows the pid it started, so it can sample that process tree. It also removes a
//! whole class of unfair comparison, because every subject gets the same treatment — a
//! fresh data directory, the same fixtures, the same readiness definition, and the same
//! teardown.

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::process::{Child, Command};

use crate::subject::Launch;

/// A subject the runner started and is responsible for stopping.
pub struct Running {
    pub base_url: String,
    pub pid: u32,
    /// Where the subject was told to keep its state. The persistence probe watches this
    /// grow, which is the only way to ask every subject the same storage question without
    /// knowing how any of them stores anything.
    pub data_dir: PathBuf,
    child: Child,
    log_path: PathBuf,
}

/// What a manifest may interpolate into its command, arguments, environment and URLs.
pub struct Placeholders {
    pub port: u16,
    pub model_base_url: String,
    pub environment_base_url: String,
    pub data_dir: String,
}

impl Placeholders {
    fn apply(&self, template: &str) -> String {
        template
            .replace("{port}", &self.port.to_string())
            .replace("{model_base_url}", &self.model_base_url)
            .replace("{environment_base_url}", &self.environment_base_url)
            .replace("{data_dir}", &self.data_dir)
    }
}

/// A port nothing is listening on. Bound and immediately released, so there is a small
/// race with anything else on the box claiming it — acceptable on a dedicated benchmark
/// instance, and far better than a hardcoded port colliding across subjects in one run.
pub fn free_port() -> Result<u16> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .context("finding a free port for the subject")?;
    Ok(listener.local_addr()?.port())
}

/// Starts the subject and waits until it reports ready.
///
/// Every subject gets an empty data directory. Reusing one would let a previous run's
/// session log sit in the page cache and in the subject's own state, which moves both the
/// memory numbers and the latency numbers — and would move them by different amounts for
/// different subjects.
pub async fn start(
    launch: &Launch,
    subject: &str,
    run_id: &str,
    model_base_url: &str,
    environment_base_url: &str,
) -> Result<Running> {
    start_in(
        launch,
        subject,
        run_id,
        model_base_url,
        environment_base_url,
        None,
    )
    .await
}

/// Like [`start`], but the caller may supply the data directory. `Some(dir)` reuses it
/// as-is — the recovery probe's whole point — while `None` gets the usual fresh one.
pub async fn start_in(
    launch: &Launch,
    subject: &str,
    run_id: &str,
    model_base_url: &str,
    environment_base_url: &str,
    reuse_data_dir: Option<PathBuf>,
) -> Result<Running> {
    let port = free_port()?;
    let data_dir = match reuse_data_dir {
        Some(existing) => existing,
        None => {
            let fresh = std::env::temp_dir().join(format!("brain-bench-{run_id}-{subject}"));
            // Removed first in case a previous interrupted run left one behind: on spot
            // capacity the teardown does not always get to run.
            let _ = std::fs::remove_dir_all(&fresh);
            std::fs::create_dir_all(&fresh)
                .with_context(|| format!("creating data directory {}", fresh.display()))?;
            fresh
        }
    };

    let placeholders = Placeholders {
        port,
        model_base_url: model_base_url.to_owned(),
        environment_base_url: environment_base_url.to_owned(),
        data_dir: data_dir.display().to_string(),
    };

    // Beside the data directory, never inside it. The persistence probe measures every
    // byte under `data_dir`, and the runner's own capture of the subject's stdout is not
    // something the subject chose to persist: at ~10 KiB per 100 turns it was noise for a
    // framework that writes megabytes and about 40% of the total for one that writes tens
    // of kilobytes, which is exactly the comparison the probe exists to make.
    let log_path = data_dir.with_file_name(format!("brain-bench-{run_id}-{subject}.log"));
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("creating {}", log_path.display()))?;
    let stderr = log.try_clone()?;

    let mut command = Command::new(placeholders.apply(&launch.command));
    for argument in &launch.args {
        command.arg(placeholders.apply(argument));
    }
    for (key, value) in &launch.env {
        command.env(key, placeholders.apply(value));
    }
    command.stdout(log).stderr(stderr).kill_on_drop(true);

    // Its own process group, so teardown can reach the children too. Brain runs loop
    // workers, and a subject whose children outlive it would hold ports and memory into
    // the next subject's measurement.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let child = command
        .spawn()
        .with_context(|| format!("starting {} for {subject}", launch.command))?;
    let pid = child
        .id()
        .context("subject exited before it could be sampled")?;

    // Pin the subject where the run says it runs. Recording `subject_cpus` without
    // applying it described a run that did not happen, and left the subject free to
    // contend with the load generator for the same core.
    if let Ok(cpus) = std::env::var("BENCH_SUBJECT_CPUS") {
        crate::host::pin(pid, &cpus)
            .with_context(|| format!("pinning {subject} to CPUs {cpus}"))?;
    }

    let mut running = Running {
        base_url: placeholders.apply(&launch.base_url),
        pid,
        child,
        data_dir,
        log_path,
    };

    let ready_url = placeholders.apply(&launch.ready_url);
    if let Err(error) = wait_ready(&ready_url, launch.ready_timeout_secs).await {
        let log = running.log().unwrap_or_default();
        running.stop().await;
        anyhow::bail!("{subject} never became ready: {error:#}\n--- subject log ---\n{log}");
    }
    Ok(running)
}

async fn wait_ready(url: &str, timeout_secs: u64) -> Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut last = String::from("never answered");
    while Instant::now() < deadline {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => last = format!("{url} returned {}", response.status()),
            Err(error) => last = format!("{url}: {error}"),
        }
        // Two milliseconds, not two hundred. This loop is what times a boot, so its own
        // interval is the floor under every boot figure: at 200 ms a process that was
        // ready in five reported 206, and thirty runs agreed to within 1.3 ms because
        // they were all measuring the sleep. It is a loopback GET; polling it hard costs
        // nothing that matters.
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    anyhow::bail!("gave up after {timeout_secs}s; last attempt: {last}")
}

impl Running {
    /// The subject's own output, for when a probe fails and the reason is in its log
    /// rather than in the HTTP response.
    pub fn log(&self) -> Option<String> {
        std::fs::read_to_string(&self.log_path).ok().map(|text| {
            // Enough to see the failure, not enough to bury the error that quoted it.
            // Walked back to a character boundary, because a subject's log is arbitrary
            // bytes and slicing mid-character would panic while reporting another failure.
            let mut start = text.len().saturating_sub(4_000);
            while start < text.len() && !text.is_char_boundary(start) {
                start += 1;
            }
            text[start..].to_owned()
        })
    }

    /// Kills the subject the way a crash does — no grace, no flush — and keeps its data
    /// directory, so a relaunch can find out what survived. The recovery probe's kill.
    pub async fn kill_hard(&mut self) {
        #[cfg(unix)]
        {
            // Signal the whole group, so loop workers and other children go too.
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    /// Stops the subject and removes its data directory.
    ///
    /// Called on the normal path, on a failed readiness check, and on spot interruption.
    /// Leaving a subject running would corrupt the next one's numbers; leaving the data
    /// directory would fill the instance across a long run.
    pub async fn stop(&mut self) {
        #[cfg(unix)]
        {
            // Signal the whole group, so loop workers and other children go too.
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGTERM);
            }
            // A short grace period to exit cleanly, then the hard kill below.
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}


