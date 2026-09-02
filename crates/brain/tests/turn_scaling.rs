//! Reproduces brain#124: turn round-trip degrading as sessions accumulate.
//!
//! Pure runtime, scripted executors — if latency scales with resident session count
//! here, the cost is in the runtime or journal, not the HTTP or loophost layers.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use brain::{
    Error, JournalStore, LoopExecutor, ModelExecutor, ObservedJournal, Session, SessionConfig,
    ToolExecutor,
};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, Decision, MessageRequest, ModelBinding,
    ModelRequest, ModelResult, ModelStreamEvent, Observation, Outcome, ResolvedSessionRequest,
    SealedSessionConfig, SessionId, ToolCancellation, ToolDefinition, ToolDispatch,
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

struct OneModelTurn;

#[async_trait]
impl LoopExecutor for OneModelTurn {
    async fn activate(
        &self,
        _session: &brain_protocol::SessionId,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, Error> {
        let decision = match input.observation {
            Observation::ModelCompleted { .. } => Decision::Finish { result: None },
            _ => Decision::Model {
                request: ModelRequest {
                    system: None,
                    tools: None,
                    messages: vec![brain_protocol::Message::user_text("hello")],
                    response_format: None,
                    max_output_tokens: Some(16),
                },
            },
        };
        Ok(ActivationOutput {
            context: input.context,
            decision,
        })
    }
}

struct InstantModel;

#[async_trait]
impl ModelExecutor for InstantModel {
    async fn execute(
        &self,
        _binding: &ModelBinding,
        _request: ModelRequest,
        _tools: &[ToolDefinition],
        _on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error> {
        Ok(ModelResult {
            message: brain_protocol::Message::assistant(vec![brain_protocol::ContentBlock::text(
                "hi",
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
        unreachable!()
    }
    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), Error> {
        unreachable!()
    }
}

fn sealed() -> SealedSessionConfig {
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

fn resolved(sealed: &SealedSessionConfig) -> ResolvedSessionRequest {
    ResolvedSessionRequest {
        history: Vec::new(),
        agentloop_identity: sealed.agentloop_identity.clone(),
        brain_configuration: sealed.brain_configuration.clone(),
        model: sealed.model.clone(),
        system: sealed.system.clone(),
        tools: sealed.tools.clone(),
        environments: Vec::new(),
        tool_bindings: Vec::new(),
    }
}

/// Run turns at several accumulated-session counts and print the medians. Marked
/// ignored: it is a measurement, not an assertion — run with
/// `cargo test -p brain --test turn_scaling -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn turn_latency_versus_resident_sessions() {
    let data_dir = std::env::temp_dir().join(format!("brain-scaling-{}", rand::random::<u64>()));
    std::fs::create_dir(&data_dir).unwrap();
    let (publisher, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &data_dir,
        publisher,
        8,
        brain::DEFAULT_TOOL_DEADLINE_MS,
        Arc::new(OneModelTurn),
        Arc::new(InstantModel),
        Arc::new(NoTools),
    );

    let mut created = 0_u64;
    for checkpoint in [10_u64, 250, 500, 1000, 2000, 4000] {
        // Accumulate sessions, one settled turn each, like the bench box does.
        while created < checkpoint {
            let handle = Session::begin(
                runtime.store(),
                runtime.config.clone(),
                &resolved(&sealed()),
            )
            .unwrap()
            .complete(sealed())
            .unwrap();
            handle
                .message(MessageRequest {
                    input: "warm".into(),
                })
                .await
                .unwrap();
            created += 1;
        }
        // Measure fresh turns on one new session at this population.
        let probe = Session::begin(
            runtime.store(),
            runtime.config.clone(),
            &resolved(&sealed()),
        )
        .unwrap()
        .complete(sealed())
        .unwrap();
        created += 1;
        let mut samples = Vec::with_capacity(100);
        for turn in 0..100 {
            let started = Instant::now();
            probe
                .message(MessageRequest {
                    input: format!("turn {turn}").into(),
                })
                .await
                .unwrap();
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "sessions={:>5}  turn p50={:.3} ms  p90={:.3} ms  max={:.3} ms",
            created, samples[50], samples[90], samples[99],
        );
    }

    drop(runtime);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _ = std::fs::remove_dir_all(data_dir);
}
