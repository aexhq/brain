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
    /// One finite page after a journal sequence. Reading does not advance the activation cursor.
    async fn events(&self, after: u64) -> Result<brain_protocol::EventPage, Error>;
    /// Replaces conversation state and returns its durable journal sequence.
    async fn set_transcript(&self, messages: Vec<brain_protocol::Message>) -> Result<u64, Error>;
    /// Saves one value and returns its durable journal sequence.
    async fn kv_put(&self, request: brain_protocol::KvPutRequest) -> Result<u64, Error>;
    /// Reads current state; absence is distinct from JSON null.
    async fn kv_read(&self, key: String) -> Result<Option<serde_json::Value>, Error>;
    /// Removes one key durably. Missing keys are a no-op.
    async fn kv_delete(&self, key: String) -> Result<u64, Error>;
    /// One model call. What the request leaves unsaid is what the session was created
    /// with. The request is journaled independently of conversation state.
    async fn model(&self, request: ModelRequest) -> Result<ModelResult, Error>;
    /// One or many tool calls, run together. Calling this once per call is sequential
    /// dispatch. The results come back in the calls' order.
    async fn dispatch(&self, calls: Vec<ToolInvocation>) -> Result<Vec<ToolResult>, Error>;
    /// The loop's own record on the session's feed. Brain's lifecycle and effect kinds
    /// are refused. Returns the record's sequence.
    async fn emit(&self, kind: String, payload: serde_json::Value) -> Result<u64, Error>;
    /// Fire and forget.
    fn telemetry(&self, record: serde_json::Value);
    /// Whether the turn has been cancelled or has run out of time. An executor that
    /// runs the loop elsewhere polls this to tell the loop's host to stop.
    fn cancelled(&self) -> bool;
}
