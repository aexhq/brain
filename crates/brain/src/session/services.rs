use async_trait::async_trait;
use brain_protocol::{ModelRequest, ModelResult, ToolInvocation, ToolResult};

use crate::Error;

/// What the agentloop can ask Brain to do while a turn is running.
///
/// Every effect a loop wants goes through here, and every call journals before it acts,
/// so the loop has control of the turn and Brain keeps authority over what happens. A
/// call made after the turn was cancelled or ran out of budget fails with that code; the
/// loop propagates it by returning an error from the turn.
#[async_trait]
pub trait TurnServices: Send + Sync {
    /// One model call. What the request leaves unsaid is what the session was created
    /// with; the messages are the transcript as the loop wants the model to see it, and
    /// Brain journals how they differ from what it last recorded.
    async fn model(&self, request: ModelRequest) -> Result<ModelResult, Error>;
    /// One or many tool calls, run together. Calling this once per call is sequential
    /// dispatch. The results come back in the calls' order.
    async fn dispatch(&self, calls: Vec<ToolInvocation>) -> Result<Vec<ToolResult>, Error>;
    /// The loop's own record on the session's feed. Brain's lifecycle and effect kinds
    /// are refused. Returns the record's sequence.
    async fn append(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error>;
    /// Fire and forget.
    fn telemetry(&self, record: serde_json::Value);
    /// Whether the turn has been cancelled or has run out of time. An executor that
    /// runs the loop elsewhere polls this to tell the loop's host to stop.
    fn cancelled(&self) -> bool;
}
