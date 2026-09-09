use super::*;
use async_trait::async_trait;
use brain::LoopExecutor;
use brain_protocol::{AgentloopRef, ModelBinding};
use brain_protocol::{
    Message, ModelRequest, ModelResult, ModelStreamEvent, Outcome, ToolCancellation, ToolDispatch,
    TurnInput, TurnOutput,
};

struct Echo;

#[tokio::test]
async fn graceful_drain_keeps_turn_services_alive_and_refuses_new_work() {
    struct Held {
        entered: tokio::sync::Notify,
        finish: tokio::sync::Notify,
    }
    #[async_trait]
    impl LoopExecutor for Held {
        async fn turn(
            &self,
            _: &SessionId,
            _: u64,
            _: &AgentloopRef,
            _: &Environment,
            _: TurnInput,
            services: Arc<dyn brain::TurnServices>,
        ) -> Result<TurnOutput, brain::Error> {
            self.entered.notify_one();
            self.finish.notified().await;
            services
                .set_transcript(vec![Message::user_text("finished while draining")])
                .await?;
            services
                .kv_put(brain_protocol::KvPutRequest {
                    key: "saved".into(),
                    value: serde_json::json!(true),
                })
                .await?;
            Ok(TurnOutput::default())
        }
    }
    let root = root("drain");
    let mut api = api(&root);
    let held = Arc::new(Held {
        entered: tokio::sync::Notify::new(),
        finish: tokio::sync::Notify::new(),
    });
    Arc::get_mut(&mut Arc::get_mut(&mut api.resources).unwrap().session_runtime)
        .unwrap()
        .loop_executor = held.clone();
    let store = seed(&api, "ses_drain");
    let running_api = api.clone();
    let running = tokio::spawn(async move {
        running_api
            .send_message(
                SessionId::new("ses_drain"),
                MessageRequest { input: "go".into() },
            )
            .await
    });
    held.entered.notified().await;
    let draining_api = api.clone();
    let draining = tokio::spawn(async move { draining_api.drain().await });
    while !api.draining.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }
    assert!(!draining.is_finished());
    assert_eq!(
        api.send_message(
            SessionId::new("ses_drain"),
            MessageRequest {
                input: "later".into()
            }
        )
        .await
        .unwrap_err()
        .code,
        "overloaded"
    );
    held.finish.notify_one();
    running.await.unwrap().unwrap();
    draining.await.unwrap();
    assert_eq!(store.fold().unwrap().kv["saved"], true);
    assert_eq!(
        store.fold().unwrap().transcript,
        vec![Message::user_text("finished while draining")]
    );
}

#[async_trait]
impl LoopExecutor for Echo {
    async fn turn(
        &self,
        _: &SessionId,
        _: u64,
        _: &AgentloopRef,
        _: &Environment,
        input: TurnInput,
        services: Arc<dyn brain::TurnServices>,
    ) -> Result<TurnOutput, brain::Error> {
        let mut transcript = input.transcript;
        transcript.push(Message::user_text(input.input.message));
        services.set_transcript(transcript).await?;
        Ok(TurnOutput { result: None })
    }
}

#[async_trait]
impl brain::ModelExecutor for Echo {
    async fn execute(
        &self,
        _: &SessionId,
        _: &ModelBinding,
        _: ModelRequest,
        _: &[brain_protocol::ToolDefinition],
        _: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, brain::Error> {
        panic!("echo does not call a model")
    }
}

#[async_trait]
impl brain::ToolExecutor for Echo {
    async fn execute(
        &self,
        _: ToolDispatch,
        _: std::sync::Arc<dyn brain::ToolServices>,
    ) -> Result<Outcome, brain::Error> {
        panic!("echo does not call tools")
    }
    async fn cancel(&self, _: ToolCancellation) -> Result<(), brain::Error> {
        panic!("echo does not cancel tools")
    }
}

fn api(root: &std::path::Path) -> Sessions {
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let feed = Arc::new(Feed::new(telemetry.clone()));
    Sessions::new(SessionResources {
        sessions_dir: root.join("sessions"),
        writer: Writer::spawn(),
        feed: feed.clone(),
        session_runtime: Arc::new(SessionRuntime {
            limits: brain::Limits {
                max_model_calls: 4,
                max_turn_secs: 1,
                max_tool_secs: 1,
                ..Default::default()
            },
            loop_executor: Arc::new(Echo),
            model_executor: Arc::new(Echo),
            tool_executor: Arc::new(Echo),
            live: feed,
            telemetry,
        }),
        session_idle_ttl: None,
        environments: Arc::new(EnvironmentRegistry::new(Arc::new(UnusedEnvironment))),
    })
    .unwrap()
}
struct UnusedEnvironment;
#[async_trait]
impl brain::environment::EnvironmentAdapter for UnusedEnvironment {
    async fn execute(
        &self,
        _: &Environment,
        _: &brain_protocol::EnvironmentOperation,
        _: brain::environment::Services,
    ) -> Result<brain_protocol::EnvironmentReceipt, brain::Error> {
        panic!("seeded sessions do not call an Environment")
    }
}
fn seed(api: &Sessions, id: &str) -> Arc<LocalSessionStore> {
    let config: SessionConfig = serde_json::from_value(serde_json::json!({
        "agentloop": {"implementation": {"type": "brain_component", "entrypoint": "turn", "id": "a".repeat(64)}, "configuration": {}, "environment": "brain"},
        "model": {"provider": "openai", "name": "test"}, "system": "test", "tools": [],
        "environments": [{"name": "brain", "driver": "brain"}]
    }))
    .unwrap();
    let store = LocalSessionStore::create(
        &api.resources.sessions_dir.join(id),
        SessionId::new(id),
        &serde_json::to_value(&config).unwrap(),
        api.resources.writer.clone(),
        api.resources.feed.clone(),
    )
    .unwrap();
    let session = Session::begin(
        store.clone(),
        api.resources.session_runtime.clone(),
        &config,
        &[],
    )
    .unwrap()
    .complete(config)
    .unwrap();
    api.remember(store.clone(), session).unwrap();
    store
}

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("brain-server-{name}-{}", brain::random_id("test")))
}

