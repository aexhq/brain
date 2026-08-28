//! The journal must not grow with the *square* of a turn's decision count.
//!
//! The context envelope grows monotonically within a turn, so anything the kernel writes
//! per decision costs the sum of every intermediate size, not the final one. At the
//! production ceiling of `BRAIN_MAX_DECISIONS=128` that turns a megabyte of conversation
//! into tens of megabytes of permanently retained journal, on an EFS-backed database that
//! is never vacuumed and is replayed in full on every restart.
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
use brain::{
    Kernel, KernelConfig, KernelError, LoopExecutor, ModelExecutor, SessionHandle, ToolExecutor,
};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, Decision, EnvironmentRequirement,
    Identity, MessageRequest, ModelBinding, ModelPresentation, ModelRequest, ModelResult,
    ModelStreamEvent, OperationId, RequestedToolBinding, ResolvedSessionRequest,
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
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let activation = self.activations.fetch_add(1, Ordering::Relaxed);
        let mut context = input.context;
        context.items.push(serde_json::json!({
            "role": "assistant",
            "content": "x".repeat(ITEM_BYTES),
        }));
        // A counter seeded at `DECISIONS - 1` finishes on its first activation of every
        // turn, which is how the turn-axis test below gets one decision per turn.
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
        _request_digest: &Identity,
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
    let handle = start(&kernel, request());
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
    // Dropping the kernel drains the journal writer, so the segments on disk are the
    // whole journal.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let bytes = fs::read_dir(data_dir.join("journal"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
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
    let handle = start(&kernel, request());
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
    let store = brain::SegmentJournal::open(&data_dir.join("journal")).unwrap();
    let row = brain::JournalStore::session_row(&store, &session_id)
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
        _agentloop: &AgentloopIdentity,
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

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("brain-journal-growth-{}", rand::random::<u64>()));
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

/// The same defect on the other axis: turns, not decisions.
///
/// `finish_turn` writes a `$session` state frame carrying the whole context at the end
/// of every turn, and the context grows across turns as the conversation accumulates. So
/// a session that runs T turns, each adding C bytes, writes `C*T*(T+1)/2` bytes of
/// journal — the sum of every intermediate context, not the final one. Unlike the
/// decision axis, which the kernel caps at `BRAIN_MAX_DECISIONS`, nothing bounds T.
///
/// Every one of those bytes is also read back and parsed at every restart, so the same
/// term shows up in cold start.
///
/// Measured at `TURNS = 64` on 2026-08-28: one 16 KiB item per turn produced 34,199,242
/// bytes of journal for a 1 MiB context, 32.6x. The closed form predicts `(T+1)/2` =
/// 32.5x, so what is being measured is the per-turn write and not overhead around it.
///
/// Session state now lives in a file per session that is rewritten in place rather than
/// appended, so the same run leaves 1,084,376 bytes — 1.0x, and flat in the turn count.
#[tokio::test]
async fn a_session_does_not_journal_one_context_copy_per_turn() {
    /// Turns in the measured session. Far below anything a real conversation reaches.
    const TURNS: usize = 64;
    /// The same constant-multiple bound the decision-axis test uses, for the same
    /// reason: a fixed number of copies of the final context is fine, one per turn
    /// is not.
    const MAX_RATIO: u64 = 8;

    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 4,
            loop_executor: Arc::new(GrowingLoop {
                // One activation per turn, so the growth measured is turns.
                activations: AtomicUsize::new(DECISIONS - 1),
            }),
            model_executor: Arc::new(TinyModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();
    let handle = start(&kernel, request());
    for _ in 0..TURNS {
        handle
            .message(MessageRequest {
                content: serde_json::json!("go"),
            })
            .await
            .unwrap();
    }
    drop(handle);
    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let journal_bytes: u64 = fs::read_dir(data_dir.join("journal"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    let context_bytes = (TURNS * ITEM_BYTES) as u64;
    let ratio = journal_bytes as f64 / context_bytes as f64;
    let held = journal_bytes <= context_bytes * MAX_RATIO;
    fs::remove_dir_all(data_dir).unwrap();
    assert!(
        held,
        "a {TURNS}-turn session journalled {journal_bytes} bytes for a {context_bytes}-byte \
         context ({ratio:.1}x, bound {MAX_RATIO}x): the kernel is writing the whole context \
         once per turn, so the log grows with the square of the turn count"
    );
}
