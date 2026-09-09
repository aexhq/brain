use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use brain_env::{
    Access, EnvLimits, HostCall, NativeEnvironment, NativeToolInput, TurnBridge, WorkerPool,
    Workspace,
};
use brain_protocol::{RuntimeEnvelope, TurnError, TurnInput};

/// A bridge that answers every model call with a fixed assistant message and records
/// what the guest asked for.
struct RecordingBridge {
    calls: Mutex<Vec<String>>,
    kv: Mutex<std::collections::BTreeMap<String, serde_json::Value>>,
    cancelled: AtomicBool,
}

#[async_trait]
impl TurnBridge for RecordingBridge {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::KvPut { key, value_json } => {
                self.kv
                    .lock()
                    .unwrap()
                    .insert(key.clone(), serde_json::from_str(&value_json).unwrap());
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("kv {key} {value_json}"));
                Ok("7".into())
            }
            HostCall::KvRead { key } => Ok(match self.kv.lock().unwrap().get(&key) {
                Some(value) => serde_json::json!({"value": value}),
                None => serde_json::json!({}),
            }
            .to_string()),
            HostCall::KvDelete { key } => {
                self.kv.lock().unwrap().remove(&key);
                Ok("7".into())
            }
            HostCall::SetTranscript { messages_json } => {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("transcript {messages_json}"));
                Ok("7".into())
            }
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
        kv: Default::default(),
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
        kv: Mutex<serde_json::Value>,
        transcript: Mutex<Vec<brain_protocol::Message>>,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl TurnBridge for Model {
        async fn call(&self, call: HostCall) -> Result<String, TurnError> {
            let answer = match call {
                HostCall::KvPut { key, value_json } => {
                    self.kv.lock().unwrap()[key] = serde_json::from_str(&value_json).unwrap();
                    serde_json::json!(7)
                }
                HostCall::KvRead { key } => match self.kv.lock().unwrap().get(&key) {
                    Some(value) => serde_json::json!({"value": value}),
                    None => serde_json::json!({}),
                },
                HostCall::KvDelete { key } => {
                    self.kv
                        .lock()
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .remove(&key);
                    serde_json::json!(7)
                }
                HostCall::SetTranscript { messages_json } => {
                    *self.transcript.lock().unwrap() =
                        serde_json::from_str(&messages_json).unwrap();
                    serde_json::json!(7)
                }
                HostCall::Events { after: 0 } => {
                    serde_json::json!({"events": [{"sequence": 3, "recorded_at_ms": 1, "event_type": "turn_failed", "data": {"code": "interrupted"}}], "next_cursor": 3})
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
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        EnvLimits::default(),
        std::num::NonZeroUsize::new(1).unwrap(),
    );
    let path = std::env::var("BRAIN_TEST_REFERENCE_AGENTLOOP")
        .expect("BRAIN_TEST_REFERENCE_AGENTLOOP must name the reference Component");
    let digest = pool
        .admit(tokio::fs::read(path).await.unwrap())
        .await
        .unwrap();
    let model = Model {
        kv: Mutex::new(serde_json::json!({})),
        transcript: Mutex::new(Vec::new()),
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let _output = pool
        .turn(
            digest,
            environment(),
            TurnInput {
                tools: vec![brain_protocol::ActivationTool {
                    definition: brain_protocol::ToolDefinition {
                        name: "echo".into(),
                        description: "Echo".into(),
                        input_schema: serde_json::json!({"type":"object"}),
                        output_schema: None,
                    },
                    environments: vec![brain_protocol::EnvironmentName::new("remote")],
                }],
                ..input("continue")
            },
            &model,
        )
        .await
        .unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(model.kv.lock().unwrap()["observed_sequence"], 3);
    assert_eq!(model.transcript.lock().unwrap().len(), 5);
}

fn environment() -> NativeEnvironment {
    NativeEnvironment::default()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_worker_crash_does_not_replay_or_stop_its_sibling_and_shutdown_reaps_both() {
    struct Held {
        entered: tokio::sync::Barrier,
        release: tokio::sync::Notify,
    }
    #[async_trait]
    impl TurnBridge for Held {
        async fn call(&self, call: HostCall) -> Result<String, TurnError> {
            match call {
                HostCall::KvPut { .. } => Ok("7".into()),
                HostCall::KvRead { .. } => Ok("{}".into()),
                HostCall::Events { after } => {
                    Ok(serde_json::json!({"events": [], "next_cursor": after}).to_string())
                }
                HostCall::Emit { .. } => {
                    let released = self.release.notified();
                    self.entered.wait().await;
                    released.await;
                    Ok("7".into())
                }
                _ => unreachable!(),
            }
        }
        fn cancelled(&self) -> bool {
            false
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let pool = Arc::new(WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        EnvLimits::default(),
        std::num::NonZeroUsize::new(2).unwrap(),
    ));
    pool.start().await.unwrap();
    let root = directory.path();
    let pid = |index| async move {
        tokio::net::UnixStream::connect(root.join(format!("run/{index}/brain-env-worker.sock")))
            .await
            .unwrap()
            .peer_cred()
            .unwrap()
            .pid()
            .unwrap()
    };
    let first = pid(0).await;
    let second = pid(1).await;
    assert_ne!(first, second);
    let digest = pool
        .admit(tokio::fs::read(package_path()).await.unwrap())
        .await
        .unwrap();
    let bridge = Arc::new(Held {
        entered: tokio::sync::Barrier::new(3),
        release: tokio::sync::Notify::new(),
    });
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..2 {
        let (pool, digest, bridge) = (pool.clone(), digest.clone(), bridge.clone());
        tasks.spawn(async move {
            pool.turn(digest, environment(), input("held"), &*bridge)
                .await
        });
    }
    tokio::time::timeout(std::time::Duration::from_secs(20), bridge.entered.wait())
        .await
        .unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-KILL", &first.to_string()])
            .status()
            .unwrap()
            .success()
    );
    bridge.release.notify_waiters();
    let mut succeeded = 0;
    let mut failed = 0;
    while let Some(result) = tasks.join_next().await {
        if result.unwrap().is_ok() {
            succeeded += 1;
        } else {
            failed += 1;
        }
    }
    assert_eq!((succeeded, failed), (1, 1));
    pool.ready().await.unwrap();
    assert_eq!(pid(1).await, second);
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        kv: Mutex::new(Default::default()),
        cancelled: AtomicBool::new(false),
    };
    for _ in 0..2 {
        pool.turn(digest.clone(), environment(), input("fresh"), &bridge)
            .await
            .unwrap();
    }
    let replacement = pid(0).await;
    assert_ne!(replacement, first);
    assert_eq!(pid(1).await, second);
    pool.shutdown().await;
    assert!(pool.ready().await.is_err());
    for process in [replacement, second] {
        assert!(!std::path::Path::new(&format!("/proc/{process}")).exists());
    }
}

#[tokio::test]
async fn saturated_parent_turns_can_all_invoke_native_tools() {
    struct Nested {
        pool: Arc<WorkerPool>,
        tool: brain_protocol::ToolId,
        parents: tokio::sync::Barrier,
    }
    #[async_trait]
    impl TurnBridge for Nested {
        fn can_dispatch(&self) -> bool {
            true
        }
        async fn call(&self, call: HostCall) -> Result<String, TurnError> {
            match call {
                HostCall::KvPut { .. } => Ok("7".into()),
                HostCall::KvRead { .. } => Ok("{}".into()),
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
                            self.tool.clone(),
                            environment(),
                            NativeToolInput {
                                input: serde_json::json!({"nested": true}),
                                configuration: serde_json::json!({}),
                                deadline_at_ms: u64::MAX,
                            },
                            &RecordingBridge {
                                calls: Mutex::new(Vec::new()),
                                kv: Mutex::new(Default::default()),
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
    let limits = EnvLimits::default();
    let count = limits.max_concurrent_executions * 2;
    let pool = Arc::new(WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        limits,
        std::num::NonZeroUsize::new(2).unwrap(),
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
            let _ = index;
            pool.turn(agentloop, environment(), input("nested"), &*bridge)
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

/// The grants a Tool needing `file:///workspace?access=write` receives: the session's
/// directory, writable.
fn workspace_environment(root: &std::path::Path, session: &str) -> NativeEnvironment {
    let path = root.join("native-workspaces").join(session);
    std::fs::create_dir_all(&path).unwrap();
    NativeEnvironment {
        workspace: Some(Workspace {
            path: path.to_string_lossy().into_owned(),
            access: Access::Write,
        }),
        ..NativeEnvironment::default()
    }
}

#[tokio::test]
async fn real_worker_admits_and_runs_a_turn_of_the_diagnostic_loop() {
    let package = tokio::fs::read(package_path()).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        EnvLimits::default(),
        std::num::NonZeroUsize::new(1).unwrap(),
    );
    let digest = pool.admit(package).await.unwrap();
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        kv: Mutex::new(Default::default()),
        cancelled: AtomicBool::new(false),
    };
    let output = pool
        .turn(digest, environment(), input("kv"), &bridge)
        .await
        .unwrap();
    assert!(
        bridge
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call == "kv memory {\"turns\":1}")
    );
    assert_eq!(
        output.result,
        Some(serde_json::json!({"turns": 1, "message": "kv"}))
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
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        EnvLimits::default(),
        std::num::NonZeroUsize::new(1).unwrap(),
    );
    let digest = pool.admit_tool(component).await.unwrap();
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        kv: Mutex::new(Default::default()),
        cancelled: AtomicBool::new(false),
    };
    let invoke = |input: serde_json::Value| NativeToolInput {
        input,
        configuration: serde_json::json!({}),
        deadline_at_ms: 1_000,
    };

    let written = pool
        .tool(
            digest.clone(),
            workspace_environment(directory.path(), "ses_a"),
            invoke(serde_json::json!({"workspace": true, "write": "private"})),
            &bridge,
        )
        .await
        .unwrap();
    assert_eq!(written["marker"], "private");

    let same_session = pool
        .tool(
            digest.clone(),
            workspace_environment(directory.path(), "ses_a"),
            invoke(serde_json::json!({"workspace": true})),
            &bridge,
        )
        .await
        .unwrap();
    assert_eq!(same_session["marker"], "private");

    let other_session = pool
        .tool(
            digest,
            workspace_environment(directory.path(), "ses_b"),
            invoke(serde_json::json!({"workspace": true})),
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
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        EnvLimits::default(),
        std::num::NonZeroUsize::new(1).unwrap(),
    ));
    let digest = pool.admit(package).await.unwrap();

    let mut turns = Vec::with_capacity(AT_ONCE);
    for index in 0..AT_ONCE {
        let pool = pool.clone();
        let digest = digest.clone();
        turns.push(tokio::spawn(async move {
            let bridge = RecordingBridge {
                calls: Mutex::new(Vec::new()),
                kv: Mutex::new(Default::default()),
                cancelled: AtomicBool::new(false),
            };
            let output = pool
                .turn(
                    digest,
                    environment(),
                    input(&format!("turn {index}")),
                    &bridge,
                )
                .await?;
            assert!(
                bridge
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|call| call == "kv memory {\"turns\":1}")
            );
            Ok(output)
        }));
    }

    let mut reached = 0;
    let mut refused = Vec::new();
    for turn in turns {
        match turn.await.unwrap() {
            Ok::<_, brain_env::LoopError>(_output) => {
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
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        EnvLimits::default(),
        std::num::NonZeroUsize::new(1).unwrap(),
    );
    let digest = pool.admit(package).await.unwrap();
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        kv: Mutex::new(Default::default()),
        cancelled: AtomicBool::new(true),
    };
    let error = pool
        .turn(digest, environment(), input("hello"), &bridge)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("cancelled"), "{error}");
}

#[tokio::test]
async fn host_calls_queued_before_cancel_are_not_answered_after_cancel() {
    use brain_env::{WorkerClient, WorkerRequest, WorkerResponse};

    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("worker.sock");
    let mut listener = brain_env::listen(&socket).unwrap();
    let worker = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        assert!(matches!(
            brain_env_worker::worker_read(&mut stream, &EnvLimits::default())
                .await
                .unwrap(),
            WorkerRequest::Execute { .. }
        ));
        assert!(matches!(
            brain_env_worker::worker_read(&mut stream, &EnvLimits::default())
                .await
                .unwrap(),
            WorkerRequest::Cancel
        ));
        brain_env_worker::worker_write(
            &mut stream,
            &WorkerResponse::HostCall {
                id: 1,
                call: HostCall::Emit {
                    kind: "note".into(),
                    payload_json: "{}".into(),
                },
            },
            &EnvLimits::default(),
        )
        .await
        .unwrap();
        brain_env_worker::worker_write(
            &mut stream,
            &WorkerResponse::TurnFailed {
                error: TurnError::new(brain_protocol::codes::failure::CANCELLED, "cancelled"),
            },
            &EnvLimits::default(),
        )
        .await
        .unwrap();
        assert!(
            brain_env_worker::worker_read(&mut stream, &EnvLimits::default())
                .await
                .is_err()
        );
    });
    let bridge = RecordingBridge {
        calls: Mutex::new(Vec::new()),
        kv: Mutex::new(Default::default()),
        cancelled: AtomicBool::new(true),
    };
    let error = WorkerClient::new(socket, &EnvLimits::default())
        .turn(
            brain_protocol::AgentloopId::new("diagnostic"),
            NativeEnvironment::default(),
            input("hello"),
            &bridge,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"), "{error}");
    assert!(bridge.calls.lock().unwrap().is_empty());
    worker.await.unwrap();
}
