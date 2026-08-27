#![cfg(unix)]

use brain_loophost::{LoopLimits, WorkerPool};
use brain_protocol::{
    ActivationInput, ContextEnvelope, Decision, Observation, Presentation, RuntimeEnvelope,
};

#[tokio::test]
async fn real_worker_admits_and_activates_the_typescript_diagnostic_loop() {
    let package_path = std::env::var("BRAIN_TEST_AGENTLOOP_PACKAGE")
        .expect("BRAIN_TEST_AGENTLOOP_PACKAGE must name the built diagnostic package");
    let package = tokio::fs::read(package_path).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    );
    let digest = pool.admit(package).await.unwrap();
    let output = pool
        .activate(
            digest,
            ActivationInput {
                context: ContextEnvelope {
                    protocol_version: "context/v1".into(),
                    items: Vec::new(),
                    state: None,
                },
                observation: Observation::UserMessage {
                    content: serde_json::json!({"scenario":"tools"}),
                },
                presentation: Presentation {
                    bytes: Vec::new(),
                    digest: "presentation".into(),
                },
                runtime: RuntimeEnvelope {
                    logical_time_ms: 1,
                    deterministic_seed: vec![1, 2, 3],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(output.context.state.unwrap()["activations"], 1);
    match output.decision {
        Decision::Tools { calls } => {
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].name, "diagnostic");
        }
        decision => panic!("expected parallel Tool decision, got {decision:?}"),
    }
}
