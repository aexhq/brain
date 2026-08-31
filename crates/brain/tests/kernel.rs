use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use brain::{
    Kernel, KernelConfig, KernelError, LoopExecutor, ModelExecutor, SessionHandle, ToolExecutor,
};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, AttachmentId, Decision,
    EnvironmentAttachment, EnvironmentBinding, EnvironmentId, EnvironmentRequest,
    EnvironmentRequirement, Identity, LifecyclePolicy, MessageRequest, ModelBinding,
    ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, Observation, OperationId,
    RequestedToolBinding, ResolvedSessionRequest, SealedSessionConfig, ToolBinding,
    ToolCancellation, ToolDefinition, ToolDispatch, ToolInvocation, ToolResult,
};
use brain_telemetry::telemetry_channel;
use tokio::sync::Notify;

struct ScriptedLoop {
    calls: AtomicUsize,
}

#[async_trait]
impl LoopExecutor for ScriptedLoop {
    async fn activate(
        &self,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        let decision = if call == 0 {
            assert!(matches!(input.observation, Observation::UserMessage { .. }));
            Decision::Model {
                request: ModelRequest {
                    messages: vec![brain_protocol::Message::user_text("hello")],
                    response_format: None,
                    max_output_tokens: Some(16),
                },
            }
        } else {
            assert!(matches!(
                input.observation,
                Observation::ModelCompleted { .. }
            ));
            Decision::Finish {
                result: Some(serde_json::json!({"ok":true})),
            }
        };
        Ok(ActivationOutput {
            context: input.context,
            decision,
        })
    }
}

struct ScriptedModel;

#[async_trait]
impl ModelExecutor for ScriptedModel {
    async fn execute(
        &self,
        _operation_id: &OperationId,
        _request_digest: &Identity,
        _binding: &ModelBinding,
        _presentation: &ModelPresentation,
        _request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        on_event(ModelStreamEvent::TextDelta {
            index: 0,
            text: "hello".into(),
        });
        Ok(ModelResult {
            message: brain_protocol::Message::assistant(vec![brain_protocol::ContentBlock::text(
                "hello",
            )]),
            stop_reason: brain_protocol::StopReason::EndTurn,
            usage: brain_protocol::Usage::default(),
        })
    }
}

struct NoTools;

#[async_trait]
impl ToolExecutor for NoTools {
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<ToolResult, KernelError> {
        panic!("unexpected Tool dispatch")
    }

    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), KernelError> {
        panic!("unexpected Tool cancellation")
    }
}

struct SlowModel {
    started: Arc<Notify>,
}

struct ToolLoop;

#[async_trait]
impl LoopExecutor for ToolLoop {
    async fn activate(
        &self,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        Ok(ActivationOutput {
            context: input.context,
            decision: Decision::Tools {
                calls: vec![ToolInvocation {
                    call_id: "call-1".into(),
                    name: "slow".into(),
                    input: serde_json::json!({}),
                }],
            },
        })
    }
}

struct NoModels;

#[async_trait]
impl ModelExecutor for NoModels {
    async fn execute(
        &self,
        _operation_id: &OperationId,
        _request_digest: &Identity,
        _binding: &ModelBinding,
        _presentation: &ModelPresentation,
        _request: ModelRequest,
        _on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        panic!("unexpected model request")
    }
}

struct SlowTools {
    started: Arc<Notify>,
    cancelled: Arc<AtomicBool>,
}

