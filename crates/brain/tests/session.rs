//! One session, one turn at a time: the loop drives the turn through Brain's services,
//! every effect is journalled before it happens, and what the journal says is what a
//! client can read back.

mod common;

use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use brain::{Error, SessionStore, ToolExecutor};
use brain_protocol::{
    ContentBlock, Driver, Environment, EnvironmentName, HostId, LiveEvent, Message, MessageRequest,
    ModelRequest, Outcome, OutcomeError, SessionConfig, Tool, ToolCancellation, ToolDefinition,
    ToolDispatch, ToolInvocation, TurnOutput,
};
use brain_telemetry::telemetry_channel;
use common::{
    NoModels, NoTools, Runtime, ScriptedModel, SlowModel, config, scripted, temporary_directory,
};

fn user(text: &str) -> Message {
    Message::user_text(text)
}

async fn done(
    services: &dyn brain::TurnServices,
    transcript: Vec<Message>,
) -> Result<TurnOutput, Error> {
    services.set_transcript(transcript).await?;
    Ok(TurnOutput {
        result: Some(serde_json::json!({"ok": true})),
    })
}

fn request(messages: Vec<Message>) -> ModelRequest {
    ModelRequest {
        options: Default::default(),
        system: None,
        tools: None,
        messages,
        response_format: None,
        max_output_tokens: Some(16),
    }
}

fn invocation(name: &str, call_id: &str) -> ToolInvocation {
    ToolInvocation {
        call_id: call_id.into(),
        name: name.into(),
        input: serde_json::json!({}),
        environment: brain_protocol::EnvironmentName::new("workspace"),
    }
}

/// A configuration placing one Tool in the Agentloop's Environment.
fn tool_config(tool_name: &str) -> SessionConfig {
    let mut config = config();
    config.tools = vec![Tool {
        name: tool_name.into(),
        description: "a tool".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: None,
        placements: std::collections::BTreeMap::from([(
            EnvironmentName::new("workspace"),
            brain_protocol::ToolPlacement {
                implementation: serde_json::json!({"kind": "test"}),
            },
        )]),
    }];
    config
}

/// A configuration placing one Tool in a host env: the process that registered as the
/// host answers it, so it carries no implementation.
fn host_tool_config(tool_name: &str) -> SessionConfig {
    let mut config = config();
    config.environments.push(Environment {
        name: EnvironmentName::new("app"),
        driver: Driver::Host {
            host_id: HostId::new("host_12345678901234567890"),
        },
        configuration: serde_json::json!({}),
    });
    config.tools = vec![Tool {
        name: tool_name.into(),
        description: "answered by the application".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: None,
        placements: std::collections::BTreeMap::from([(
            EnvironmentName::new("app"),
            brain_protocol::ToolPlacement {
                implementation: serde_json::json!({"type": "host_function", "name": tool_name}),
            },
        )]),
    }];
    config
}

/// A tool executor that answers each call with a scripted outcome and remembers every
/// cancellation it was asked for.
struct OutcomeTools {
    outcome: Outcome,
    delay: Duration,
    entered: tokio::sync::Notify,
    cancelled: Mutex<Vec<u64>>,
}

#[async_trait]
impl ToolExecutor for OutcomeTools {
    async fn execute(
        &self,
        _: ToolDispatch,
        _: std::sync::Arc<dyn brain::ToolServices>,
    ) -> Result<Outcome, Error> {
        self.entered.notify_one();
        tokio::time::sleep(self.delay).await;
        Ok(self.outcome.clone())
    }
    async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), Error> {
        self.cancelled
            .lock()
            .unwrap()
            .push(cancellation.target_sequence);
        Ok(())
    }
}

fn runtime(
    data_dir: &std::path::Path,
    loop_executor: Arc<dyn brain::LoopExecutor>,
    model_executor: Arc<dyn brain::ModelExecutor>,
    tool_executor: Arc<dyn ToolExecutor>,
) -> Runtime {
    let (publisher, _worker) = telemetry_channel();
    Runtime::open(
        data_dir,
        publisher,
        8,
        120,
        loop_executor,
        model_executor,
        tool_executor,
    )
}

fn runtime_with_deadline(
    data_dir: &std::path::Path,
    loop_executor: Arc<dyn brain::LoopExecutor>,
    tool_executor: Arc<dyn ToolExecutor>,
    max_tool_secs: u64,
) -> Runtime {
    let (publisher, _worker) = telemetry_channel();
    Runtime::open(
        data_dir,
        publisher,
        8,
        max_tool_secs,
        loop_executor,
        Arc::new(NoModels),
        tool_executor,
    )
}

