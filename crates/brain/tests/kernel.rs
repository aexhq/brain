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
    AppendRecord, JournalStore, Kernel, KernelConfig, KernelError, LoopExecutor, ModelExecutor,
    SegmentJournal, SessionHandle, SessionUpdate, ToolExecutor,
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
async fn intent_precedes_model_effect_and_reopen_preserves_events() {
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
            content: serde_json::json!("hello"),
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
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let reopened = Kernel::open(config(), publisher).unwrap();
    let reopened_events = reopened.events(&session_id, 0, 100).unwrap().events;
    assert_eq!(reopened_events.len(), events.events.len());
    assert_eq!(
        reopened_events
            .iter()
            .map(|event| event.recorded_at_ms)
            .collect::<Vec<_>>(),
        recorded_at_ms
    );
    drop(reopened);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn restart_marks_an_inflight_turn_ambiguous_instead_of_guessing() {
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
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let store = SegmentJournal::open(
        &data_dir.join("journal"),
        brain::DEFAULT_IDEMPOTENCY_RETENTION,
    )
    .unwrap();
    let row = store.session_row(&session_id).unwrap().unwrap();
    store
        .append(
            &session_id,
            row.through_sequence,
            &[AppendRecord::new(
                "model_intent",
                serde_json::json!({"operation_id":"op_interrupted"}),
            )],
            SessionUpdate {
                status: Some(brain_protocol::SessionStatus::Running),
                context: None,
                configuration: None,
            },
        )
        .unwrap();
    drop(store);

    let recovered = Kernel::open(config(), publisher).unwrap();
    assert!(matches!(
        recovered.session(&session_id).unwrap().status,
        brain_protocol::SessionStatus::Failed
    ));
    let events = recovered.events(&session_id, 0, 100).unwrap();
    assert_eq!(
        events.events.last().unwrap().event_type,
        "recovery_interrupted"
    );
    assert_eq!(
        events.events.last().unwrap().data["classification"],
        "operation_outcome_ambiguous"
    );
    drop(recovered);
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
                    content: serde_json::json!("hello"),
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
                    content: serde_json::json!("run"),
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
    let resolved = ResolvedSessionRequest {
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
    };
    kernel
        .begin_session(&resolved)
        .unwrap()
        .complete(sealed)
        .unwrap()
}
