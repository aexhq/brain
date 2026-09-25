use super::*;
use brain::environment::{EnvironmentAdapter, Services};
use brain_protocol::{
    EnvironmentControlRequest as Request, EnvironmentEvent, EnvironmentObservation,
    EnvironmentOperation, EnvironmentOutput, EnvironmentReceipt, EnvironmentRef,
    EnvironmentRequest,
};
use serde_json::json;
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct Adapter {
    executions: AtomicUsize,
}

#[async_trait]
impl EnvironmentAdapter for Adapter {
    async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let services = services.unwrap();
        if environment.name.as_str() != "brain" {
            let controller = services.controller().unwrap();
            let readable = controller
                .call("environments", json!({"operation":"list"}))
                .await?;
            assert_eq!(readable.as_array().unwrap().len(), 1);
            assert_eq!(readable[0]["reference"]["name"], "brain");
        }
        Ok(match operation.request {
            EnvironmentRequest::Execute { .. } => {
                self.executions.fetch_add(1, Ordering::SeqCst);
                let readable = services
                    .call("environments", json!({"operation":"list"}))
                    .await?;
                assert!(
                    readable
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|view| view["template"] == "workspace")
                );
                assert_eq!(readable.as_array().unwrap().len(), 2);
                EnvironmentReceipt::Result {
                    output: json!({"worked":true}),
                }
            }
            _ => EnvironmentReceipt::Accepted { on_turn_end: None },
        })
    }
}

#[derive(Default)]
struct Observer {
    activations: AtomicUsize,
    events: StdMutex<Vec<brain_protocol::Event>>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[tokio::test]
async fn cross_environment_deletions_settle_without_waiting_on_each_others_control_effects() {
    struct Deleter(tokio::sync::Barrier);
    #[async_trait]
    impl brain::ToolExecutor for Deleter {
        async fn execute(
            &self,
            dispatch: ToolDispatch,
            services: Arc<dyn brain::ToolServices>,
        ) -> Result<Option<Outcome>, brain::Error> {
            self.0.wait().await;
            services
                .environments(Request::Delete {
                    environment: EnvironmentRef {
                        name: EnvironmentName::new(dispatch.invocation.input.as_str().unwrap()),
                        sequence: 1,
                    },
                })
                .await?;
            Ok(Some(Outcome::Ok { value: json!(null) }))
        }
        async fn cancel(&self, _: ToolCancellation) -> Result<(), brain::Error> {
            Ok(())
        }
    }
    struct Dispatch;
    #[async_trait]
    impl LoopExecutor for Dispatch {
        async fn turn(
            &self,
            _: &SessionId,
            _: u64,
            _: &AgentloopRef,
            _: &Environment,
            input: TurnInput,
            services: Arc<dyn brain::TurnServices>,
        ) -> Result<TurnOutput, brain::Error> {
            if input.input.is_some() {
                services.dispatch(vec![
                    serde_json::from_value(json!({"name":"left","environment":"left","call_id":"left","input":"right"})).unwrap(),
                    serde_json::from_value(json!({"name":"right","environment":"right","call_id":"right","input":"left"})).unwrap(),
                ]).await?;
            }
            let page = services.events(0).await?;
            services.acknowledge(page.next_cursor).await?;
            Ok(TurnOutput::default())
        }
    }
    let root = root("cross-environment-delete");
    let mut api = api(&root);
    let registry = Arc::new(EnvironmentRegistry::new(Arc::new(Adapter::default())));
    let resources = Arc::get_mut(&mut api.resources).unwrap();
    resources.environments = registry.clone();
    let runtime = Arc::get_mut(&mut resources.session_runtime).unwrap();
    runtime.environment_control = Some(registry.clone());
    runtime.tool_executor = Arc::new(Deleter(tokio::sync::Barrier::new(2)));
    runtime.loop_executor = Arc::new(Dispatch);
    registry.bind_executions(&runtime.tool_executions);
    let mut config = session_config();
    for (name, other) in [("left", "right"), ("right", "left")] {
        config.environments.push(
            serde_json::from_value(json!({"name":name,"driver":"brain","lifecycle":"automatic",
            "environments":[{"environment":"brain","permissions":["read"]}]}))
            .unwrap(),
        );
        config.tools.push(serde_json::from_value(json!({"name":name,"description":"Delete peer","input_schema":{"type":"string"},
            "placements":{name:{"implementation":{}}},"environments":[{"environment":other,"permissions":["delete"]}]})).unwrap());
    }
    let id = SessionId::new("ses_cross_delete");
    api.create_session(id.clone(), config.clone(), vec![])
        .await
        .unwrap();
    api.send_message(
        id.clone(),
        MessageRequest {
            input: "delete".into(),
        },
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), api.drain())
        .await
        .unwrap();
    let store = api.store(&id).await.unwrap();
    let views = brain::environment::environments(&*store, &config).unwrap();
    assert_eq!(
        views[&EnvironmentName::new("left")].state,
        brain_protocol::EnvironmentState::Deleted
    );
    assert_eq!(
        views[&EnvironmentName::new("right")].state,
        brain_protocol::EnvironmentState::Deleted
    );
}