async fn settle(runtime: Runtime, data_dir: std::path::PathBuf) {
    runtime.drain();
    drop(runtime);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = fs::remove_dir_all(data_dir);
}

/// A model executor that looks at the feed when it is called: the `model_call_started`
/// record must already be there.
struct RecordingModel {
    seen_started: AtomicUsize,
    feed: Mutex<Option<tokio::sync::broadcast::Receiver<(brain_protocol::SessionId, LiveEvent)>>>,
}

#[async_trait]
impl brain::ModelExecutor for RecordingModel {
    async fn execute(
        &self,
        _session: &brain_protocol::SessionId,
        _binding: &brain_protocol::ModelBinding,
        _request: ModelRequest,
        _tools: &[ToolDefinition],
        _on_event: &mut (dyn FnMut(brain_protocol::ModelStreamEvent) + Send),
    ) -> Result<brain_protocol::ModelResult, Error> {
        let mut feed = self.feed.lock().unwrap();
        let receiver = feed.as_mut().unwrap();
        while let Ok((_, event)) = receiver.try_recv() {
            if let LiveEvent::Recorded(event) = event
                && event.event_type == "model_call_started"
            {
                self.seen_started.fetch_add(1, Ordering::SeqCst);
            }
        }
        Ok(brain_protocol::ModelResult {
            message: Message::assistant(vec![ContentBlock::text("ok")]),
            stop_reason: brain_protocol::StopReason::EndTurn,
            usage: Default::default(),
        })
    }
}

