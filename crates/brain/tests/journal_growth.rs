//! The journal must not grow with the *square* of a turn's decision count.
//!
//! The context envelope grows monotonically within a turn, so anything the kernel writes
//! per decision costs the sum of every intermediate size, not the final one. At the
//! production ceiling of `BRAIN_MAX_DECISIONS=128` that turns a megabyte of conversation
//! into tens of megabytes of permanently retained journal, on an EFS-backed database that
//! is never vacuumed and is re-checksummed in full on every restart.
//!
//! These tests pin the bound and the two correctness properties a fix must not break: the
//! session row still carries the final context, and reopening the kernel rehydrates it.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use brain::{Kernel, KernelConfig, KernelError, LoopExecutor, ModelExecutor, ToolExecutor};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopDigest, Decision, MessageRequest, ModelBinding,
    ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, OperationId,
    SealedSessionConfig, SessionId, ToolCancellation, ToolDispatch, ToolResult,
};
use brain_telemetry::telemetry_channel;

/// Bytes each activation appends to the context. Large enough that the quadratic term
/// dominates fixed per-record overhead, small enough to keep the test quick.
const ITEM_BYTES: usize = 16 * 1024;

/// Decisions in the measured turn. Half the production ceiling; the defect is quadratic,
/// so the smaller count still separates the two regimes by an order of magnitude.
const DECISIONS: usize = 64;

/// The journal may hold a constant number of copies of the final context plus per-record
/// overhead, not one copy per decision. Measured at `DECISIONS = 64`: the journal was
/// 34.8x the context with the per-decision write and is 1.1x without it, and one events
/// page was 32.6x and is now 0.1x. The bound sits far from both regimes.
const MAX_JOURNAL_TO_CONTEXT_RATIO: u64 = 8;

/// An agentloop that grows its context every activation, the way a real one accumulates
/// model turns and tool results, while keeping its model requests small so the
/// measurement isolates the context write.
struct GrowingLoop {
    activations: AtomicUsize,
}

#[async_trait]
impl LoopExecutor for GrowingLoop {
    async fn activate(
        &self,
        _agentloop: &AgentloopDigest,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let activation = self.activations.fetch_add(1, Ordering::Relaxed);
        let mut context = input.context;
        context.items.push(serde_json::json!({
            "role": "assistant",
            "content": "x".repeat(ITEM_BYTES),
        }));
        let decision = if activation + 1 < DECISIONS {
            Decision::Model {
                request: ModelRequest {
                    messages: vec![serde_json::json!({"role": "user", "content": "next"})],
                    response_format: None,
                    max_output_tokens: Some(16),
                },
            }
        } else {
            Decision::Finish {
                result: Some(serde_json::json!({"ok": true})),
            }
        };
        Ok(ActivationOutput { context, decision })
    }
}

struct TinyModel;

