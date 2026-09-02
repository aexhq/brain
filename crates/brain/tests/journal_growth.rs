//! The journal must not grow with the *square* of a turn's decision count.
//!
//! The context envelope grows monotonically within a turn, so anything the runtime writes
//! per decision costs the sum of every intermediate size, not the final one. At the
//! production ceiling of `BRAIN_MAX_DECISIONS=128` that turns a megabyte of conversation
//! into tens of megabytes of permanently retained journal, on an EFS-backed database that
//! is never vacuumed and is replayed in full on every restart.
//!
//! These tests pin the bound and the two correctness properties a fix must not break: the
//! session row still carries the final context, and reopening the runtime rehydrates it.

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
    Error, JournalStore, LoopExecutor, ModelExecutor, ObservedJournal, Session, SessionConfig,
    ToolExecutor,
};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, Decision, MessageRequest, ModelBinding,
    ModelRequest, ModelResult, ModelStreamEvent, Observation, Outcome, RequestedToolBinding,
    ResolvedEnvironment, ResolvedSessionRequest, SealedSessionConfig, SessionId, ToolCancellation,
    ToolDefinition, ToolDispatch,
};
use brain_telemetry::telemetry_channel;
/// What the host gives every session: the store it journals to and the executors it
/// performs effects with. Built the way the server builds it.
struct Runtime {
    store: Arc<ObservedJournal>,
    config: Arc<SessionConfig>,
}

#[allow(dead_code)]
impl Runtime {
    fn open(
        data_dir: &Path,
        telemetry: brain_telemetry::TelemetryPublisher,
        max_decisions_per_turn: usize,
        tool_deadline_ms: u64,
        loop_executor: Arc<dyn LoopExecutor>,
        model_executor: Arc<dyn ModelExecutor>,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        let journal: Arc<dyn brain::JournalStore> =
            Arc::new(brain::SegmentJournal::open(&data_dir.join("journal")).unwrap());
        let store = Arc::new(ObservedJournal::new(journal, telemetry));
        brain::interrupt_unfinished_turns(&*store).unwrap();
        let config = Arc::new(SessionConfig {
            max_decisions_per_turn,
            tool_deadline_ms,
            loop_executor,
            model_executor,
            tool_executor,
            live: store.live_sender(),
        });
        Self { store, config }
    }

    fn store(&self) -> Arc<dyn brain::JournalStore> {
        self.store.clone()
    }

    fn events(
        &self,
        session_id: &SessionId,
        after: u64,
        limit: usize,
    ) -> brain_protocol::EventPage {
        brain::event_page(
            self.store.records_after(session_id, after, limit).unwrap(),
            after,
        )
    }

    fn session(&self, session_id: &SessionId) -> brain_protocol::SessionSummary {
        self.store.session_summary(session_id).unwrap().unwrap()
    }

    fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, brain_protocol::LiveEvent)> {
        self.store.subscribe()
    }
}

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
        _session: &brain_protocol::SessionId,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, Error> {
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
                    system: None,
                    tools: None,
                    messages: vec![brain_protocol::Message::user_text("next")],
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
        _binding: &ModelBinding,
        _request: ModelRequest,
        _tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error> {
        on_event(ModelStreamEvent::TextDelta {
            index: 0,
            text: "ok".into(),
        });
        Ok(ModelResult {
            message: brain_protocol::Message::assistant(vec![brain_protocol::ContentBlock::text(
                "ok",
            )]),
            stop_reason: brain_protocol::StopReason::EndTurn,
            usage: brain_protocol::Usage::default(),
        })
    }
}

struct NoTools;

#[async_trait]
impl ToolExecutor for NoTools {
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<Outcome, Error> {
        panic!("unexpected Tool dispatch")
    }

    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), Error> {
        panic!("unexpected Tool cancellation")
    }
}

