use super::*;
use brain_protocol::{
    Message, ModelRequest, ModelResult, ModelStreamEvent, Outcome, ToolCancellation, ToolDispatch,
    TurnInput, TurnOutput,
};

struct Echo;

#[async_trait]
impl LoopExecutor for Echo {
    async fn turn(
        &self,
        _: &SessionId,
        _: u64,
        _: &AgentloopRef,
        _: &Environment,
        input: TurnInput,
        _: Arc<dyn brain::TurnServices>,
    ) -> Result<TurnOutput, brain::Error> {
        let mut transcript = input.transcript;
        transcript.push(Message::user_text(input.input.message));
        Ok(TurnOutput {
            transcript,
            kv: input.kv,
            result: None,
        })
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
        _: &dyn brain::ToolServices,
    ) -> Result<Outcome, brain::Error> {
        panic!("echo does not call tools")
    }
    async fn cancel(&self, _: ToolCancellation) -> Result<(), brain::Error> {
        panic!("echo does not cancel tools")
    }
}

fn api(root: &std::path::Path) -> ServerApi {
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let feed = Arc::new(Feed::new(telemetry.clone()));
    let credentials =
        Arc::new(crate::metadata::ServerMetadata::open(&root.join("metadata")).unwrap());
    let loops = Arc::new(WorkerPool::new(
        "unused",
        root.join("run"),
        root.join("components"),
        Default::default(),
    ));
    ServerApi::new(ServerResources {
        sessions_dir: root.join("sessions"),
        writer: Writer::spawn(),
        feed: feed.clone(),
        session_runtime: Arc::new(SessionRuntime {
            max_model_calls_per_turn: 4,
            max_turn_ms: 1000,
            tool_deadline_ms: 1000,
            loop_executor: Arc::new(Echo),
            model_executor: Arc::new(Echo),
            tool_executor: Arc::new(Echo),
            live: feed,
            telemetry,
        }),
        session_idle_ttl: None,
        idempotency: IdempotencyStore::open(&root.join("requests/log"), Duration::from_secs(60))
            .unwrap(),
        loops: loops.clone(),
        turns: Arc::new(crate::Turns::default()),
        environments: Arc::new(EnvironmentRegistry::new(
            Arc::new(crate::BrainEnvironment::new(
                loops,
                Default::default(),
                root.join("native-workspaces"),
            )),
            crate::HostEnvironment::open(&root.join("hosts/log")).unwrap(),
            Arc::new(crate::HttpEnvironmentAdapter::new(
                reqwest::Client::new(),
                credentials.clone(),
                0,
            )),
        )),
        credentials,
        providers: Arc::new(brain::model::ProviderRegistry::default_set()),
    })
    .unwrap()
}

fn seed(api: &ServerApi, id: &str) -> Arc<LocalSessionStore> {
    let config: SessionConfig = serde_json::from_value(serde_json::json!({
        "agentloop": {"id": "a".repeat(64), "configuration": {}, "environment": "brain"},
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
    std::env::temp_dir().join(format!("brain-server-{name}-{}", rand::random::<u64>()))
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
            "one".into(),
            MessageRequest { input: "".into() }
        )
        .await
        .is_err()
    );
    api.send_message(
        SessionId::new("ses_test"),
        "one".into(),
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
        "two".into(),
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

#[tokio::test]
async fn unsuccessful_artifact_preparation_does_not_claim_an_agent_effect() {
    let root = root("admission");
    let api = api(&root);
    for _ in 0..2 {
        let error = api
            .admit_agentloop("prepare".into(), vec![1])
            .await
            .unwrap_err();
        assert!(!error.message.contains("already accepted"));
    }
    assert!(
        api.resources
            .idempotency
            .replay("admit_agentloop", "prepare", &vec![1_u8])
            .unwrap()
            .is_none()
    );
    drop(api);
    std::fs::remove_dir_all(root).unwrap();
}

/// A session is created through the whole path: credentials sealed, the brain env set
/// up with what is placed in it, the configuration admitted without a credential in it,
/// and everything forgotten again at delete.
#[tokio::test]
async fn a_session_is_created_set_up_and_deleted_through_its_environments() {
    let root = root("create");
    let api = api(&root);
    let request: CreateSessionRequest = serde_json::from_value(serde_json::json!({
        "agentloop": {"id": "a".repeat(64), "configuration": {}, "environment": "brain"},
        "model": {"provider": "openai", "name": "gpt-5-mini", "api_key": "model-key"},
        "tools": [],
        "environments": [
            {"name": "brain", "driver": "brain"},
            {"name": "sandbox", "driver": "http", "url": "http://127.0.0.1:1", "credential": "sandbox-key"}
        ]
    }))
    .unwrap();
    // The sandbox is unreachable, so its setup fails and the create with it: the brain
    // env set up before it is torn down, and nothing of the session's credentials stays.
    let error = api
        .create_session("first".into(), request.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, "ambiguous", "{}", error.message);
    let failed = api.list_sessions().await.unwrap().sessions;
    assert_eq!(failed.len(), 1);
    assert!(matches!(failed[0].status, SessionStatus::Failed));
    assert!(
        api.resources
            .credentials
            .model(&failed[0].session_id)
            .unwrap()
            .is_none()
    );
    let events = api
        .events(failed[0].session_id.clone(), None)
        .await
        .unwrap()
        .events;
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert!(kinds.contains(&"environment_setup_ended"), "{kinds:?}");
    assert!(kinds.contains(&"environment_setup_failed"), "{kinds:?}");
    assert!(kinds.contains(&"environment_teardown_ended"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"environment_closed"));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("sandbox-key"),
        "a credential never enters the journal"
    );

    let mut only_brain = request.clone();
    only_brain.environments.truncate(1);
    let created = api
        .create_session("second".into(), only_brain)
        .await
        .unwrap();
    assert!(matches!(created.status, SessionStatus::Idle));
    assert_eq!(
        api.resources
            .credentials
            .model(&created.session_id)
            .unwrap()
            .unwrap()
            .api_key
            .as_str(),
        "model-key"
    );
    api.end_session(created.session_id.clone(), "end".into())
        .await
        .unwrap();
    api.delete_session(created.session_id.clone(), "delete".into())
        .await
        .unwrap();
    assert!(
        api.resources
            .credentials
            .model(&created.session_id)
            .unwrap()
            .is_none()
    );
    drop(api);
    std::fs::remove_dir_all(root).unwrap();
}
