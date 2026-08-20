//! Generic HTTP adapter for host-executed tools.
//!
//! Tool declarations live in the sealed session prefix, while the executor URL and service
//! credential are process configuration. This keeps customer prompts and model arguments from
//! choosing where privileged host work runs.

use std::time::Duration;

use aex_contracts::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use tokio_util::sync::CancellationToken;

use crate::adapter::ExternalToolExecutor;
use crate::{BrainError, Result};

const MAX_RESPONSE_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub struct HttpExternalToolExecutor {
    client: reqwest::Client,
    endpoint: String,
    bearer: Option<String>,
    timeout: Duration,
}

impl HttpExternalToolExecutor {
    pub fn new(endpoint: impl Into<String>, bearer: Option<String>, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
            bearer,
            timeout,
        }
    }
}

impl std::fmt::Debug for HttpExternalToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpExternalToolExecutor")
            .field("endpoint", &self.endpoint)
            .field("bearer", &self.bearer.as_ref().map(|_| "<redacted>"))
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[async_trait::async_trait]
impl ExternalToolExecutor for HttpExternalToolExecutor {
    async fn call(
        &self,
        request: ExternalToolCallRequest,
        cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
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
        serde_json::from_slice(&bytes)
            .map_err(|error| BrainError::Protocol(format!("external tool executor response: {error}")))
    }
}
