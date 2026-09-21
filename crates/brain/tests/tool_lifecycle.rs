mod common;

use async_trait::async_trait;
use brain::{Error, SessionStore, ToolExecutor, ToolServices, TurnServices};
use brain_protocol::{
    Message, MessageRequest, Outcome, ToolCancellation, ToolDispatch, ToolInvocation, TurnOutput,
};
use common::{NoModels, Runtime, config, scripted, temporary_directory};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Background {
    services: Mutex<Option<Arc<dyn ToolServices>>>,
}
#[async_trait]
impl ToolExecutor for Background {
    async fn execute(
        &self,
        _: ToolDispatch,
        services: Arc<dyn ToolServices>,
    ) -> Result<Option<Outcome>, Error> {
        services
            .result(Outcome::Ok {
                value: json!("synchronous"),
            })
            .await?;
        *self.services.lock().unwrap() = Some(services);
        Ok(None)
    }
    async fn cancel(&self, _: ToolCancellation) -> Result<(), Error> {
        Ok(())
    }
}
fn tool_config() -> brain_protocol::SessionConfig {
    let mut config = config();
    config.tools =
        vec![serde_json::from_value(json!({
        "name": "stream", "description": "Stream", "input_schema": {"type":"object"},
        "output_schema": {"type":"string"}, "placements": {"workspace": {"implementation": {}}}
    })).unwrap()];
    config
}
fn call() -> ToolInvocation {
    serde_json::from_value(
        json!({"name":"stream","call_id":"call","environment":"workspace","input":{}}),
    )
    .unwrap()
}
fn runtime(
    loop_executor: Arc<dyn brain::LoopExecutor>,
    tools: Arc<dyn ToolExecutor>,
    seconds: u64,
) -> Runtime {
    let directory = temporary_directory("tool-lifecycle");
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    Runtime::open(
        &directory,
        telemetry,
        8,
        seconds,
        loop_executor,
        Arc::new(NoModels),
        tools,
    )
}
async fn acknowledge_all(services: &dyn TurnServices) -> Result<u64, Error> {
    let mut through = 0;
    loop {
        let page = services.events(through).await?;
        if page.events.is_empty() {
            break;
        }
        through = page.next_cursor;
    }
    services.acknowledge(through).await?;
    Ok(through)
}

#[tokio::test]
async fn background_results_survive_turn_return_and_completion_is_an_ordered_event() {
    let tools = Arc::new(Background::default());
    let retained = Arc::new(Mutex::new(None::<Arc<dyn TurnServices>>));
    let executor = scripted({
        let retained = retained.clone();
        move |_, services| {
            let retained = retained.clone();
            async move {
                let returned = services.dispatch(vec![call()]).await?;
                assert_eq!(returned.len(), 1);
                assert!(!returned[0].finished);
                assert_eq!(
                    returned[0]
                        .events
                        .iter()
                        .filter(|event| event.event_type == "tool_result_emitted")
                        .count(),
                    1
                );
                assert_eq!(
                    returned[0].events.last().unwrap().event_type,
                    "tool_call_returned"
                );
                services
                    .set_transcript(vec![Message::user_text("loop-selected")])
                    .await?;
                acknowledge_all(&*services).await?;
                *retained.lock().unwrap() = Some(services);
                Ok(TurnOutput::default())
            }
        }
    });
    let runtime = runtime(executor, tools.clone(), 0);
    let session = runtime.create(&tool_config(), &[]).unwrap();
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let store = runtime.store(session.id());
    let group = session.tools();
    assert!(group.is_active());
    let old_loop = retained.lock().unwrap().take().unwrap();
    let before = store.session_summary().unwrap().last_sequence;
    assert!(old_loop.emit("stale".into(), json!({})).await.is_err());
    assert!(old_loop.set_transcript(vec![]).await.is_err());
    assert!(old_loop.acknowledge(before).await.is_err());
    assert_eq!(store.session_summary().unwrap().last_sequence, before);
    drop(session);

    let tool = tools.services.lock().unwrap().take().unwrap();
    let result = tool
        .result(Outcome::Ok {
            value: json!("asynchronous"),
        })
        .await
        .unwrap();
    let finish = tool.finish(None).await.unwrap();
    assert!(finish > result);
    assert!(tool.emit("late".into(), json!({})).await.is_err());
    assert!(
        tool.result(Outcome::Ok {
            value: json!("late")
        })
        .await
        .is_err()
    );
    assert!(tool.finish(None).await.is_err());
    group.wait().await;
    assert_eq!(group.next_wakeup().await.unwrap().through, finish);
    assert!(group.next_wakeup().await.is_none());

    let records = store.records_after(before, 100).unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record.kind.as_str())
            .collect::<Vec<_>>(),
        ["tool_result_emitted", "tool_call_ended"]
    );
    assert_eq!(
        store.fold().unwrap().transcript,
        vec![Message::user_text("loop-selected")]
    );
    let reopened = brain::LocalSessionStore::open(
        store.directory(),
        runtime.writer.clone(),
        runtime.feed.clone(),
    )
    .unwrap();
    assert!(
        !reopened.interrupt_unfinished_turn().unwrap(),
        "finish must close the pending effect"
    );
    assert_eq!(reopened.fold().unwrap(), store.fold().unwrap());
}

