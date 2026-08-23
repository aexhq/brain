//! H0.75 S2: daemon memory under many long-suspended guest awaits. Each parked activation
//! holds one resident wasmtime instance plus one suspended async fiber awaiting a ctx
//! result, which is exactly the shape of a session parked on a slow model round. Run with:
//! `cargo test -p brain-loophost --test fiber_density -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use brain::Result;
use brain::agentloop::{Agentloop, ContractOpOutcome, LoopTerminal, TurnCtx, op_error};
use brain_loophost::remote::{RemoteAgentloop, SpawnedLoopHost, WireClient};
use brain_protocol::agentloop::{AgentloopErrorCode, CtxOp};

fn component_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("guest/dist/aex-loop.component.wasm")
}

/// A ctx that parks the first model round until released, then aborts the turn.
struct ParkingCtx {
    session: String,
    release: Arc<tokio::sync::Notify>,
    released: Arc<std::sync::atomic::AtomicBool>,
    parked: Arc<AtomicU64>,
    terminal: Option<LoopTerminal>,
}

#[async_trait]
impl TurnCtx for ParkingCtx {
    async fn contract_op(&mut self, op: CtxOp) -> Result<ContractOpOutcome> {
        match op {
            CtxOp::ModelStream { .. } => {
                self.parked.fetch_add(1, Ordering::AcqRel);
                loop {
                    let notified = self.release.notified();
                    if self.released.load(Ordering::Acquire) {
                        break;
                    }
                    notified.await;
                }
                Ok(Err(op_error(
                    AgentloopErrorCode::Aborted,
                    "fiber-density park released",
                    false,
                )))
            }
            _ => Ok(Err(op_error(
                AgentloopErrorCode::Internal,
                "unexpected op in the fiber-density harness",
                false,
            ))),
        }
    }

    fn activation_message(&self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "message": { "seq": 1, "content": [{"type": "text", "text": "park"}] },
            "session": {
                "id": self.session,
                "limits": { "max_rounds_per_turn": 4 },
                "metadata": {},
            },
        }))
    }

    async fn session_start_payload(&mut self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "kv": {},
            "latest_mark": null,
            "tail": [],
            "resumed": false,
            "session": { "id": self.session },
        }))
    }

    fn loop_terminal(&self) -> Option<&LoopTerminal> {
        self.terminal.as_ref()
    }
}

fn daemon_rss_bytes(pid: u32) -> Option<u64> {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().find(|line| line.contains(&pid.to_string()))?;
        let field = line.rsplit('"').nth(1)?;
        let digits: String = field.chars().filter(char::is_ascii_digit).collect();
        Some(digits.parse::<u64>().ok()? * 1024)
    }
    #[cfg(not(windows))]
    {
        let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "H0.75 S2 measurement; run explicitly with --ignored --nocapture"]
async fn s2_fiber_stack_memory_under_parked_awaits() {
    let parked_target: usize = std::env::var("S2_PARKED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000);
    let component = component_path();
    assert!(
        component.exists(),
        "build the guest component first (run the loop_e2e suite once)"
    );
    let host = SpawnedLoopHost::spawn(Path::new(env!("CARGO_BIN_EXE_loophost")), &component)
        .expect("loop-host daemon");
    let pid = host.pid();
    let client = WireClient::connect(host.addr, &host.token)
        .await
        .expect("connect");
    let agentloop = Arc::new(RemoteAgentloop::new(client));

    let baseline = daemon_rss_bytes(pid).expect("baseline rss");
    let release = Arc::new(tokio::sync::Notify::new());
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let parked = Arc::new(AtomicU64::new(0));

    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..parked_target {
        let agentloop = agentloop.clone();
        let release = release.clone();
        let released = released.clone();
        let parked = parked.clone();
        tasks.spawn(async move {
            let mut ctx = ParkingCtx {
                session: format!("ses_fiberdensity{index:012}"),
                release,
                released,
                parked,
                terminal: None,
            };
            agentloop.drive_turn(&mut ctx).await
        });
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while (parked.load(Ordering::Acquire) as usize) < parked_target {
        assert!(
            std::time::Instant::now() < deadline,
            "only {} of {parked_target} activations parked",
            parked.load(Ordering::Acquire)
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let parked_rss = daemon_rss_bytes(pid).expect("parked rss");

    released.store(true, Ordering::Release);
    release.notify_waiters();
    let mut finished = 0usize;
    while let Some(joined) = tasks.join_next().await {
        if let Err(error) = joined.expect("task") {
            panic!("drive_turn failed: {error}");
        }
        finished += 1;
    }
    assert_eq!(finished, parked_target);
    let after_rss = daemon_rss_bytes(pid).expect("after rss");

    let delta = parked_rss.saturating_sub(baseline);
    println!(
        "S2 fiber density: baseline {:.1} MiB; {parked_target} parked awaits {:.1} MiB \
         (delta {:.1} MiB, {:.1} KiB per parked activation); after release {:.1} MiB",
        baseline as f64 / 1048576.0,
        parked_rss as f64 / 1048576.0,
        delta as f64 / 1048576.0,
        delta as f64 / 1024.0 / parked_target as f64,
        after_rss as f64 / 1048576.0,
    );
}
