//! Firecracker, driven through its own API socket.
//!
//! An isolation substrate, not a sandbox and not a session kernel: the unit of work is one
//! microVM, and the only two questions it can answer here are what one costs to bring up
//! and what one costs to keep. Its numbers belong beside Wasmtime and V8, never in a table
//! with a session kernel.
//!
//! Firecracker has no TCP surface. Each microVM gets its own Unix socket, and the VMM is
//! configured over HTTP written onto that socket — three requests, then the guest boots.
//! That is why this driver spawns the process itself rather than being pointed at a
//! `--base-url`: there is nothing to point at until the microVM exists.
//!
//! The guest is judged ready when it has *run something*, not when the API returns. The
//! `InstanceStart` call returns as soon as the vCPUs are released, which is long before
//! Linux has booted, so timing that would report the VMM's cost and call it a microVM. The
//! initramfs init writes a marker to the serial console, Firecracker puts the serial
//! console on its stdout, and the sample ends when that marker reaches this process.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
};

use crate::driver::{Driver, Unit};

/// What the guest's init writes to the serial console once it is executing. The sample
/// ends here, so this marker is the definition of "created" for this subject.
const MARKER: &str = "BENCH-GUEST-READY";
/// Firecracker's own getting-started boot arguments, unchanged. A rival measured at
/// settings we tuned is a rival measured wrong, and `quiet` alone moves a boot number by
/// more than anything else on this line.
const BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off";
/// A boot that has not printed by here is not slow, it is broken.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct FirecrackerDriver {
    binary: PathBuf,
    kernel: PathBuf,
    initrd: PathBuf,
    vcpus: u32,
    mem_mib: u32,
    workdir: PathBuf,
    next: AtomicU64,
    /// The live VMMs, so `destroy` can reach one it did not create in the same call.
    live: Mutex<HashMap<String, Child>>,
}

impl FirecrackerDriver {
    pub fn new() -> Result<Self> {
        let path = |key: &str, fallback: &str| {
            std::env::var(key)
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(fallback))
        };
        let number = |key: &str, fallback: u32| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(fallback)
        };
        let driver = Self {
            binary: path("BENCH_FIRECRACKER_BIN", "/opt/firecracker/firecracker"),
            kernel: path("BENCH_FIRECRACKER_KERNEL", "/opt/firecracker/vmlinux"),
            initrd: path("BENCH_FIRECRACKER_INITRD", "/opt/firecracker/initrd.cpio"),
            // Firecracker's own defaults, stated rather than inherited so the run records
            // the guest it measured.
            vcpus: number("BENCH_FIRECRACKER_VCPUS", 1),
            mem_mib: number("BENCH_FIRECRACKER_MEM_MIB", 128),
            workdir: std::env::temp_dir().join("brain-bench-firecracker"),
            next: AtomicU64::new(0),
            live: Mutex::new(HashMap::new()),
        };
        std::fs::create_dir_all(&driver.workdir).with_context(|| {
            format!(
                "creating the microVM working directory {}",
                driver.workdir.display()
            )
        })?;
        for (what, file) in [
            ("the firecracker binary", &driver.binary),
            ("the guest kernel", &driver.kernel),
            ("the guest initramfs", &driver.initrd),
        ] {
            anyhow::ensure!(
                file.is_file(),
                "{what} is not at {}; set BENCH_FIRECRACKER_BIN, BENCH_FIRECRACKER_KERNEL \
                 and BENCH_FIRECRACKER_INITRD to the artefacts this run should measure",
                file.display()
            );
        }
        // Said here rather than found out as a boot failure thirty samples in.
        anyhow::ensure!(
            Path::new("/dev/kvm").exists(),
            "/dev/kvm does not exist: Firecracker needs nested KVM, which on EC2 is only \
             available on .metal capacity"
        );
        Ok(driver)
    }

    fn live(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Child>>> {
        self.live
            .lock()
            .map_err(|_| anyhow::anyhow!("the microVM table was poisoned by a panic"))
    }

    fn socket_for(&self, id: &str) -> PathBuf {
        self.workdir.join(format!("{id}.sock"))
    }

    /// Configure the VMM and start it, returning when the guest has printed the marker.
    async fn boot(
        &self,
        socket: &Path,
        ready: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        let kernel = self.kernel.display();
        let initrd = self.initrd.display();
        api::request(
            socket,
            "PUT",
            "/boot-source",
            &format!(
                r#"{{"kernel_image_path":"{kernel}","initrd_path":"{initrd}","boot_args":"{BOOT_ARGS}"}}"#
            ),
        )
        .await?;
        api::request(
            socket,
            "PUT",
            "/machine-config",
            &format!(
                r#"{{"vcpu_count":{},"mem_size_mib":{}}}"#,
                self.vcpus, self.mem_mib
            ),
        )
        .await?;
        api::request(
            socket,
            "PUT",
            "/actions",
            r#"{"action_type":"InstanceStart"}"#,
        )
        .await?;
        tokio::time::timeout(BOOT_TIMEOUT, ready)
            .await
            .with_context(|| {
                format!(
                    "the guest did not print {MARKER:?} within {}s of InstanceStart",
                    BOOT_TIMEOUT.as_secs()
                )
            })?
            .context("firecracker closed the guest console before the guest printed anything")?;
        Ok(())
    }
}

