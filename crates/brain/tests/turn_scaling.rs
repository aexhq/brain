//! Reproduces brain#124: turn round-trip degrading as sessions accumulate.
//!
//! Pure runtime, scripted executors — if latency scales with resident session count
//! here, the cost is in the runtime or journal, not the HTTP or loophost layers.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use brain::{Error, LoopExecutor, ModelExecutor, ToolExecutor};
use brain_protocol::{
    ActivationInput, ActivationOutput, AgentloopIdentity, Decision, MessageRequest, ModelBinding,
    ModelRequest, ModelResult, ModelStreamEvent, Observation, Outcome, SessionConfig,
    ToolCancellation, ToolDefinition, ToolDispatch,
};
use brain_telemetry::telemetry_channel;
mod common;
use common::Runtime;

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

fn sealed() -> SessionConfig {
    SessionConfig {
        agentloop_identity: AgentloopIdentity::new("a".repeat(64)),
        brain_configuration: serde_json::json!({}),
        model: ModelBinding {
            binding_id: "gateway".into(),
            model: "openai/test".into(),
        },
        system: "test".into(),
        response_format: None,
        tools: Vec::new(),
        environments: Vec::new(),
        tool_bindings: Vec::new(),
        idle_ttl_ms: None,
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
            let handle = runtime.create(&sealed(), &[]).unwrap();
            handle
                .message(MessageRequest {
                    input: "warm".into(),
                })
                .await
                .unwrap();
            created += 1;
        }
        // Measure fresh turns on one new session at this population.
        let probe = runtime.create(&sealed(), &[]).unwrap();
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
