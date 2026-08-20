//! An instant in-process hand: tool calls return immediately with a tiny result. The bench
//! measures the BRAIN — journal, admission, turn loop, event fanout, HTTP+SSE — so the hand
//! must cost nothing. Built against the public adapter traits, same as any third-party
//! substrate (`tests/custom_adapter.rs` is the annotated exemplar).

use brain::adapter::{
    ArtifactMeta, CallOutcome, CallRequest, HandAdapter, HandFactory, HandSpec, LostReport,
    OutputSink, SeedFile,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct EchoHand {
    pub calls: AtomicU64,
}

#[async_trait::async_trait]
impl HandAdapter for EchoHand {
    async fn ensure_ready(&self) -> brain::Result<Option<LostReport>> {
        Ok(None)
    }

    async fn call(
        &self,
        req: CallRequest,
        _cancel: CancellationToken,
        _sink: OutputSink,
    ) -> CallOutcome {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let content = format!("ok:{}", req.call_id);
        CallOutcome {
            outcome: "completed".into(),
            value: Some(Value::String(content.clone())),
            content,
            is_error: false,
            exit_code: Some(0),
            duration_ms: 0,
            truncated: false,
            terminal: None,
        }
    }

    async fn release(&self) -> brain::Result<()> {
        Ok(())
    }

    async fn persist(
        &self,
        _name: &str,
        _path: &str,
        _media_type: Option<&str>,
    ) -> brain::Result<ArtifactMeta> {
        Err(brain::BrainError::Hand(
            "bench hand does not persist".into(),
        ))
    }

    fn hand_info(&self) -> brain_protocol::session::HandInfo {
        use brain_protocol::session::{HandInfo, HandShape, HandState};
        HandInfo {
            generation: Some(1),
            last_sync_at: None,
            live_jobs: Some(0),
            shape: HandShape::X1gb,
            started_at: None,
            state: HandState::Ready,
            wall_deadline_at: None,
        }
    }

    fn state(&self) -> Value {
        json!({"echo": true})
    }
}

pub struct EchoFactory {
    /// One shared hand: per-session adapter state would be pure overhead here, and the bench
    /// counts calls across the run.
    pub hand: Arc<EchoHand>,
}

#[async_trait::async_trait]
impl HandFactory for EchoFactory {
    async fn create(
        &self,
        _spec: &HandSpec,
        _seeds: &[SeedFile<'_>],
        _bundles: &[brain::adapter::ToolBundleFile<'_>],
    ) -> brain::Result<Value> {
        Ok(json!({"echo": true}))
    }

    async fn open(&self, _spec: &HandSpec, _state: Value) -> brain::Result<Arc<dyn HandAdapter>> {
        Ok(self.hand.clone())
    }

    async fn purge(&self, _session_id: &str) -> brain::Result<()> {
        Ok(())
    }

    async fn artifact_url(&self, _session_id: &str, _location: &str) -> Option<String> {
        None
    }
}
