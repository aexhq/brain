//! The official child capability must create an ordinary, separately journaled session.

use brain::adapter::DisabledToolExecutor;
use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::journal::{Journal, Record};
use brain::message::{Message, StopReason, Usage};
use brain::provider::{ModelRequest, Provider, ProviderEvent};
use brain::session::{Brain, BrainConfig, BrainServices};
use brain::{BrainError, Result};
use brain_protocol::session::{CreateSessionRequest, MessageRequestContent};
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

#[derive(Debug, Default)]
struct OrdinaryChildProvider {
    requests: Mutex<Vec<Value>>,
    block_root_continuation: AtomicBool,
    root_continuation_started: AtomicBool,
    root_continuation_notify: Notify,
    root_continuation_release: Notify,
}

impl OrdinaryChildProvider {
    fn is_root_continuation(request: &Value) -> bool {
        let messages = request["messages"].as_array().into_iter().flatten();
        messages
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .any(|block| block["type"] == "tool_use" && block["name"] == "subagents")
    }

    async fn wait_for_root_continuation(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.root_continuation_started.load(Ordering::Acquire) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "root provider continuation did not start"
            );
            tokio::select! {
                () = self.root_continuation_notify.notified() => {}
                () = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    }

    fn response(request: &Value) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        let messages = request["messages"]
            .as_array()
            .ok_or_else(|| BrainError::Protocol("scripted request has no messages".into()))?;
        let text = messages
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .next_back()
            .unwrap_or_default();
        let has_subagent_call = messages
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .any(|block| block["type"] == "tool_use" && block["name"] == "subagents");

        let events = if text == "child prompt" {
            vec![
                ProviderEvent::TextDelta {
                    index: 0,
                    text: "child answer".into(),
                },
                ProviderEvent::MessageDone {
                    stop_reason: StopReason::EndTurn,
                    usage: zero_usage(),
                },
            ]
        } else if !has_subagent_call {
            vec![
                ProviderEvent::ToolUseStart {
                    index: 0,
                    id: "provider-spawn".into(),
                    name: "subagents".into(),
                },
                ProviderEvent::ToolInputDelta {
                    index: 0,
                    partial_json: serde_json::to_string(&json!({
                        "action": "spawn_agent",
                        "task_name": "worker",
                        "message": "child prompt",
                        "fork_turns": "all"
                    }))?,
                },
                ProviderEvent::BlockDone { index: 0 },
                ProviderEvent::MessageDone {
                    stop_reason: StopReason::ToolUse,
                    usage: zero_usage(),
                },
            ]
        } else {
            vec![
                ProviderEvent::TextDelta {
                    index: 0,
                    text: "root answer".into(),
                },
                ProviderEvent::MessageDone {
                    stop_reason: StopReason::EndTurn,
                    usage: zero_usage(),
                },
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok),
        )))
    }
}

#[async_trait::async_trait]
impl Provider for OrdinaryChildProvider {
    fn dialect(&self) -> Dialect {
        Dialect::AnthropicMessages
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        brain::provider::anthropic::Anthropic::build_request(prefix, history, key, base_url)
    }

    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        let body: Value = serde_json::from_slice(&request.body)?;
        self.requests
            .lock()
            .expect("provider requests")
            .push(body.clone());
        if self.block_root_continuation.load(Ordering::Acquire) && Self::is_root_continuation(&body)
        {
            self.root_continuation_started
                .store(true, Ordering::Release);
            self.root_continuation_notify.notify_waiters();
            self.root_continuation_release.notified().await;
        }
        Self::response(&body)
    }
}

fn zero_usage() -> Usage {
    Usage {
        input_tokens: Some(1),
        output_tokens: Some(1),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reasoning_tokens: None,
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "brain-ordinary-child-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test data directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn create_request() -> CreateSessionRequest {
    serde_json::from_value(json!({
        "model": {
            "provider": "anthropic",
            "name": "scripted",
            "api_key": "sk-fake"
        },
        "system_prompt": "ordinary child e2e",
        "tools": {
            "items": [{
                "definition": {
                    "name": "subagents",
                    "description": "Create and interact with durable direct child sessions.",
                    "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "action": {"const": "spawn_agent"},
                            "task_name": {"type": "string"},
                            "message": {"type": "string"},
                            "fork_turns": {"type": "string"}
                        },
                        "required": ["action", "task_name", "message"],
                        "additionalProperties": false
                    },
                    "output_schema": {}
                },
                "executor": {"kind": "engine", "capability": "brain.subagents"}
            }]
        }
    }))
    .expect("typed create request")
}

