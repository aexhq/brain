use super::*;
use brain_protocol::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct Gate {
    entered: AtomicUsize,
    changed: Notify,
    release: Semaphore,
}
struct Services {
    gate: Arc<Gate>,
    values: Mutex<BTreeMap<String, Value>>,
}
#[async_trait]
impl TurnServices for Services {
    async fn events(&self, _: u64) -> Result<EventPage, Error> {
        unreachable!()
    }
    async fn set_transcript(&self, _: Vec<Message>) -> Result<u64, Error> {
        Ok(1)
    }
    async fn kv_put(&self, value: KvPutRequest) -> Result<u64, Error> {
        self.values.lock().unwrap().insert(value.key, value.value);
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
        self.gate.entered.fetch_add(1, Ordering::SeqCst);
        self.gate.changed.notify_one();
        self.gate.release.acquire().await.unwrap().forget();
        Ok(ModelResult {
            message: Message::assistant(vec![]),
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
    async fn dispatch(&self, _: Vec<ToolInvocation>) -> Result<Vec<ToolReturn>, Error> {
        unreachable!()
    }
    async fn emit(&self, _: String, _: Value) -> Result<u64, Error> {
        Ok(1)
    }
    fn telemetry(&self, _: Value) {}
    fn cancelled(&self) -> bool {
        false
    }
}

fn gate(permits: usize) -> Arc<Gate> {
    Arc::new(Gate {
        entered: AtomicUsize::new(0),
        changed: Notify::new(),
        release: Semaphore::new(permits),
    })
}
fn config(name: &str) -> SessionConfig {
    serde_json::from_value(json!({"agentloop":{"name":name},"model":{"provider":"openai","name":"fixture"},"tools":[],"environments":[]})).unwrap()
}
async fn start(path: &Path, names: &[&str], workers: usize, capacity: usize) -> AgentloopRegistry {
    let registrations = names.iter().map(|name| (name.to_string(), serde_json::from_value(json!({
        "command": std::env::var("BRAIN_TEST_PYTHON").unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into()),
        "args":[format!("{}/../../tests/fixtures/agentloop-pool.py", env!("CARGO_MANIFEST_DIR"))],
        "configuration":{"registration":name}, "workers":workers,"max_concurrency":capacity,
    })).unwrap())).collect();
    AgentloopRegistry::start(
        &registrations,
        path,
        Arc::new(brain::model::ProviderRegistry::compose(vec![], &[]).unwrap()),
    )
    .await
    .unwrap()
}
async fn run(
    registry: &AgentloopRegistry,
    id: usize,
    config: &SessionConfig,
    gate: Arc<Gate>,
    settings: Value,
) -> Result<Value, Error> {
    let input = serde_json::from_value(json!({"input":{"message":"test"},"transcript":[],"kv":{},"events":[],
        "configuration":settings,"system":"","tools":[],"runtime":{"logical_time_ms":1,"deterministic_seed":vec![0;32]}})).unwrap();
    registry
        .turn(
            &SessionId::new(format!("ses_{id:020}")),
            3,
            config,
            input,
            Arc::new(Services {
                gate,
                values: Mutex::new(BTreeMap::new()),
            }),
        )
        .await
        .map(|output| output.result.unwrap())
}

#[tokio::test]
async fn mixed_sessions_use_independent_workers_and_respect_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(start(directory.path(), &["pi", "codex", "opencode"], 2, 4).await);
    let gate = gate(0);
    let mut runs = tokio::task::JoinSet::new();
    for id in 0..96 {
        let registry = registry.clone();
        let gate = gate.clone();
        runs.spawn(async move {
            let name = ["pi", "codex", "opencode"][id % 3];
            let value = run(&registry, id, &config(name), gate, json!({"request":id}))
                .await
                .unwrap();
            assert_eq!(value["session"], format!("ses_{id:020}"));
            assert_eq!(value["defaults"]["registration"], name);
            assert_eq!(value["configuration"]["request"], id);
            assert_eq!(value["count"], 1);
            value["pid"].as_u64().unwrap()
        });
    }
    tokio::time::timeout(Duration::from_secs(15), async {
        while gate.entered.load(Ordering::SeqCst) < 24 {
            gate.changed.notified().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        gate.entered.load(Ordering::SeqCst),
        24,
        "six workers admit four active runs each"
    );
    gate.release.add_permits(96);
    let mut processes = BTreeSet::new();
    while let Some(value) = runs.join_next().await {
        processes.insert(value.unwrap());
    }
    assert_eq!(processes.len(), 6);
    registry.shutdown().await;
}

#[tokio::test]
async fn failed_worker_does_not_replay_or_stop_another_registration() {
    let directory = tempfile::tempdir().unwrap();
    let registry = start(directory.path(), &["pi", "codex"], 1, 2).await;
    assert!(registry.validate(&config("unknown").agentloop).is_err());
    let gate = gate(4);
    assert!(
        run(
            &registry,
            1,
            &config("pi"),
            gate.clone(),
            json!({"crash":true})
        )
        .await
        .is_err()
    );
    assert!(registry.ready());
    assert!(registry.validate(&config("pi").agentloop).is_err());
    assert!(
        run(&registry, 1, &config("pi"), gate.clone(), json!({}))
            .await
            .is_err()
    );
    assert_eq!(
        run(&registry, 2, &config("codex"), gate.clone(), json!({}))
            .await
            .unwrap()["count"],
        1
    );
    assert_eq!(gate.entered.load(Ordering::SeqCst), 1);
    registry.shutdown().await;
}

#[tokio::test]
async fn native_state_survives_worker_release_and_is_deleted_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let registry = start(directory.path(), &["pi"], 2, 2).await;
    let config = config("pi");
    let gate = gate(4);
    let session = SessionId::new(format!("ses_{:020}", 1));
    let first = run(&registry, 1, &config, gate.clone(), json!({}))
        .await
        .unwrap();
    let again = run(&registry, 1, &config, gate.clone(), json!({}))
        .await
        .unwrap();
    assert_eq!(
        first["pid"], again["pid"],
        "an open session stays with its worker"
    );
    registry.close(&session).await.unwrap();
    assert_eq!(
        run(&registry, 1, &config, gate, json!({})).await.unwrap()["count"],
        3
    );
    registry.shutdown().await;
    let reopened = start(directory.path(), &["pi"], 1, 2).await;
    reopened.delete(&session, &config).await.unwrap();
    assert!(!directory.path().join("pi").join(session.as_str()).exists());
    reopened.shutdown().await;
}