#[async_trait]
impl LoopExecutor for Observer {
    async fn turn(
        &self,
        _: &SessionId,
        _: u64,
        _: &AgentloopRef,
        _: &Environment,
        input: TurnInput,
        services: Arc<dyn brain::TurnServices>,
    ) -> Result<TurnOutput, brain::Error> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        match input.input.as_ref().map(|input| input.message.as_str()) {
            Some("configure") => {
                let result = services.dispatch(vec![serde_json::from_value(json!({"name":"work","call_id":"before","environment":"workspace","input":{}})).unwrap()]).await?;
                assert!(result[0].finished);
                assert!(
                    result[0]
                        .events
                        .iter()
                        .any(|event| event.data.to_string().contains("environment_not_ready"))
                );
                services
                    .environments(Request::Setup {
                        environment: EnvironmentRef {
                            name: EnvironmentName::new("workspace"),
                            sequence: 1,
                        },
                    })
                    .await?;
                let view = services
                    .environments(Request::Create {
                        template: EnvironmentName::new("workspace"),
                        name: EnvironmentName::new("copy"),
                        configuration: json!({}),
                    })
                    .await?;
                let reference =
                    serde_json::from_value::<EnvironmentRef>(view["reference"].clone()).unwrap();
                services
                    .environments(Request::Setup {
                        environment: reference.clone(),
                    })
                    .await?;
                let result = services.dispatch(vec![serde_json::from_value(json!({"name":"work","call_id":"after","environment":"copy","environment_sequence":reference.sequence,"input":{}})).unwrap()]).await?;
                assert!(
                    result[0]
                        .events
                        .iter()
                        .any(|event| event.data.to_string().contains("worked"))
                );
            }
            Some("wait") => {
                self.entered.notify_one();
                self.release.notified().await;
                return Ok(TurnOutput::default());
            }
            _ => {}
        }
        let mut through = input
            .kv
            .get(brain::LAST_ACTIVATION_KEY)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        loop {
            let page = services.events(through).await?;
            if page.events.is_empty() {
                break;
            }
            through = page.next_cursor;
            self.events.lock().unwrap().extend(page.events);
        }
        services.acknowledge(through).await?;
        Ok(TurnOutput::default())
    }
}