/// Runs one turn of `DECISIONS` activations and returns the closed journal's size on disk
/// alongside the session it wrote.
async fn measure_one_turn(data_dir: &Path) -> (u64, SessionId) {
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        data_dir,
        publisher,
        DECISIONS,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(GrowingLoop {
            activations: AtomicUsize::new(0),
        }),
        Arc::new(TinyModel),
        Arc::new(NoTools),
    );
    let handle = start(&runtime, request());
    let session_id = handle.id().clone();
    let finished = handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    assert!(
        matches!(finished.status, brain_protocol::SessionStatus::Idle),
        "the turn must reach its Finish decision, not the decision limit"
    );
    drop(handle);
    drop(runtime);
    // Dropping the runtime drains the journal writer, so the segments on disk are the
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
         ({ratio:.1}x, bound {MAX_JOURNAL_TO_CONTEXT_RATIO}x): the runtime is writing the \
         context once per decision instead of once per turn"
    );

    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn the_event_stream_does_not_carry_a_context_copy_per_decision() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        DECISIONS,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(GrowingLoop {
            activations: AtomicUsize::new(0),
        }),
        Arc::new(TinyModel),
        Arc::new(NoTools),
    );
    let handle = start(&runtime, request());
    let session_id = handle.id().clone();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();

    // `events` is client-facing and pages by record count, never by bytes, so any
    // per-decision context record is materialised in full on every poll.
    let page = runtime.events(&session_id, 0, 1_000);
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
    drop(runtime);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn the_session_row_holds_the_final_context_after_the_turn() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        DECISIONS,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(GrowingLoop {
            activations: AtomicUsize::new(0),
        }),
        Arc::new(TinyModel),
        Arc::new(NoTools),
    );
    let store = runtime.store();
    let handle = start(&runtime, request());
    let session_id = handle.id().clone();
    handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();

    // Moving the context write off the per-decision path must not leave the row stale.
    // Read while the process that owns the session is still alive: that is the only place
    // a session exists, so it is the only place worth asserting about.
    let row = brain::JournalStore::session_row(&*store, &session_id)
        .unwrap()
        .unwrap();
    let items = row.context["items"].as_array().unwrap().len();
    assert_eq!(
        items, DECISIONS,
        "the session row must hold every item the turn produced"
    );

    drop(handle);
    drop(runtime);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
        system: "test".into(),
        tools: Vec::new(),
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
/// then the sealed configuration completes it. There is no shortcut past `Session::begin`,
/// so these tests exercise the same validation the production path enforces.
fn start(runtime: &Runtime, sealed: SealedSessionConfig) -> Session {
    let resolved = ResolvedSessionRequest {
        history: Vec::new(),
        agentloop_identity: sealed.agentloop_identity.clone(),
        brain_configuration: sealed.brain_configuration.clone(),
        model: sealed.model.clone(),
        system: sealed.system.clone(),
        tools: sealed.tools.clone(),
        environments: sealed
            .environments
            .iter()
            .map(|environment| ResolvedEnvironment {
                environment_id: environment.binding.environment_id.clone(),
                configuration: serde_json::json!({}),
                lifecycle_policy: environment.binding.lifecycle_policy.clone(),
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
                needs: binding.needs.clone(),
                binding_names: binding.binding_names.clone(),
                hosting: binding.hosting,
                program: binding.program.clone(),
            })
            .collect(),
    };
    Session::begin(runtime.store(), runtime.config.clone(), &resolved)
        .unwrap()
        .complete(sealed)
        .unwrap()
}

