use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use brain::{
    AppendRecord, JournalStore, Kernel, KernelConfig, KernelError, LoopExecutor, ModelExecutor,
    SessionUpdate, SqliteJournal, ToolExecutor,
};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopDigest, Decision, MessageRequest, ModelBinding,
    ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, Observation, OperationId,
    SealedSessionConfig, ToolDispatch, ToolResult,
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
        _agentloop: &AgentloopDigest,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        let decision = if call == 0 {
            assert!(matches!(input.observation, Observation::UserMessage { .. }));
            Decision::Model {
                request: ModelRequest {
                    messages: vec![serde_json::json!({"role":"user","content":"hello"})],
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
        _request_digest: &str,
        _binding: &ModelBinding,
        _presentation: &ModelPresentation,
        _request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        on_event(ModelStreamEvent::TextDelta {
            text: "hello".into(),
        });
        Ok(ModelResult {
            response: serde_json::json!({"text":"hello"}),
            usage: None,
        })
    }
}

struct NoTools;

#[async_trait]
impl ToolExecutor for NoTools {
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<ToolResult, KernelError> {
        panic!("unexpected Tool dispatch")
    }
}

struct SlowModel {
    started: Arc<Notify>,
}

#[async_trait]
impl ModelExecutor for SlowModel {
    async fn execute(
        &self,
        _operation_id: &OperationId,
        _request_digest: &str,
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
    let handle = kernel.create_session(request()).await.unwrap();
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
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let reopened = Kernel::open(config(), publisher).unwrap();
    assert_eq!(
        reopened.events(&session_id, 0, 100).unwrap().events.len(),
        events.events.len()
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
    let handle = kernel.create_session(request()).await.unwrap();
    let session_id = handle.id().clone();
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let store = SqliteJournal::open(&data_dir.join("brain.sqlite3")).unwrap();
    let row = store.session(&session_id).unwrap().unwrap();
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
    let handle = kernel.create_session(request()).await.unwrap();
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

fn request() -> SealedSessionConfig {
    SealedSessionConfig {
        agentloop_digest: AgentloopDigest::new("a".repeat(64)),
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
        metadata: serde_json::json!({}),
    }
}

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("brain-kernel-test-{}", rand::random::<u64>()));
    fs::create_dir(&path).unwrap();
    path
}
