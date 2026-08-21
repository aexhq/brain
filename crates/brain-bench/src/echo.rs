//! An instant trusted host executor for the engine benchmark.
//!
//! The benchmark measures Brain's journal, admission, turn loop and event fanout, so the
//! executor deliberately contributes only an atomic counter and a tiny canonical response.

use brain::adapter::ToolExecutor;
use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct EchoExecutor {
    pub calls: AtomicU64,
}

#[async_trait::async_trait]
impl ToolExecutor for EchoExecutor {
    fn supports(&self, capability: &str) -> bool {
        capability == "bench.echo"
    }

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        cancel: CancellationToken,
    ) -> brain::Result<ExternalToolCallResponse> {
        if !self.supports(capability) {
            return Err(brain::BrainError::Invalid(
                "benchmark executor received an unknown capability".into(),
            ));
        }
        if cancel.is_cancelled() {
            return Err(brain::BrainError::Cancelled);
        }
        // The production server executor crosses an HTTP boundary. Retain one scheduling point
        // without adding wall-clock latency so the benchmark measures Brain instead of a
        // synthetic single-task event burst.
        tokio::task::yield_now().await;
        self.calls.fetch_add(1, Ordering::Relaxed);
        let value = serde_json::json!({"call_id": request.call_id, "ok": true});
        serde_json::from_value(serde_json::json!({
            "outcome": "completed",
            "content": serde_json::to_string(&value).expect("benchmark value serializes"),
            "is_error": false,
            "result": value,
            "disposition": "continue",
        }))
        .map_err(brain::BrainError::from)
    }
}
