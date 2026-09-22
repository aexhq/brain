mod common;

use async_trait::async_trait;
use brain::{Error, ModelExecutor};
use brain_protocol::{
    Message, MessageRequest, ModelBinding, ModelRequest, ModelResult, ModelStreamEvent, SessionId,
    ToolDefinition, Usage,
};
use brain_telemetry::telemetry_channel;
use common::{NoTools, Runtime, config, scripted, temporary_directory};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

struct ReportingModel {
    observed: Arc<Notify>,
    interrupted: bool,
}

#[async_trait]
impl ModelExecutor for ReportingModel {
    async fn execute(
        &self,
        _: &SessionId,
        _: &ModelBinding,
        _: ModelRequest,
        _: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error> {
        on_event(ModelStreamEvent::Usage {
            usage: Usage {
                total_input_tokens: Some(91),
                input_tokens: Some(91),
                output_tokens: Some(7),
                ..Default::default()
            },
        });
        for _ in 0..8192 {
            on_event(ModelStreamEvent::TextDelta {
                index: 0,
                text: "x".into(),
            });
        }
        self.observed.notify_one();
        if self.interrupted {
            std::future::pending::<()>().await;
        }
        Err(Error::Ambiguous("provider stream disconnected".into()))
    }
}

#[tokio::test]
async fn partial_usage_survives_errors_and_cancellation_without_waiting_for_observers() {
    for interrupted in [false, true] {
        let observed = Arc::new(Notify::new());
        let model = Arc::new(ReportingModel {
            observed: observed.clone(),
            interrupted,
        });
        let loop_executor = scripted(|_, services| async move {
            services
                .model(ModelRequest {
                    system: None,
                    tools: None,
                    messages: vec![Message::user_text("hello")],
                    response_format: None,
                    max_output_tokens: None,
                    options: Default::default(),
                })
                .await?;
            unreachable!()
        });
        let directory = temporary_directory("usage-observations");
        let (telemetry, _worker) = telemetry_channel();
        let runtime = Runtime::open(
            &directory,
            telemetry,
            8,
            120,
            loop_executor,
            model,
            Arc::new(NoTools),
        );
        let handle = runtime.create(&config(), &[]).unwrap();
        let _unread_subscriber = interrupted.then(|| runtime.feed.subscribe(handle.id()));
        let sending = {
            let handle = handle.clone();
            tokio::spawn(async move { handle.message(MessageRequest { input: "go".into() }).await })
        };
        tokio::time::timeout(Duration::from_secs(5), observed.notified())
            .await
            .unwrap();
        if interrupted {
            handle.cancel().await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), sending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        runtime.drain();
        let events = runtime.events(handle.id(), 0, 1000).events;
        let failed = events
            .iter()
            .find(|event| event.event_type == "model_call_failed")
            .unwrap();
        assert_eq!(failed.data["response"]["usage"]["total_input_tokens"], 91);
        assert_eq!(failed.data["response"]["usage"]["output_tokens"], 7);
        assert_eq!(failed.data["response"]["usage_complete"], false);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "model_call_started")
                .count(),
            1
        );
        drop(handle);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
