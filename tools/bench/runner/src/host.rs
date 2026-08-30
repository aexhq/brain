//! Host identity, tuning, and the two things that make a spot instance different from a
//! laptop: it can be taken away mid-run, and its CPU can be stolen while you measure.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::schema::{Ec2, Host, Steal, Tuning};

const IMDS: &str = "http://169.254.169.254";
/// Above this fraction of stolen CPU, a latency percentile is the neighbour's, not the
/// subject's. Deliberately tight: the numbers under test are single-digit milliseconds.
pub const STEAL_TOLERANCE: f64 = 0.01;

pub async fn detect(label: impl Into<String>) -> Host {
    Host {
        label: label.into(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        kernel: read_trimmed("/proc/sys/kernel/osrelease"),
        cpus: std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(0),
        ec2: ec2().await,
        tuning: tuning(),
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn tuning() -> Tuning {
    Tuning {
        // Reported as "always [madvise] never"; the bracketed value is in effect.
        transparent_hugepages: read_trimmed("/sys/kernel/mm/transparent_hugepage/enabled")
            .and_then(|value| {
                value
                    .split_whitespace()
                    .find(|part| part.starts_with('['))
                    .map(|part| part.trim_matches(['[', ']']).to_owned())
            }),
        cpu_governor: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        subject_cpus: std::env::var("BENCH_SUBJECT_CPUS").ok(),
        generator_cpus: std::env::var("BENCH_GENERATOR_CPUS").ok(),
    }
}

/// IMDSv2. A token is required; IMDSv1 is disabled on current AMIs. Off EC2 this returns
/// `None` quickly rather than hanging, because the link-local address is unroutable.
async fn ec2() -> Option<Ec2> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(400))
        .build()
        .ok()?;
    let token = client
        .put(format!("{IMDS}/latest/api/token"))
        .header("x-aws-ec2-metadata-token-ttl-seconds", "60")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;

    let get = async |path: &str| -> Option<String> {
        client
            .get(format!("{IMDS}/latest/meta-data/{path}"))
            .header("x-aws-ec2-metadata-token", &token)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .text()
            .await
            .ok()
    };

    let instance_type = get("instance-type").await?;
    let availability_zone = get("placement/availability-zone").await?;
    Some(Ec2 {
        metal: instance_type.ends_with(".metal")
            || instance_type.contains(".metal-")
            || instance_type.ends_with("metal"),
        region: get("placement/region").await.unwrap_or_else(|| {
            availability_zone
                .trim_end_matches(char::is_alphabetic)
                .to_owned()
        }),
        instance_id: get("instance-id").await.unwrap_or_default(),
        // Absent on on-demand capacity, which is how the two are told apart.
        lifecycle: get("instance-life-cycle")
            .await
            .unwrap_or_else(|| "unknown".to_owned()),
        instance_type,
        availability_zone,
    })
}

/// Polls the spot interruption endpoint. Resolves when a termination notice appears,
/// which on EC2 is about two minutes before the instance goes away — enough to finish the
/// probe in flight, write a footer, and leave the rest resumable.
///
/// Off EC2 this never resolves, so it is safe to select on unconditionally.
pub async fn spot_interruption() {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(400))
        .build()
    else {
        std::future::pending::<()>().await;
        unreachable!()
    };
    loop {
        let token = client
            .put(format!("{IMDS}/latest/api/token"))
            .header("x-aws-ec2-metadata-token-ttl-seconds", "60")
            .send()
            .await
            .ok()
            .and_then(|response| response.error_for_status().ok());
        if let Some(token) = token
            && let Ok(token) = token.text().await
        {
            let response = client
                .get(format!("{IMDS}/latest/meta-data/spot/instance-action"))
                .header("x-aws-ec2-metadata-token", token)
                .send()
                .await;
            // 200 means a notice is posted. 404 is the normal, uninterrupted state.
            if let Ok(response) = response
                && response.status().is_success()
            {
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Cumulative CPU jiffies from `/proc/stat`, as (total, steal).
fn cpu_jiffies() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse().ok())
        .collect();
    // user nice system idle iowait irq softirq steal guest guest_nice
    let steal = *fields.get(7)?;
    Some((fields.iter().sum(), steal))
}

/// Brackets a probe to report the share of CPU the hypervisor took while it ran.
pub struct StealWindow(Option<(u64, u64)>);

impl StealWindow {
    pub fn open() -> Self {
        Self(cpu_jiffies())
    }

    pub fn close(self) -> Steal {
        let (Some((total_before, steal_before)), Some((total_after, steal_after))) =
            (self.0, cpu_jiffies())
        else {
            // Not Linux, or /proc unreadable. Report zero rather than inventing a number;
            // the platform gate elsewhere already refuses memory work here.
            return Steal::default();
        };
        let total = total_after.saturating_sub(total_before);
        let stolen = steal_after.saturating_sub(steal_before);
        let fraction = if total == 0 {
            0.0
        } else {
            stolen as f64 / total as f64
        };
        Steal {
            fraction,
            exceeded: fraction > STEAL_TOLERANCE,
        }
    }
}

/// Refuses to proceed when the host cannot support private-memory sampling, instead of
/// quietly substituting RSS.
/// Memory the machine has left, in KiB, as the kernel reckons it.
///
/// The outside-in measurement: what a person watching the box would see. It counts
/// everything a session actually costs — the server's own heap, the worker processes it
/// spawns, the page cache its writes pull in, the kernel structures behind its sockets —
/// none of which appear in one process's RSS. Slope-fitting a process tree's private
/// memory against session count was the sophisticated version of this, and it could not
/// resolve 8,192 sessions: r-squared 0.255 against a line, on a ramp big enough to be
/// 50 MB of real memory.
///
/// `MemAvailable` rather than `MemFree`, because the kernel will hand back reclaimable
/// cache on demand and counting it as consumed would charge every subject for its own
/// page cache twice.
pub fn machine_available_kib() -> Result<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo")
        .context("reading /proc/meminfo; this measurement is Linux-only")?;
    meminfo
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("MemAvailable:")?;
            rest.split_whitespace().next()?.parse::<u64>().ok()
        })
        .context("/proc/meminfo carried no MemAvailable")
}