#[tokio::test]
async fn all_extension_roles_share_scoped_control_and_live_events_follow_the_agentloop_watermark() {
    let root = root("environment-control");
    let mut api = api(&root);
    let adapter = Arc::new(Adapter::default());
    let observer = Arc::new(Observer::default());
    let registry = Arc::new(EnvironmentRegistry::new(adapter.clone()));
    let resources = Arc::get_mut(&mut api.resources).unwrap();
    resources.environments = registry.clone();
    let runtime = Arc::get_mut(&mut resources.session_runtime).unwrap();
    runtime.environment_control = Some(registry.clone());
    runtime.tool_executor = Arc::new(crate::SessionToolExecutor::new(registry.clone()));
    runtime.loop_executor = observer.clone();
    runtime.limits.max_turn_secs = 5;
    registry.bind_executions(&runtime.tool_executions);
    let config = serde_json::from_value(json!({
        "agentloop":{"environment":"brain","implementation":{},"configuration":{},
            "environments":[{"environment":"workspace","permissions":["read","create","setup"]}]},
        "model":{"provider":"openai","name":"test"},
        "tools":[{"name":"work","description":"Work","input_schema":{},"placements":{"workspace":{"implementation":{}}},
            "environments":[{"environment":"workspace","permissions":["read"]}]}],
        "environments":[{"name":"brain","driver":"brain","lifecycle":"automatic"},
            {"name":"workspace","driver":"http","url":"http://example.test","lifecycle":"manual",
             "template":{"max_instances":2,"configuration_schema":{"type":"object","additionalProperties":false}},
             "environments":[{"environment":"brain","permissions":["read"]}]}]
    })).unwrap();
    let id = SessionId::new("ses_environment_control");
    api.create_session(id.clone(), config, vec![])
        .await
        .unwrap();
    assert_eq!(adapter.executions.load(Ordering::SeqCst), 0);
    api.send_message(
        id.clone(),
        MessageRequest {
            input: "configure".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(adapter.executions.load(Ordering::SeqCst), 1);
    let store = api.store(&id).await.unwrap();
    let config = brain::session_config(&*store).unwrap();
    let reference = brain::environment::environments(&*store, &config).unwrap()
        [&EnvironmentName::new("copy")]
        .reference
        .clone();
    let report = || EnvironmentEvent::Result {
        output: EnvironmentOutput {
            observation: EnvironmentObservation::Resource {
                resource: "job".into(),
                code: "stopped".into(),
                message: "inspect the worker".into(),
            },
            content: None,
        },
    };
    let idle = api
        .environment_event(id.clone(), reference.clone(), report())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.processed_through().unwrap() < idle {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(store.fold().unwrap().transcript.is_empty());
    let sending_api = api.clone();
    let sending_id = id.clone();
    let sending = tokio::spawn(async move {
        sending_api
            .send_message(
                sending_id,
                MessageRequest {
                    input: "wait".into(),
                },
            )
            .await
    });
    observer.entered.notified().await;
    let first = api
        .environment_event(id.clone(), reference.clone(), report())
        .await
        .unwrap();
    let last = api
        .environment_event(id.clone(), reference.clone(), report())
        .await
        .unwrap();
    assert!(store.processed_through().unwrap() < first);
    observer.release.notify_one();
    sending.await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.processed_through().unwrap() < last {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(observer.activations.load(Ordering::SeqCst), 4);
    api.cancel_session(id.clone()).await.unwrap();
    let suppressed = api
        .environment_event(id.clone(), reference, report())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(store.processed_through().unwrap() < suppressed);
    assert_eq!(observer.activations.load(Ordering::SeqCst), 4);
    api.send_message(
        id.clone(),
        MessageRequest {
            input: "resume".into(),
        },
    )
    .await
    .unwrap();
    assert!(store.processed_through().unwrap() >= suppressed);
    tokio::time::timeout(Duration::from_secs(2), api.drain())
        .await
        .unwrap();
    let events = observer.events.lock().unwrap();
    for sequence in [idle, first, last, suppressed] {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.sequence == sequence)
                .count(),
            1
        );
    }
    assert!(
        events
            .iter()
            .filter(|event| event.event_type == codes::event::ENVIRONMENT_OBSERVATION)
            .all(|event| matches!(
                event.origin,
                Some(brain_protocol::EventOrigin::Environment { .. })
            ))
    );
}
