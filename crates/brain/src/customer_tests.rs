use super::*;

#[test]
fn process_limit_parser_is_fail_fast_and_bounded() {
    assert_eq!(
        parse_customer_limit("TEST_LIMIT", None, 7, 1, 9).unwrap(),
        7
    );
    assert_eq!(
        parse_customer_limit("TEST_LIMIT", Some("1"), 7, 1, 9).unwrap(),
        1
    );
    assert_eq!(
        parse_customer_limit("TEST_LIMIT", Some("9"), 7, 1, 9).unwrap(),
        9
    );
    assert!(parse_customer_limit("TEST_LIMIT", Some("0"), 7, 1, 9).is_err());
    assert!(parse_customer_limit("TEST_LIMIT", Some("10"), 7, 1, 9).is_err());
    assert!(parse_customer_limit("TEST_LIMIT", Some("wat"), 7, 1, 9).is_err());
}

async fn connected(
    capacity: usize,
) -> (
    Arc<CustomerCoordinator>,
    CustomerGrant,
    mpsc::Receiver<CustomerCommand>,
    String,
    u64,
) {
    connected_with_config(
        capacity,
        CustomerTransportConfig::new(
            "ws://127.0.0.1:3210/v1/customer-hand/socket",
            "http://127.0.0.1:3210",
        )
        .unwrap(),
    )
    .await
}

async fn connected_with_config(
    capacity: usize,
    config: CustomerTransportConfig,
) -> (
    Arc<CustomerCoordinator>,
    CustomerGrant,
    mpsc::Receiver<CustomerCommand>,
    String,
    u64,
) {
    let coordinator = CustomerCoordinator::new(config, None);
    let grant = coordinator.grant("tenant", "app").await.unwrap();
    let proof = frame_proof(&grant.protocol);
    let connection_id = "conn_test".to_owned();
    coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Connect,
            connection_id: connection_id.clone(),
            request_id: "req_connect".into(),
            route_key: "$connect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: Some(grant.protocol.clone()),
            body: None,
        })
        .await
        .unwrap();
    let (sender, mut receiver) = mpsc::channel(capacity);
    coordinator
        .bind_local_sender(&connection_id, sender)
        .await
        .unwrap();
    coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Message,
            connection_id: connection_id.clone(),
            request_id: "req_register".into(),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                serde_json::json!({
                    "type":"register", "client_id":"app", "process_id":"process:test",
                    "proof": proof
                })
                .to_string(),
            ),
        })
        .await
        .unwrap();
    let CustomerCommand::Ready { epoch } = receiver.recv().await.unwrap() else {
        panic!("ready")
    };
    coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Message,
            connection_id: connection_id.clone(),
            request_id: "req_tools".into(),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                serde_json::json!({
                    "type":"register_tools", "epoch":epoch, "batch_id":"batch_test",
                    "proof": proof,
                    "registrations":[{
                        "registration":"lookup", "name":"lookup",
                        "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }]
                })
                .to_string(),
            ),
        })
        .await
        .unwrap();
    assert!(matches!(
        receiver.recv().await,
        Some(CustomerCommand::Registered { .. })
    ));
    (coordinator, grant, receiver, connection_id, epoch)
}

#[tokio::test]
async fn local_offer_receipt_terminal_and_ack_round_trip() {
    let (coordinator, grant, mut receiver, _, epoch) = connected(4).await;
    let running = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            coordinator
                .execute(
                    "tenant",
                    "app",
                    1,
                    "ses_test",
                    "op_test",
                    "lookup",
                    "lookup",
                    &"a".repeat(64),
                    serde_json::json!({"id":7}),
                    crate::wall_ms() + 5_000,
                    CancellationToken::new(),
                )
                .await
        })
    };
    let CustomerCommand::Offer(offer) = receiver.recv().await.unwrap() else {
        panic!("offer")
    };
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Receipt {
                epoch,
                operation_id: offer.operation_id.clone(),
                request_digest: offer.request_digest.clone(),
                replayed: false,
            },
        )
        .await
        .unwrap();
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Terminal {
                epoch,
                operation_id: offer.operation_id,
                request_digest: offer.request_digest,
                ok: true,
                output: Some(serde_json::json!({"ok":true})),
                error: None,
            },
        )
        .await
        .unwrap();
    let outcome = running.await.unwrap();
    assert_eq!(outcome.outcome.outcome, "completed");
    assert_eq!(outcome.outcome.value, Some(serde_json::json!({"ok":true})));
    assert!(
        receiver.try_recv().is_err(),
        "terminal is not acked before commit"
    );
    coordinator
        .acknowledge_terminal(outcome.terminal_receipt.as_ref().unwrap())
        .await
        .unwrap();
    let Some(CustomerCommand::Ack {
        operation_id,
        request_digest,
        terminal_digest,
        ..
    }) = receiver.recv().await
    else {
        panic!("ack")
    };
    let receipt = outcome.terminal_receipt.unwrap();
    assert_eq!(operation_id, receipt.operation_id);
    assert_eq!(request_digest, receipt.request_digest);
    assert_eq!(terminal_digest, receipt.terminal_digest);
}

