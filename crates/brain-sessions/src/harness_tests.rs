use super::StdioHarness;
use async_trait::async_trait;
use brain::{AgentHarness, Error, TurnServices};
use brain_protocol::*;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct Services {
    values: Mutex<std::collections::BTreeMap<String, Value>>,
    calls: AtomicUsize,
    fail: AtomicBool,
    cancelled: AtomicBool,
}
#[async_trait]
impl TurnServices for Services {
    async fn events(&self, _: u64) -> Result<EventPage, Error> {
        unreachable!()
    }
    async fn set_transcript(&self, _: Vec<Message>) -> Result<u64, Error> {
        Ok(1)
    }
    async fn kv_put(&self, request: KvPutRequest) -> Result<u64, Error> {
        self.values
            .lock()
            .unwrap()
            .insert(request.key, request.value);
        Ok(1)
    }
    async fn kv_read(&self, key: String) -> Result<Option<Value>, Error> {
        Ok(self.values.lock().unwrap().get(&key).cloned())
    }
    async fn kv_delete(&self, key: String) -> Result<u64, Error> {
        self.values.lock().unwrap().remove(&key);
        Ok(1)
    }
    async fn model(&self, _: ModelRequest) -> Result<ModelResult, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::Journal("commit failed".into()));
        }
        Ok(ModelResult {
            message: Message::assistant(vec![ContentBlock::Text { text: "ok".into() }]),
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
    async fn dispatch(&self, _: Vec<ToolInvocation>) -> Result<Vec<ToolReturn>, Error> {
        unreachable!()
    }
    async fn emit(&self, _: String, _: Value) -> Result<u64, Error> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(1)
    }
    fn telemetry(&self, _: Value) {}
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
async fn start(directory: &std::path::Path, mode: &str) -> StdioHarness {
    StdioHarness::start(
        &HarnessCommand {
            command: std::env::var("BRAIN_TEST_PYTHON")
                .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into()),
            args: vec![
                format!(
                    "{}/../../tests/fixtures/harness.py",
                    env!("CARGO_MANIFEST_DIR")
                ),
                mode.into(),
            ],
            configuration: Value::Null,
        },
        directory,
        Arc::new(brain::model::ProviderRegistry::compose(vec![], &[]).unwrap()),
    )
    .await
    .unwrap()
}
async fn run(harness: &StdioHarness, services: Arc<Services>) -> Result<TurnOutput, Error> {
    let config = serde_json::from_value(
        json!({"model":{"provider":"openai","name":"fixture"},"agentloop":{"name":"fixture",},"tools":[],"environments":[]}),
    )
    .unwrap();
    let kv = services.values.lock().unwrap().clone();
    let input = serde_json::from_value(json!({"input":{"message":"test"},"transcript":[],"kv":kv,"events":[],
        "configuration":{},"system":"","tools":[],"runtime":{"logical_time_ms":1,"deterministic_seed":vec![0; 32]}})).unwrap();
    harness
        .turn(
            &SessionId::new("ses_12345678901234567890"),
            3,
            &config,
            input,
            services,
        )
        .await
}

#[tokio::test]
async fn python_harness_calls_scoped_services_and_closes_native_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let harness = start(directory.path(), "normal").await;
    let services = Arc::new(Services::default());
    assert_eq!(
        run(&harness, services.clone()).await.unwrap().result,
        Some(json!({"ok":true}))
    );
    assert_eq!(services.calls.load(Ordering::SeqCst), 1);
    assert!(services.values.lock().unwrap().is_empty());
    harness
        .close(&SessionId::new("ses_12345678901234567890"))
        .await
        .unwrap();
    run(&harness, services.clone()).await.unwrap();
    assert_eq!(services.calls.load(Ordering::SeqCst), 2);
    harness.shutdown().await;
    assert!(!harness.ready());
}

#[tokio::test]
async fn fatal_callback_cannot_be_swallowed_or_replayed_after_process_restart() {
    let directory = tempfile::tempdir().unwrap();
    let harness = start(directory.path(), "swallow").await;
    let services = Arc::new(Services::default());
    services.fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        run(&harness, services.clone()).await,
        Err(Error::Journal(_))
    ));
    assert_eq!(services.calls.load(Ordering::SeqCst), 1);
    harness.shutdown().await;
    let resumed = start(directory.path(), "swallow").await;
    assert!(matches!(
        run(&resumed, services.clone()).await,
        Err(Error::InvalidState(_))
    ));
    assert_eq!(services.calls.load(Ordering::SeqCst), 1);
    resumed.shutdown().await;
}

#[tokio::test]
async fn transport_loss_is_ambiguous_and_cancellation_is_served_during_run() {
    for mode in ["exit", "cancel"] {
        let directory = tempfile::tempdir().unwrap();
        let harness = start(directory.path(), mode).await;
        let services = Arc::new(Services::default());
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(10), run(&harness, services))
                .await
                .unwrap();
        if mode == "exit" {
            assert!(matches!(result, Err(Error::Ambiguous(_))));
            assert!(!harness.ready());
        } else {
            assert!(matches!(result, Err(Error::Cancelled(_))));
        }
        harness.shutdown().await;
    }
}