#[tokio::test]
async fn the_started_record_precedes_the_model_effect() {
    let data_dir = temporary_directory("started-first");
    let model = Arc::new(RecordingModel {
        seen_started: AtomicUsize::new(0),
        feed: Mutex::new(None),
    });
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        let result = services.model(request(transcript.clone())).await?;
        transcript.push(result.message);
        done(&*services, transcript).await
    });
    let runtime = runtime(&data_dir, loop_executor, model.clone(), Arc::new(NoTools));
    let handle = runtime.create(&config(), &[]).unwrap();
    *model.feed.lock().unwrap() = Some(runtime.subscribe(handle.id()));
    handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        model.seen_started.load(Ordering::SeqCst),
        1,
        "model_call_started must be on the feed before the executor runs"
    );
    let kinds = runtime.kinds(handle.id());
    let started = kinds
        .iter()
        .position(|kind| kind == "model_call_started")
        .unwrap();
    let ended = kinds
        .iter()
        .position(|kind| kind == "model_call_ended")
        .unwrap();
    assert!(started < ended);
    assert_eq!(kinds.last().unwrap(), "turn_ended");
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn cancel_interrupts_an_inflight_model_request() {
    let data_dir = temporary_directory("cancel-model");
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        let result = services.model(request(transcript.clone())).await?;
        transcript.push(result.message);
        done(&*services, transcript).await
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(SlowModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    let mut feed = runtime.subscribe(handle.id());
    let turning = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .message(MessageRequest {
                    input: "hello".into(),
                })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let (_, LiveEvent::Recorded(event)) = feed.recv().await.unwrap()
                && event.event_type == "model_call_started"
            {
                break;
            }
        }
    })
    .await
    .expect("the model call must start before cancellation");
    handle.cancel().await.unwrap();
    let summary = tokio::time::timeout(Duration::from_secs(5), turning)
        .await
        .expect("a cancelled model call must not wait for the provider")
        .unwrap()
        .unwrap();
    assert!(matches!(
        summary.status,
        brain_protocol::SessionStatus::Idle
    ));
    let events = runtime.events(handle.id(), 0, 1_000).events;
    let last = events.last().unwrap();
    assert_eq!(last.event_type, "turn_failed");
    assert_eq!(last.data["code"], "cancelled");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "model_call_failed"),
        "the abandoned model call is recorded as failed"
    );
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn cancel_forwards_inflight_tool_cancellation_to_the_environment_port() {
    let data_dir = temporary_directory("cancel-tool");
    let tools = Arc::new(OutcomeTools {
        outcome: Outcome::Ok {
            value: serde_json::json!({}),
        },
        delay: Duration::from_secs(30),
        entered: tokio::sync::Notify::new(),
        cancelled: Mutex::new(Vec::new()),
    });
    let loop_executor = scripted(|input, services| async move {
        let results = services
            .dispatch(vec![invocation("slow", "call_1")])
            .await?;
        let mut transcript = input.transcript;
        transcript.push(user(&format!("{} results", results.len())));
        done(&*services, transcript).await
    });
    let runtime = runtime_with_deadline(&data_dir, loop_executor, tools.clone(), 120);
    let handle = runtime.create(&tool_config("slow"), &[]).unwrap();
    let turning = {
        let handle = handle.clone();
        tokio::spawn(async move { handle.message(MessageRequest { input: "go".into() }).await })
    };
    tokio::time::timeout(Duration::from_secs(5), tools.entered.notified())
        .await
        .unwrap();
    handle.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), turning)
        .await
        .expect("a cancelled tool call must not wait for the environment")
        .unwrap()
        .unwrap();
    let kinds = runtime.kinds(handle.id());
    assert!(kinds.iter().any(|kind| kind == "tool_cancel_started"));
    assert!(kinds.iter().any(|kind| kind == "tool_cancel_ended"));
    assert_eq!(kinds.last().unwrap(), "turn_failed");
    assert_eq!(tools.cancelled.lock().unwrap().len(), 1);
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn wall_deadline_keeps_completed_tool_results_and_records_unknown_cancellation() {
    struct Tools;
    #[async_trait]
    impl ToolExecutor for Tools {
        async fn execute(
            &self,
            call: ToolDispatch,
            _: std::sync::Arc<dyn brain::ToolServices>,
        ) -> Result<Outcome, Error> {
            if call.invocation.call_id == "slow" {
                std::future::pending::<()>().await;
            }
            Ok(Outcome::Ok {
                value: serde_json::json!("known answer"),
            })
        }
        async fn cancel(&self, _: ToolCancellation) -> Result<(), Error> {
            Err(Error::Ambiguous("cancel response lost".into()))
        }
    }
    let data_dir = temporary_directory("deadline-parallel-tools");
    let executor = scripted(|input, services| async move {
        services
            .dispatch(vec![
                invocation("lookup", "fast"),
                invocation("lookup", "slow"),
            ])
            .await?;
        done(&*services, input.transcript).await
    });
    let mut runtime = runtime(&data_dir, executor, Arc::new(NoModels), Arc::new(Tools));
    Arc::get_mut(&mut runtime.config)
        .unwrap()
        .limits
        .max_turn_secs = 2;
    let session = runtime.create(&tool_config("lookup"), &[]).unwrap();
    let turning = tokio::spawn({
        let session = session.clone();
        async move { session.message(MessageRequest { input: "go".into() }).await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let records = runtime.store(session.id()).records_after(0, 100).unwrap();
            if records.iter().any(|record| {
                record.kind == "tool_call_ended" && record.payload["result"]["call_id"] == "fast"
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a known result must commit while its sibling is still waiting");
    tokio::time::timeout(Duration::from_secs(3), turning)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let records = runtime.store(session.id()).records_after(0, 100).unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == "tool_call_ended")
            .count(),
        2
    );
    assert!(
        records.iter().any(
            |record| record.kind == "tool_cancel_failed" && record.payload["ambiguous"] == true
        )
    );
    assert_eq!(records.last().unwrap().kind, "turn_failed");
    drop(session);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn a_subscriber_sees_model_output_while_the_turn_is_running() {
    let data_dir = temporary_directory("streaming");
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        let result = services.model(request(transcript.clone())).await?;
        transcript.push(result.message);
        done(&*services, transcript).await
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    let mut feed = runtime.subscribe(handle.id());
    handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();
    let mut streamed = 0;
    while let Ok((_, event)) = feed.try_recv() {
        if let LiveEvent::Streaming(streaming) = event {
            assert_eq!(streaming.event_type, "assistant_delta");
            streamed += 1;
        }
    }
    assert_eq!(streamed, 1, "the delta reaches subscribers");
    let kinds = runtime.kinds(handle.id());
    assert!(
        !kinds.iter().any(|kind| kind == "assistant_delta"),
        "deltas are never journalled"
    );
    drop(handle);
    settle(runtime, data_dir).await;
}

/// A session created with a transcript opens on it: the loop's first turn sees the
/// messages the caller carried forward.
#[tokio::test]
async fn a_session_can_be_created_with_a_transcript() {
    let data_dir = temporary_directory("seed");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                *seen.lock().unwrap() = input.transcript.clone();
                let mut transcript = input.transcript;
                transcript.push(user(&input.input.message));
                done(&*services, transcript).await
            }
        })
    };
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let seed = vec![user("earlier"), user("and earlier still")];
    let handle = runtime.create(&config(), &seed).unwrap();
    handle
        .message(MessageRequest {
            input: "now".into(),
        })
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), seed);
    let folded = runtime.store(handle.id()).fold().unwrap();
    assert_eq!(folded.transcript.len(), 3);
    drop(handle);
    settle(runtime, data_dir).await;
}