#[async_trait]
impl ModelExecutor for TinyModel {
    async fn execute(
        &self,
        _operation_id: &OperationId,
        _request_digest: &str,
        _binding: &ModelBinding,
        _presentation: &ModelPresentation,
        _request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
        on_event(ModelStreamEvent::TextDelta { text: "ok".into() });
        Ok(ModelResult {
            response: serde_json::json!({"text": "ok"}),
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

    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), KernelError> {
        panic!("unexpected Tool cancellation")
    }
}

/// Runs one turn of `DECISIONS` activations and returns the closed journal's size on disk
/// alongside the session it wrote.
async fn measure_one_turn(data_dir: &Path) -> (u64, SessionId) {
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.to_path_buf(),
            max_decisions_per_turn: DECISIONS,
            loop_executor: Arc::new(GrowingLoop {
                activations: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(TinyModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = kernel.create_session(request()).await.unwrap();
    let session_id = handle.id().clone();
    let finished = handle
        .message(MessageRequest {
            content: serde_json::json!("go"),
        })
        .await
        .unwrap();
    assert!(
        matches!(finished.status, brain_protocol::SessionStatus::Idle),
        "the turn must reach its Finish decision, not the decision limit"
    );
    drop(handle);
    drop(kernel);
    // Closing the last connection checkpoints and removes the WAL, so the database file
    // alone is the whole journal.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let bytes = fs::metadata(data_dir.join("brain.sqlite3")).unwrap().len();
    (bytes, session_id)
}

#[tokio::test]
async fn a_turn_does_not_journal_one_context_copy_per_decision() {
    let data_dir = temporary_directory();
    let (journal_bytes, _) = measure_one_turn(&data_dir).await;

    let context_bytes = (DECISIONS * ITEM_BYTES) as u64;
    let ratio = journal_bytes as f64 / context_bytes as f64;
    assert!(
        journal_bytes <= context_bytes * MAX_JOURNAL_TO_CONTEXT_RATIO,
        "journal grew to {journal_bytes} bytes for a {context_bytes}-byte context \
         ({ratio:.1}x, bound {MAX_JOURNAL_TO_CONTEXT_RATIO}x): the kernel is writing the \
         context once per decision instead of once per turn"
    );

    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn the_event_stream_does_not_carry_a_context_copy_per_decision() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: DECISIONS,
            loop_executor: Arc::new(GrowingLoop {
                activations: AtomicUsize::new(0),
            }),
            model_executor: Arc::new(TinyModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = kernel.create_session(request()).await.unwrap();
    let session_id = handle.id().clone();
    handle
        .message(MessageRequest {
            content: serde_json::json!("go"),
        })
        .await
        .unwrap();

    // `events` is client-facing and pages by record count, never by bytes, so any
    // per-decision context record is materialised in full on every poll.
    let page = kernel.events(&session_id, 0, 1_000).unwrap();
    let page_bytes = serde_json::to_vec(&page).unwrap().len() as u64;
    let context_bytes = (DECISIONS * ITEM_BYTES) as u64;
    let ratio = page_bytes as f64 / context_bytes as f64;
    assert!(
        page_bytes <= context_bytes * MAX_JOURNAL_TO_CONTEXT_RATIO,
        "one events page serialised to {page_bytes} bytes for a {context_bytes}-byte \
         context ({ratio:.1}x, bound {MAX_JOURNAL_TO_CONTEXT_RATIO}x): the client-facing \
         stream is replaying the whole context once per decision"
    );

    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn the_session_row_still_holds_the_final_context_after_the_turn() {
    let data_dir = temporary_directory();
    let (_, session_id) = measure_one_turn(&data_dir).await;

    // Moving the context write off the per-decision path must not leave the row stale:
    // the row is the only thing rehydration reads, and `Decision::Finish` historically
    // relied on the per-decision write having already persisted it.
    let store = brain::SqliteJournal::open(&data_dir.join("brain.sqlite3")).unwrap();
    let row = brain::JournalStore::session(&store, &session_id)
        .unwrap()
        .unwrap();
    let items = row.context["items"].as_array().unwrap().len();
    assert_eq!(
        items, DECISIONS,
        "the session row must hold every item the turn produced"
    );
    drop(store);

    // And the rehydrated kernel must resume from that context rather than an empty one.
    let (publisher, _worker) = telemetry_channel();
    let reopened = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 1,
            loop_executor: Arc::new(AssertContext {
                expected_items: DECISIONS,
            }),
            model_executor: Arc::new(TinyModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = reopened.handle(&session_id).unwrap();
    handle
        .message(MessageRequest {
            content: serde_json::json!("again"),
        })
        .await
        .unwrap();
    drop(handle);
    drop(reopened);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

/// Fails the turn unless the kernel handed it the context the previous turn ended on.
struct AssertContext {
    expected_items: usize,
}

#[async_trait]
impl LoopExecutor for AssertContext {
    async fn activate(
        &self,
        _agentloop: &AgentloopDigest,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        assert_eq!(
            input.context.items.len(),
            self.expected_items,
            "rehydration must restore the context the previous turn ended on"
        );
        Ok(ActivationOutput {
            context: input.context,
            decision: Decision::Finish { result: None },
        })
    }
}

fn request() -> SealedSessionConfig {
    SealedSessionConfig {
        agentloop_digest: AgentloopDigest::new("a".repeat(64)),
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

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("brain-journal-growth-{}", rand::random::<u64>()));
    fs::create_dir(&path).unwrap();
    path
}