/// A turn that was in flight when the process stopped is closed, and says so.
///
/// Whether the model call or the tool call actually happened is not in the journal, so
/// Brain records exactly that and returns the session to Idle. It does not decide for the
/// client: an agentloop, a tool or an SDK client sees `turn_failed` on the stream and
/// resumes or abandons on its own terms.
#[tokio::test]
async fn an_interrupted_turn_is_closed_and_recorded() {
    let data_dir = temporary_directory();
    let (_, session_id) = measure_one_turn(&data_dir).await;

    // Leave the session mid-turn, the way a process that died during one would.
    {
        let store = brain::SegmentJournal::open(&data_dir.join("journal")).unwrap();
        let row = brain::JournalStore::session_row(&store, &session_id)
            .unwrap()
            .unwrap();
        brain::JournalStore::append(
            &store,
            &session_id,
            row.through_sequence,
            &[brain::AppendRecord::new(
                "turn_started",
                serde_json::json!({"content": "and then the lights went out"}),
            )],
            brain::SessionUpdate {
                status: Some(brain_protocol::SessionStatus::Running),
                context: None,
                configuration: None,
            },
        )
        .unwrap();
        drop(store);
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (publisher, _worker) = telemetry_channel();
    let reopened = Runtime::open(
        &data_dir,
        publisher,
        4,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(GrowingLoop {
            activations: AtomicUsize::new(0),
        }),
        Arc::new(TinyModel),
        Arc::new(NoTools),
    );

    let session = reopened.session(&session_id);
    assert!(
        matches!(session.status, brain_protocol::SessionStatus::Idle),
        "an interrupted turn must leave the session able to take another, not stuck: {:?}",
        session.status
    );
    let events = reopened.events(&session_id, 0, 1_000).events;
    let last = events.last().expect("the session has records");
    assert_eq!(last.event_type, "turn_failed");
    assert_eq!(
        last.data["code"], "interrupted",
        "the break must be in the record, so a client can see it and decide"
    );

    drop(reopened);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// A session comes back from its own journal, best effort.
///
/// The journal is the record of the session, so folding it back in write order rebuilds
/// everything except the one thing the records never held: the agentloop's context, which
/// is handed back to the loop to rebuild. Nothing is fsynced to make this true — a crash
/// can lose the tail and a session can return a few records behind, which is the trade
/// this design accepts.
#[tokio::test]
async fn a_session_comes_back_from_its_journal() {
    let data_dir = temporary_directory();
    let (_, session_id) = measure_one_turn(&data_dir).await;

    let store = brain::SegmentJournal::open(&data_dir.join("journal")).unwrap();
    let row = brain::JournalStore::session_row(&store, &session_id)
        .unwrap()
        .expect("a session must be rebuilt from the records it left behind");
    assert!(
        matches!(row.status, brain_protocol::SessionStatus::Idle),
        "a session whose last turn finished comes back idle, not mid-turn: {:?}",
        row.status
    );

    // Its history reads back whole, and densely: `records_after` indexes by sequence, so a
    // session rebuilt with a gap would answer for the wrong record under the right number.
    let records = brain::JournalStore::records_after(&store, &session_id, 0, 1_000).unwrap();
    let sequences: Vec<u64> = records.iter().map(|record| record.sequence).collect();
    let dense: Vec<u64> = (1..=records.len() as u64).collect();
    assert_eq!(
        sequences, dense,
        "a restored session's records must stay dense"
    );
    assert_eq!(
        records.first().map(|record| record.kind.as_str()),
        Some("session_creation_started"),
        "the session must be rebuilt from its genesis record, not from wherever the log \
         happened to still have records"
    );
    drop(store);
    let _ = fs::remove_dir_all(data_dir);
}

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
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        4,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(GrowingLoop {
            // One activation per turn, so the growth measured is turns.
            activations: AtomicUsize::new(DECISIONS - 1),
        }),
        Arc::new(TinyModel),
        Arc::new(NoTools),
    );
    let handle = start(&runtime, request());
    for _ in 0..TURNS {
        handle
            .message(MessageRequest { input: "go".into() })
            .await
            .unwrap();
    }
    drop(handle);
    drop(runtime);
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
         context ({ratio:.1}x, bound {MAX_RATIO}x): the runtime is writing the whole context \
         once per turn, so the log grows with the square of the turn count"
    );
}

/// Bytes in each message a turn adds to the model request.
const MESSAGE_BYTES: usize = 8 * 1024;

/// An agentloop that talks to the model the way a real one does: every turn resends the
/// whole conversation so far, one message longer than the last.
///
/// `GrowingLoop` above deliberately keeps its model requests tiny so that it measures the
/// context write alone, which is why nothing here caught the journal recording a fresh
/// copy of the transcript on every decision.
struct ResendingLoop {
    turns: AtomicUsize,
}