#[async_trait]
impl Driver for FirecrackerDriver {
    /// One microVM, outside the measurement. It fails loudly on a bad kernel or a
    /// permission problem instead of turning every sample into the same timeout, and it
    /// leaves the kernel image in page cache so the first timed boot is not the only one
    /// that pays to read it off disk.
    async fn prepare(&mut self) -> Result<()> {
        let unit = self
            .create()
            .await
            .context("the warm-up microVM never reached its guest")?;
        self.destroy(&unit).await
    }

    async fn create(&self) -> Result<Unit> {
        let id = format!("bench{}", self.next.fetch_add(1, Ordering::Relaxed));
        let socket = self.socket_for(&id);
        // A stale socket from an interrupted run makes Firecracker exit at once.
        let _ = std::fs::remove_file(&socket);

        let mut child = Command::new(&self.binary)
            .arg("--api-sock")
            .arg(&socket)
            .arg("--id")
            .arg(&id)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {}", self.binary.display()))?;

        let console = child
            .stdout
            .take()
            .context("firecracker was spawned without a console to read")?;
        let (announce, ready) = tokio::sync::oneshot::channel();
        // Drains for the life of the VMM rather than stopping at the marker: a full pipe
        // would block the guest's console writes, and a microVM held for the memory ramp
        // would then be stalled by this process not reading rather than idle.
        tokio::spawn(async move {
            let mut lines = BufReader::new(console).lines();
            let mut announce = Some(announce);
            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains(MARKER)
                    && let Some(announce) = announce.take()
                {
                    let _ = announce.send(());
                }
            }
        });

        match self.boot(&socket, ready).await {
            Ok(()) => {
                self.live()?.insert(id.clone(), child);
                Ok(Unit { id })
            }
            Err(error) => {
                // Firecracker puts its own faults on stderr, and they are the only thing
                // that distinguishes a missing /dev/kvm from an unbootable kernel from a
                // guest that booted and printed nothing.
                let complaint = stderr_of(&mut child).await;
                let _ = child.kill().await;
                let _ = std::fs::remove_file(&socket);
                Err(error.context(format!("firecracker said: {complaint}")))
            }
        }
    }

    async fn ttfb_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!(
            "not wired: a microVM has no submitted work to take a first byte from, which is \
             why this subject declares only create and resident"
        )
    }

    async fn round_trip_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!(
            "not wired: driving work inside the guest needs an agent in the guest, and the \
             number would then measure that agent rather than Firecracker"
        )
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        // Taken out of the table before awaiting, so no lock is held across an await and
        // a second destroy of the same unit is a no-op rather than a hang.
        let child = self.live()?.remove(&unit.id);
        if let Some(mut child) = child {
            // Firecracker has no shutdown that returns when the memory is gone; killing
            // the VMM and reaping it does, and a reclaim number taken before the process
            // is reaped would be measuring a process that still exists.
            child
                .kill()
                .await
                .with_context(|| format!("killing the VMM for microVM {}", unit.id))?;
        }
        let _ = std::fs::remove_file(self.socket_for(&unit.id));
        Ok(())
    }

    /// Every VMM is a child of this process, so the tree walk that sums private memory for
    /// any other subject sums the microVMs here. The runner's own footprint lands in the
    /// fit's intercept, which is exactly what the intercept is for; the quotable number is
    /// the slope, and the slope is per microVM.
    fn pid(&self) -> Option<u32> {
        Some(std::process::id())
    }

    fn turns_requested(&self) -> u64 {
        // No turn probe is declared, so nothing is claimed and nothing needs checking.
        0
    }
}

