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
use brain::{Error, JournalStore, ToolExecutor};
use brain_protocol::{
    AttachmentId, ContentBlock, EnvironmentAttachment, EnvironmentBinding, EnvironmentId,
    LiveEvent, Message, MessageRequest, ModelRequest, Outcome, OutcomeError,
    Runtime as EnvironmentRuntime, SessionConfig, ToolBinding, ToolCancellation, ToolDefinition,
    ToolDispatch, ToolHosting, ToolInvocation, TurnOutput,
};
use brain_telemetry::telemetry_channel;
use common::{
    NoModels, NoTools, Runtime, ScriptedModel, SlowModel, config, echo_loop, scripted,
    temporary_directory,
};

fn user(text: &str) -> Message {
    Message::user_text(text)
}

fn done(transcript: Vec<Message>) -> Result<TurnOutput, Error> {
    Ok(TurnOutput {
        transcript,
        slots: Default::default(),
        result: Some(serde_json::json!({"ok": true})),
    })
}

fn request(messages: Vec<Message>) -> ModelRequest {
    ModelRequest {
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
    }
}

/// A configuration binding one tool with the given `needs` to one environment declaring
/// the given resources.
fn tool_config(tool_name: &str, needs: Vec<&str>, declares: Vec<&str>) -> SessionConfig {
    let environment = EnvironmentBinding {
        environment_id: EnvironmentId::new("workspace"),
        directory_generation: 1,
    };
    let mut config = config();
    config.tools = vec![ToolDefinition {
        name: tool_name.into(),
        description: "a tool".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: None,
    }];
    config.environments = vec![EnvironmentAttachment {
        environment_id: EnvironmentId::new("workspace"),
        binding: Some(environment.clone()),
        attachment_id: Some(AttachmentId::new("attachment")),
        runtimes: vec![EnvironmentRuntime::Esm],
        resources: declares
            .into_iter()
            .map(|name| (name.to_string(), serde_json::json!({})))
            .collect(),
    }];
    config.tool_bindings = vec![ToolBinding {
        name: tool_name.into(),
        environment_id: Some(EnvironmentId::new("workspace")),
        environment: Some(environment),
        attachment_id: Some(AttachmentId::new("attachment")),
        needs: needs.into_iter().map(String::from).collect(),
        binding_names: Vec::new(),
        hosting: ToolHosting::Provisioned,
        program: None,
    }];
    config
}

/// A configuration binding one client-hosted tool: no environment anywhere.
fn client_tool_config(tool_name: &str) -> SessionConfig {
    let mut config = config();
    config.tools = vec![ToolDefinition {
        name: tool_name.into(),
        description: "answered by the session's creator".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: None,
    }];
    config.tool_bindings = vec![ToolBinding {
        name: tool_name.into(),
        environment_id: None,
        environment: None,
        attachment_id: None,
        needs: Vec::new(),
        binding_names: Vec::new(),
        hosting: ToolHosting::Client,
        program: None,
    }];
    config
}

/// A tool executor that answers each call with a scripted outcome and remembers every
/// cancellation it was asked for.
struct OutcomeTools {
    outcome: Outcome,
    delay: Duration,
    cancelled: Mutex<Vec<u64>>,
}

#[async_trait]
impl ToolExecutor for OutcomeTools {
    async fn execute(&self, _: ToolDispatch) -> Result<Outcome, Error> {
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
        brain::DEFAULT_TOOL_DEADLINE_MS,
        loop_executor,
        model_executor,
        tool_executor,
    )
}