#[tokio::test]
async fn startup_does_not_open_histories_and_reads_do_not_start_execution() {
    let root = root("lazy");
    let session_dir = root.join("sessions/broken/journal");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(
        session_dir.join("00000000000000000000.segment"),
        b"incomplete",
    )
    .unwrap();
    let api = api(&root);
    assert!(api.sessions.lock().unwrap().is_empty());
    assert!(api.stores.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(session_dir.join("00000000000000000000.segment")).unwrap(),
        b"incomplete"
    );
    let store = seed(&api, "ses_test");
    let weak = Arc::downgrade(&store);
    drop(store);
    let subscription = api.subscribe(&SessionId::new("ses_test"));
    assert!(
        api.send_message(
            SessionId::new("ses_test"),
            MessageRequest { input: "".into() }
        )
        .await
        .is_err()
    );
    api.send_message(
        SessionId::new("ses_test"),
        MessageRequest {
            input: "hello".into(),
        },
    )
    .await
    .unwrap();
    assert!(api.sessions.lock().unwrap().is_empty());
    tokio::time::timeout(Duration::from_secs(5), async {
        while weak.strong_count() > 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        api.transcript(SessionId::new("ses_test"))
            .await
            .unwrap()
            .messages,
        vec![Message::user_text("hello")]
    );
    assert!(
        !api.events(SessionId::new("ses_test"), None)
            .await
            .unwrap()
            .events
            .is_empty()
    );
    assert!(api.sessions.lock().unwrap().is_empty());
    api.send_message(
        SessionId::new("ses_test"),
        MessageRequest {
            input: "again".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        api.transcript(SessionId::new("ses_test"))
            .await
            .unwrap()
            .messages
            .len(),
        2
    );
    assert!(api.sessions.lock().unwrap().is_empty());
    drop(subscription);
    drop(api);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn concurrent_cold_reads_share_one_store_and_recovery_never_starts_a_turn() {
    let root = root("one-store");
    let api = api(&root);
    let store = seed(&api, "ses_test");
    api.passivate(store.session_id()).await.unwrap();
    let weak = Arc::downgrade(&store);
    drop(store);
    tokio::time::timeout(Duration::from_secs(5), async {
        while weak.strong_count() > 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let id = SessionId::new("ses_test");
    let (first, second) = tokio::join!(api.store(&id), api.store(&id));
    let (first, second) = (first.unwrap(), second.unwrap());
    assert!(Arc::ptr_eq(&first, &second));
    first
        .append_sync(
            &[brain::AppendRecord::new(
                "turn_started",
                serde_json::json!({}),
            )],
            brain::SessionUpdate {
                status: Some(SessionStatus::Running),
                configuration: None,
            },
        )
        .unwrap();
    drop((first, second));
    assert!(matches!(
        api.get_session(id.clone()).await.unwrap().status,
        SessionStatus::Idle
    ));
    let events = api.events(id, None).await.unwrap().events;
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "turn_failed" && event.data["code"] == "interrupted")
    );
    assert!(api.sessions.lock().unwrap().is_empty());
    drop(api);
    std::fs::remove_dir_all(root).unwrap();
}
