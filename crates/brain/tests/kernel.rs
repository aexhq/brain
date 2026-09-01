use std::{
    fs,
    path::{Path, PathBuf},
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
    ActivationInput, ActivationOutput, AgentloopIdentity, AttachmentId, Capability, Decision,
    EnvironmentAttachment, EnvironmentBinding, EnvironmentId, EnvironmentRequest, Identity,
    LifecyclePolicy, MessageRequest, ModelBinding, ModelPresentation, ModelRequest, ModelResult,
    ModelStreamEvent, Observation, OperationId, Outcome, OutcomeError, RequestedToolBinding,
    ResolvedEnvironment, ResolvedSessionRequest, SealedSessionConfig, ToolBinding,
    ToolCancellation, ToolDefinition, ToolDispatch, ToolHosting, ToolInvocation,
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
        _session: &brain_protocol::SessionId,
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
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<Outcome, KernelError> {
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
        _session: &brain_protocol::SessionId,
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
    async fn execute(&self, dispatch: ToolDispatch) -> Result<Outcome, KernelError> {
        let request = EnvironmentRequest::Invoke {
            call_id: dispatch.invocation.call_id.clone(),
            tool: dispatch.binding.name.clone(),
            input: dispatch.invocation.input.clone(),
            deadline_ms: dispatch.deadline_ms,
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
        tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
    tool_request_with("slow", Vec::new(), Vec::new())
}

/// A sealed configuration binding one tool with the given `requires` to one
/// environment reporting the given `provides`.
fn tool_request_with(
    tool_name: &str,
    requires: Vec<Capability>,
    provides: Vec<Capability>,
) -> SealedSessionConfig {
    let environment = EnvironmentBinding {
        environment_id: EnvironmentId::new("workspace"),
        configuration_identity: Identity::of(&"configuration").unwrap(),
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
                name: tool_name.into(),
                description: "wait".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: None,
            }],
            response_format: None,
        },
        environments: vec![EnvironmentAttachment {
            binding: environment.clone(),
            attachment_id: AttachmentId::new("attachment"),
            provides,
        }],
        tool_bindings: vec![ToolBinding {
            name: tool_name.into(),
            environment: Some(environment),
            attachment_id: Some(AttachmentId::new("attachment")),
            requires,
            binding_names: Vec::new(),
            hosting: ToolHosting::Provisioned,
            payload: None,
        }],
    }
}

