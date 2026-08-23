//! Contract-mode BuiltinAexLoop smoke: one text turn and one tool turn complete, with the
//! real error surfaced when they do not.

use brain::adapter::DisabledToolExecutor;
use brain::config::Dialect;
use brain::journal::{Journal, Record};
use brain::provider::Provider;
use brain::provider::fake::{FakeMode, FakeProvider};
use brain::session::{Brain, BrainConfig, BrainServices};
use brain_protocol::session::MessageRequestContent;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builtin_contract_loop_completes_text_and_tool_turns() {
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.set_mode(FakeMode::Policy {
        tool_rounds: 1,
        parallel: 2,
        tool: "bash".into(),
        text_bytes: 128,
    });
    let provider = fake.clone();
    let journal = Journal::new_memory("builtin-smoke");
    let mut cfg = BrainConfig {
        idle_discard: Duration::from_secs(60),
        ..BrainConfig::default()
    };
    cfg.official_capabilities.insert(
        "aex.bench_echo".into(),
        brain::config::ServerToolPolicy {
            capability: "bench.echo".into(),
            scope: brain_protocol::session::ExternalToolScope::All,
            completion: brain_protocol::session::ExternalToolCompletion::Continue,
            effect: brain_protocol::session::ExternalToolEffect::ReplaySafe,
            max_input_bytes: brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES,
        },
    );
    #[derive(Default)]
    struct Echo;
    #[async_trait::async_trait]
    impl brain::adapter::ToolExecutor for Echo {
        fn supports(&self, capability: &str) -> bool {
            capability == "bench.echo"
        }
        async fn call(
            &self,
            _capability: &str,
            request: brain_protocol::session::ExternalToolCallRequest,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> brain::Result<brain_protocol::session::ExternalToolCallResponse> {
            let value = json!({"call_id": request.call_id, "ok": true});
            Ok(serde_json::from_value(json!({
                "outcome": "completed",
                "content": value.to_string(),
                "is_error": false,
            }))
            .expect("echo response"))
        }
    }
    let _ = DisabledToolExecutor;
    let brain = Brain::with_parts_and_services(
        cfg,
        journal.clone(),
        Arc::new(brain::keys::PlainCustody),
        Arc::new(Echo),
        BrainServices::default(),
        Arc::new(move |_| provider.clone() as Arc<dyn Provider>),
    );
    let session = brain
        .create_session(
            serde_json::from_value(json!({
                "model": {"provider":"anthropic","name":"scripted","api_key":"sk-x"},
                "system_prompt": "smoke",
                "tools": {"items": [{
                    "definition": {
                        "name": "bash",
                        "description": "echo tool",
                        "contract_digest": "a".repeat(64),
                        "input_schema": {"type":"object","additionalProperties":true},
                        "output_schema": {"type":"object","additionalProperties":true}
                    },
                    "executor": {"kind":"engine","capability":"aex.bench_echo"}
                }]}
            }))
            .unwrap(),
            None,
        )
        .await
        .expect("create");
    let id = session.id.to_string();
    brain
        .message(&id, MessageRequestContent::String("go".parse().unwrap()))
        .await
        .expect("admit");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let s = brain.get(&id).await.expect("get");
        if s.current_turn.is_none() {
            break;
        }
        assert!(Instant::now() < deadline, "turn did not finish");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let records = journal.read_records(&id, 0).await.expect("records");
    let failure = records.iter().find_map(|entry| match &entry.record {
        Record::TurnFailed { code, message, .. } => Some(format!("{code}: {message}")),
        _ => None,
    });
    assert!(failure.is_none(), "turn failed: {}", failure.unwrap());
    assert!(
        records
            .iter()
            .any(|entry| matches!(&entry.record, Record::LoopMark { .. })),
        "no mark journaled"
    );
}