async fn wait_turn_finished(brain: &Arc<Brain>, session_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let session = brain.get(session_id).await.expect("session status");
        if session.current_turn.is_none() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "turn did not finish for {session_id}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn request_blocks(request: &Value) -> impl Iterator<Item = &Value> {
    request["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| message["content"].as_array())
        .flatten()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_spawn_creates_an_ordinary_child_at_a_complete_fork_boundary() {
    let _tmp = TempDir::new();
    let journal = Journal::new_memory("ordinary-child-e2e");
    let provider = Arc::new(OrdinaryChildProvider::default());
    let provider_factory = provider.clone();
    // One turn permit makes the parent finish its spawning round before the child begins,
    // producing deterministic request ordering while still exercising the recovery scheduler.
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            max_concurrent_model_rounds: 1,
            max_concurrent_turns: 1,
            idle_discard: Duration::from_secs(60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider_factory.clone() as Arc<dyn Provider>),
    );

    let root = brain
        .create_session(create_request(), Some("ordinary-child-root"))
        .await
        .expect("create root");
    let root_id = root.id.to_string();
    brain
        .message(
            &root_id,
            MessageRequestContent::String("root prompt".parse().expect("message")),
        )
        .await
        .expect("start root turn");
    wait_turn_finished(&brain, &root_id).await;

    let (children, cursor) = brain
        .list_children(&root_id, None, 100)
        .await
        .expect("list direct children");
    assert!(cursor.is_none());
    assert_eq!(children.len(), 1);
    let child_json = serde_json::to_value(&children[0]).expect("child session JSON");
    let child_id = child_json["id"].as_str().expect("child id").to_owned();
    assert_eq!(child_json["parent_id"], root_id);
    assert_eq!(child_json["root_id"], root_id);
    assert_eq!(child_json["name"], "worker");
    assert_eq!(child_json["context_fork"]["mode"], "all");
    wait_turn_finished(&brain, &child_id).await;

    let root_records = journal
        .read_records(&root_id, 0)
        .await
        .expect("root journal");
    let child_records = journal
        .read_records(&child_id, 0)
        .await
        .expect("child journal");
    assert!(root_records.iter().any(|entry| matches!(
        &entry.record,
        Record::ToolCall { agent, name, .. } if agent == "root" && name == "subagents"
    )));
    assert!(root_records.iter().any(|entry| matches!(
        &entry.record,
        Record::ToolResult { agent, name, is_error: false, .. }
            if agent == "root" && name == "subagents"
    )));
    assert!(!root_records.iter().any(|entry| matches!(
        &entry.record,
        Record::UserMessage { content, .. }
            if content.iter().any(|block| matches!(block, brain::ContentBlock::Text { text } if text == "child prompt"))
    )));
    assert!(child_records.iter().any(|entry| matches!(
        &entry.record,
        Record::UserMessage { content, starts_turn: true, .. }
            if content.iter().any(|block| matches!(block, brain::ContentBlock::Text { text } if text == "child prompt"))
    )));

    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(
        requests.len(),
        3,
        "root spawn, root completion, child completion"
    );
    let child_request = requests
        .iter()
        .find(|request| {
            request_blocks(request)
                .any(|block| block["type"] == "text" && block["text"] == "child prompt")
        })
        .expect("child provider request");
    let child_blocks = request_blocks(child_request).collect::<Vec<_>>();
    assert!(
        child_blocks
            .iter()
            .any(|block| block["text"] == "root prompt")
    );
    assert!(
        child_blocks
            .iter()
            .any(|block| block["text"] == "child prompt")
    );
    assert!(child_blocks.iter().all(|block| block["type"] != "tool_use"));
    assert!(
        child_blocks
            .iter()
            .all(|block| block["type"] != "tool_result")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn end_fences_a_root_while_its_post_spawn_provider_call_is_stalled() {
    let _tmp = TempDir::new();
    let journal = Journal::new_memory("ordinary-child-stalled-root-e2e");
    let provider = Arc::new(OrdinaryChildProvider::default());
    provider
        .block_root_continuation
        .store(true, Ordering::Release);
    let provider_factory = provider.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            max_concurrent_model_rounds: 2,
            max_concurrent_turns: 2,
            idle_discard: Duration::from_secs(60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider_factory.clone() as Arc<dyn Provider>),
    );

    let root = brain
        .create_session(create_request(), Some("ordinary-child-stalled-root"))
        .await
        .expect("create root");
    let root_id = root.id.to_string();
    brain
        .message(
            &root_id,
            MessageRequestContent::String("root prompt".parse().expect("message")),
        )
        .await
        .expect("start root turn");
    provider.wait_for_root_continuation().await;

    let (children, _) = brain
        .list_children(&root_id, None, 100)
        .await
        .expect("list direct children");
    assert_eq!(
        children.len(),
        1,
        "spawn committed before the provider stall"
    );

    let accepted = tokio::time::timeout(Duration::from_secs(1), brain.end(&root_id))
        .await
        .expect("END must not wait for the stalled provider")
        .expect("END admission fence");
    assert_eq!(
        accepted.state,
        brain_protocol::session::SessionState::Ending
    );

    provider.root_continuation_release.notify_waiters();
}