/// Whatever Firecracker complained about before it died. Bounded by a timeout because a
/// VMM that is still alive will never close the pipe.
async fn stderr_of(child: &mut Child) -> String {
    let Some(mut stderr) = child.stderr.take() else {
        return "<nothing on stderr>".to_owned();
    };
    let mut said = String::new();
    match tokio::time::timeout(Duration::from_secs(2), stderr.read_to_string(&mut said)).await {
        Ok(Ok(_)) if said.trim().is_empty() => "<nothing on stderr>".to_owned(),
        Ok(Ok(_)) => said.trim().to_owned(),
        Ok(Err(error)) => format!("<stderr unreadable: {error}>"),
        Err(_) => format!("<still running, stderr so far: {}>", said.trim()),
    }
}

/// HTTP/1.1 written onto a Unix socket by hand.
///
/// Firecracker's API is four requests and a 204, and every HTTP client in the tree speaks
/// TCP only. Writing the request rather than adding a Unix-socket transport keeps the
/// runner's dependency list the same for every subject, which is the point of a benchmark
/// that links nothing it measures.
#[cfg(unix)]
mod api {
    use std::{path::Path, time::Duration, time::Instant};

    use anyhow::{Context, Result};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// How long Firecracker gets to create its API socket after exec.
    const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

    pub async fn request(socket: &Path, method: &str, path: &str, body: &str) -> Result<()> {
        let mut stream = connect(socket).await?;
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .with_context(|| format!("sending {method} {path} to the Firecracker API"))?;
        let (status, said) = response(&mut stream)
            .await
            .with_context(|| format!("reading the answer to {method} {path}"))?;
        // The body, never just the status: Firecracker answers every rejection with 400
        // and puts the only useful part — which field, and what was wrong with it — in a
        // `fault_message`. Discarding it makes every misconfiguration look identical.
        anyhow::ensure!(
            (200..300).contains(&status),
            "{method} {path} {body}: HTTP {status}: {said}"
        );
        Ok(())
    }

    /// Firecracker creates its socket a moment after exec, so the first request of a
    /// microVM's life races it. Retrying the connect is that wait, and it is inside the
    /// measurement because an orchestrator pays it too.
    async fn connect(socket: &Path) -> Result<tokio::net::UnixStream> {
        let deadline = Instant::now() + SOCKET_TIMEOUT;
        loop {
            match tokio::net::UnixStream::connect(socket).await {
                Ok(stream) => return Ok(stream),
                Err(error) if Instant::now() >= deadline => {
                    return Err(anyhow::Error::from(error).context(format!(
                        "the Firecracker API socket {} never appeared within {}s",
                        socket.display(),
                        SOCKET_TIMEOUT.as_secs()
                    )));
                }
                Err(_) => tokio::time::sleep(Duration::from_micros(200)).await,
            }
        }
    }

    /// Status and body of one response. Content-Length rather than connection close,
    /// because Firecracker keeps the connection open and waiting for an EOF would hang.
    async fn response(stream: &mut tokio::net::UnixStream) -> Result<(u16, String)> {
        let mut buffer = Vec::new();
        let head = loop {
            if let Some(end) = find(&buffer, b"\r\n\r\n") {
                break end;
            }
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await?;
            anyhow::ensure!(
                read > 0,
                "the Firecracker API closed the connection without answering"
            );
            buffer.extend_from_slice(&chunk[..read]);
        };
        let headers = String::from_utf8_lossy(&buffer[..head]).into_owned();
        let mut lines = headers.lines();
        let status: u16 = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .with_context(|| format!("the Firecracker API answered {headers:?}"))?;
        let length: usize = lines
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())?
            })
            .unwrap_or(0);

        let mut body = buffer.split_off(head + 4);
        while body.len() < length {
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }
        Ok((status, String::from_utf8_lossy(&body).into_owned()))
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }
}

/// Off Unix there is no socket to write to, and saying so beats a driver that compiles
/// everywhere and can only run in one place without admitting it.
#[cfg(not(unix))]
mod api {
    use std::path::Path;

    use anyhow::Result;

    pub async fn request(_socket: &Path, method: &str, path: &str, _body: &str) -> Result<()> {
        anyhow::bail!(
            "{method} {path}: the Firecracker API is a Unix socket and this host has none; \
             this subject runs only on a Linux .metal instance"
        )
    }
}