#[async_trait]
impl LoopExecutor for ResendingLoop {
    async fn activate(
        &self,
        _session: &brain_protocol::SessionId,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, Error> {
        // Branch on what happened, not on the context: a marker left in the items would
        // still be there on the next turn and every turn after the first would finish
        // without ever reaching the model.
        let mut context = input.context;
        match input.observation {
            Observation::UserMessage { .. } => {
                let turn = self.turns.fetch_add(1, Ordering::Relaxed);
                context
                    .items
                    .push(serde_json::json!({ "role": "user", "turn": turn }));
                // The whole conversation so far, one message longer every turn, which is
                // what an agentloop actually sends a model.
                let messages: Vec<brain_protocol::Message> = (0..=turn)
                    .map(|_| brain_protocol::Message::user_text("x".repeat(MESSAGE_BYTES)))
                    .collect();
                Ok(ActivationOutput {
                    context,
                    decision: Decision::Model {
                        request: ModelRequest {
                            system: None,
                            tools: None,
                            messages,
                            response_format: None,
                            max_output_tokens: Some(16),
                        },
                    },
                })
            }
            _ => Ok(ActivationOutput {
                context,
                decision: Decision::Finish {
                    result: Some(serde_json::json!({"ok": true})),
                },
            }),
        }
    }
}

/// A model request carries the whole conversation, so journalling it whole on every
/// decision writes the transcript again each turn and the journal grows with the square of
/// the turn count. Measured on a real conversation before this: 5.02 MiB at 250 turns,
/// 18.85 at 500 and 72.99 at 1000 — 3.8x per doubling.
///
/// The journal may hold the conversation a constant number of times over. It may not hold
/// it once per turn.
#[tokio::test]
async fn a_session_does_not_journal_the_whole_transcript_once_per_turn() {
    const TURNS: usize = 48;
    /// Measured at `TURNS = 48`: the journal was 49.4x the transcript when both the
    /// activation record and the model call record carried the whole request, 25.8x with one of
    /// them fixed, and is 1.2x with neither. The bound sits between the regimes.
    const MAX_RATIO: u64 = 6;

    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        4,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(ResendingLoop {
            turns: AtomicUsize::new(0),
        }),
        Arc::new(TinyModel),
        Arc::new(NoTools),
    );
    let handle = start(&runtime, request());
    for _ in 0..TURNS {
        handle
            .message(MessageRequest { input: "go".into() })
            .await
            .unwrap();
    }
    drop(handle);
    drop(runtime);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let journal_bytes: u64 = fs::read_dir(data_dir.join("journal"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    // What the conversation itself weighs by the end.
    let transcript_bytes = (TURNS * MESSAGE_BYTES) as u64;
    let ratio = journal_bytes as f64 / transcript_bytes as f64;
    let held = journal_bytes <= transcript_bytes * MAX_RATIO;
    fs::remove_dir_all(data_dir).unwrap();

    assert!(
        held,
        "a {TURNS}-turn session journalled {journal_bytes} bytes for a {transcript_bytes}-byte \
         transcript ({ratio:.1}x, bound {MAX_RATIO}x): the runtime is recording the whole \
         conversation once per turn, so the journal grows with the square of the turn count"
    );
}

/// Deltas per model call, and the size of each.
const DELTAS: usize = 512;
const DELTA_BYTES: usize = 1024;

/// An agentloop that asks the model once and finishes.
struct AskOnce;

#[async_trait]
impl LoopExecutor for AskOnce {
    async fn activate(
        &self,
        _session: &brain_protocol::SessionId,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, Error> {
        let context = input.context;
        match input.observation {
            Observation::UserMessage { .. } => Ok(ActivationOutput {
                context,
                decision: Decision::Model {
                    request: ModelRequest {
                        system: None,
                        tools: None,
                        messages: vec![brain_protocol::Message::user_text("go")],
                        response_format: None,
                        max_output_tokens: Some(16),
                    },
                },
            }),
            _ => Ok(ActivationOutput {
                context,
                decision: Decision::Finish {
                    result: Some(serde_json::json!({"ok": true})),
                },
            }),
        }
    }
}

/// A model that says a great deal in small pieces, and returns one short answer.
struct ChattyModel;

#[async_trait]
impl ModelExecutor for ChattyModel {
    async fn execute(
        &self,
        _binding: &ModelBinding,
        _request: ModelRequest,
        _tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error> {
        for _ in 0..DELTAS {
            on_event(ModelStreamEvent::TextDelta {
                index: 0,
                text: "x".repeat(DELTA_BYTES),
            });
        }
        Ok(ModelResult {
            message: brain_protocol::Message::assistant(vec![brain_protocol::ContentBlock::text(
                "done",
            )]),
            stop_reason: brain_protocol::StopReason::EndTurn,
            usage: brain_protocol::Usage::default(),
        })
    }
}

/// Model output is streamed, not stored.
///
/// `model_call_ended` carried the whole list of deltas beside the assembled response, so a
/// turn wrote its own output twice — once in pieces and once whole — and nothing ever read
/// the pieces. A client that wants them takes them off the live stream as they arrive.
#[tokio::test]
async fn a_turn_does_not_journal_the_pieces_its_answer_arrived_in() {
    let data_dir = temporary_directory();
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        4,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(AskOnce),
        Arc::new(ChattyModel),
        Arc::new(NoTools),
    );
    let handle = start(&runtime, request());
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    drop(handle);
    drop(runtime);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let journal_bytes: u64 = fs::read_dir(data_dir.join("journal"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    let streamed_bytes = (DELTAS * DELTA_BYTES) as u64;
    let _ = fs::remove_dir_all(data_dir);

    assert!(
        journal_bytes < streamed_bytes / 4,
        "one turn journalled {journal_bytes} bytes after streaming {streamed_bytes} bytes of \
         model output: the pieces the answer arrived in are being written to disk beside the \
         answer itself"
    );
}
