use super::*;
use brain::{
    SessionStore,
    environment::{EnvironmentAdapter, Services},
};
use brain_protocol::ToolResult;

struct Adapter(EnvironmentReceipt);
#[async_trait]
impl EnvironmentAdapter for Adapter {
    async fn execute(
        &self,
        _: &Environment,
        _: &EnvironmentOperation,
        _: Services,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        Ok(self.0.clone())
    }
}
struct Leaf;
#[async_trait]
impl ToolServices for Leaf {
    async fn model(
        &self,
        _: brain_protocol::ModelRequest,
    ) -> Result<brain_protocol::ModelResult, brain::Error> {
        unreachable!()
    }
    async fn result(&self, _: brain_protocol::ToolOutput) -> Result<u64, brain::Error> {
        unreachable!()
    }
    async fn returned(&self, _: Option<brain_protocol::ToolOutput>) -> Result<u64, brain::Error> {
        unreachable!()
    }
    async fn finish(&self, _: Option<brain_protocol::ToolOutput>) -> Result<u64, brain::Error> {
        unreachable!()
    }
    async fn closed(&self) {
        unreachable!()
    }
    async fn emit(&self, _: String, _: Value) -> Result<u64, brain::Error> {
        unreachable!()
    }
    fn telemetry(&self, _: Value) {}
}

fn dispatch() -> ToolDispatch {
    serde_json::from_value(json!({
        "sequence":1,"session_id":"ses_test", "tool":{"name":"test","description":"Test","input_schema":{},"placements":{"remote":{"implementation":{}}}},
        "placement":{"implementation":{}}, "environment":{"name":"remote","lifecycle": "automatic", "driver": "http","url":"https://example.com"},
        "invocation":{"name":"test","environment":"remote","call_id":"one","input":{}},"deadline_ms":1000
    })).unwrap()
}

fn executor(
    receipt: EnvironmentReceipt,
) -> (
    tempfile::TempDir,
    Arc<brain::LocalSessionStore>,
    SessionToolExecutor,
) {
    let root = tempfile::tempdir().unwrap();
    let dispatch = dispatch();
    let config = json!({"agentloop":{"environment":"remote","implementation":{},"configuration":{}},
        "model":{"provider":"openai","name":"test"},"tools":[dispatch.tool],"environments":[dispatch.environment]});
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let store = brain::LocalSessionStore::create(
        &root.path().join("session"),
        dispatch.session_id,
        &config,
        brain::Writer::spawn(),
        Arc::new(brain::Feed::new(telemetry)),
    )
    .unwrap();
    store
        .append_sync(
            &[
                brain::AppendRecord::new(codes::event::SESSION_CREATION_STARTED, config),
                brain::AppendRecord::new(
                    codes::event::ENVIRONMENT_SETUP_STARTED,
                    json!({"environment":"remote"}),
                ),
                brain::AppendRecord::new(
                    codes::event::ENVIRONMENT_SETUP_ENDED,
                    json!({"sequence":2,"result":{"type":"accepted"}}),
                ),
            ],
            brain::SessionUpdate::default(),
        )
        .unwrap();
    let registry = Arc::new(EnvironmentRegistry::new(Arc::new(Adapter(receipt))));
    registry.track(store.clone());
    (root, store, SessionToolExecutor::new(registry))
}

#[tokio::test]
async fn environment_errors_reach_tools_without_losing_information() {
    let (_root, _store, executor) = executor(EnvironmentReceipt::Failure {
        code: "rate_limited".into(),
        message: "wait".into(),
        retryable: true,
        details: Some(json!({"retry_after_ms":1000})),
    });
    let outcome = executor.execute(dispatch(), Arc::new(Leaf)).await.unwrap();
    let result = ToolResult::from_outcome("one".into(), outcome.unwrap());
    assert!(result.is_error);
    assert_eq!(
        result.output,
        json!({"code":"rate_limited","message":"wait","retryable":true,"details":{"retry_after_ms":1000}})
    );
}

#[tokio::test]
async fn an_explicit_unknown_receipt_is_a_tool_outcome_not_a_transport_failure() {
    let (_root, _store, executor) = executor(EnvironmentReceipt::Unknown {
        message: "result lost after dispatch".into(),
    });
    assert_eq!(
        executor.execute(dispatch(), Arc::new(Leaf)).await.unwrap(),
        Some(Outcome::Unknown {
            message: "result lost after dispatch".into()
        })
    );
}

#[tokio::test]
async fn nonterminal_execute_receipts_leave_the_effect_unknown() {
    for receipt in [
        EnvironmentReceipt::Accepted { on_turn_end: None },
        EnvironmentReceipt::Progress { data: json!({}) },
    ] {
        let (_root, _store, executor) = executor(receipt);
        assert!(matches!(
            executor.execute(dispatch(), Arc::new(Leaf)).await,
            Err(brain::Error::Ambiguous(_))
        ));
    }
}