#[async_trait]
impl ToolExecutor for SlowTools {
    async fn execute(&self, dispatch: ToolDispatch) -> Result<ToolResult, KernelError> {
        let request = EnvironmentRequest::Execute {
            tool: dispatch.invocation.clone(),
            remote_tool_id: dispatch.binding.remote_tool_id.clone(),
            tool_configuration: dispatch.binding.tool_configuration.clone(),
            grant: dispatch.binding.grant.clone(),
        };
        assert_eq!(dispatch.request_identity, Identity::of(&request).unwrap());
        self.started.notify_one();
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        unreachable!("cancel must drop the in-flight Tool request")
    }

    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), KernelError> {
        assert_ne!(cancellation.target_operation_id, cancellation.operation_id);
        assert!(cancellation.target_operation_id.as_str().starts_with("op_"));
        self.cancelled.store(true, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl ModelExecutor for SlowModel {
    async fn execute(
        &self,
        _operation_id: &OperationId,
        _request_digest: &Identity,
        _binding: &ModelBinding,
        _presentation: &ModelPresentation,
        _request: ModelRequest,
        _on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        self.started.notify_one();
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        unreachable!("cancel must drop the in-flight model request")
    }
}

#[tokio::test]
async fn intent_precedes_model_effect() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let config = || KernelConfig {
        data_dir: data_dir.clone(),
        max_decisions_per_turn: 8,
        loop_executor: Arc::new(ScriptedLoop {
            calls: AtomicUsize::new(0),
        }),
        model_executor: Arc::new(ScriptedModel),
        tool_executor: Arc::new(NoTools),
    };
    let kernel = Kernel::open(config(), publisher.clone()).unwrap();
    let handle = start(&kernel, request());
    let session_id = handle.id().clone();
    let finished = handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        finished.status,
        brain_protocol::SessionStatus::Idle
    ));
    let events = kernel.events(&session_id, 0, 100).unwrap();
    let kinds: Vec<&str> = events
        .events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    let intent = kinds
        .iter()
        .position(|kind| *kind == "model_intent")
        .unwrap();
    let result = kinds
        .iter()
        .position(|kind| *kind == "model_result")
        .unwrap();
    assert!(intent < result);
    assert!(
        events
            .events
            .windows(2)
            .all(|pair| pair[0].recorded_at_ms <= pair[1].recorded_at_ms)
    );
    let recorded_at_ms: Vec<_> = events
        .events
        .iter()
        .map(|event| event.recorded_at_ms)
        .collect();
    assert_eq!(
        publisher.metrics().queued_records(),
        events.events.len(),
        "every committed event enters the bounded telemetry queue"
    );
    let _ = recorded_at_ms;
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn cancel_interrupts_an_inflight_model_request() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let started = Arc::new(Notify::new());
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ScriptedLoop {
                calls: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(SlowModel {
                started: started.clone(),
            }),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = start(&kernel, request());
    let session_id = handle.id().clone();
    let running = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .message(MessageRequest {
                    input: "hello".into(),
                })
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .unwrap();
    handle.cancel().await.unwrap();
    let session = tokio::time::timeout(std::time::Duration::from_secs(1), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        session.status,
        brain_protocol::SessionStatus::Idle
    ));
    let events = kernel.events(&session_id, 0, 100).unwrap().events;
    assert_eq!(events.last().unwrap().event_type, "turn_failed");
    assert_eq!(events.last().unwrap().data["code"], "cancelled");
    assert!(
        !events
            .iter()
            .any(|event| event.event_type == "model_result")
    );
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn cancel_forwards_inflight_tool_cancellation_to_the_environment_port() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ToolLoop),
            model_executor: Arc::new(NoModels),
            tool_executor: Arc::new(SlowTools {
                started: started.clone(),
                cancelled: cancelled.clone(),
            }),
        },
        publisher,
    )
    .unwrap();
    let handle = start(&kernel, tool_request());
    let session_id = handle.id().clone();
    let running = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .message(MessageRequest {
                    input: "run".into(),
                })
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .unwrap();
    handle.cancel().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(cancelled.load(Ordering::Acquire));
    let kinds: Vec<_> = kernel
        .events(&session_id, 0, 100)
        .unwrap()
        .events
        .into_iter()
        .map(|event| event.event_type)
        .collect();
    assert!(kinds.iter().any(|kind| kind == "tool_cancel_intent"));
    assert!(kinds.iter().any(|kind| kind == "tool_cancel_result"));
    assert_eq!(kinds.last().unwrap(), "turn_failed");
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

fn request() -> SealedSessionConfig {
    SealedSessionConfig {
        agentloop_identity: AgentloopIdentity::new("a".repeat(64)),
        brain_configuration: serde_json::json!({}),
        model: ModelBinding {
            binding_id: "gateway".into(),
            model: "openai/test".into(),
        },
        presentation: ModelPresentation {
            system: "test".into(),
            tools: Vec::new(),
            response_format: None,
        },
        environments: Vec::new(),
        tool_bindings: Vec::new(),
    }
}