/// A loop may append its own records but never Brain's: the kinds a restart reads a
/// session's state out of, and the kinds Brain's services write.
#[tokio::test]
async fn a_loop_cannot_append_brains_own_kinds() {
    let data_dir = temporary_directory("reserved");
    let loop_executor = scripted(|input, services| async move {
        for kind in [
            "turn_ended",
            "session_ended",
            "model_call_started",
            "kv_set",
            "transcript_delta",
        ] {
            let refused = services.emit(kind.into(), serde_json::json!({})).await;
            assert!(refused.is_err(), "{kind} must be refused");
        }
        services
            .emit(
                "output_emitted".into(),
                serde_json::json!({"type": "assistant_message"}),
            )
            .await?;
        services
            .emit("note".into(), serde_json::json!({"text": "mine"}))
            .await?;
        done(&*services, input.transcript).await
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let kinds = runtime.kinds(handle.id());
    assert!(kinds.iter().any(|kind| kind == "output_emitted"));
    assert!(kinds.iter().any(|kind| kind == "note"));
    assert_eq!(kinds.iter().filter(|kind| *kind == "turn_ended").count(), 1);
    drop(handle);
    settle(runtime, data_dir).await;
}

/// What Brain leaves on disk, asserted rather than assumed: a session's configuration
/// and its segments, and nothing else.
#[tokio::test]
async fn the_journal_is_the_only_thing_written() {
    let data_dir = temporary_directory("files");
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        let result = services.model(request(transcript.clone())).await?;
        transcript.push(result.message);
        done(&*services, transcript).await
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    for _ in 0..10 {
        handle
            .message(MessageRequest {
                input: "hello".into(),
            })
            .await
            .unwrap();
    }
    drop(handle);
    runtime.drain();
    drop(runtime);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut found = Vec::new();
    fn walk(dir: &std::path::Path, prefix: &str, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.metadata().is_ok_and(|meta| meta.is_dir()) {
                walk(&entry.path(), &format!("{name}/"), found);
            } else {
                found.push(name);
            }
        }
    }
    walk(&data_dir, "", &mut found);
    found.sort();
    let _ = fs::remove_dir_all(&data_dir);
    assert!(
        found.iter().all(|name| name.ends_with(".segment")),
        "a session's directory holds only its canonical journal segments; found {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|name| name.contains("/journal/") && name.ends_with(".segment"))
    );
    assert!(!found.iter().any(|name| name.contains("/events/")));
}

/// The started record identifies the authorized placement used for this invocation.
#[tokio::test]
async fn a_tool_call_record_names_the_tool_and_nothing_else_about_it() {
    let data_dir = temporary_directory("tool-record");
    let tools = Arc::new(OutcomeTools {
        outcome: Outcome::Ok {
            value: serde_json::json!({}),
        },
        delay: Duration::ZERO,
        entered: tokio::sync::Notify::new(),
        cancelled: Mutex::new(Vec::new()),
    });
    let loop_executor = scripted(|input, services| async move {
        services
            .dispatch(vec![invocation("bash", "call_1")])
            .await?;
        done(&*services, input.transcript).await
    });
    let runtime = runtime_with_deadline(&data_dir, loop_executor, tools, 5);
    let handle = runtime.create(&tool_config("bash"), &[]).unwrap();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let events = runtime.events(handle.id(), 0, 1_000).events;
    let started = events
        .iter()
        .find(|event| event.event_type == "tool_call_started")
        .unwrap();
    assert_eq!(
        started.data,
        serde_json::json!({
            "tool": "bash",
            "environment": "workspace",
            "invocation": {"call_id": "call_1", "name": "bash", "environment": "workspace", "input": {}},
            "deadline_ms": 5_000,
        })
    );
    let ended = events
        .iter()
        .find(|event| event.event_type == "tool_call_ended")
        .unwrap();
    assert_eq!(ended.data["sequence"], started.sequence);
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn invoke_outcomes_map_onto_tool_results() {
    for (outcome, code) in [
        (
            Outcome::Error {
                error: OutcomeError {
                    retryable: false,
                    code: "boom".into(),
                    message: "it broke".into(),
                    details: None,
                },
            },
            Some("boom"),
        ),
        (Outcome::Timeout, Some("timeout")),
        (Outcome::Cancelled, Some("cancelled")),
        (
            Outcome::Unknown {
                message: "result lost".into(),
            },
            Some("unknown"),
        ),
        (
            Outcome::Ok {
                value: serde_json::json!({"content": "done"}),
            },
            None,
        ),
    ] {
        let data_dir = temporary_directory("outcomes");
        let tools = Arc::new(OutcomeTools {
            outcome,
            delay: Duration::ZERO,
            entered: tokio::sync::Notify::new(),
            cancelled: Mutex::new(Vec::new()),
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let loop_executor = {
            let seen = seen.clone();
            scripted(move |input, services| {
                let seen = seen.clone();
                async move {
                    let results = services
                        .dispatch(vec![invocation("tool", "call_1")])
                        .await?;
                    *seen.lock().unwrap() = results;
                    done(&*services, input.transcript).await
                }
            })
        };
        let runtime = runtime_with_deadline(&data_dir, loop_executor, tools, 5);
        let handle = runtime.create(&tool_config("tool"), &[]).unwrap();
        handle
            .message(MessageRequest { input: "go".into() })
            .await
            .unwrap();
        let results = seen.lock().unwrap().clone();
        assert_eq!(results.len(), 1);
        match code {
            Some(code) => {
                assert!(results[0].is_error);
                assert_eq!(results[0].output["code"], code);
            }
            None => {
                assert!(!results[0].is_error);
                assert_eq!(results[0].output["content"], "done");
            }
        }
        drop(handle);
        settle(runtime, data_dir).await;
    }
}

#[tokio::test]
async fn an_overdue_invoke_is_cancelled_and_recorded_as_timeout() {
    let data_dir = temporary_directory("tool-timeout");
    let tools = Arc::new(OutcomeTools {
        outcome: Outcome::Ok {
            value: serde_json::json!({}),
        },
        delay: Duration::from_secs(30),
        entered: tokio::sync::Notify::new(),
        cancelled: Mutex::new(Vec::new()),
    });
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                let results = services
                    .dispatch(vec![invocation("slow", "call_1")])
                    .await?;
                *seen.lock().unwrap() = results;
                done(&*services, input.transcript).await
            }
        })
    };
    let runtime = runtime_with_deadline(&data_dir, loop_executor, tools.clone(), 1);
    let handle = runtime.create(&tool_config("slow"), &[]).unwrap();
    let started = std::time::Instant::now();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    let results = seen.lock().unwrap().clone();
    assert!(results[0].is_error);
    assert_eq!(results[0].output["code"], "timeout");
    assert_eq!(
        tools.cancelled.lock().unwrap().len(),
        1,
        "the overdue call is cancelled where it runs"
    );
    let kinds = runtime.kinds(handle.id());
    assert!(kinds.iter().any(|kind| kind == "tool_cancel_started"));
    assert_eq!(kinds.last().unwrap(), "turn_ended");
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn a_tool_in_a_host_env_uses_the_configured_executor() {
    let data_dir = temporary_directory("host-tool");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                let results = services
                    .dispatch(vec![ToolInvocation {
                        environment: EnvironmentName::new("app"),
                        ..invocation("pick_file", "call_1")
                    }])
                    .await?;
                *seen.lock().unwrap() = results;
                done(&*services, input.transcript).await
            }
        })
    };
    let tools = Arc::new(OutcomeTools {
        outcome: Outcome::Ok {
            value: serde_json::json!({"path": "README.md"}),
        },
        delay: Duration::ZERO,
        entered: tokio::sync::Notify::new(),
        cancelled: Mutex::new(Vec::new()),
    });
    let runtime = runtime_with_deadline(&data_dir, loop_executor, tools, 5);
    let handle = runtime.create(&host_tool_config("pick_file"), &[]).unwrap();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let results = seen.lock().unwrap().clone();
    assert_eq!(results[0].output["path"], "README.md");
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn an_unanswered_host_call_times_out_and_journals_the_cancellation() {
    let data_dir = temporary_directory("host-timeout");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                let results = services
                    .dispatch(vec![ToolInvocation {
                        environment: EnvironmentName::new("app"),
                        ..invocation("pick_file", "call_1")
                    }])
                    .await?;
                *seen.lock().unwrap() = results;
                done(&*services, input.transcript).await
            }
        })
    };
    let tools = Arc::new(OutcomeTools {
        outcome: Outcome::Ok {
            value: serde_json::json!({}),
        },
        delay: Duration::from_secs(5),
        entered: tokio::sync::Notify::new(),
        cancelled: Mutex::new(Vec::new()),
    });
    let runtime = runtime_with_deadline(&data_dir, loop_executor, tools, 1);
    let handle = runtime.create(&host_tool_config("pick_file"), &[]).unwrap();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let results = seen.lock().unwrap().clone();
    assert!(results[0].is_error);
    assert_eq!(results[0].output["code"], "timeout");
    let kinds = runtime.kinds(handle.id());
    assert!(kinds.iter().any(|kind| kind == "tool_cancel_started"));
    assert_eq!(kinds.last().unwrap(), "turn_ended");
    drop(handle);
    settle(runtime, data_dir).await;
}

