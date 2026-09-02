#![cfg(unix)]

use std::sync::Arc;

use brain_loophost::{LoopLimits, WorkerPool};
use brain_protocol::{ActivationInput, ContextEnvelope, Decision, Observation, RuntimeEnvelope};

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
    let (output, _context_attached) = pool
        .activate(
            "ses_worker_test".into(),
            digest,
            true,
            ActivationInput {
                context: ContextEnvelope {
                    protocol_version: "agentloop/v1".into(),
                    items: Vec::new(),
                    state: None,
                },
                observation: Observation::UserMessage {
                    input: "tools scenario".into(),
                },
                configuration: serde_json::json!({}),
                system: String::new(),
                tools: Vec::new(),
                runtime: RuntimeEnvelope::at(&brain_protocol::SessionId::new("ses_test"), 1, 0),
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

/// The real ceiling was about three sessions: the supervisor admitted three activations
/// and then held a process-global mutex across the whole of each one, and the worker
/// served one connection at a time behind that. At concurrency 64, only 12 of 256 turns
/// reached the model, and everything else came back as a failed session that still
/// answered HTTP 200.
///
/// Twelve at once is comfortably past the old cap of three and inside the default of
/// sixteen, so every one of these must complete.
#[tokio::test]
async fn concurrent_activations_all_reach_the_agentloop() {
    const AT_ONCE: usize = 12;

    let package_path = std::env::var("BRAIN_TEST_AGENTLOOP_PACKAGE")
        .expect("BRAIN_TEST_AGENTLOOP_PACKAGE must name the built diagnostic package");
    let package = tokio::fs::read(package_path).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let pool = Arc::new(WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-loop-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        LoopLimits::default(),
    ));
    let digest = pool.admit(package).await.unwrap();

    let mut activations = Vec::with_capacity(AT_ONCE);
    for index in 0..AT_ONCE {
        let pool = pool.clone();
        let digest = digest.clone();
        activations.push(tokio::spawn(async move {
            pool.activate(format!("ses_{index}"), digest, true, activation(index))
                .await
                .map(|(output, _)| output)
        }));
    }

    let mut reached = 0;
    let mut refused = Vec::new();
    for activation in activations {
        match activation.await.unwrap() {
            Ok(output) => {
                assert_eq!(output.context.state.unwrap()["slots"][0]["activations"], 1);
                reached += 1;
            }
            Err(message) => refused.push(message),
        }
    }
    assert_eq!(
        reached, AT_ONCE,
        "only {reached} of {AT_ONCE} concurrent activations reached the Agentloop;          the rest were refused: {refused:?}"
    );
}

fn activation(index: usize) -> ActivationInput {
    ActivationInput {
        context: ContextEnvelope {
            protocol_version: "agentloop/v1".into(),
            items: Vec::new(),
            state: None,
        },
        observation: Observation::UserMessage {
            input: format!("tools scenario {index}").into(),
        },
        configuration: serde_json::json!({}),
        system: String::new(),
        tools: Vec::new(),
        runtime: RuntimeEnvelope::at(&brain_protocol::SessionId::new("ses_test"), 1, 0),
    }
}
