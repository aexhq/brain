//! Drives one session against a hand over its WebSocket: hello, write a file, bash a build,
//! detached job + poll + cancel, then prints a one-line PASS/FAIL. Used by tools/smoke.sh to
//! prove the guest agent works inside a plain Docker container.
//!
//!   cargo run -p brain-hand-client --example smoke -- ws://127.0.0.1:8080/ <token>
use std::time::Duration;

use base64::Engine;
use brain_hand_client::{HandClient, root_lane, start_request};
use brain_protocol::abi::{
    CancelRequest, Cursor, HelloRequest, LaneMode, LaneRef, OperationStatus, PollRequest,
    ProtocolVersion, Stream, SyncScope,
};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "ws://127.0.0.1:8080/".into());
    let token = args.next().unwrap_or_else(|| "tok".into());

    let c = HandClient::connect(&url, 1).await?;
    let hello = c
        .hello(HelloRequest {
            protocol: ProtocolVersion::CURRENT,
            session_id: "ses_smoke0000000000000000000".parse().unwrap(),
            session_token: token,
            expected_generation_id: None,
            tool_manifest: brain_protocol::tools::manifest_v1().clone(),
            tool_manifest_digest: brain_protocol::tools::TOOL_MANIFEST_V1_DIGEST
                .trim()
                .parse()
                .unwrap(),
            env: Default::default(),
            sync: SyncScope {
                roots: vec![hello_ws(), "/home/agent".into()],
                exclude: vec![],
            },
            restore: None,
            heartbeat_ms: 2000,
        })
        .await?;
    println!(
        "hello: generation={} tools={:?}",
        *hello.generation_id,
        hello
            .tools
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(hello.tools.len(), 7);

    // write -> read round trip via typed tools.
    let w = c
        .start(start_request(
            "s-write",
            "write",
            json!({"path": "hello.txt", "content": "hi from the hand\n"}),
            root_lane(),
            None,
            false,
            10_000,
            0,
        ))
        .await?;
    assert_eq!(w.view.terminal.as_ref().unwrap().exit_code, Some(0));
    let r = c
        .start(start_request(
            "s-read",
            "read",
            json!({"path": "hello.txt"}),
            root_lane(),
            None,
            false,
            10_000,
            4096,
        ))
        .await?;
    let text = decode(&r.slices);
    assert_eq!(text, "hi from the hand\n");
    println!("write/read: ok");

    // bash: environment + toolchain present.
    let b = c
        .start(start_request(
            "s-bash",
            "bash",
            json!({"command": "python3 --version; node --version; git --version; echo cwd=$(pwd)"}),
            root_lane(),
            None,
            false,
            30_000,
            65536,
        ))
        .await?;
    println!("bash toolchain:\n{}", indent(&decode(&b.slices)));
    assert_eq!(b.view.terminal.as_ref().unwrap().exit_code, Some(0));

    // detached job, poll, cancel.
    c.start(start_request(
        "s-job",
        "bash",
        json!({"command": "for i in $(seq 1 100); do echo tick $i; sleep 0.1; done"}),
        LaneRef {
            id: "L".parse().unwrap(),
            mode: LaneMode::Persistent,
            parent: None,
        },
        None,
        true,
        0,
        0,
    ))
    .await?;
    let p = c
        .poll(PollRequest {
            operation_id: "s-job".parse().unwrap(),
            cursors: vec![Cursor {
                stream: Stream::Stdout,
                offset: 0,
            }],
            max_bytes: 4096,
            wait_ms: 2000,
        })
        .await?;
    assert!(decode(&p.slices).starts_with("tick 1\n"));
    assert_eq!(p.view.status, OperationStatus::Running);
    let cr = c
        .cancel(CancelRequest {
            operation_id: "s-job".parse().unwrap(),
            grace_ms: Some(500),
        })
        .await?;
    assert!(cr.accepted);
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("detached job: polled and cancelled");

    println!("SMOKE PASS");
    Ok(())
}

fn hello_ws() -> String {
    std::env::var("HAND_WORKSPACE").unwrap_or_else(|_| "/workspace".into())
}
fn decode(slices: &[brain_protocol::abi::OutputSlice]) -> String {
    let mut v = Vec::new();
    for s in slices.iter().filter(|s| s.stream == Stream::Stdout) {
        v.extend(
            base64::engine::general_purpose::STANDARD
                .decode(s.data_base64.as_bytes())
                .unwrap(),
        );
    }
    String::from_utf8_lossy(&v).into_owned()
}
fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