/// A sealed configuration binding one client-hosted tool: no environment anywhere.
fn client_tool_request(tool_name: &str) -> SealedSessionConfig {
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
                name: tool_name.into(),
                description: "answered by the session's creator".into(),
                input_schema: serde_json::json!({"type":"object"}),
                output_schema: None,
            }],
            response_format: None,
        },
        environments: Vec::new(),
        tool_bindings: vec![ToolBinding {
            name: tool_name.into(),
            environment: None,
            attachment_id: None,
            requires: Vec::new(),
            binding_names: Vec::new(),
            hosting: ToolHosting::Client,
            payload: None,
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
            .map(|environment| ResolvedEnvironment {
                environment_id: environment.binding.environment_id.clone(),
                configuration: serde_json::json!({}),
                lifecycle_policy: environment.binding.lifecycle_policy.clone(),
                grants: Default::default(),
                binding_identities: Default::default(),
            })
            .collect(),
        tool_bindings: sealed
            .tool_bindings
            .iter()
            .map(|binding| RequestedToolBinding {
                name: binding.name.clone(),
                environment_id: binding
                    .environment
                    .as_ref()
                    .map(|environment| environment.environment_id.clone()),
                requires: binding.requires.clone(),
                binding_names: binding.binding_names.clone(),
                hosting: binding.hosting,
                payload: binding.payload.clone(),
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
        _session: &brain_protocol::SessionId,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
        _session: &brain_protocol::SessionId,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
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

/// A first activation that asks for one tool call, then finishes with whatever came
/// back, so a test can read the observed results out of the turn's final record.
struct OneToolTurn {
    tool: &'static str,
}

#[async_trait]
impl LoopExecutor for OneToolTurn {
    async fn activate(
        &self,
        _session: &brain_protocol::SessionId,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let decision = match input.observation {
            Observation::ToolsCompleted { results } => Decision::Finish {
                result: Some(serde_json::json!({ "results": results })),
            },
            _ => Decision::Tools {
                calls: vec![ToolInvocation {
                    call_id: "call-1".into(),
                    name: self.tool.into(),
                    input: serde_json::json!({}),
                }],
            },
        };
        Ok(ActivationOutput {
            context: input.context,
            decision,
        })
    }
}

/// Answers every invoke with a scripted outcome.
struct ScriptedOutcome {
    outcome: Outcome,
}

#[async_trait]
impl ToolExecutor for ScriptedOutcome {
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<Outcome, KernelError> {
        Ok(self.outcome.clone())
    }

    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), KernelError> {
        Ok(())
    }
}

fn kernel_with(
    data_dir: &Path,
    loop_executor: Arc<dyn LoopExecutor>,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_deadline_ms: u64,
) -> Kernel {
    let (publisher, _worker) = telemetry_channel();
    Kernel::open(
        KernelConfig {
            data_dir: data_dir.to_path_buf(),
            max_decisions_per_turn: 8,
            tool_deadline_ms,
            loop_executor,
            model_executor: Arc::new(NoModels),
            tool_executor,
        },
        publisher,
    )
    .unwrap()
}

/// The bind-time capability check: a tool whose `requires` is not covered by its
/// environment's `provides` is rejected at create, and the error names the capability,
/// the tool, and the environment — the bash-on-a-browser mistake surfaces before a
/// session exists instead of at runtime.
#[tokio::test]
async fn requires_beyond_provides_rejects_create_naming_all_three_parties() {
    let data_dir = temporary_directory();
    let kernel = kernel_with(
        &data_dir,
        Arc::new(OneToolTurn { tool: "bash" }),
        Arc::new(NoTools),
        brain::DEFAULT_TOOL_DEADLINE_MS,
    );

    let sealed = tool_request_with("bash", vec![Capability::Exec], vec![Capability::Js]);
    let resolved = resolved_from(&sealed);
    let error = match kernel.begin_session(&resolved).unwrap().complete(sealed) {
        Ok(_) => {
            panic!("a tool requiring `exec` must not bind to an environment providing only `js`")
        }
        Err(error) => error,
    };
    let message = error.to_string();
    for named in ["exec", "bash", "workspace"] {
        assert!(
            message.contains(named),
            "the rejection must name {named:?}: {message}"
        );
    }

    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// A tool that declares nothing binds anywhere — including to an environment that
/// provides nothing at all.
#[tokio::test]
async fn empty_requires_binds_to_any_environment() {
    let data_dir = temporary_directory();
    let kernel = kernel_with(
        &data_dir,
        Arc::new(OneToolTurn { tool: "plain" }),
        Arc::new(ScriptedOutcome {
            outcome: Outcome::Ok {
                value: serde_json::json!({"ok": true}),
            },
        }),
        brain::DEFAULT_TOOL_DEADLINE_MS,
    );

    let sealed = tool_request_with("plain", Vec::new(), Vec::new());
    let handle = start(&kernel, sealed);
    let session = handle
        .message(MessageRequest {
            input: "run".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        session.status,
        brain_protocol::SessionStatus::Idle
    ));

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// Non-ok invoke outcomes land in the loop's view as failed tool results whose output
/// carries a readable code, and an ok outcome passes its value through untouched.
#[tokio::test]
async fn invoke_outcomes_map_onto_tool_results() {
    let cases: Vec<(Outcome, bool, serde_json::Value)> = vec![
        (
            Outcome::Ok {
                value: serde_json::json!({"content": "done"}),
            },
            false,
            serde_json::json!("done"),
        ),
        (
            Outcome::Error {
                error: OutcomeError {
                    code: "exec_denied".into(),
                    message: "policy".into(),
                    details: None,
                },
            },
            true,
            serde_json::json!("exec_denied"),
        ),
        (Outcome::Timeout, true, serde_json::json!("timeout")),
        (Outcome::Cancelled, true, serde_json::json!("cancelled")),
    ];
    for (outcome, is_error, marker) in cases {
        let data_dir = temporary_directory();
        let kernel = kernel_with(
            &data_dir,
            Arc::new(OneToolTurn { tool: "slow" }),
            Arc::new(ScriptedOutcome {
                outcome: outcome.clone(),
            }),
            brain::DEFAULT_TOOL_DEADLINE_MS,
        );
        let handle = start(&kernel, tool_request());
        let session_id = handle.id().clone();
        handle
            .message(MessageRequest {
                input: "run".into(),
            })
            .await
            .unwrap();
        let events = kernel.events(&session_id, 0, 100).unwrap().events;
        let result = events
            .iter()
            .find(|event| event.event_type == "tool_result")
            .expect("the invoke must record a tool result");
        assert_eq!(
            result.data["result"]["is_error"], is_error,
            "{outcome:?} must map to is_error={is_error}"
        );
        let output = &result.data["result"]["output"];
        let carried = if is_error {
            output["code"].clone()
        } else {
            output["content"].clone()
        };
        assert_eq!(carried, marker, "{outcome:?} produced {output}");
        drop(handle);
        drop(kernel);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = fs::remove_dir_all(data_dir);
    }
}

/// The deadline is enforced by the caller: an invoke that outlives `deadline_ms` is
/// dropped, recorded as a `timeout` tool result, and its environment is told to cancel
/// — because the remote cannot be trusted to stop on its own.
#[tokio::test]
async fn an_overdue_invoke_is_killed_and_recorded_as_timeout() {
    let data_dir = temporary_directory();
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let kernel = kernel_with(
        &data_dir,
        Arc::new(OneToolTurn { tool: "slow" }),
        Arc::new(SlowTools {
            started: started.clone(),
            cancelled: cancelled.clone(),
        }),
        50,
    );
    let handle = start(&kernel, tool_request());
    let session_id = handle.id().clone();
    let session = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handle.message(MessageRequest {
            input: "run".into(),
        }),
    )
    .await
    .expect("the deadline must end the call long before the executor's 60s sleep")
    .unwrap();
    assert!(matches!(
        session.status,
        brain_protocol::SessionStatus::Idle
    ));
    assert!(
        cancelled.load(Ordering::Acquire),
        "the environment must be told to stop the abandoned work"
    );
    let events = kernel.events(&session_id, 0, 100).unwrap().events;
    let result = events
        .iter()
        .find(|event| event.event_type == "tool_result")
        .expect("the expired invoke must still record a tool result");
    assert_eq!(result.data["result"]["is_error"], true);
    assert_eq!(result.data["result"]["output"]["code"], "timeout");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "tool_cancel_intent"),
        "the kill must be journalled as a cancellation intent"
    );

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// A dispatch that reaches the environment executor for a client-hosted call is the
/// bug this executor exists to catch.
struct RefusesDispatch;

#[async_trait]
impl ToolExecutor for RefusesDispatch {
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<Outcome, KernelError> {
        Err(KernelError::InvalidState(
            "a client-hosted call must never reach the environment executor".into(),
        ))
    }

    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), KernelError> {
        Err(KernelError::InvalidState(
            "a client-hosted cancellation must never reach the environment executor".into(),
        ))
    }
}

/// A client-hosted tool call parks the turn on its `tool_intent` and finishes when the
/// outcome is POSTed back: no environment executor is on the path, the intent on the
/// feed carries the operation id the answer is correlated by, and the answered value
/// lands in the journalled `tool_result` untouched.
#[tokio::test]
async fn a_client_tool_call_parks_until_its_outcome_is_posted() {
    let data_dir = temporary_directory();
    let kernel = kernel_with(
        &data_dir,
        Arc::new(OneToolTurn { tool: "local" }),
        Arc::new(RefusesDispatch),
        brain::DEFAULT_TOOL_DEADLINE_MS,
    );
    let handle = start(&kernel, client_tool_request("local"));
    let session_id = handle.id().clone();
    let turn = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .message(MessageRequest {
                    input: "run".into(),
                })
                .await
        })
    };
    let mut operation_id = None;
    for _ in 0..500 {
        let events = kernel.events(&session_id, 0, 100).unwrap().events;
        if let Some(intent) = events
            .iter()
            .find(|event| event.event_type == "tool_intent")
        {
            assert_eq!(intent.data["binding"]["hosting"], "client");
            assert!(
                intent.data["binding"].get("environment").is_none()
                    || intent.data["binding"]["environment"].is_null(),
                "a client tool intent must not carry an environment binding"
            );
            operation_id = Some(
                serde_json::from_value::<brain_protocol::OperationId>(
                    intent.data["operation_id"].clone(),
                )
                .unwrap(),
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let operation_id = operation_id.expect("the parked call must journal its tool_intent");
    kernel
        .resolve_tool_call(
            &session_id,
            &operation_id,
            Outcome::Ok {
                value: serde_json::json!({"content": "from the client"}),
            },
        )
        .expect("the parked call must accept its outcome");
    let session = tokio::time::timeout(std::time::Duration::from_secs(5), turn)
        .await
        .expect("the turn must finish once the outcome lands")
        .unwrap()
        .unwrap();
    assert!(matches!(
        session.status,
        brain_protocol::SessionStatus::Idle
    ));
    let events = kernel.events(&session_id, 0, 100).unwrap().events;
    let result = events
        .iter()
        .find(|event| event.event_type == "tool_result")
        .expect("the answered call must journal a tool result");
    assert_eq!(result.data["result"]["is_error"], false);
    assert_eq!(
        result.data["result"]["output"]["content"],
        "from the client"
    );

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// An answer for a call nobody is waiting on — unknown, expired, or already answered —
/// is refused, so a duplicate POST past the idempotency window cannot invent a result.
#[tokio::test]
async fn resolving_an_unknown_operation_is_refused() {
    let data_dir = temporary_directory();
    let kernel = kernel_with(
        &data_dir,
        Arc::new(OneToolTurn { tool: "local" }),
        Arc::new(RefusesDispatch),
        brain::DEFAULT_TOOL_DEADLINE_MS,
    );
    let handle = start(&kernel, client_tool_request("local"));
    let error = kernel
        .resolve_tool_call(
            handle.id(),
            &brain_protocol::OperationId::new(format!("op_{}", "0".repeat(32))),
            Outcome::Ok {
                value: serde_json::json!(null),
            },
        )
        .expect_err("an unknown operation must be refused");
    assert!(error.to_string().contains("no client Tool call is pending"));

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// A client call nobody answers dies at the kernel's deadline exactly like an overdue
/// environment invoke: a `timeout` tool result, a journalled cancellation intent for
/// the client to abort on, and no environment executor touched anywhere.
#[tokio::test]
async fn an_unanswered_client_call_times_out_and_journals_the_cancellation() {
    let data_dir = temporary_directory();
    let kernel = kernel_with(
        &data_dir,
        Arc::new(OneToolTurn { tool: "local" }),
        Arc::new(RefusesDispatch),
        50,
    );
    let handle = start(&kernel, client_tool_request("local"));
    let session_id = handle.id().clone();
    let session = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handle.message(MessageRequest {
            input: "run".into(),
        }),
    )
    .await
    .expect("the deadline must end the unanswered call")
    .unwrap();
    assert!(matches!(
        session.status,
        brain_protocol::SessionStatus::Idle
    ));
    let events = kernel.events(&session_id, 0, 100).unwrap().events;
    let result = events
        .iter()
        .find(|event| event.event_type == "tool_result")
        .expect("the expired call must still record a tool result");
    assert_eq!(result.data["result"]["is_error"], true);
    assert_eq!(result.data["result"]["output"]["code"], "timeout");
    let cancel = events
        .iter()
        .find(|event| event.event_type == "tool_cancel_result")
        .expect("dropping the park must be journalled as the cancellation");
    assert_eq!(cancel.data["cancelled"], true);

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}