fn runtime_with_deadline(
    data_dir: &std::path::Path,
    loop_executor: Arc<dyn brain::LoopExecutor>,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_deadline_ms: u64,
) -> Runtime {
    let (publisher, _worker) = telemetry_channel();
    Runtime::open(
        data_dir,
        publisher,
        8,
        tool_deadline_ms,
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
        done(transcript)
    });
    let runtime = runtime(&data_dir, loop_executor, model.clone(), Arc::new(NoTools));
    *model.feed.lock().unwrap() = Some(runtime.subscribe());
    let handle = runtime.create(&config(), &[]).unwrap();
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
        done(transcript)
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(SlowModel),
        Arc::new(NoTools),
    );
    let handle = runtime.create(&config(), &[]).unwrap();
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
    tokio::time::sleep(Duration::from_millis(100)).await;
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
        cancelled: Mutex::new(Vec::new()),
    });
    let loop_executor = scripted(|input, services| async move {
        let results = services
            .dispatch(vec![invocation("slow", "call_1")])
            .await?;
        let mut transcript = input.transcript;
        transcript.push(user(&format!("{} results", results.len())));
        done(transcript)
    });
    let runtime = runtime_with_deadline(
        &data_dir,
        loop_executor,
        tools.clone(),
        brain::DEFAULT_TOOL_DEADLINE_MS,
    );
    let handle = runtime
        .create(&tool_config("slow", vec![], vec![]), &[])
        .unwrap();
    let turning = {
        let handle = handle.clone();
        tokio::spawn(async move { handle.message(MessageRequest { input: "go".into() }).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
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
async fn a_subscriber_sees_model_output_while_the_turn_is_running() {
    let data_dir = temporary_directory("streaming");
    let loop_executor = scripted(|input, services| async move {
        let mut transcript = input.transcript;
        transcript.push(user(&input.input.message));
        let result = services.model(request(transcript.clone())).await?;
        transcript.push(result.message);
        done(transcript)
    });
    let runtime = runtime(
        &data_dir,
        loop_executor,
        Arc::new(ScriptedModel),
        Arc::new(NoTools),
    );
    let mut feed = runtime.subscribe();
    let handle = runtime.create(&config(), &[]).unwrap();
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
        scripted(move |input, _services| {
            let seen = seen.clone();
            async move {
                *seen.lock().unwrap() = input.transcript.clone();
                let mut transcript = input.transcript;
                transcript.push(user(&input.input.message));
                done(transcript)
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
        for kind in ["turn_ended", "session_ended", "model_call_started"] {
            let refused = services.append(kind.into(), serde_json::json!({})).await;
            assert!(refused.is_err(), "{kind} must be refused");
        }
        services
            .append(
                "output_emitted".into(),
                serde_json::json!({"type": "assistant_message"}),
            )
            .await?;
        services
            .append("note".into(), serde_json::json!({"text": "mine"}))
            .await?;
        done(input.transcript)
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
        done(transcript)
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
        found
            .iter()
            .all(|name| name.ends_with(".segment") || name.ends_with("/config.json")),
        "a session's directory holds its configuration and its segments and nothing else; found {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|name| name.contains("/journal/") && name.ends_with(".segment"))
    );
    assert!(
        found
            .iter()
            .any(|name| name.contains("/events/") && name.ends_with(".segment"))
    );
}

/// The bind check: a tool whose `needs` is not covered by its environment's declared
/// resources is rejected at create, and the error names the resource, the tool, and
/// the environment.
#[tokio::test]
async fn needs_beyond_declared_resources_rejects_create_naming_all_three_parties() {
    let data_dir = temporary_directory("bind-check");
    let runtime = runtime(
        &data_dir,
        echo_loop(),
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let error = match runtime.create(&tool_config("bash", vec!["process"], vec!["dom"]), &[]) {
        Ok(_) => {
            panic!("a tool needing `process` must not bind to an environment declaring only `dom`")
        }
        Err(error) => error,
    };
    let message = error.to_string();
    for named in ["process", "bash", "workspace"] {
        assert!(
            message.contains(named),
            "the rejection must name {named:?}: {message}"
        );
    }
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn empty_needs_binds_to_any_environment() {
    let data_dir = temporary_directory("bind-any");
    let runtime = runtime(
        &data_dir,
        echo_loop(),
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime
        .create(&tool_config("note", vec![], vec![]), &[])
        .unwrap();
    assert!(matches!(
        runtime.session(handle.id()).status,
        brain_protocol::SessionStatus::Idle
    ));
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn invoke_outcomes_map_onto_tool_results() {
    for (outcome, code) in [
        (
            Outcome::Error {
                error: OutcomeError {
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
                    done(input.transcript)
                }
            })
        };
        let runtime = runtime_with_deadline(&data_dir, loop_executor, tools, 5_000);
        let handle = runtime
            .create(&tool_config("tool", vec![], vec![]), &[])
            .unwrap();
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
async fn an_overdue_invoke_is_killed_and_recorded_as_timeout() {
    let data_dir = temporary_directory("tool-timeout");
    let tools = Arc::new(OutcomeTools {
        outcome: Outcome::Ok {
            value: serde_json::json!({}),
        },
        delay: Duration::from_secs(30),
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
                done(input.transcript)
            }
        })
    };
    let runtime = runtime_with_deadline(&data_dir, loop_executor, tools.clone(), 200);
    let handle = runtime
        .create(&tool_config("slow", vec![], vec![]), &[])
        .unwrap();
    let started = std::time::Instant::now();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    let results = seen.lock().unwrap().clone();
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
async fn a_client_tool_call_parks_until_its_outcome_is_posted() {
    let data_dir = temporary_directory("client-tool");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                let results = services
                    .dispatch(vec![invocation("pick_file", "call_1")])
                    .await?;
                *seen.lock().unwrap() = results;
                done(input.transcript)
            }
        })
    };
    let runtime = runtime_with_deadline(&data_dir, loop_executor, Arc::new(NoTools), 5_000);
    let mut feed = runtime.subscribe();
    let handle = runtime
        .create(&client_tool_config("pick_file"), &[])
        .unwrap();
    let turning = {
        let handle = handle.clone();
        tokio::spawn(async move { handle.message(MessageRequest { input: "go".into() }).await })
    };
    // Answer off the feed, as a client would: the started record names the call.
    let sequence = loop {
        let (_, event) = tokio::time::timeout(Duration::from_secs(5), feed.recv())
            .await
            .unwrap()
            .unwrap();
        if let LiveEvent::Recorded(event) = event
            && event.event_type == "tool_call_started"
        {
            break event.sequence;
        }
    };
    handle
        .resolve_tool_call(
            sequence,
            Outcome::Ok {
                value: serde_json::json!({"path": "README.md"}),
            },
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), turning)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let results = seen.lock().unwrap().clone();
    assert_eq!(results[0].output["path"], "README.md");
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn resolving_an_unknown_call_is_refused() {
    let data_dir = temporary_directory("unknown-call");
    let runtime = runtime(
        &data_dir,
        echo_loop(),
        Arc::new(NoModels),
        Arc::new(NoTools),
    );
    let handle = runtime
        .create(&client_tool_config("pick_file"), &[])
        .unwrap();
    let error = handle
        .resolve_tool_call(
            42,
            Outcome::Ok {
                value: serde_json::json!({}),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("no client Tool call is pending"));
    drop(handle);
    settle(runtime, data_dir).await;
}

#[tokio::test]
async fn an_unanswered_client_call_times_out_and_journals_the_cancellation() {
    let data_dir = temporary_directory("client-timeout");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let loop_executor = {
        let seen = seen.clone();
        scripted(move |input, services| {
            let seen = seen.clone();
            async move {
                let results = services
                    .dispatch(vec![invocation("pick_file", "call_1")])
                    .await?;
                *seen.lock().unwrap() = results;
                done(input.transcript)
            }
        })
    };
    let runtime = runtime_with_deadline(&data_dir, loop_executor, Arc::new(NoTools), 200);
    let handle = runtime
        .create(&client_tool_config("pick_file"), &[])
        .unwrap();
    handle
        .message(MessageRequest { input: "go".into() })
        .await
        .unwrap();
    let results = seen.lock().unwrap().clone();
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
        let mut slots = std::collections::BTreeMap::new();
        slots.insert("memory".to_string(), serde_json::json!({"turns": 1}));
        Ok(TurnOutput {
            transcript,
            slots,
            result: None,
        })
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
    assert_eq!(folded.slots["memory"], serde_json::json!({"turns": 1}));
    assert!(folded.slots.contains_key(brain::LAST_ACTIVATION_SLOT));
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
                done(transcript)
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
        scripted(move |input, _services| {
            let seen = seen.clone();
            async move {
                *seen.lock().unwrap() = input
                    .events
                    .iter()
                    .map(|event| event.event_type.clone())
                    .collect();
                done(input.transcript)
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
            serde_json::json!({"environment_id": "env_1"}),
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
async fn a_turn_that_exceeds_its_model_call_budget_fails_with_decision_limit() {
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
    assert_eq!(last.data["code"], "decision_limit");
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
