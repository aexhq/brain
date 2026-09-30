use brain_env::{
    ComponentKind, EnvLimits, HostCall, NativeEnvironment, TurnBridge, WorkerCode, WorkerPool,
};
use brain_protocol::{BrainPreparation, RuntimeEnvelope, SessionId, TurnError, TurnInput};

struct NoEffects;

#[async_trait::async_trait]
impl TurnBridge for NoEffects {
    async fn call(&self, _: HostCall) -> Result<String, TurnError> {
        panic!("preparation entered guest code");
    }
    fn cancelled(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn preparation_loads_every_worker_and_programs_keep_invocation_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let limits = EnvLimits::default();
    let pool = WorkerPool::new(
        env!("CARGO_BIN_EXE_brain-env-worker"),
        directory.path().join("run"),
        directory.path().join("packages"),
        limits.clone(),
        std::num::NonZeroUsize::new(2).unwrap(),
    );
    let path = std::env::var("BRAIN_TEST_AGENTLOOP_PACKAGE")
        .expect("diagnostic Agentloop fixture is required");
    let runtime = pool
        .admit(tokio::fs::read(path).await.unwrap())
        .await
        .unwrap();
    let program = pool
        .admit_program("new program source".into())
        .await
        .unwrap();
    let preparation = BrainPreparation {
        agentloops: vec![runtime.clone()],
        programs: vec![program.clone()],
        ..Default::default()
    };
    let (first, second) = tokio::join!(pool.prepare(&preparation), pool.prepare(&preparation));
    first.unwrap();
    second.unwrap();
    for session in ["first", "second"] {
        let input = TurnInput {
            input: Some("program-envelope".into()),
            transcript: Vec::new(),
            kv: Default::default(),
            events: Vec::new(),
            configuration: serde_json::json!({"session": session}),
            system: String::new(),
            tools: Vec::new(),
            runtime: RuntimeEnvelope::at(&SessionId::new(format!("ses_{session}")), 1),
        };
        let output = pool
            .execute(
                WorkerCode {
                    kind: ComponentKind::Agentloop,
                    digest: runtime.to_string(),
                    program: Some(program.to_string()),
                },
                NativeEnvironment::default(),
                serde_json::to_value(input).unwrap(),
                &NoEffects,
            )
            .await
            .unwrap();
        assert_eq!(output["result"]["source"], "new program source");
        assert_eq!(output["result"]["configuration"]["session"], session);
    }
    for index in 0..2 {
        let client = brain_env::WorkerClient::new(
            directory
                .path()
                .join("run")
                .join(index.to_string())
                .join("brain-env-worker.sock"),
            &limits,
        );
        let output = client
            .turn(
                runtime.clone(),
                NativeEnvironment::default(),
                TurnInput {
                    input: Some("program-envelope".into()),
                    transcript: Vec::new(),
                    kv: Default::default(),
                    events: Vec::new(),
                    configuration: serde_json::json!({"direct": index}),
                    system: String::new(),
                    tools: Vec::new(),
                    runtime: RuntimeEnvelope::at(&SessionId::new("ses_direct"), 1),
                },
                &NoEffects,
            )
            .await
            .unwrap();
        assert_eq!(output.result.unwrap()["direct"], index);
    }
    pool.shutdown().await;
    assert!(pool.prepare(&preparation).await.is_err());
}
