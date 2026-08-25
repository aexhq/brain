//! Composition-owned execution for trusted external capabilities.
//!
<<<<<<< HEAD
//! Brain itself owns the three closed engine capabilities (`brain.subagents`, `brain.storage`,
//! and `brain.sandbox`) and routes managed execution through the typed Environment ports. Arbitrary
//! trusted host capabilities use [`ToolExecutor`]; model-visible names never select code.
=======
//! Trusted host capabilities use [`ToolExecutor`]; model-visible names never select code.
>>>>>>> origin/main

use crate::Result;
use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// What one durable Tool call produced.
#[derive(Debug, Clone)]
pub struct CallOutcome {
    pub outcome: brain_protocol::environment::TerminalOutcome,
    /// Successful structured value before presentation formatting. Brain validates this against
    /// the sealed output schema immediately before it commits the result.
    pub value: Option<Value>,
    pub content: String,
    pub is_error: bool,
    pub exit_code: Option<i64>,
    pub duration_ms: u64,
    pub truncated: bool,
    /// Present only when a host-executed return-direct tool asks Brain to end the turn.
    pub terminal: Option<TurnTerminal>,
}

/// A trusted external executor may return a replayable client value or a structured turn error.
/// Brain does not interpret either payload; it only journals it with the turn terminal.
#[derive(Debug, Clone)]
pub enum TurnTerminal {
    Complete {
        value: Value,
        metadata: std::collections::HashMap<String, String>,
    },
    Fail {
        error: brain_protocol::session::ApiError,
    },
}

impl CallOutcome {
    pub fn failed(content: impl Into<String>) -> Self {
        Self {
            outcome: brain_protocol::environment::TerminalOutcome::Failed,
            value: None,
            content: content.into(),
            is_error: true,
            exit_code: None,
            duration_ms: 0,
            truncated: false,
            terminal: None,
        }
    }
}

/// Trusted host-side execution registered under stable capability identifiers. A composition
/// advertises availability before a session is created; model-visible names never select code.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    fn supports(&self, capability: &str) -> bool;

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse>;
}

/// Default composition for deployments that do not expose host-executed tools.
pub struct DisabledToolExecutor;

#[async_trait::async_trait]
impl ToolExecutor for DisabledToolExecutor {
    fn supports(&self, _capability: &str) -> bool {
        false
    }

    async fn call(
        &self,
        _capability: &str,
        _request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        Err(crate::BrainError::Invalid(
            "no external tool executor is configured on this Brain host".into(),
        ))
    }
}
