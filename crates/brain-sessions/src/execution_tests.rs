use super::*;
use brain::environment::{EnvironmentAdapter, Services};
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
    async fn emit(&self, _: String, _: Value) -> Result<u64, brain::Error> {
        unreachable!()
    }
    fn telemetry(&self, _: Value) {}
}

fn dispatch() -> ToolDispatch {
    serde_json::from_value(json!({
        "sequence":1,"session_id":"ses_test", "tool":{"name":"test","description":"Test","input_schema":{},"placements":{"remote":{"implementation":{}}}},
        "placement":{"implementation":{}}, "environment":{"name":"remote","driver":"http","url":"https://example.com"},
        "invocation":{"name":"test","environment":"remote","call_id":"one","input":{}},"deadline_ms":1000
    })).unwrap()
}

#[tokio::test]
async fn environment_errors_reach_tools_without_losing_information() {
    let executor = SessionToolExecutor::new(Arc::new(EnvironmentRegistry::new(Arc::new(Adapter(
        EnvironmentReceipt::Failure {
            code: "rate_limited".into(),
            message: "wait".into(),
            retryable: true,
            details: Some(json!({"retry_after_ms":1000})),
        },
    )))));
    let outcome = executor.execute(dispatch(), Arc::new(Leaf)).await.unwrap();
    let result = ToolResult::from_outcome("one".into(), outcome);
    assert!(result.is_error);
    assert_eq!(
        result.output,
        json!({"code":"rate_limited","message":"wait","retryable":true,"details":{"retry_after_ms":1000}})
    );
}

#[tokio::test]
async fn an_explicit_unknown_receipt_is_a_tool_outcome_not_a_transport_failure() {
    let executor = SessionToolExecutor::new(Arc::new(EnvironmentRegistry::new(Arc::new(Adapter(
        EnvironmentReceipt::Unknown {
            message: "result lost after dispatch".into(),
        },
    )))));
    assert_eq!(
        executor.execute(dispatch(), Arc::new(Leaf)).await.unwrap(),
        Outcome::Unknown {
            message: "result lost after dispatch".into()
        }
    );
}

#[tokio::test]
async fn nonterminal_execute_receipts_leave_the_effect_unknown() {
    for receipt in [
        EnvironmentReceipt::Accepted,
        EnvironmentReceipt::Progress { data: json!({}) },
    ] {
        let executor = SessionToolExecutor::new(Arc::new(EnvironmentRegistry::new(Arc::new(
            Adapter(receipt),
        ))));
        assert!(matches!(
            executor.execute(dispatch(), Arc::new(Leaf)).await,
            Err(brain::Error::Ambiguous(_))
        ));
    }
}
