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
                    protocol_version: "agentloop/v1".into(),
                    items: Vec::new(),
                    state: None,
                },
                observation: Observation::UserMessage {
                    content: serde_json::json!({"scenario":"tools"}),
                },
                configuration: serde_json::json!({}),
                presentation: Presentation {
                    bytes: Vec::new(),
                    identity: brain_protocol::Identity::of(&"presentation").unwrap(),
                },
                runtime: RuntimeEnvelope::at(&brain_protocol::JournalId::new("jrn_test"), 1, 0),
            },
        )
        .await
        .unwrap();
    assert_eq!(output.context.state.unwrap()["slots"][0]["activations"], 1);
    match output.decision {
        Decision::Finish { result } => assert_eq!(
            result,
            Some(serde_json::json!({"activations":1,"observation":"user_message"}))
        ),
        decision => panic!("expected terminal Brain decision, got {decision:?}"),
    }
}