#[tokio::test]
async fn delivered_offer_without_receipt_is_resent_once_to_the_same_operation() {
    let (coordinator, grant, mut receiver, _, epoch) = connected(4).await;
    let running = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            coordinator
                .execute(
                    "tenant",
                    "app",
                    1,
                    "ses_test",
                    "op_retry",
                    "lookup",
                    "lookup",
                    &"a".repeat(64),
                    serde_json::json!({"id":9}),
                    crate::wall_ms() + 5_000,
                    CancellationToken::new(),
                )
                .await
        })
    };
    let Some(CustomerCommand::Offer(first)) = receiver.recv().await else {
        panic!("first offer")
    };
    let Some(CustomerCommand::Offer(second)) =
        tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("bounded receipt wait triggers replacement send")
    else {
        panic!("replacement offer")
    };
    assert_eq!(first.operation_id, second.operation_id);
    assert_eq!(first.request_digest, second.request_digest);
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Receipt {
                epoch,
                operation_id: second.operation_id.clone(),
                request_digest: second.request_digest.clone(),
                replayed: true,
            },
        )
        .await
        .unwrap();
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Terminal {
                epoch,
                operation_id: second.operation_id,
                request_digest: second.request_digest,
                ok: true,
                output: Some(serde_json::json!({"ok":true})),
                error: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(running.await.unwrap().outcome.outcome, "completed");
}

#[tokio::test]
async fn strict_submit_retry_zero_sends_once_and_reports_unknown_without_receipt() {
    let (coordinator, _, mut receiver, _, _) = connected(2).await;
    let running = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            coordinator
                .execute(
                    "tenant",
                    "app",
                    0,
                    "ses_test",
                    "op_strict",
                    "lookup",
                    "lookup",
                    &"a".repeat(64),
                    serde_json::json!({}),
                    crate::wall_ms() + 5_000,
                    CancellationToken::new(),
                )
                .await
        })
    };
    assert!(matches!(
        receiver.recv().await,
        Some(CustomerCommand::Offer(_))
    ));
    let outcome = tokio::time::timeout(Duration::from_secs(1), running)
        .await
        .expect("strict classification is bounded")
        .unwrap();
    assert_eq!(outcome.outcome.outcome, "interrupted");
    assert!(outcome.outcome.content.contains("unknown"));
    assert!(
        receiver.try_recv().is_err(),
        "strict mode sent a replacement"
    );
}

#[tokio::test]
async fn ambiguous_ack_delivery_retains_the_exact_terminal_until_retry() {
    let (coordinator, grant, mut receiver, connection_id, epoch) = connected(1).await;
    let running = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            coordinator
                .execute(
                    "tenant",
                    "app",
                    0,
                    "ses_test",
                    "op_ack_retry",
                    "lookup",
                    "lookup",
                    &"a".repeat(64),
                    serde_json::json!({}),
                    crate::wall_ms() + 5_000,
                    CancellationToken::new(),
                )
                .await
        })
    };
    let Some(CustomerCommand::Offer(offer)) = receiver.recv().await else {
        panic!("offer")
    };
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Receipt {
                epoch,
                operation_id: offer.operation_id.clone(),
                request_digest: offer.request_digest.clone(),
                replayed: false,
            },
        )
        .await
        .unwrap();
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Terminal {
                epoch,
                operation_id: offer.operation_id,
                request_digest: offer.request_digest,
                ok: true,
                output: Some(serde_json::json!({"kept":true})),
                error: None,
            },
        )
        .await
        .unwrap();
    let execution = running.await.unwrap();
    let receipt = execution.terminal_receipt.unwrap();
    assert_eq!(execution.outcome.outcome, "completed");

    assert_eq!(
        coordinator
            .deliver(CustomerDeliveryRequest {
                connection_id,
                command: CustomerCommand::Heartbeat {
                    epoch,
                    nonce: "fill".into(),
                },
            })
            .await
            .unwrap(),
        CustomerDelivery::Delivered
    );
    assert!(coordinator.acknowledge_terminal(&receipt).await.is_err());
    {
        let state = coordinator.state.lock().await;
        assert!(state.pending_operations.contains_key("op_ack_retry"));
        assert_eq!(
            state.pending_terminal_bytes,
            CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES
        );
    }
    assert!(matches!(
        receiver.recv().await,
        Some(CustomerCommand::Heartbeat { .. })
    ));
    coordinator.acknowledge_terminal(&receipt).await.unwrap();
    assert!(matches!(
        receiver.recv().await,
        Some(CustomerCommand::Ack { .. })
    ));
    let state = coordinator.state.lock().await;
    assert!(!state.pending_operations.contains_key("op_ack_retry"));
    assert_eq!(state.pending_terminal_bytes, 0);
}

