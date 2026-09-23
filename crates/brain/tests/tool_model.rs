mod common;

use async_trait::async_trait;
use brain::{Error, ModelExecutor, SessionStore, ToolExecutor, ToolServices};
use brain_protocol::{
    ModelBinding, ModelRequest, ModelResult, ModelStreamEvent, Outcome, SessionId,
    ToolCancellation, ToolDefinition, ToolDispatch, ToolInvocation, ToolOutput, TurnOutput,
};
use common::{Runtime, config, scripted, temporary_directory};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Tools(Mutex<Vec<Arc<dyn ToolServices>>>);
#[async_trait]
impl ToolExecutor for Tools {
    async fn execute(
        &self,
        _: ToolDispatch,
        services: Arc<dyn ToolServices>,
    ) -> Result<Option<Outcome>, Error> {
        self.0.lock().unwrap().push(services);
        Ok(None)
    }
    async fn cancel(&self, _: ToolCancellation) -> Result<(), Error> {
        Ok(())
    }
}

struct Models {
    concurrent: tokio::sync::Barrier,
    started: tokio::sync::Notify,
    block: bool,
}
#[async_trait]
impl ModelExecutor for Models {
    async fn execute(
        &self,
        _: &SessionId,
        binding: &ModelBinding,
        request: ModelRequest,
        tools: &[ToolDefinition],
        emit: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error> {
        assert_eq!(binding.name, "openai/test");
        assert_eq!(request.system.as_deref(), Some(""));
        assert!(tools.is_empty());
        assert!(request.response_format.is_none());
        self.concurrent.wait().await;
        emit(ModelStreamEvent::TextDelta {
            index: 0,
            text: "private Tool reasoning".into(),
        });
        emit(ModelStreamEvent::Usage {
            usage: serde_json::from_value(json!({"total_input_tokens":7,"output_tokens":3}))
                .unwrap(),
        });
        self.started.notify_one();
        if self.block {
            std::future::pending::<()>().await;
        }
        Ok(serde_json::from_value(json!({"message":request.messages[0],"stop_reason":"end_turn","usage":{"total_input_tokens":7,"output_tokens":3}})).unwrap())
    }
}

fn request(text: &str) -> ModelRequest {
    serde_json::from_value(
        json!({"messages":[{"role":"user","content":[{"type":"text","text":text}]}]}),
    )
    .unwrap()
}

async fn setup(
    count: usize,
    limit: usize,
    block: bool,
) -> (Runtime, brain::Session, Arc<Tools>, Arc<Models>) {
    let tools = Arc::new(Tools::default());
    let models = Arc::new(Models {
        concurrent: tokio::sync::Barrier::new(count),
        started: tokio::sync::Notify::new(),
        block,
    });
    let executor = scripted(move |_, services| async move {
        let calls: Vec<ToolInvocation> = (0..count).map(|i| serde_json::from_value(json!({"environment":"workspace","call_id":i.to_string(),"name":"work","input":{}})).unwrap()).collect();
        services.dispatch(calls).await?;
        services
            .set_transcript(vec![brain_protocol::Message::user_text("conversation")])
            .await?;
        Ok(TurnOutput::default())
    });
    let (telemetry, _) = brain_telemetry::telemetry_channel();
    let runtime = Runtime::open(
        &temporary_directory("tool-model"),
        telemetry,
        limit,
        0,
        executor,
        models.clone(),
        tools.clone(),
    );
    let mut config = config();
    config.response_format = Some(json!({"type":"json_object"}));
    config.tools = vec![serde_json::from_value(json!({"name":"work","description":"Work","input_schema":{"type":"object"},"output_schema":{"type":"string"},"placements":{"workspace":{"implementation":{}}}})).unwrap()];
    let session = runtime.create(&config, &[]).unwrap();
    session
        .message(brain_protocol::MessageRequest { input: "go".into() })
        .await
        .unwrap();
    (runtime, session, tools, models)
}

#[tokio::test]
async fn independent_calls_outlive_the_loop_keep_identity_and_share_its_budget() {
    let (runtime, session, tools, _) = setup(2, 2, false).await;
    let calls = tools.0.lock().unwrap().clone();
    let mut live = runtime.subscribe(session.id());
    let (a, b) = tokio::join!(
        calls[0].model(request("first")),
        calls[1].model(request("second"))
    );
    assert_eq!(a.unwrap().message, request("first").messages[0]);
    assert_eq!(b.unwrap().message, request("second").messages[0]);
    assert!(matches!(
        calls[0].model(request("excess")).await,
        Err(Error::Budget(_))
    ));
    assert!(
        calls[0]
            .result(ToolOutput {
                outcome: Outcome::Ok { value: json!(123) },
                content: Some("cannot bypass output validation".into())
            })
            .await
            .is_err()
    );
    calls[0]
        .finish(Some(ToolOutput {
            outcome: Outcome::Ok {
                value: json!("full structured value"),
            },
            content: Some("summary".into()),
        }))
        .await
        .unwrap();
    calls[1]
        .finish(Some(ToolOutput {
            outcome: Outcome::Error {
                error: serde_json::from_value(json!({"code":"tool_error","message":"details"}))
                    .unwrap(),
            },
            content: Some("explanation".into()),
        }))
        .await
        .unwrap();
    session.tools().wait().await;
    assert!(calls[0].model(request("late")).await.is_err());
    let store = runtime.store(session.id());
    let events = store.records_after(0, 100).unwrap();
    let started: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "model_call_started")
        .collect();
    assert_eq!(started.len(), 2);
    for start in started {
        assert!(matches!(
            start.clone().into_event().origin,
            Some(brain_protocol::EventOrigin::Tool { .. })
        ));
        let end = events
            .iter()
            .find(|e| e.kind == "model_call_ended" && e.payload["sequence"] == start.sequence)
            .unwrap();
        assert_eq!(
            serde_json::to_value(end.clone().into_event().origin).unwrap(),
            serde_json::to_value(start.clone().into_event().origin).unwrap()
        );
        assert_eq!(end.payload["result"]["usage"]["total_input_tokens"], 7);
    }
    let results: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "tool_result_emitted")
        .map(|e| &e.payload["result"])
        .collect();
    assert_eq!(results[0]["output"], "full structured value");
    assert_eq!(results[0]["content"], "summary");
    assert_eq!(results[1]["is_error"], true);
    assert_eq!(results[1]["content"], "explanation");
    assert_eq!(
        store.fold().unwrap().transcript,
        vec![brain_protocol::Message::user_text("conversation")]
    );
    while let Ok((_, event)) = live.try_recv() {
        if let brain_protocol::LiveEvent::Streaming(event) = event {
            assert_ne!(event.event_type, "assistant_delta");
        }
    }
}

#[tokio::test]
async fn closing_or_interrupting_a_tool_records_unknown_calls_even_after_callback_disconnect() {
    for interrupt in [false, true] {
        let (runtime, session, tools, models) = setup(1, 1, true).await;
        let service = tools.0.lock().unwrap()[0].clone();
        let background = {
            let service = service.clone();
            tokio::spawn(async move { service.model(request("slow")).await })
        };
        models.started.notified().await;
        background.abort();
        if interrupt {
            session.cancel().await.unwrap();
        } else {
            service.finish(None).await.unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), session.tools().wait())
            .await
            .unwrap();
        let records = runtime.store(session.id()).records_after(0, 100).unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|r| r.kind == "model_call_started")
                .count(),
            1
        );
        let failed = records
            .iter()
            .find(|r| r.kind == "model_call_failed")
            .unwrap();
        assert_eq!(failed.payload["ambiguous"], true);
        assert_eq!(failed.payload["response"]["usage"]["total_input_tokens"], 7);
        assert_eq!(failed.payload["response"]["usage_complete"], false);
        assert!(service.model(request("late")).await.is_err());
    }
}
