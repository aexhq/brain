mod common;

use brain::{Error, LocalSessionStore, SessionStore, ToolExecutor, ToolServices};
use brain_protocol::{
    EventOrigin, KvSetRequest, Message, MessageRequest, ModelRequest, Outcome, ToolCancellation,
    ToolDispatch, ToolInvocation, TurnOutput,
};
use common::{Runtime, ScriptedModel, config, scripted, temporary_directory};
use serde_json::json;
use std::sync::Arc;

struct EmittingTool;

fn conversation() -> Vec<Message> {
    serde_json::from_value(json!([
        {"role":"user","content":[{"type":"text","text":"conversation"},{"type":"image","url":"https://example.com/image.png"}]},
        {"role":"assistant","content":[{"type":"native","format":"anthropic.messages.v1","data":{"type":"thinking","thinking":"retained","signature":"opaque"}},{"type":"text","text":"answer"}]}
    ])).unwrap()
}
#[async_trait::async_trait]
impl ToolExecutor for EmittingTool {
    async fn execute(
        &self,
        _: ToolDispatch,
        services: Arc<dyn ToolServices>,
    ) -> Result<Outcome, Error> {
        services
            .emit(
                "checkpoint".into(),
                json!({"origin": {"kind": "agentloop", "sequence": 1}, "state": "tool"}),
            )
            .await?;
        Ok(Outcome::Ok {
            value: json!("done"),
        })
    }
    async fn cancel(&self, _: ToolCancellation) -> Result<(), Error> {
        Ok(())
    }
}

#[tokio::test]
async fn inline_state_and_execution_provenance_survive_a_failed_turn_and_reopen() {
    let directory = temporary_directory("protocol-freeze");
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let runtime = Runtime::open(
        &directory,
        telemetry,
        4,
        10,
        scripted(|_, services| async move {
            services.set_transcript(conversation()).await?;
            services
                .set_kv(KvSetRequest {
                    key: "phase".into(),
                    value: json!("saved"),
                })
                .await?;
            assert!(
                services
                    .set_kv(KvSetRequest {
                        key: "brain.last_activation".into(),
                        value: json!(0)
                    })
                    .await
                    .is_err()
            );
            assert!(
                services
                    .emit("_extension_event".into(), json!({}))
                    .await
                    .is_err()
            );
            services
                .emit("checkpoint".into(), json!({"state": "loop"}))
                .await?;
            services
                .model(ModelRequest {
                    system: None,
                    tools: Some(vec![]),
                    messages: vec![Message::user_text("auxiliary")],
                    response_format: None,
                    max_output_tokens: None,
                    options: Default::default(),
                })
                .await?;
            services
                .dispatch(vec![
                    ToolInvocation {
                        environment: brain_protocol::EnvironmentName::new("workspace"),
                        name: "emit".into(),
                        call_id: "one".into(),
                        input: json!({}),
                    },
                    ToolInvocation {
                        environment: brain_protocol::EnvironmentName::new("workspace"),
                        name: "emit".into(),
                        call_id: "two".into(),
                        input: json!({}),
                    },
                ])
                .await?;
            Err(Error::Executor("loop failed after saving".into()))
        }),
        Arc::new(ScriptedModel),
        Arc::new(EmittingTool),
    );
    let mut config = config();
    config.tools = vec![serde_json::from_value(json!({"name":"emit", "description":"Emit", "input_schema":{"type":"object"}, "placements":{"workspace":{"implementation":{}, "needs":[]}}})).unwrap()];
    let session = runtime.create(&config, &[]).unwrap();
    let mut live = runtime.feed.subscribe(session.id());
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let store = runtime.store(session.id());
    let folded = store.fold().unwrap();
    assert_eq!(folded.transcript, conversation());
    assert_eq!(folded.kv["phase"], "saved");
    let records = store.records_after(0, 100).unwrap();
    let activation = records
        .iter()
        .find(|r| r.kind == "activation_started")
        .unwrap()
        .sequence;
    let checkpoints: Vec<_> = records.iter().filter(|r| r.kind == "checkpoint").collect();
    assert_eq!(checkpoints.len(), 3);
    assert_eq!(
        checkpoints[0].origin,
        Some(EventOrigin::Agentloop {
            sequence: activation
        })
    );
    let starts: Vec<_> = records
        .iter()
        .filter(|r| r.kind == "tool_call_started")
        .map(|r| r.sequence)
        .collect();
    let mut origins: Vec<_> = checkpoints[1..]
        .iter()
        .map(|r| match r.origin {
            Some(EventOrigin::Tool { sequence }) => sequence,
            _ => panic!("Tool claimed loop origin"),
        })
        .collect();
    origins.sort();
    assert_eq!(origins, starts);
    let mut live_events = Vec::new();
    while let Ok((_, event)) = live.try_recv() {
        if let brain_protocol::LiveEvent::Recorded(event) = event
            && event.event_type == "checkpoint"
        {
            live_events.push(event);
        }
    }
    assert_eq!(live_events.len(), 3);
    for event in live_events {
        assert_eq!(
            event.origin,
            records
                .iter()
                .find(|r| r.sequence == event.sequence)
                .unwrap()
                .origin
        );
    }
    let path = runtime.sessions_dir().join(session.id().as_str());
    drop(session);
    let reopened =
        LocalSessionStore::open(&path, runtime.writer.clone(), runtime.feed.clone()).unwrap();
    assert_eq!(reopened.fold().unwrap(), folded);
    let replay = reopened.records_after(0, 100).unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(records).unwrap()
    );
}

#[test]
fn turn_return_cannot_overwrite_inline_state() {
    assert!(serde_json::from_value::<TurnOutput>(json!({"transcript": [], "kv": {}})).is_err());
    assert!(serde_json::from_value::<TurnOutput>(json!({"result": "done"})).is_ok());
}