#[tokio::test]
async fn local_delivery_is_bounded_and_an_oversized_offer_is_rejected_before_admission() {
    let (coordinator, _, mut receiver, connection_id, epoch) = connected(1).await;
    assert_eq!(
        coordinator
            .deliver(CustomerDeliveryRequest {
                connection_id,
                command: CustomerCommand::Heartbeat {
                    epoch,
                    nonce: "fills-queue".into()
                },
            })
            .await
            .unwrap(),
        CustomerDelivery::Delivered
    );
    let saturated = coordinator
        .execute(
            "tenant",
            "app",
            0,
            "ses_test",
            "op_saturated",
            "lookup",
            "lookup",
            &"a".repeat(64),
            serde_json::json!({}),
            crate::wall_ms() + 1_000,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(saturated.outcome.outcome, "interrupted");
    let _ = receiver.recv().await;

    let oversized = coordinator
        .execute(
            "tenant",
            "app",
            0,
            "ses_test",
            "op_large",
            "lookup",
            "lookup",
            &"a".repeat(64),
            serde_json::json!({"value":"x".repeat(MAX_CUSTOMER_WS_FRAME_BYTES)}),
            crate::wall_ms() + 1_000,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(oversized.outcome.outcome, "failed");
    assert!(oversized.outcome.content.contains("24 KiB"));
    assert!(
        receiver.try_recv().is_err(),
        "oversized offer was never queued"
    );
}

#[tokio::test]
async fn observation_url_never_contains_secret_and_id_token_pairs_cannot_be_swapped() {
    let coordinator = CustomerCoordinator::new(
        CustomerTransportConfig::new(
            "wss://example.test/v1/customer-hand/socket",
            "https://example.test",
        )
        .unwrap(),
        None,
    );
    let first = coordinator.grant("tenant", "app").await.unwrap();
    let second = coordinator.grant("tenant", "app").await.unwrap();
    assert!(first.observation_url.ends_with(&first.grant_id));
    assert!(!first.observation_url.contains(&first.observation_token));
    assert_ne!(first.grant_id, first.observation_token);
    let observation = CustomerObservation::Receipt {
        epoch: 1,
        operation_id: "op_missing".into(),
        request_digest: "a".repeat(64),
        replayed: false,
    };
    assert!(
        coordinator
            .observation(&first.grant_id, &second.observation_token, observation)
            .await
            .is_err(),
        "a token minted for another grant id was accepted"
    );
}

#[tokio::test]
async fn a_spoofed_later_gateway_frame_cannot_mutate_a_proven_connection() {
    let (coordinator, _, mut receiver, connection_id, epoch) = connected(4).await;
    let before = coordinator
        .state
        .lock()
        .await
        .connections
        .values()
        .next()
        .unwrap()
        .registrations
        .len();
    let result = coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Message,
            connection_id,
            request_id: "spoof".into(),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                serde_json::json!({
                    "type":"register_tools", "epoch":epoch, "batch_id":"evil",
                    "proof":"0".repeat(64),
                    "registrations":[{
                        "registration":"evil", "name":"evil",
                        "contract_digest":"b".repeat(64)
                    }]
                })
                .to_string(),
            ),
        })
        .await;
    assert!(result.is_err());
    let after = coordinator
        .state
        .lock()
        .await
        .connections
        .values()
        .next()
        .unwrap()
        .registrations
        .len();
    assert_eq!(before, after);
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn reconnect_storm_keeps_exactly_one_connection_and_reverse_key() {
    let (coordinator, _, _, _, _) = connected(4).await;
    for index in 0..64 {
        let grant = coordinator.grant("tenant", "app").await.unwrap();
        let proof = frame_proof(&grant.protocol);
        let connection_id = format!("conn_reconnect_{index}");
        coordinator
            .receive(CustomerGatewayInput {
                route: CustomerGatewayRoute::Connect,
                connection_id: connection_id.clone(),
                request_id: format!("connect_{index}"),
                route_key: "$connect".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: Some(grant.protocol),
                body: None,
            })
            .await
            .unwrap();
        let (sender, mut receiver) = mpsc::channel(1);
        coordinator
            .bind_local_sender(&connection_id, sender)
            .await
            .unwrap();
        coordinator
            .receive(CustomerGatewayInput {
                route: CustomerGatewayRoute::Message,
                connection_id,
                request_id: format!("register_{index}"),
                route_key: "$default".into(),
                source_ip: "127.0.0.1".into(),
                subprotocol: None,
                body: Some(
                    serde_json::json!({
                        "type":"register", "client_id":"app",
                        "process_id":"process:test", "proof":proof
                    })
                    .to_string(),
                ),
            })
            .await
            .unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(CustomerCommand::Ready { .. })
        ));
    }
    let state = coordinator.state.lock().await;
    assert_eq!(state.connections.len(), 1);
    assert_eq!(state.connection_keys.len(), 1);
    assert_eq!(state.registration_bytes, 0);
    assert!(state.pending_connections.is_empty());
    assert!(state.local_senders.is_empty());
}

