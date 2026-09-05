#![cfg(unix)]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use brain_loophost::{HostCall, LoopLimits, NativePolicy, NativeToolInput, TurnBridge, WorkerPool};
use brain_protocol::{RuntimeEnvelope, TurnError, TurnInput};

/// A bridge that answers every model call with a fixed assistant message and records
/// what the guest asked for.
struct RecordingBridge {
    calls: Mutex<Vec<String>>,
    cancelled: AtomicBool,
}

#[async_trait]
impl TurnBridge for RecordingBridge {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::Events { after } => {
                Ok(serde_json::json!({"events": [], "next_cursor": after}).to_string())
            }
            HostCall::Model { request_json } => {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("model {request_json}"));
                Ok(serde_json::json!({
                    "message": {"role": "assistant", "content": [{"type": "text", "text": "ok"}]},
                    "stop_reason": "end_turn",
                    "usage": {}
                })
                .to_string())
            }
            HostCall::Dispatch { calls_json } => {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("dispatch {calls_json}"));
                Ok("[]".into())
            }
            HostCall::Emit { kind, payload_json } => {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("emit {kind} {payload_json}"));
                Ok("7".into())
            }
            HostCall::Telemetry { record_json } => {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("telemetry {record_json}"));
                Ok(String::new())
            }
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

fn input(message: &str) -> TurnInput {
    TurnInput {
        input: message.into(),
        transcript: Vec::new(),
        slots: Default::default(),
        events: Vec::new(),
        configuration: serde_json::json!({}),
        system: String::new(),
        tools: Vec::new(),
        runtime: RuntimeEnvelope::at(&brain_protocol::SessionId::new("ses_test"), 1),
    }
}

fn package_path() -> String {
    std::env::var("BRAIN_TEST_AGENTLOOP_PACKAGE")
        .expect("BRAIN_TEST_AGENTLOOP_PACKAGE must name the built diagnostic package")
}

fn tool_path() -> String {
    std::env::var("BRAIN_TEST_TOOL_COMPONENT")
        .expect("BRAIN_TEST_TOOL_COMPONENT must name the built diagnostic Tool")
}

