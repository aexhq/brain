//! Reproduces brain#124: turn round-trip degrading as sessions accumulate.
//!
//! Pure kernel, scripted executors — if latency scales with resident session count
//! here, the cost is in the kernel or journal, not the HTTP or loophost layers.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use brain::{Kernel, KernelConfig, KernelError, LoopExecutor, ModelExecutor, ToolExecutor};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, Decision, Identity, MessageRequest,
    ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, Observation,
    OperationId, Outcome, ResolvedSessionRequest, SealedSessionConfig, ToolCancellation,
    ToolDispatch,
};
use brain_telemetry::telemetry_channel;

struct OneModelTurn;

#[async_trait]
impl LoopExecutor for OneModelTurn {
    async fn activate(
        &self,
        _session: &brain_protocol::SessionId,
        _agentloop: &AgentloopIdentity,
        input: ActivationInput,
    ) -> Result<ActivationOutput, KernelError> {
        let decision = match input.observation {
            Observation::ModelCompleted { .. } => Decision::Finish { result: None },
            _ => Decision::Model {
                request: ModelRequest {
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
        _operation_id: &OperationId,
        _request_digest: &Identity,
        _binding: &ModelBinding,
        _presentation: &ModelPresentation,
        _request: ModelRequest,
        _on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError> {
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
    async fn execute(&self, _dispatch: ToolDispatch) -> Result<Outcome, KernelError> {
        unreachable!()
    }
    async fn cancel(&self, _cancellation: ToolCancellation) -> Result<(), KernelError> {
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
        presentation: ModelPresentation {
            system: "test".into(),
            tools: Vec::new(),
            response_format: None,
        },
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
        presentation: sealed.presentation.clone(),
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
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: data_dir.clone(),
            max_decisions_per_turn: 8,
            tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
            loop_executor: Arc::new(OneModelTurn),
            model_executor: Arc::new(InstantModel),
            tool_executor: Arc::new(NoTools),
        },
        publisher,
    )
    .unwrap();

    let mut created = 0_u64;
    for checkpoint in [10_u64, 250, 500, 1000, 2000, 4000] {
        // Accumulate sessions, one settled turn each, like the bench box does.
        while created < checkpoint {
            let handle = kernel
                .begin_session(&resolved(&sealed()))
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
        let probe = kernel
            .begin_session(&resolved(&sealed()))
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

    drop(kernel);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _ = std::fs::remove_dir_all(data_dir);
}