#[tokio::test]
async fn expired_unregistered_connect_prunes_its_local_sender() {
    let coordinator = CustomerCoordinator::new(
        CustomerTransportConfig::new(
            "ws://127.0.0.1:3210/v1/customer-hand/socket",
            "http://127.0.0.1:3210",
        )
        .unwrap(),
        None,
    );
    let grant = coordinator.grant("tenant", "app").await.unwrap();
    coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Connect,
            connection_id: "conn_abandoned".into(),
            request_id: "connect_abandoned".into(),
            route_key: "$connect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: Some(grant.protocol),
            body: None,
        })
        .await
        .unwrap();
    let (sender, _) = mpsc::channel(1);
    coordinator
        .bind_local_sender("conn_abandoned", sender)
        .await
        .unwrap();
    let mut state = coordinator.state.lock().await;
    state
        .pending_connections
        .get_mut("conn_abandoned")
        .unwrap()
        .claims
        .expires_at_ms = 0;
    coordinator.prune(&mut state, crate::wall_ms());
    assert!(state.pending_connections.is_empty());
    assert!(state.local_senders.is_empty());
}

#[tokio::test]
async fn register_tools_batch_conflict_is_atomic() {
    let (coordinator, grant, mut receiver, connection_id, epoch) = connected(4).await;
    let before = {
        let state = coordinator.state.lock().await;
        let connection = state.connections.values().next().unwrap();
        (
            connection.registrations.len(),
            connection.registration_bytes,
        )
    };
    let result = coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Message,
            connection_id,
            request_id: "batch_conflict".into(),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                serde_json::json!({
                    "type":"register_tools", "epoch":epoch, "batch_id":"conflict",
                    "proof":frame_proof(&grant.protocol),
                    "registrations":[
                        {"registration":"new", "name":"new", "contract_digest":"b".repeat(64)},
                        {"registration":"lookup", "name":"different", "contract_digest":"a".repeat(64)}
                    ]
                })
                .to_string(),
            ),
        })
        .await;
    assert!(result.is_err());
    let state = coordinator.state.lock().await;
    let connection = state.connections.values().next().unwrap();
    assert_eq!(
        (
            connection.registrations.len(),
            connection.registration_bytes
        ),
        before
    );
    assert!(!connection.registrations.contains_key("new"));
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn process_registration_byte_admission_is_atomic() {
    let lookup = CustomerRegistration {
        registration: "lookup".into(),
        name: "lookup".into(),
        contract_digest: "a".repeat(64),
    };
    let lookup_bytes = serde_json::to_vec(&lookup).unwrap().len();
    let mut config = CustomerTransportConfig::new(
        "ws://127.0.0.1:3210/v1/customer-hand/socket",
        "http://127.0.0.1:3210",
    )
    .unwrap();
    config.max_registration_bytes = lookup_bytes;
    let (coordinator, grant, mut receiver, connection_id, epoch) =
        connected_with_config(4, config).await;

    let result = coordinator
        .receive(CustomerGatewayInput {
            route: CustomerGatewayRoute::Message,
            connection_id,
            request_id: "batch_process_bytes".into(),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                serde_json::json!({
                    "type":"register_tools", "epoch":epoch, "batch_id":"process_bytes",
                    "proof":frame_proof(&grant.protocol),
                    "registrations":[{
                        "registration":"second", "name":"second",
                        "contract_digest":"b".repeat(64)
                    }]
                })
                .to_string(),
            ),
        })
        .await;
    assert!(matches!(result, Err(BrainError::Overloaded)));
    let state = coordinator.state.lock().await;
    let connection = state.connections.values().next().unwrap();
    assert_eq!(state.registration_bytes, lookup_bytes);
    assert_eq!(connection.registration_bytes, lookup_bytes);
    assert_eq!(connection.registrations.len(), 1);
    assert!(!connection.registrations.contains_key("second"));
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn retained_terminal_applies_backpressure_without_eviction() {
    let mut config = CustomerTransportConfig::new(
        "ws://127.0.0.1:3210/v1/customer-hand/socket",
        "http://127.0.0.1:3210",
    )
    .unwrap();
    config.max_pending_operations = 2;
    config.max_pending_terminal_bytes = CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES;
    let (coordinator, grant, mut receiver, _, epoch) = connected_with_config(4, config).await;
    let running = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            coordinator
                .execute(
                    "tenant",
                    "app",
                    0,
                    "ses_test",
                    "op_retained",
                    "lookup",
                    "lookup",
                    &"a".repeat(64),
                    serde_json::json!({}),
                    crate::wall_ms() + 5_000,
                    CancellationToken::new(),
                )
                .await
        })
    };
    let Some(CustomerCommand::Offer(offer)) = receiver.recv().await else {
        panic!("offer")
    };
    coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Terminal {
                epoch,
                operation_id: offer.operation_id,
                request_digest: offer.request_digest,
                ok: true,
                output: Some(serde_json::json!({"kept":true})),
                error: None,
            },
        )
        .await
        .unwrap();
    let retained = running.await.unwrap();
    assert!(retained.terminal_receipt.is_some());

    let blocked = coordinator
        .execute(
            "tenant",
            "app",
            0,
            "ses_test",
            "op_blocked",
            "lookup",
            "lookup",
            &"a".repeat(64),
            serde_json::json!({}),
            crate::wall_ms() + 5_000,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(blocked.outcome.outcome, "interrupted");
    assert!(blocked.outcome.content.contains("capacity"));
    let state = coordinator.state.lock().await;
    assert_eq!(state.pending_operations.len(), 1);
    assert!(state.pending_operations.contains_key("op_retained"));
    assert_eq!(
        state.pending_terminal_bytes,
        CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES
    );
}