#[tokio::test]
async fn reference_loop_reads_interruptions_and_hands_tool_failures_to_the_model() {
    struct Model {
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl TurnBridge for Model {
        async fn call(&self, call: HostCall) -> Result<String, TurnError> {
            let answer = match call {
                HostCall::Events { after: 0 } => {
                    serde_json::json!({"events": [{"event_id": "ses_ref:3", "sequence": 3, "recorded_at_ms": 1, "event_type": "turn_failed", "data": {"code": "interrupted"}}], "next_cursor": 3})
                }
                HostCall::Events { after } => {
                    serde_json::json!({"events": [], "next_cursor": after})
                }
                HostCall::Model { request_json } => {
                    let request: brain_protocol::ModelRequest =
                        serde_json::from_str(&request_json).unwrap();
                    assert!(
                        serde_json::to_string(&request.messages)
                            .unwrap()
                            .contains("interrupted")
                    );
                    let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
                    if !first {
                        assert!(
                            serde_json::to_string(&request.messages)
                                .unwrap()
                                .contains("\"is_error\":true")
                        );
                    }
                    serde_json::json!({"message": {"role": "assistant", "content": if first { serde_json::json!([{"type": "tool_use", "id": "call_one", "name": "echo", "input": {}}]) } else { serde_json::json!([{"type": "text", "text": "Environment is unavailable"}]) }}, "stop_reason": if first { "tool_use" } else { "end_turn" }, "usage": {}})
                }
                HostCall::Dispatch { .. } => {
                    serde_json::json!([{"call_id": "call_one", "output": {"code": "expired", "message": "Environment expired"}, "is_error": true}])
                }
                _ => {
                    return Err(TurnError::new(
                        "unexpected",
                        "unexpected reference host call",
                    ));
                }
            };
            Ok(answer.to_string())
        }
        fn cancelled(&self) -> bool {
            false
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    );
    let path = std::env::var("BRAIN_TEST_REFERENCE_AGENTLOOP")
        .expect("BRAIN_TEST_REFERENCE_AGENTLOOP must name the reference Component");
    let digest = pool
        .admit(tokio::fs::read(path).await.unwrap())
        .await
        .unwrap();
    let model = Model {
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let output = pool
        .turn(
            "ses_ref".into(),
            digest,
            environment(),
            input("continue"),
            &model,
        )
        .await
        .unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(output.slots["observed_sequence"], 3);
    assert_eq!(output.transcript.len(), 5);
}

fn environment() -> serde_json::Value {
    serde_json::json!({
        "driver": "brain_wasm",
        "network": {"allow": []},
        "filesystem": {"workspace": false},
        "secrets": []
    })
}

#[tokio::test]
async fn saturated_parent_turns_can_all_invoke_native_tools() {
    struct Nested {
        pool: Arc<WorkerPool>,
        tool: brain_protocol::ToolIdentity,
        parents: tokio::sync::Barrier,
    }
    #[async_trait]
    impl TurnBridge for Nested {
        async fn call(&self, call: HostCall) -> Result<String, TurnError> {
            match call {
                HostCall::Events { after } => {
                    Ok(serde_json::json!({"events": [], "next_cursor": after}).to_string())
                }
                HostCall::Emit { .. } => {
                    self.parents.wait().await;
                    self.pool
                        .ready()
                        .await
                        .map_err(|error| TurnError::new("readiness", error.to_string()))?;
                    let answer = self
                        .pool
                        .tool(
                            "ses_nested".into(),
                            self.tool.clone(),
                            environment(),
                            NativeToolInput {
                                call_id: "nested".into(),
                                input: serde_json::json!({"nested": true}),
                                configuration: serde_json::json!({}),
                                deadline_at_ms: u64::MAX,
                            },
                            &RecordingBridge {
                                calls: Mutex::new(Vec::new()),
                                cancelled: AtomicBool::new(false),
                            },
                        )
                        .await
                        .map_err(|error| TurnError::new("nested_failed", error.to_string()))?;
                    assert_eq!(answer["echo"]["nested"], true);
                    Ok("7".into())
                }
                _ => Err(TurnError::new(
                    "unexpected_host_call",
                    "diagnostic turn only emits",
                )),
            }
        }
        fn cancelled(&self) -> bool {
            false
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let limits = LoopLimits::default();
    let count = limits.concurrent_turns_per_worker;
    let pool = Arc::new(WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        limits,
    ));
    let agentloop = pool
        .admit(tokio::fs::read(package_path()).await.unwrap())
        .await
        .unwrap();
    let tool = pool
        .admit_tool(tokio::fs::read(tool_path()).await.unwrap())
        .await
        .unwrap();
    let bridge = Arc::new(Nested {
        pool: pool.clone(),
        tool,
        parents: tokio::sync::Barrier::new(count),
    });
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..count {
        let (pool, agentloop, bridge) = (pool.clone(), agentloop.clone(), bridge.clone());
        tasks.spawn(async move {
            pool.turn(
                format!("ses_parent_{index}"),
                agentloop,
                environment(),
                input("nested"),
                &*bridge,
            )
            .await
        });
    }
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Some(result) = tasks.join_next().await {
            result.unwrap().unwrap();
        }
    })
    .await
    .expect("nested tools must not wait behind their parents");
}

fn workspace_environment() -> serde_json::Value {
    serde_json::json!({
        "driver": "brain_wasm",
        "network": {"allow": []},
        "filesystem": {"workspace": true},
        "secrets": []
    })
}

#[tokio::test]
async fn real_worker_admits_and_runs_a_turn_of_the_diagnostic_loop() {
    let package = tokio::fs::read(package_path()).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    );
    let digest = pool.admit(package).await.unwrap();
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        cancelled: AtomicBool::new(false),
    };
    let output = pool
        .turn(
            "ses_worker_test".into(),
            digest,
            environment(),
            input("hello"),
            &bridge,
        )
        .await
        .unwrap();
    assert_eq!(output.slots["memory"]["turns"], 1);
    assert_eq!(
        output.result,
        Some(serde_json::json!({"turns": 1, "message": "hello"}))
    );
    // The diagnostic loop emits one note through the host before it finishes.
    let calls = bridge.calls.lock().unwrap();
    assert!(
        calls.iter().any(|call| call.starts_with("emit note")),
        "the guest's host call must reach the bridge: {calls:?}"
    );
}

