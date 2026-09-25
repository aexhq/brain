use super::*;
use brain::environment::{EnvironmentAdapter, Services as ExecutionContext};
use brain_protocol::{Environment, SessionConfig};

#[derive(Default)]
struct Adapter {
    requests: Mutex<Vec<EnvironmentOperation>>,
    contexts: Mutex<Vec<Arc<dyn ExecutionServices>>>,
    next: Mutex<Option<EnvironmentReceipt>>,
}

#[async_trait]
impl EnvironmentAdapter for Adapter {
    async fn execute(
        &self,
        _: &Environment,
        operation: &EnvironmentOperation,
        services: ExecutionContext,
    ) -> Result<EnvironmentReceipt, Error> {
        self.requests.lock().unwrap().push(operation.clone());
        if let Some(controller) = services.and_then(|services| services.controller()) {
            if matches!(operation.request, EnvironmentRequest::Teardown) {
                controller
                    .call("emit", json!({"event_type":"teardown_progress","data":{}}))
                    .await?;
            }
            self.contexts.lock().unwrap().push(controller);
        }
        Ok(self
            .next
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| match &operation.request {
                EnvironmentRequest::Call { input, .. } => EnvironmentReceipt::Result {
                    output: input.clone(),
                },
                _ => EnvironmentReceipt::Accepted { on_turn_end: None },
            }))
    }
}

fn fixture(
    lifecycle: &str,
) -> (
    tempfile::TempDir,
    Arc<brain::LocalSessionStore>,
    EnvironmentRegistry,
    Arc<Adapter>,
    Vec<EnvironmentGrant>,
) {
    let root = tempfile::tempdir().unwrap();
    let config: SessionConfig = serde_json::from_value(json!({
        "agentloop": {"environment":"brain", "implementation":{}, "configuration":{}},
        "model":{"provider":"openai", "name":"test"}, "tools":[],
        "environments":[
            {"name":"brain", "driver":"brain", "lifecycle":"automatic"},
            {"name":"workspace", "driver":"http", "url":"https://example.com", "lifecycle":lifecycle,
             "configuration":{"region":"eu"},
             "template":{"max_instances":2,"configuration_schema":{"type":"object","required":["region"],"properties":{"region":{"const":"eu"}},"additionalProperties":false}},
             "methods":{"inspect":{"description":"Read provider state","input_schema":{"type":"object"},"output_schema":{"type":"object"}},
                        "replace":{"effect":"replace","description":"Replace runtime","input_schema":{"type":"object"}}}}
        ]
    })).unwrap();
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let store = brain::LocalSessionStore::create(
        &root.path().join("session"),
        SessionId::new("ses_env"),
        &json!(config),
        brain::Writer::spawn(),
        Arc::new(brain::Feed::new(telemetry)),
    )
    .unwrap();
    store
        .append_sync(
            &[AppendRecord::new(
                codes::event::SESSION_CREATION_STARTED,
                json!(config),
            )],
            SessionUpdate {
                status: Some(brain_protocol::SessionStatus::Idle),
                configuration: None,
            },
        )
        .unwrap();
    let adapter = Arc::new(Adapter::default());
    let registry = EnvironmentRegistry::new(adapter.clone());
    registry.track(store.clone());
    let grants = vec![EnvironmentGrant {
        environment: EnvironmentName::new("workspace"),
        permissions: vec![
            Permission::Read,
            Permission::Create,
            Permission::Setup,
            Permission::Update,
            Permission::Delete,
            Permission::Call,
        ],
        methods: vec!["inspect".into(), "replace".into()],
    }];
    (root, store, registry, adapter, grants)
}

fn reference() -> EnvironmentRef {
    EnvironmentRef {
        name: EnvironmentName::new("workspace"),
        sequence: 1,
    }
}