#[tokio::test]
async fn interrupt_closes_an_idle_background_tool_and_preserves_its_first_terminal_cause() {
    let tools = Arc::new(Background::default());
    let runtime = runtime(
        scripted(|_, services| async move {
            services.dispatch(vec![call()]).await?;
            Ok(TurnOutput::default())
        }),
        tools.clone(),
        0,
    );
    let session = runtime.create(&tool_config(), &[]).unwrap();
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let tool = tools.services.lock().unwrap().take().unwrap();
    session.cancel().await.unwrap();
    session.tools().wait().await;
    assert!(tool.cancelled());
    assert!(tool.finish(None).await.is_err());
    let records = runtime.store(session.id()).records_after(0, 100).unwrap();
    let terminal = records
        .iter()
        .filter(|record| record.kind == "tool_call_ended")
        .collect::<Vec<_>>();
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0].payload["outcome"]["status"], "cancelled");
    assert!(
        records
            .iter()
            .any(|record| record.kind == "tool_cancel_ended")
    );
}

#[tokio::test]
async fn the_original_deadline_still_bounds_a_tool_after_return() {
    let tools = Arc::new(Background::default());
    let runtime = runtime(
        scripted(|_, services| async move {
            services.dispatch(vec![call()]).await?;
            Ok(TurnOutput::default())
        }),
        tools.clone(),
        1,
    );
    let session = runtime.create(&tool_config(), &[]).unwrap();
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let tool = tools.services.lock().unwrap().take().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), session.tools().wait())
        .await
        .unwrap();
    assert!(
        tool.result(Outcome::Ok {
            value: json!("late")
        })
        .await
        .is_err()
    );
    let records = runtime.store(session.id()).records_after(0, 100).unwrap();
    assert_eq!(
        records
            .iter()
            .find(|record| record.kind == "tool_call_ended")
            .unwrap()
            .payload["outcome"]["status"],
        "timeout"
    );
}

#[tokio::test]
async fn acknowledgements_are_explicit_monotonic_and_cannot_run_ahead_of_history() {
    let runtime = runtime(
        scripted(|input, services| async move {
            let last = input.events.last().unwrap().sequence;
            let committed = services.acknowledge(last).await?;
            assert!(services.acknowledge(last - 1).await.is_err());
            assert!(services.acknowledge(u64::MAX).await.is_err());
            assert_eq!(services.acknowledge(last).await?, committed);
            Ok(TurnOutput::default())
        }),
        Arc::new(Background::default()),
        0,
    );
    let session = runtime.create(&tool_config(), &[]).unwrap();
    session
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let store = runtime.store(session.id());
    let through = store.processed_through().unwrap();
    assert!(through > 0);
    assert!(through < store.session_summary().unwrap().last_sequence);
}