pub fn require_private_memory_support() -> Result<()> {
    anyhow::ensure!(
        cfg!(target_os = "linux"),
        "private memory sampling needs Linux /proc/*/smaps_rollup; run this arm on the \
         benchmark instance, and never substitute RSS for private memory"
    );
    Ok(())
}

/// Parses a CPU list — `0-1`, `2,3`, `0-1,4` — into the ids it names.
pub fn cpu_list(list: &str) -> Option<Vec<usize>> {
    let mut cpus = Vec::new();
    for part in list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('-') {
            Some((first, last)) => {
                let first: usize = first.trim().parse().ok()?;
                let last: usize = last.trim().parse().ok()?;
                if last < first {
                    return None;
                }
                cpus.extend(first..=last);
            }
            None => cpus.push(part.parse().ok()?),
        }
    }
    (!cpus.is_empty()).then_some(cpus)
}

/// Pin `pid` — or this process, for pid 0 — to `cpus`.
///
/// `BENCH_SUBJECT_CPUS` and `BENCH_GENERATOR_CPUS` were read, recorded in the run and
/// printed in the report, and never applied to anything. A report that says the subject
/// ran on CPUs 2-3 while it in fact floated across every core describes a run that did
/// not happen, and the generator and the subject were free to contend for the same core.
#[cfg(target_os = "linux")]
pub fn pin(pid: u32, list: &str) -> anyhow::Result<()> {
    let cpus = cpu_list(list)
        .ok_or_else(|| anyhow::anyhow!("{list:?} is not a CPU list like `0-1` or `2,3`"))?;
    // SAFETY: `set` is zeroed before use and only indices below `CPU_SETSIZE` are set.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        for cpu in &cpus {
            anyhow::ensure!(
                *cpu < libc::CPU_SETSIZE as usize,
                "CPU {cpu} is outside the set this kernel supports"
            );
            libc::CPU_SET(*cpu, &mut set);
        }
        if libc::sched_setaffinity(pid as libc::pid_t, std::mem::size_of_val(&set), &set) != 0 {
            return Err(anyhow::Error::from(std::io::Error::last_os_error())
                .context(format!("pinning pid {pid} to CPUs {list}")));
        }
    }
    Ok(())
}

/// Affinity is a Linux measurement control. Elsewhere, saying so beats recording a
/// tuning the run did not have.
#[cfg(not(target_os = "linux"))]
pub fn pin(_pid: u32, list: &str) -> anyhow::Result<()> {
    // Read first, so an unusable list is the same error everywhere and is not hidden
    // behind the platform that cannot apply it.
    cpu_list(list)
        .ok_or_else(|| anyhow::anyhow!("{list:?} is not a CPU list like `0-1` or `2,3`"))?;
    anyhow::bail!("CPU pinning needs Linux")
}

#[cfg(test)]
mod tests {
    use super::cpu_list;

    #[test]
    fn a_cpu_list_reads_the_forms_taskset_accepts() {
        assert_eq!(cpu_list("3"), Some(vec![3]));
        assert_eq!(cpu_list("0-1"), Some(vec![0, 1]));
        assert_eq!(cpu_list("2,3"), Some(vec![2, 3]));
        assert_eq!(cpu_list("0-1,4"), Some(vec![0, 1, 4]));
        assert_eq!(cpu_list(" 0 - 1 , 4 "), Some(vec![0, 1, 4]));
    }

    /// A list that cannot be read must not silently pin nothing: the run would record a
    /// tuning it did not have.
    #[test]
    fn an_unreadable_cpu_list_is_refused() {
        assert_eq!(cpu_list(""), None);
        assert_eq!(cpu_list("all"), None);
        assert_eq!(cpu_list("1-0"), None, "a reversed range names no CPUs");
        assert_eq!(cpu_list("0..2"), None);
    }
}