/// The journal holds the transcript as deltas: after two model calls that each extend
/// it, folding the journal yields exactly what the loop last sent.
#[tokio::test]
async fn the_transcript_folds_back_from_its_deltas() {
    let data_dir = temporary_directory("deltas");
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        let first = services.model(request(transcript.clone())).await?;
        transcript.push(first.message);
        transcript.push(user("and then"));
        let second = services.model(request(transcript.clone())).await?;
        transcript.push(second.message);
        services.set_transcript(transcript).await?;
        services
            .kv_put(brain_protocol::KvPutRequest {
                key: "memory".into(),
                value: serde_json::json!({"turns": 1}),
            })
            .await?;
        Ok(TurnOutput { result: None })
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();
    let folded = runtime.store(handle.id()).fold().unwrap();
    assert_eq!(folded.transcript.len(), 4);
    assert_eq!(folded.kv["memory"], serde_json::json!({"turns": 1}));
    assert!(folded.kv.contains_key(brain::LAST_ACTIVATION_KEY));
    drop(handle);
    settle(runtime, data_dir).await;
}

/// A session whose task was dropped comes back from its store and carries on: the
/// records stay dense across the gap and the next turn sees the same transcript.
#[tokio::test]
async fn a_session_resumes_from_its_store_after_its_task_is_dropped() {
    let data_dir = temporary_directory("resume");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                *seen.lock().unwrap() = input.transcript.clone();
                let mut transcript = input.transcript;
                transcript.push(user(&input.input.message));
                let result = services.model(request(transcript.clone())).await?;
                transcript.push(result.message);
                done(&*services, transcript).await
            }
        })
    };
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    let session_id = handle.id().clone();
    let first = handle
        .message(MessageRequest {
            input: "hello".into(),
        })
        .await
        .unwrap();
    drop(handle);
    tokio::time::sleep(Duration::from_millis(20)).await;

    let resumed = runtime.open_session(&session_id).unwrap();
    let second = resumed
        .message(MessageRequest {
            input: "again".into(),
        })
        .await
        .unwrap();
    assert!(second.last_sequence > first.last_sequence);
    assert_eq!(
        seen.lock().unwrap().len(),
        2,
        "the resumed turn opens on the transcript the first one left"
    );
    let events = runtime.events(&session_id, 0, 1_000).events;
    let sequences: Vec<u64> = events.iter().map(|event| event.sequence).collect();
    assert!(sequences.windows(2).all(|pair| pair[1] > pair[0]));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "turn_ended")
            .count(),
        2
    );
    drop(resumed);
    settle(runtime, data_dir).await;
}

