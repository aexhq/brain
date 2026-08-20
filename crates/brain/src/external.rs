//! Generic HTTP adapter for host-executed tools.
//!
//! Tool declarations live in the sealed session prefix, while the executor URL and service
//! credential are process configuration. This keeps customer prompts and model arguments from
//! choosing where privileged host work runs.

use std::collections::HashSet;
use std::time::Duration;

use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use tokio_util::sync::CancellationToken;

use crate::adapter::ToolExecutor;
use crate::{BrainError, Result};

const MAX_RESPONSE_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub struct HttpExternalToolExecutor {
    client: reqwest::Client,
    endpoint: String,
    bearer: Option<String>,
    timeout: Duration,
    capabilities: HashSet<String>,
}

impl HttpExternalToolExecutor {
    pub fn new(
        endpoint: impl Into<String>,
        bearer: Option<String>,
        timeout: Duration,
        capabilities: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
            bearer,
            timeout,
            capabilities: capabilities.into_iter().collect(),
        }
    }
}

impl std::fmt::Debug for HttpExternalToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpExternalToolExecutor")
            .field("endpoint", &self.endpoint)
            .field("bearer", &self.bearer.as_ref().map(|_| "<redacted>"))
            .field("timeout", &self.timeout)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

#[async_trait::async_trait]
impl ToolExecutor for HttpExternalToolExecutor {
    fn supports(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }

    async fn call(
        &self,
        capability: &str,
        mut request: ExternalToolCallRequest,
        cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        request
            .context
            .insert("brain.capability".into(), capability.into());
        let mut send = self.client.post(&self.endpoint).json(&request);
        if let Some(bearer) = &self.bearer {
            send = send.bearer_auth(bearer);
        }
        let response = tokio::select! {
            response = tokio::time::timeout(self.timeout, send.send()) => {
                response
                    .map_err(|_| BrainError::Transport("external tool executor timed out".into()))?
                    .map_err(|error| BrainError::Transport(format!("external tool executor: {error}")))?
            }
            () = cancel.cancelled() => return Err(BrainError::Cancelled),
        };
        let status = response.status();
        let bytes = tokio::select! {
            bytes = response.bytes() => bytes
                .map_err(|error| BrainError::Transport(format!("external tool executor body: {error}")))?,
            () = cancel.cancelled() => return Err(BrainError::Cancelled),
        };
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(BrainError::Protocol(format!(
                "external tool executor response exceeds {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        if !status.is_success() {
            let preview = String::from_utf8_lossy(&bytes);
            return Err(BrainError::Transport(format!(
                "external tool executor returned {status}: {}",
                preview.chars().take(512).collect::<String>()
            )));
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            BrainError::Protocol(format!("external tool executor response: {error}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicitly_registered_capabilities_are_advertised() {
        let executor = HttpExternalToolExecutor::new(
            "http://127.0.0.1:1",
            None,
            Duration::from_secs(1),
            ["example.lookup.v1".to_string()],
        );
        assert!(executor.supports("example.lookup.v1"));
        assert!(!executor.supports("lookup"));
        assert!(!executor.supports("example.delete.v1"));
    }
}
