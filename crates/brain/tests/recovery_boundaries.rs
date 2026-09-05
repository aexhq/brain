mod common;

use brain::{LocalSessionStore, Session, SessionStore};
use brain_protocol::{MessageRequest, ModelRequest, SessionId, SessionStatus, TurnOutput};
use brain_telemetry::telemetry_channel;
use common::{NoModels, NoTools, Runtime, SlowModel, config, scripted};
use std::sync::Arc;

#[tokio::test]
async fn interrupted_ending_is_terminal_and_pending_detach_is_unknown() {
    let directory = common::temporary_directory("regression-ending");
    let (telemetry, _) = telemetry_channel();
    let runtime = Runtime::open(
        &directory,
        telemetry,
        4,
        1000,
        common::echo_loop(),
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let session = runtime.create(&config(), &[]).unwrap();
    session
        .record("session_end_started", serde_json::json!({}))
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .store(session.id())
            .session_summary()
            .unwrap()
            .status,
        SessionStatus::Ending
    ));
    session
        .record_call_started("environment_detach", &serde_json::json!({}))
        .await
        .unwrap();
    assert!(
        session
            .message(MessageRequest {
                input: "too late".into()
            })
            .await
            .is_err()
    );
    let recovered = LocalSessionStore::open_all(
        &runtime.sessions_dir(),
        runtime.writer.clone(),
        runtime.feed.clone(),
    )
    .unwrap();
    assert!(matches!(
        recovered[0].session_summary().unwrap().status,
        SessionStatus::Ended
    ));
    let records = recovered[0].records_after(0, 100).unwrap();
    assert!(
        records
            .iter()
            .any(|record| record.kind == "environment_detach_failed"
                && record.payload["ambiguous"] == true)
    );
    assert!(!recovered[0].interrupt_unfinished_turn().unwrap());
}

#[tokio::test]
async fn emitted_internal_kind_is_rejected_without_corrupting_recovery() {
    let directory = common::temporary_directory("regression-internal-kind");
    let (telemetry, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &directory,
        telemetry,
        4,
        1000,
        scripted(|input, services| async move {
            services
                .emit("state_set".into(), serde_json::json!({"ordinary":"event"}))
                .await?;
            Ok(TurnOutput {
                transcript: input.transcript,
                slots: Default::default(),
                result: None,
            })
        }),
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let session = runtime.create(&config(), &[]).unwrap();
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let store = runtime.store(session.id());
    let reopened = LocalSessionStore::open(
        store.directory(),
        runtime.writer.clone(),
        runtime.feed.clone(),
    );
    assert!(reopened.is_ok());
    assert!(
        !runtime
            .kinds(session.id())
            .iter()
            .any(|kind| kind == "state_set")
    );
}

#[tokio::test]
async fn interrupted_creation_is_failed() {
    let directory = common::temporary_directory("regression-creation");
    let (telemetry, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &directory,
        telemetry,
        4,
        1000,
        common::echo_loop(),
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let session_id = SessionId::new("ses_review");
    let store = LocalSessionStore::create(
        &runtime.sessions_dir().join(session_id.as_str()),
        session_id,
        &serde_json::to_value(config()).unwrap(),
        runtime.writer.clone(),
        runtime.feed.clone(),
    )
    .unwrap();
    let creation = Session::begin(store.clone(), runtime.config.clone(), &config(), &[]).unwrap();
    drop(creation);
    let recovered = LocalSessionStore::open_all(
        &runtime.sessions_dir(),
        runtime.writer.clone(),
        runtime.feed.clone(),
    )
    .unwrap();
    assert!(matches!(
        recovered[0].session_summary().unwrap().status,
        SessionStatus::Failed
    ));
}

#[tokio::test]
async fn wall_deadline_records_unknown_model_outcome() {
    let directory = common::temporary_directory("regression-deadline");
    let (telemetry, _worker) = telemetry_channel();
    let mut runtime = Runtime::open(
        &directory,
        telemetry,
        4,
        1000,
        scripted(|_input, services| async move {
            services
                .model(ModelRequest {
                    system: None,
                    tools: None,
                    messages: vec![brain_protocol::Message::user_text("go")],
                    response_format: None,
                    max_output_tokens: Some(16),
                })
                .await?;
            unreachable!()
        }),
        Arc::new(SlowModel),
        Arc::new(NoTools),
    );
    Arc::get_mut(&mut runtime.config).unwrap().max_turn_ms = 100;
    let session = runtime.create(&config(), &[]).unwrap();
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let kinds = runtime.kinds(session.id());
    assert!(kinds.iter().any(|k| k == "model_call_started"));
    assert!(kinds.iter().any(|k| k == "turn_failed"));
    assert!(kinds.iter().any(|k| k == "model_call_failed"));
}

#[tokio::test]
async fn model_cancellation_records_ambiguous_failure() {
    let directory = common::temporary_directory("regression-cancel");
    let (telemetry, _worker) = telemetry_channel();
    let runtime = Runtime::open(
        &directory,
        telemetry,
        4,
        1000,
        scripted(|_input, services| async move {
            services
                .model(ModelRequest {
                    system: None,
                    tools: None,
                    messages: vec![brain_protocol::Message::user_text("go")],
                    response_format: None,
                    max_output_tokens: Some(16),
                })
                .await?;
            unreachable!()
        }),
        Arc::new(SlowModel),
        Arc::new(NoTools),
    );
    let session = runtime.create(&config(), &[]).unwrap();
    let handle = session.clone();
    let running =
        tokio::spawn(async move { handle.message(MessageRequest { input: "go".into() }).await });
    loop {
        if runtime
            .kinds(session.id())
            .iter()
            .any(|k| k == "model_call_started")
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    session.cancel().await.unwrap();
    running.await.unwrap().unwrap();
    let records = runtime.store(session.id()).records_after(0, 100).unwrap();
    let failed = records
        .iter()
        .find(|r| r.kind == "model_call_failed")
        .unwrap();
    assert_eq!(
        failed.payload.get("ambiguous"),
        Some(&serde_json::json!(true))
    );
}