fn tool_request() -> SealedSessionConfig {
    let environment = EnvironmentBinding {
        environment_id: EnvironmentId::new("workspace"),
        configuration_identity: Identity::of(&"configuration").unwrap(),
        adapter_binding: "sealed".into(),
        directory_generation: 1,
        lifecycle_policy: LifecyclePolicy::Shared,
    };
    SealedSessionConfig {
        agentloop_identity: AgentloopIdentity::new("a".repeat(64)),
        brain_configuration: serde_json::json!({}),
        model: ModelBinding {
            binding_id: "gateway".into(),
            model: "openai/test".into(),
        },
        presentation: ModelPresentation {
            system: "test".into(),
            tools: vec![ToolDefinition {
                name: "slow".into(),
                description: "wait".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: None,
            }],
            response_format: None,
        },
        environments: vec![EnvironmentAttachment {
            binding: environment.clone(),
            attachment_id: AttachmentId::new("attachment"),
        }],
        tool_bindings: vec![ToolBinding {
            name: "slow".into(),
            environment,
            attachment_id: AttachmentId::new("attachment"),
            remote_tool_id: "slow".into(),
            tool_configuration: serde_json::json!({}),
            grant: serde_json::json!({}),
        }],
    }
}

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("brain-kernel-test-{}", rand::random::<u64>()));
    fs::create_dir(&path).unwrap();
    path
}

/// Sessions are created the way the server creates them: a resolved request is admitted,
/// then the sealed configuration completes it. There is no shortcut past `begin_session`,
/// so these tests exercise the same validation the production path enforces.
fn start(kernel: &Kernel, sealed: SealedSessionConfig) -> SessionHandle {
    let resolved = resolved_from(&sealed);
    kernel
        .begin_session(&resolved)
        .unwrap()
        .complete(sealed)
        .unwrap()
}

fn resolved_from(sealed: &SealedSessionConfig) -> ResolvedSessionRequest {
    ResolvedSessionRequest {
        history: Vec::new(),
        agentloop_identity: sealed.agentloop_identity.clone(),
        brain_configuration: sealed.brain_configuration.clone(),
        model: sealed.model.clone(),
        presentation: sealed.presentation.clone(),
        environments: sealed
            .environments
            .iter()
            .map(|environment| EnvironmentRequirement {
                environment_id: environment.binding.environment_id.clone(),
                configuration: serde_json::json!({}),
                lifecycle_policy: environment.binding.lifecycle_policy.clone(),
            })
            .collect(),
        tool_bindings: sealed
            .tool_bindings
            .iter()
            .map(|binding| RequestedToolBinding {
                name: binding.name.clone(),
                environment_id: binding.environment.environment_id.clone(),
                remote_tool_id: binding.remote_tool_id.clone(),
                tool_configuration: binding.tool_configuration.clone(),
                grant: binding.grant.clone(),
            })
            .collect(),
    }
}

/// A client watching a session must see the model's output while the turn is running.
///
/// The actor received deltas from the model and pushed them into a local buffer, and
/// nothing forwarded them: the whole assistant message appeared at once, when the turn was
/// already over, so there was no first token to wait for. The deltas are not journalled --
/// recording one would be a durable write per token -- so a subscription is the only place
/// they appear.
#[tokio::test]
async fn a_subscriber_sees_model_output_while_the_turn_is_running() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ScriptedLoop {
                calls: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    // Subscribed before the turn, the way a client opens a stream and then sends.
    let mut live = kernel.subscribe();
    let handle = start(&kernel, request());
    handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();

    let mut streamed = Vec::new();
    let mut recorded_after_the_first_delta = false;
    let mut seen_delta = false;
    while let Ok((_, event)) = live.try_recv() {
        match event {
            brain_protocol::LiveEvent::Streaming(streaming) => {
                seen_delta = true;
                streamed.push(streaming);
            }
            brain_protocol::LiveEvent::Recorded(record) => {
                if seen_delta && record.event_type == "model_result" {
                    recorded_after_the_first_delta = true;
                }
            }
        }
    }
    drop(handle);
    drop(kernel);
    // Best-effort: the writer thread may still hold a segment open, and the assertions
    // below are what this test is for.
    let _ = std::fs::remove_dir_all(data_dir);

    let text = streamed
        .iter()
        .find(|streaming| streaming.event_type == "assistant_delta")
        .map(|streaming| streaming.data.clone());
    assert_eq!(
        text,
        Some(serde_json::json!({ "index": 0, "text": "hello" })),
        "the model's output never reached a subscriber: {streamed:?}"
    );
    assert!(
        recorded_after_the_first_delta,
        "the delta did not arrive before the record that holds the finished message, so it \
         is not telling a watcher anything the journal was not already going to"
    );
}