#[tokio::test]
async fn manual_configuration_and_setup_use_the_same_public_service() {
    let (_root, store, registry, adapter, grants) = fixture("manual");
    let view = registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Get {
                environment: reference(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view["state"], "declared");
    assert!(adapter.requests.lock().unwrap().is_empty());
    registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Update {
                environment: reference(),
                configuration: json!({"region":"eu"}),
            },
        )
        .await
        .unwrap();
    let view = registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Setup {
                environment: reference(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view["state"], "ready");
    assert_eq!(adapter.requests.lock().unwrap().len(), 1);
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Setup {
                    environment: reference()
                }
            )
            .await
            .is_err()
    );
    let records = store.records_after(0, 100).unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == codes::event::ENVIRONMENT_SETUP_STARTED)
            .count(),
        1
    );
    let context = adapter.contexts.lock().unwrap()[0].clone();
    assert!(
        context
            .call("environments", json!({"operation":"list"}))
            .await
            .is_err(),
        "operation context closes on return"
    );
}

#[tokio::test]
async fn dynamic_instances_inherit_limits_and_stale_references_cannot_mutate_replacements() {
    let (_root, store, registry, adapter, grants) = fixture("automatic");
    let create = || Request::Create {
        template: EnvironmentName::new("workspace"),
        name: EnvironmentName::new("second"),
        configuration: json!({"region":"eu"}),
    };
    let second: EnvironmentView = serde_json::from_value(
        registry
            .control(store.clone(), None, &grants, create())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(second.state, EnvironmentState::Ready);
    assert_eq!(
        adapter.requests.lock().unwrap()[0].binding,
        Some(second.reference.clone())
    );
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Create {
                    template: EnvironmentName::new("workspace"),
                    name: EnvironmentName::new("third"),
                    configuration: json!({"region":"eu"})
                }
            )
            .await
            .is_err()
    );
    registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Delete {
                environment: second.reference.clone(),
            },
        )
        .await
        .unwrap();
    let replacement: EnvironmentView = serde_json::from_value(
        registry
            .control(store.clone(), None, &grants, create())
            .await
            .unwrap(),
    )
    .unwrap();
    assert_ne!(replacement.reference, second.reference);
    assert!(
        registry
            .control(
                store,
                None,
                &grants,
                Request::Delete {
                    environment: second.reference
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn authority_and_configuration_are_checked_before_effects() {
    let (_root, store, registry, adapter, grants) = fixture("manual");
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &[],
                Request::Setup {
                    environment: reference()
                }
            )
            .await
            .is_err()
    );
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Call {
                    environment: reference(),
                    method: "restart".into(),
                    input: json!({})
                }
            )
            .await
            .is_err()
    );
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Update {
                    environment: reference(),
                    configuration: json!({"region":"us"})
                }
            )
            .await
            .is_err()
    );
    assert!(adapter.requests.lock().unwrap().is_empty());
    assert_eq!(
        registry
            .control(store, None, &[], Request::List)
            .await
            .unwrap(),
        json!([])
    );
}