#[tokio::test]
async fn tool_workspaces_are_shared_within_a_session_and_isolated_between_sessions() {
    let component = tokio::fs::read(tool_path()).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    )
    .with_native_policy(NativePolicy {
        filesystem: std::collections::HashSet::from(["workspace".into()]),
        ..NativePolicy::default()
    });
    let digest = pool.admit_tool(component).await.unwrap();
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        cancelled: AtomicBool::new(false),
    };
    let invoke = |call_id: &str, input: serde_json::Value| NativeToolInput {
        call_id: call_id.into(),
        input,
        configuration: serde_json::json!({}),
        deadline_at_ms: 1_000,
    };

    let written = pool
        .tool(
            "ses_a".into(),
            digest.clone(),
            workspace_environment(),
            invoke(
                "write",
                serde_json::json!({"workspace": true, "write": "private"}),
            ),
            &bridge,
        )
        .await
        .unwrap();
    assert_eq!(written["marker"], "private");

    let same_session = pool
        .tool(
            "ses_a".into(),
            digest.clone(),
            workspace_environment(),
            invoke("read_a", serde_json::json!({"workspace": true})),
            &bridge,
        )
        .await
        .unwrap();
    assert_eq!(same_session["marker"], "private");

    let other_session = pool
        .tool(
            "ses_b".into(),
            digest,
            workspace_environment(),
            invoke("read_b", serde_json::json!({"workspace": true})),
            &bridge,
        )
        .await
        .unwrap();
    assert_eq!(other_session["marker"], serde_json::Value::Null);
}

/// Turns from many sessions run at once; every one of them completes.
#[tokio::test]
async fn concurrent_turns_all_reach_the_agentloop() {
    const AT_ONCE: usize = 8;

    let package = tokio::fs::read(package_path()).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = Arc::new(WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    ));
    let digest = pool.admit(package).await.unwrap();

    let mut turns = Vec::with_capacity(AT_ONCE);
    for index in 0..AT_ONCE {
        let pool = pool.clone();
        let digest = digest.clone();
        turns.push(tokio::spawn(async move {
            let bridge = RecordingBridge {
                calls: Mutex::new(Vec::new()),
                cancelled: AtomicBool::new(false),
            };
            pool.turn(
                format!("ses_{index}"),
                digest,
                environment(),
                input(&format!("turn {index}")),
                &bridge,
            )
            .await
        }));
    }

    let mut reached = 0;
    let mut refused = Vec::new();
    for turn in turns {
        match turn.await.unwrap() {
            Ok(output) => {
                assert_eq!(output.slots["memory"]["turns"], 1);
                reached += 1;
            }
            Err(error) => refused.push(error.to_string()),
        }
    }
    assert_eq!(
        reached, AT_ONCE,
        "only {reached} of {AT_ONCE} concurrent turns reached the Agentloop; the rest were refused: {refused:?}"
    );
}

/// A cancel reaches the guest through its next host call: the turn ends with the
/// cancellation instead of finishing.
#[tokio::test]
async fn a_cancelled_turn_ends_at_its_next_host_call() {
    let package = tokio::fs::read(package_path()).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    );
    let digest = pool.admit(package).await.unwrap();
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        cancelled: AtomicBool::new(true),
    };
    let error = pool
        .turn(
            "ses_cancel".into(),
            digest,
            environment(),
            input("hello"),
            &bridge,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("cancelled"), "{error}");
}

#[tokio::test]
async fn host_calls_queued_before_cancel_are_not_answered_after_cancel() {
    use brain_loophost::{NativeEnvironment, WorkerClient, WorkerRequest, WorkerResponse};

    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("worker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let worker = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(matches!(
            brain_loophost::worker_read(&mut stream).await.unwrap(),
            WorkerRequest::Turn { .. }
        ));
        assert!(matches!(
            brain_loophost::worker_read(&mut stream).await.unwrap(),
            WorkerRequest::Cancel
        ));
        brain_loophost::worker_write(
            &mut stream,
            &WorkerResponse::HostCall {
                id: 1,
                call: HostCall::Emit {
                    kind: "note".into(),
                    payload_json: "{}".into(),
                },
            },
        )
        .await
        .unwrap();
        brain_loophost::worker_write(
            &mut stream,
            &WorkerResponse::TurnFailed {
                error: TurnError::new(brain_protocol::codes::failure::CANCELLED, "cancelled"),
            },
        )
        .await
        .unwrap();
        assert!(brain_loophost::worker_read(&mut stream).await.is_err());
    });
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        cancelled: AtomicBool::new(true),
    };
    let error = WorkerClient::new(socket)
        .turn(
            brain_protocol::AgentloopIdentity::new("diagnostic"),
            NativeEnvironment {
                scratch: false,
                workspace: None,
                network_allow: Vec::new(),
                secrets: Default::default(),
            },
            input("hello"),
            brain_loophost::MAX_TURN_INPUT_BYTES,
            &bridge,
            std::time::Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"), "{error}");
    assert!(bridge.calls.lock().unwrap().is_empty());
    worker.await.unwrap();
}