/// Records an agentloop that was told what the session is continuing.
struct RecordsHistory {
    seen: Arc<std::sync::Mutex<Option<Vec<serde_json::Value>>>>,
}

#[async_trait]
impl LoopExecutor for RecordsHistory {
    async fn activate(
        &self,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let mut context = input.context;
        if let Observation::SessionStarted { history } = &input.observation {
            *self.seen.lock().unwrap() = Some(history.clone());
            // What a real loop does with history: turn it into its own context. The shape
            // is the loop's business, which is exactly why Brain hands over the events
            // rather than a context it built itself.
            for event in history {
                context.items.push(event.clone());
            }
            return Ok(ActivationOutput {
                context,
                decision: Decision::Finish { result: None },
            });
        }
        Ok(ActivationOutput {
            context,
            decision: Decision::Finish {
                result: Some(serde_json::json!({"ok": true})),
            },
        })
    }
}

fn history_of(count: u64) -> Vec<brain_protocol::HistoryEvent> {
    (1..=count)
        .map(|sequence| brain_protocol::HistoryEvent {
            sequence,
            recorded_at_ms: Some(1_787_846_400_000 + sequence),
            event_type: "output_emitted".to_owned(),
            data: serde_json::json!({"content": format!("earlier {sequence}")}),
        })
        .collect()
}

fn start_with_history(
    kernel: &Kernel,
    sealed: SealedSessionConfig,
    history: Vec<brain_protocol::HistoryEvent>,
) -> Result<SessionHandle, KernelError> {
    let mut resolved = resolved_from(&sealed);
    resolved.history = history;
    kernel.begin_session(&resolved)?.complete(sealed)
}

/// A session created with history has that history in its journal, and the agentloop is
/// told about it before anything is asked of it.
///
/// This is what replaces surviving a restart: the process that held the session is gone,
/// the application kept the events it was already receiving, and it hands them back.
#[tokio::test]
async fn a_session_can_be_created_with_prior_history() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let seen = Arc::new(std::sync::Mutex::new(None));
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(RecordsHistory {
                seen: Arc::clone(&seen),
            }),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();

    let handle = start_with_history(&kernel, request(), history_of(3)).unwrap();
    let session_id = handle.id().clone();
    // The actor announces history on its own task, so give it the moment it needs.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let told = seen.lock().unwrap().clone();
    let told = told.expect("the agentloop must be told the session is continuing one");
    assert_eq!(told.len(), 3, "every event handed back must reach the loop");
    assert_eq!(told[0]["content"], "earlier 1");

    // And the journal holds them, so reading the session back reads the whole conversation.
    let events = kernel.events(&session_id, 0, 100).unwrap().events;
    let replayed: Vec<&str> = events
        .iter()
        .filter(|event| event.event_type == "output_emitted")
        .filter_map(|event| event.data["content"].as_str())
        .collect();
    assert_eq!(replayed, vec!["earlier 1", "earlier 2", "earlier 3"]);

    // Sequences are this session's own, dense from its first record: the numbers the
    // caller supplied name positions in a session that no longer exists.
    let sequences: Vec<u64> = events.iter().map(|event| event.sequence).collect();
    let dense: Vec<u64> = (1..=sequences.len() as u64).collect();
    assert_eq!(
        sequences, dense,
        "records must be numbered densely from one, whatever the caller's numbering was"
    );

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// History that skips or repeats a position is a conversation with a piece missing or
/// moved, and writing it down as though it were whole would make the journal a worse
/// record than no journal.
#[tokio::test]
async fn history_out_of_order_is_refused() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ScriptedLoop {
                calls: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();

    let mut history = history_of(3);
    history[2].sequence = 2;
    let error = match start_with_history(&kernel, request(), history) {
        Ok(_) => panic!("a conversation with a repeated position must not be accepted"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("out of order"),
        "the refusal must say what is wrong with it: {error}"
    );

    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// What Brain leaves on disk, asserted rather than assumed.
///
/// The journal, and nothing else. It used to be the journal plus a state file per session
/// plus a SQLite database of encrypted credentials plus the key that opened it — all of it
/// there so a session could outlive its process, which sessions no longer do. A file
/// appearing here again should be a decision someone made, not something noticed later.
#[tokio::test]
async fn the_journal_is_the_only_thing_written() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(OrdinaryTurn),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = start(&kernel, request());
    for _ in 0..10 {
        handle
            .message(MessageRequest {
                input: "hello".into(),
            })
            .await
            .unwrap();
    }
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let mut found = Vec::new();
    fn walk(dir: &std::path::Path, prefix: &str, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.metadata().is_ok_and(|meta| meta.is_dir()) {
                walk(&entry.path(), &format!("{name}/"), found);
            } else {
                found.push(name);
            }
        }
    }
    walk(&data_dir, "", &mut found);
    found.sort();
    let _ = fs::remove_dir_all(&data_dir);

    assert!(
        found.iter().all(|name| name.ends_with(".journal")),
        "the journal is the only thing Brain writes; found {found:?}"
    );
    assert!(
        !found.is_empty(),
        "ten turns must have written a journal segment"
    );
}

