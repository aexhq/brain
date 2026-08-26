use std::path::PathBuf;

use brain_component_host::{
    CapabilityCall, CapabilityFailure, CapabilityHandler, ComponentRuntime, ComponentSource,
    WORKER_REQUEST_CAP, WorkerPool, WorkerRequest, agentloop, component_digest, environment, model,
    tool,
};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn component_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("guest/dist")
        .join(format!("{name}.component.wasm"))
}

fn component(name: &str) -> Vec<u8> {
    let path = component_path(name);
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error}; run `npm run build:components` first",
            path.display()
        )
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn four_worlds_execute_without_ambient_authority() {
    let runtime = ComponentRuntime::new().unwrap();

    let activation = runtime
        .invoke_agentloop(
            &component("agentloop"),
            agentloop::aex::agentloop::types::Activation {
                operation_id: "activation_1".into(),
                session_id: "ses_1".into(),
                kind: "message".into(),
                payload_json: r#"{"message":"hello"}"#.into(),
                config_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(activation.payload_json, r#"{"message":"hello"}"#);

    let outcome = runtime
        .invoke_tool(
            &component("tool"),
            tool::aex::tool::types::Invocation {
                metadata: tool::aex::tool::types::CallMetadata {
                    tenant_id: "tenant_1".into(),
                    session_id: "ses_1".into(),
                    turn_id: "turn_1".into(),
                    call_id: "call_1".into(),
                    tool_name: "echo".into(),
                },
                input_json: r#"{"value":"hello"}"#.into(),
                config_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert!(!outcome.is_error);
    assert_eq!(outcome.value_json, r#"{"value":"hello"}"#);

    let environment = runtime
        .exercise_environment(
            &component("environment"),
            environment::aex::environment::types::ResolveRequest {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                root_id: "ses_1".into(),
                parent_id: None,
                environment_id: "workspace".into(),
                config_json: "{}".into(),
                policy_json: "{}".into(),
            },
            environment::aex::environment::types::Operation {
                operation_id: "env_op_1".into(),
                kind: "invoke".into(),
                descriptor_json: "{}".into(),
                bundle: None,
                input_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        environment.state,
        environment::aex::environment::types::OperationState::Completed
    );

    let model = runtime
        .exercise_model(
            &component("model"),
            model::aex::model::types::Request {
                operation_id: "model_op_1".into(),
                model: "test".into(),
                messages_json: "[]".into(),
                tools_json: "[]".into(),
                response_format_json: None,
                generation_json: "{}".into(),
                provider_options_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        model.state,
        model::aex::model::types::AttemptState::Completed
    );
    assert_eq!(model.events.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bounded_worker_pool_executes_in_separate_processes() {
    let pool = WorkerPool::new(env!("CARGO_BIN_EXE_component-host"), 2)
        .await
        .unwrap();
    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let request = WorkerRequest::Tool {
        component: ComponentSource {
            path,
            sha256: component_digest(&bytes),
        },
        request: tool::aex::tool::types::Invocation {
            metadata: tool::aex::tool::types::CallMetadata {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                turn_id: "turn_1".into(),
                call_id: "call_process".into(),
                tool_name: "echo".into(),
            },
            input_json: r#"{"process":true}"#.into(),
            config_json: "{}".into(),
            deadline_at_ms: u64::MAX,
        },
        grants: Vec::new(),
    };
    let response = pool.call(request).await.unwrap();
    assert_eq!(response["value_json"], r#"{"process":true}"#);
}

struct EchoEnvironment;

#[async_trait::async_trait]
impl CapabilityHandler for EchoEnvironment {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        assert_eq!(call.capability, "tool.environment.invoke");
        assert_eq!(call.world, "tool");
        assert_eq!(call.instance_id.as_deref(), Some("call_relay"));
        Ok(Value::String(
            call.request["input_json"]
                .as_str()
                .expect("input JSON")
                .to_owned(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_relays_only_granted_component_capabilities() {
    let pool = WorkerPool::with_capabilities(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        Arc::new(EchoEnvironment),
    )
    .await
    .unwrap();
    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let request = || WorkerRequest::Tool {
        component: ComponentSource {
            path: path.clone(),
            sha256: component_digest(&bytes),
        },
        request: tool::aex::tool::types::Invocation {
            metadata: tool::aex::tool::types::CallMetadata {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                turn_id: "turn_1".into(),
                call_id: "call_relay".into(),
                tool_name: "echo".into(),
            },
            input_json: r#"{"relayed":true}"#.into(),
            config_json: r#"{"useEnvironment":true}"#.into(),
            deadline_at_ms: u64::MAX,
        },
        grants: vec!["environment".into()],
    };
    let response = pool.call(request()).await.unwrap();
    assert_eq!(response["value_json"], r#"{"relayed":true}"#);

    let denied = match request() {
        WorkerRequest::Tool {
            component, request, ..
        } => WorkerRequest::Tool {
            component,
            request,
            grants: Vec::new(),
        },
        _ => unreachable!(),
    };
    assert!(
        pool.call(denied)
            .await
            .unwrap_err()
            .to_string()
            .contains("not granted environment")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_keeps_and_releases_agentloop_instances() {
    let pool = WorkerPool::new(env!("CARGO_BIN_EXE_component-host"), 2)
        .await
        .unwrap();
    let path = component_path("agentloop");
    let bytes = std::fs::read(&path).unwrap();
    let request = || WorkerRequest::Agentloop {
        instance_id: "session-1".into(),
        component: ComponentSource {
            path: path.clone(),
            sha256: component_digest(&bytes),
        },
        request: agentloop::aex::agentloop::types::Activation {
            operation_id: "activation".into(),
            session_id: "session-1".into(),
            kind: "message".into(),
            payload_json: "{}".into(),
            config_json: r#"{"track":true}"#.into(),
            deadline_at_ms: u64::MAX,
        },
    };
    assert_eq!(
        pool.call(request()).await.unwrap()["payload_json"],
        r#"{"activations":1}"#
    );
    assert_eq!(
        pool.call(request()).await.unwrap()["payload_json"],
        r#"{"activations":2}"#
    );
    pool.call(WorkerRequest::Release {
        world: "agentloop".into(),
        instance_id: "session-1".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        pool.call(request()).await.unwrap()["payload_json"],
        r#"{"activations":1}"#
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_keeps_environment_and_model_lifecycles_resident() {
    let pool = WorkerPool::new(env!("CARGO_BIN_EXE_component-host"), 1)
        .await
        .unwrap();
    let environment_path = component_path("environment");
    let environment_bytes = std::fs::read(&environment_path).unwrap();
    let resolved = pool
        .call(WorkerRequest::EnvironmentResolve {
            instance_id: "environment-1".into(),
            component: ComponentSource {
                path: environment_path,
                sha256: component_digest(&environment_bytes),
            },
            request: environment::aex::environment::types::ResolveRequest {
                tenant_id: "tenant-1".into(),
                session_id: "session-1".into(),
                root_id: "session-1".into(),
                parent_id: None,
                environment_id: "workspace".into(),
                config_json: "{}".into(),
                policy_json: "{}".into(),
            },
        })
        .await
        .unwrap();
    let binding = resolved["binding_json"].as_str().unwrap().to_owned();
    let submitted = pool
        .call(WorkerRequest::EnvironmentSubmit {
            instance_id: "environment-1".into(),
            binding_json: binding.clone(),
            operation: environment::aex::environment::types::Operation {
                operation_id: "operation-1".into(),
                kind: "invoke".into(),
                descriptor_json: "{}".into(),
                bundle: None,
                input_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        })
        .await
        .unwrap();
    let provider_id = submitted["provider_operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let observed = pool
        .call(WorkerRequest::EnvironmentObserve {
            instance_id: "environment-1".into(),
            binding_json: binding.clone(),
            provider_operation_id: provider_id.clone(),
            cursor: None,
        })
        .await
        .unwrap();
    let terminal = observed["terminal_json"].as_str().unwrap().to_owned();
    pool.call(WorkerRequest::EnvironmentAcknowledge {
        instance_id: "environment-1".into(),
        binding_json: binding.clone(),
        provider_operation_id: provider_id,
        terminal_json: terminal,
    })
    .await
    .unwrap();
    pool.call(WorkerRequest::EnvironmentRelease {
        instance_id: "environment-1".into(),
        binding_json: binding,
    })
    .await
    .unwrap();

    let model_path = component_path("model");
    let model_bytes = std::fs::read(&model_path).unwrap();
    let started = pool
        .call(WorkerRequest::ModelStart {
            instance_id: "model-1".into(),
            component: ComponentSource {
                path: model_path,
                sha256: component_digest(&model_bytes),
            },
            request: model::aex::model::types::Request {
                operation_id: "model-operation-1".into(),
                model: "test".into(),
                messages_json: "[]".into(),
                tools_json: "[]".into(),
                response_format_json: None,
                generation_json: "{}".into(),
                provider_options_json: "{}".into(),
                deadline_at_ms: u64::MAX,
            },
        })
        .await
        .unwrap();
    let provider_id = started["provider_operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let observed = pool
        .call(WorkerRequest::ModelObserve {
            instance_id: "model-1".into(),
            provider_operation_id: provider_id.clone(),
            cursor: None,
        })
        .await
        .unwrap();
    pool.call(WorkerRequest::ModelAcknowledge {
        instance_id: "model-1".into(),
        provider_operation_id: provider_id,
        terminal_json: observed["terminal_json"].as_str().unwrap().to_owned(),
    })
    .await
    .unwrap();
}

/// The bound is a liveness check on the worker, so it must not be spent on work the parent does
/// on the request's behalf. `subagents.wait` blocks one ctx op on a child session's turn for up
/// to its own 300 s kernel bound; before this was scoped to frames, the 90 s request bound killed
/// the worker mid-wait and failed the whole turn.
const TEST_FRAME_BOUND: std::time::Duration = std::time::Duration::from_secs(5);

struct SlowKernel;

#[async_trait::async_trait]
impl CapabilityHandler for SlowKernel {
    async fn call(&self, _call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        tokio::time::sleep(TEST_FRAME_BOUND * 3).await;
        Ok(Value::String(r#"{"child":"done"}"#.into()))
    }
}

fn agentloop_activation(instance: &str, config: &str) -> WorkerRequest {
    let path = component_path("agentloop");
    let bytes = std::fs::read(&path).unwrap();
    WorkerRequest::Agentloop {
        instance_id: instance.into(),
        component: ComponentSource {
            path,
            sha256: component_digest(&bytes),
        },
        request: agentloop::aex::agentloop::types::Activation {
            operation_id: format!("activation_{instance}"),
            session_id: instance.into(),
            kind: "message".into(),
            payload_json: r#"{"activation_id":"act_1","message":{"content":[]}}"#.into(),
            config_json: config.into(),
            deadline_at_ms: u64::MAX,
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_waiting_on_the_kernel_outlives_the_worker_frame_bound() {
    let pool = WorkerPool::with_frame_timeout(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        Arc::new(SlowKernel),
        TEST_FRAME_BOUND,
    )
    .await
    .unwrap();
    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let response = pool
        .call(WorkerRequest::Tool {
            component: ComponentSource {
                path,
                sha256: component_digest(&bytes),
            },
            request: tool::aex::tool::types::Invocation {
                metadata: tool::aex::tool::types::CallMetadata {
                    tenant_id: "tenant_1".into(),
                    session_id: "ses_1".into(),
                    turn_id: "turn_1".into(),
                    call_id: "call_subagents".into(),
                    tool_name: "subagents".into(),
                },
                input_json: r#"{"action":"wait"}"#.into(),
                config_json: r#"{"useEnvironment":true}"#.into(),
                deadline_at_ms: u64::MAX,
            },
            grants: vec!["environment".into()],
        })
        .await
        .unwrap();
    assert_eq!(response["value_json"], r#"{"child":"done"}"#);
}

/// A worker that is busy is not a worker that has gone silent. Reading a component, compiling
/// it and running a guest all take longer than the bound, and none of them are the worker
/// failing to speak; the guest's own CPU slice is what stops a guest, and it says so. The
/// instance is resident first, so the only thing left to be slow is the guest.
#[tokio::test(flavor = "multi_thread")]
async fn a_busy_worker_is_not_mistaken_for_a_silent_one() {
    let pool = WorkerPool::with_frame_timeout(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        Arc::new(DenyEverything),
        TEST_FRAME_BOUND,
    )
    .await
    .unwrap();
    pool.call(agentloop_activation("ses_spin", r#"{"track":true}"#))
        .await
        .unwrap();
    let error = pool
        .call(agentloop_activation("ses_spin", r#"{"fixture":"spin"}"#))
        .await
        .unwrap_err();
    assert!(
        error.to_string().starts_with("the component exceeded its"),
        "{error}"
    );
}

struct DenyEverything;

#[async_trait::async_trait]
impl CapabilityHandler for DenyEverything {
    async fn call(&self, _call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "capability_denied".into(),
            message: "no capability is bound".into(),
            retryable: false,
        })
    }
}

/// A ctx op that needs the same worker (a subagent's child turn, pinned to it by affinity) must
/// run while the parent's request is parked in that very op. Held exclusively, the parent could
/// not observe the child it was waiting for, and `subagents` returned no child session at all.
struct NestedChild {
    pool: std::sync::OnceLock<std::sync::Weak<WorkerPool>>,
}

#[async_trait::async_trait]
impl CapabilityHandler for NestedChild {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        if call.request["op"]["op"].as_str() == Some("model_stream") {
            let pool = self.pool.get().expect("pool").upgrade().expect("pool");
            let child = tokio::time::timeout(
                TEST_FRAME_BOUND,
                pool.call(agentloop_activation("ses_child", r#"{"track":true}"#)),
            )
            .await
            .expect("the child activation must not wait for the parent to finish")
            .expect("the child activation runs on the same worker");
            assert_eq!(child["payload_json"], r#"{"activations":1}"#);
            return Ok(Value::String(
                serde_json::json!({"result":{"message":{"content":[],"stop_reason":"end_turn"}}})
                    .to_string(),
            ));
        }
        Ok(Value::String(
            serde_json::json!({ "result": Value::Null }).to_string(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nested_request_runs_while_the_parent_is_parked_in_its_own_ctx_op() {
    let capabilities = Arc::new(NestedChild {
        pool: std::sync::OnceLock::new(),
    });
    let pool = WorkerPool::with_capabilities(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        capabilities.clone(),
    )
    .await
    .unwrap();
    capabilities.pool.set(Arc::downgrade(&pool)).ok();
    let returned = pool
        .call(agentloop_activation(
            "ses_parent",
            r#"{"fixture":"sequential"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(
        returned["payload_json"],
        r#"{"activation_id":"act_1","outcome":"completed"}"#
    );
}

/// Frames are addressed, so walking away from a request cannot leave the pipe in a state the
/// next request misreads. Unaddressed, a cancelled request desynchronized the conversation and
/// the following one died on it — `component worker response id does not match request N`.
#[tokio::test(flavor = "multi_thread")]
async fn abandoning_a_request_does_not_corrupt_the_next_one() {
    let pool = WorkerPool::with_frame_timeout(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        Arc::new(SlowKernel),
        TEST_FRAME_BOUND,
    )
    .await
    .unwrap();
    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let invoke = |call: &str| WorkerRequest::Tool {
        component: ComponentSource {
            path: path.clone(),
            sha256: component_digest(&bytes),
        },
        request: tool::aex::tool::types::Invocation {
            metadata: tool::aex::tool::types::CallMetadata {
                tenant_id: "tenant_1".into(),
                session_id: "ses_1".into(),
                turn_id: "turn_1".into(),
                call_id: call.into(),
                tool_name: "subagents".into(),
            },
            input_json: r#"{"action":"spawn_agent"}"#.into(),
            config_json: r#"{"useEnvironment":true}"#.into(),
            deadline_at_ms: u64::MAX,
        },
        grants: vec!["environment".into()],
    };
    // Walk away while the worker is parked in a capability the kernel has not answered yet.
    let abandoned =
        tokio::time::timeout(TEST_FRAME_BOUND / 5, pool.call(invoke("call_abandoned"))).await;
    assert!(abandoned.is_err(), "the request is still in the capability");
    let next = pool.call(invoke("call_next")).await;
    assert_eq!(
        next.expect("the worker still speaks the frame protocol")["value_json"],
        r#"{"child":"done"}"#
    );
}

/// What the development plane did on the first deployment of multiplexing: a nested child
/// re-entering the pool while another request on the same worker has its capability refused.
struct NestedChildBesideARefusal {
    pool: std::sync::OnceLock<std::sync::Weak<WorkerPool>>,
}

#[async_trait::async_trait]
impl CapabilityHandler for NestedChildBesideARefusal {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        if call.capability == "tool.environment.invoke" {
            return Err(CapabilityFailure {
                code: "invalid_request".into(),
                message: "the Environment driver refused this submit".into(),
                retryable: false,
            });
        }
        if call.request["op"]["op"].as_str() == Some("model_stream") {
            let pool = self.pool.get().expect("pool").upgrade().expect("pool");
            pool.call(agentloop_activation("ses_child", r#"{"track":true}"#))
                .await
                .expect("the child activation runs while the parent is parked");
            return Ok(Value::String(
                serde_json::json!({"result":{"message":{"content":[],"stop_reason":"end_turn"}}})
                    .to_string(),
            ));
        }
        Ok(Value::String(
            serde_json::json!({ "result": Value::Null }).to_string(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_capability_does_not_disturb_a_nested_child_on_the_same_worker() {
    let capabilities = Arc::new(NestedChildBesideARefusal {
        pool: std::sync::OnceLock::new(),
    });
    let pool = WorkerPool::with_capabilities(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        capabilities.clone(),
    )
    .await
    .unwrap();
    capabilities.pool.set(Arc::downgrade(&pool)).ok();

    let path = component_path("tool");
    let bytes = std::fs::read(&path).unwrap();
    let refused = pool.call(WorkerRequest::Tool {
        component: ComponentSource {
            path,
            sha256: component_digest(&bytes),
        },
        request: tool::aex::tool::types::Invocation {
            metadata: tool::aex::tool::types::CallMetadata {
                tenant_id: "tenant_1".into(),
                session_id: "ses_env".into(),
                turn_id: "turn_1".into(),
                call_id: "call_refused".into(),
                tool_name: "computer".into(),
            },
            input_json: r#"{"action":"submit"}"#.into(),
            config_json: r#"{"useEnvironment":true}"#.into(),
            deadline_at_ms: u64::MAX,
        },
        grants: vec!["environment".into()],
    });
    let parent = pool.call(agentloop_activation(
        "ses_parent",
        r#"{"fixture":"sequential"}"#,
    ));
    let (parent, refused) = tokio::join!(parent, refused);
    assert_eq!(
        parent.expect("the parent activation completes")["payload_json"],
        r#"{"activation_id":"act_1","outcome":"completed"}"#
    );
    assert!(
        refused.is_err(),
        "the refused capability must still fail its own request"
    );
}

/// Counts how many requests one worker is asked to carry at the same moment.
struct CountLive {
    live: AtomicUsize,
    peak: AtomicUsize,
}

#[async_trait::async_trait]
impl CapabilityHandler for CountLive {
    async fn call(&self, call: CapabilityCall) -> Result<Value, CapabilityFailure> {
        if call.request["op"]["op"].as_str() == Some("model_stream") {
            let live = self.live.fetch_add(1, Ordering::Relaxed) + 1;
            self.peak.fetch_max(live, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(400)).await;
            self.live.fetch_sub(1, Ordering::Relaxed);
            return Ok(Value::String(
                serde_json::json!({"result":{"message":{"content":[],"stop_reason":"end_turn"}}})
                    .to_string(),
            ));
        }
        Ok(Value::String(
            serde_json::json!({ "result": Value::Null }).to_string(),
        ))
    }
}

/// Multiplexing removed the only bound on how much one worker process carries: it was measured
/// holding 48 simultaneous activations and 1.03 GiB where it had held one, which is what took
/// the development plane down. Concurrency is what the change is for, so the bound is a cap,
/// not a return to one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_worker_carries_a_bounded_number_of_requests_at_once() {
    let capabilities = Arc::new(CountLive {
        live: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
    });
    let pool = WorkerPool::with_capabilities(
        env!("CARGO_BIN_EXE_component-host"),
        1,
        capabilities.clone(),
    )
    .await
    .unwrap();
    let mut handles = Vec::new();
    for n in 0..48 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            pool.call(agentloop_activation(
                &format!("ses_{n}"),
                r#"{"fixture":"sequential"}"#,
            ))
            .await
        }));
    }
    for handle in handles {
        handle.await.expect("task").expect("activation");
    }
    let peak = capabilities.peak.load(Ordering::Relaxed);
    assert!(
        peak > 1,
        "the worker must still carry more than one at once"
    );
    assert!(
        peak <= WORKER_REQUEST_CAP,
        "one worker carried {peak} requests at once"
    );
}