/// What happened between turns reaches the loop: a record the host wrote while the
/// session sat idle is in the next turn's events.
#[tokio::test]
async fn events_since_the_last_activation_reach_the_loop() {
    let data_dir = temporary_directory("since");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                *seen.lock().unwrap() = input
                    .events
                    .iter()
                    .map(|event| event.event_type.clone())
                    .collect();
                done(&*services, input.transcript).await
            }
        })
    };
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    handle
        .message(MessageRequest {
            input: "one".into(),
        })
        .await
        .unwrap();
    let first: Vec<String> = seen.lock().unwrap().clone();
    assert!(first.iter().any(|kind| kind == "session_creation_ended"));
    handle
        .record(
            "environment_closed",
            serde_json::json!({"environment": "workspace"}),
        )
        .await
        .unwrap();
    handle
        .message(MessageRequest {
            input: "two".into(),
        })
        .await
        .unwrap();
    let second: Vec<String> = seen.lock().unwrap().clone();
    assert!(second.iter().any(|kind| kind == "environment_closed"));
    assert!(!second.iter().any(|kind| kind == "session_creation_ended"));
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn one_activation_can_read_more_than_the_initial_event_page() {
    let data_dir = temporary_directory("event-pages");
    let loop_executor = scripted(|input, services| async move {
        let mut events = input.events;
        loop {
            let after = events.last().map_or(0, |event| event.sequence);
            let page = services.events(after).await?;
            if page.events.is_empty() {
                break;
            }
            events.extend(page.events);
        }
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "last_observation")
        );
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        done(&*services, input.transcript).await
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    let mut records = (0..1100)
        .map(|_| brain::AppendRecord::new("observation", serde_json::json!({})))
        .collect::<Vec<_>>();
    records.push(brain::AppendRecord::new(
        "last_observation",
        serde_json::json!({}),
    ));
    runtime
        .store(handle.id())
        .append_sync(&records, brain::SessionUpdate::default())
        .unwrap();
    handle
        .message(MessageRequest {
            input: "read history".into(),
        })
        .await
        .unwrap();
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn transcript_replacement_reaches_the_live_feed_and_next_activation() {
    let data_dir = temporary_directory("transcript-replaced-event");
    let turn = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let turn = turn.clone();
        let seen = seen.clone();
        scripted(move |input, services| {
            let turn = turn.fetch_add(1, Ordering::SeqCst);
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(
                    input
                        .events
                        .iter()
                        .map(|event| event.event_type.clone())
                        .collect::<Vec<_>>(),
                );
                if turn == 0 {
                    done(&*services, vec![user("summary")]).await
                } else {
                    done(&*services, input.transcript).await
                }
            }
        })
    };
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime
        .create(&config(), &[user("old one"), user("old two")])
        .unwrap();
    let mut live = runtime.subscribe(handle.id());
    handle
        .message(MessageRequest {
            input: "compact".into(),
        })
        .await
        .unwrap();
    let mut replacement_was_live = false;
    while let Ok((_, event)) = live.try_recv() {
        if matches!(event, LiveEvent::Recorded(event) if event.event_type == "transcript_replaced")
        {
            replacement_was_live = true;
        }
    }
    assert!(replacement_was_live);

    handle
        .message(MessageRequest {
            input: "continue".into(),
        })
        .await
        .unwrap();
    assert!(
        seen.lock().unwrap()[1]
            .iter()
            .any(|kind| kind == "transcript_replaced")
    );
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn a_bounded_event_page_does_not_skip_the_rest() {
    let data_dir = temporary_directory("event-page");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().extend(
                    input
                        .events
                        .iter()
                        .filter(|event| event.event_type == "queued")
                        .filter_map(|event| event.data["index"].as_u64()),
                );
                done(&*services, input.transcript).await
            }
        })
    };
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    handle
        .message(MessageRequest {
            input: "one".into(),
        })
        .await
        .unwrap();
    for index in 0..1_005 {
        handle
            .record("queued", serde_json::json!({ "index": index }))
            .await
            .unwrap();
    }
    for message in ["two", "three"] {
        handle
            .message(MessageRequest {
                input: message.into(),
            })
            .await
            .unwrap();
    }
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1_005);
        assert_eq!(seen.first(), Some(&0));
        assert_eq!(seen.last(), Some(&1_004));
    }
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn a_turn_that_exceeds_its_model_call_budget_fails_with_model_call_limit() {
    let data_dir = temporary_directory("budget");
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        loop {
            let result = services.model(request(transcript.clone())).await?;
            transcript.push(result.message);
        }
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
    handle
        .message(MessageRequest {
            input: "forever".into(),
        })
        .await
        .unwrap();
    let events = runtime.events(handle.id(), 0, 1_000).events;
    let last = events.last().unwrap();
    assert_eq!(last.event_type, "turn_failed");
    assert_eq!(last.data["code"], "model_call_limit");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "model_call_ended")
            .count(),
        8
    );
    drop(handle);
    settle(runtime, data_dir).await;
}