/// A turn shaped like a real one: ask the model, emit what it said, finish.
struct OrdinaryTurn;

#[async_trait]
impl LoopExecutor for OrdinaryTurn {
    async fn activate(
        &self,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let mut context = input.context;
        let decision = match input.observation {
            Observation::UserMessage { input } => {
                context.items.push(serde_json::to_value(&input).unwrap());
                Decision::Model {
                    request: ModelRequest {
                        messages: vec![brain_protocol::Message::user_text("hello")],
                        response_format: None,
                        max_output_tokens: Some(16),
                    },
                }
            }
            Observation::ModelCompleted { response } => {
                context.items.push(response);
                Decision::Emit {
                    event: serde_json::json!({"type": "assistant_message", "content": "ok"}),
                }
            }
            _ => Decision::Finish {
                result: Some(serde_json::json!({"ok": true})),
            },
        };
        Ok(ActivationOutput { context, decision })
    }
}

/// A caller's history cannot wear a lifecycle record's name.
///
/// A restart derives a session's status by reading these kinds back out of the journal, so
/// a client able to supply `session_ended` as history could describe a session that never
/// happened and have a later restart believe it.
#[tokio::test]
async fn history_cannot_forge_a_lifecycle_record() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ScriptedLoop {
                calls: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();

    for forged in ["session_ended", "turn_started", "session_created"] {
        let history = vec![brain_protocol::HistoryEvent {
            sequence: 1,
            recorded_at_ms: None,
            event_type: forged.to_owned(),
            data: serde_json::json!({}),
        }];
        let error = match start_with_history(&kernel, request(), history) {
            Ok(_) => panic!("history must not be allowed to carry {forged}"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(forged),
            "the refusal must name the record it refused: {error}"
        );
    }

    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// A restored session whose server metadata was lost is readable, and says why it cannot
/// continue.
///
/// The `journal_id` is the server's record, not the session's, and writing it is best
/// effort like everything else. When it is gone the honest answer is the reason, not a
/// session that accepts messages and fails every turn without explaining itself.
#[tokio::test]
async fn a_session_without_its_metadata_is_readable_and_refuses_turns() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ScriptedLoop {
                calls: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = start(&kernel, request());
    let session_id = handle.id().clone();
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Reopened without ever being told the journal ids: exactly the case where the
    // metadata write did not survive.
    let (publisher, _worker) = telemetry_channel();
    let reopened = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            loop_executor: Arc::new(ScriptedLoop {
                calls: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(ScriptedModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();

    assert!(
        reopened.session(&session_id).is_ok(),
        "the session must still be listed and readable"
    );
    assert!(
        !reopened
            .events(&session_id, 0, 100)
            .unwrap()
            .events
            .is_empty(),
        "its history must still be readable"
    );
    let error = match reopened.handle(&session_id) {
        Ok(_) => panic!("a session with no journal id must not take another turn"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("metadata"),
        "the refusal must explain itself: {error}"
    );

    drop(reopened);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}