#[tokio::test]
async fn terminal_capacity_is_reserved_before_an_offer_can_reach_customer_code() {
    let mut config = CustomerTransportConfig::new(
        "ws://127.0.0.1:3210/v1/customer-hand/socket",
        "http://127.0.0.1:3210",
    )
    .unwrap();
    config.max_pending_terminal_bytes = CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES - 1;
    let (coordinator, _, mut receiver, _, _) = connected_with_config(2, config).await;
    let blocked = coordinator
        .execute(
            "tenant",
            "app",
            0,
            "ses_test",
            "op_no_capacity",
            "lookup",
            "lookup",
            &"a".repeat(64),
            serde_json::json!({}),
            crate::wall_ms() + 5_000,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(blocked.outcome.outcome, "interrupted");
    assert!(blocked.outcome.content.contains("capacity"));
    assert!(
        receiver.try_recv().is_err(),
        "no offer reached customer code"
    );
    let state = coordinator.state.lock().await;
    assert!(state.pending_operations.is_empty());
    assert_eq!(state.pending_terminal_bytes, 0);
}

#[tokio::test]
async fn direct_observations_cannot_exceed_the_reserved_envelope() {
    let (coordinator, grant, _, _, epoch) = connected(2).await;
    let result = coordinator
        .observation(
            &grant.grant_id,
            &grant.observation_token,
            CustomerObservation::Terminal {
                epoch,
                operation_id: "op_oversize".into(),
                request_digest: "a".repeat(64),
                ok: true,
                output: Some(Value::String(
                    "x".repeat(MAX_CUSTOMER_HTTP_OBSERVATION_BYTES),
                )),
                error: None,
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(BrainError::FileTooLarge {
            limit: MAX_CUSTOMER_HTTP_OBSERVATION_BYTES
        })
    ));
}