#[tokio::test]
async fn uncertain_lifecycle_survives_reopen_and_only_correlated_evidence_releases_it() {
    use brain_protocol::{
        EnvironmentEvent, EnvironmentObservation, EnvironmentOutput, EnvironmentResolution,
    };
    let (root, store, registry, adapter, grants) = fixture("manual");
    *adapter.next.lock().unwrap() = Some(EnvironmentReceipt::Unknown {
        message: "setup response lost".into(),
    });
    assert!(matches!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Setup {
                    environment: reference()
                }
            )
            .await,
        Err(Error::Ambiguous(_))
    ));
    let config = brain::session_config(&*store).unwrap();
    let view = brain::environment::environments(&*store, &config)
        .unwrap()
        .remove(&reference().name)
        .unwrap();
    assert_eq!(view.state, EnvironmentState::Unknown);
    let sequence = view.pending_operation.unwrap();
    drop(store);
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let store = brain::LocalSessionStore::open(
        &root.path().join("session"),
        brain::Writer::spawn(),
        Arc::new(brain::Feed::new(telemetry)),
    )
    .unwrap();
    registry.track(store.clone());
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Setup {
                    environment: reference()
                }
            )
            .await
            .is_err()
    );
    assert!(
        registry
            .control(
                store.clone(),
                None,
                &grants,
                Request::Delete {
                    environment: reference()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(adapter.requests.lock().unwrap().len(), 1);
    let resolution = |sequence| EnvironmentEvent::Result {
        output: EnvironmentOutput {
            observation: EnvironmentObservation::Operation {
                sequence,
                resolution: EnvironmentResolution::Ready,
            },
            content: None,
        },
    };
    assert!(
        registry
            .report(store.clone(), &reference(), resolution(sequence + 1))
            .await
            .is_err()
    );
    registry
        .report(store.clone(), &reference(), resolution(sequence))
        .await
        .unwrap();
    assert!(
        registry
            .report(store.clone(), &reference(), resolution(sequence))
            .await
            .is_err()
    );
    registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Call {
                environment: reference(),
                method: "replace".into(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    let view = brain::environment::environments(&*store, &config)
        .unwrap()
        .remove(&reference().name)
        .unwrap();
    assert_eq!(view.state, EnvironmentState::Ready);
    assert_ne!(view.reference, reference());
    let available = || EnvironmentEvent::Result {
        output: EnvironmentOutput {
            observation: EnvironmentObservation::Environment {
                availability: brain_protocol::EnvironmentAvailability::Available,
                message: "healthy".into(),
            },
            content: None,
        },
    };
    assert!(
        registry
            .report(store.clone(), &reference(), available())
            .await
            .is_err()
    );
    registry
        .report(store.clone(), &view.reference, available())
        .await
        .unwrap();
    registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Delete {
                environment: view.reference.clone(),
            },
        )
        .await
        .unwrap();
    assert!(
        registry
            .report(store, &view.reference, available())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn resource_observations_do_not_clear_environment_unavailability() {
    use brain_protocol::{
        EnvironmentAvailability, EnvironmentEvent, EnvironmentObservation, EnvironmentOutput,
    };
    let (_root, store, registry, _, _) = fixture("manual");
    for observation in [
        EnvironmentObservation::Environment {
            availability: EnvironmentAvailability::Unavailable,
            message: "sandbox stopped".into(),
        },
        EnvironmentObservation::Resource {
            resource: "tab".into(),
            code: "closed".into(),
            message: "tab closed".into(),
        },
    ] {
        registry
            .report(
                store.clone(),
                &reference(),
                EnvironmentEvent::Result {
                    output: EnvironmentOutput {
                        observation,
                        content: None,
                    },
                },
            )
            .await
            .unwrap();
    }
    let config = brain::session_config(&*store).unwrap();
    let view = brain::environment::environments(&*store, &config)
        .unwrap()
        .remove(&reference().name)
        .unwrap();
    assert_eq!(
        view.availability,
        Some(EnvironmentAvailability::Unavailable)
    );
    assert!(matches!(
        view.observation,
        Some(EnvironmentObservation::Resource { .. })
    ));
}

#[tokio::test]
async fn observations_are_attributed_bounded_and_do_not_promote_resource_failures_to_environment_failures()
 {
    let (_root, store, registry, _, grants) = fixture("manual");
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let received = wakes.clone();
    registry.on_observation(
        Some(Arc::new(move |_, _| {
            received.fetch_add(1, Ordering::SeqCst);
        })),
        512,
    );
    registry
        .report(
            store.clone(),
            &reference(),
            brain_protocol::EnvironmentEvent::Event {
                event_type: "loading".into(),
                data: json!({}),
            },
        )
        .await
        .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
    let output = brain_protocol::EnvironmentOutput {
        observation: brain_protocol::EnvironmentObservation::Resource {
            resource: "tab-1".into(),
            code: "closed".into(),
            message: "tab closed".into(),
        },
        content: None,
    };
    let sequence = registry
        .report(
            store.clone(),
            &reference(),
            brain_protocol::EnvironmentEvent::Result { output },
        )
        .await
        .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    let record = &store.records_after(sequence - 1, 1).unwrap()[0];
    assert_eq!(
        record.origin,
        Some(brain_protocol::EventOrigin::Environment {
            environment: reference()
        })
    );
    assert!(
        registry
            .report(
                store.clone(),
                &reference(),
                brain_protocol::EnvironmentEvent::Event {
                    event_type: "loading".into(),
                    data: json!("x".repeat(512))
                }
            )
            .await
            .is_err()
    );
    registry
        .control(
            store.clone(),
            None,
            &grants,
            Request::Delete {
                environment: reference(),
            },
        )
        .await
        .unwrap();
    assert!(
        registry
            .report(
                store,
                &reference(),
                brain_protocol::EnvironmentEvent::Event {
                    event_type: "loading".into(),
                    data: json!({})
                }
            )
            .await
            .is_err()
    );
}
