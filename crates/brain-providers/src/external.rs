//! Generic HTTP adapter for host-executed tools.
//!
//! Tool declarations live in the sealed session prefix, while the executor URL and service
//! credential are process configuration. This keeps customer prompts and model arguments from
//! choosing where privileged host work runs.

use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use brain::adapter::ToolExecutor;
use brain::{BrainError, Result};

#[derive(Clone)]
pub struct HttpExternalToolExecutor {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    bearer: Option<String>,
    timeout: Duration,
    capabilities: HashSet<String>,
}

impl HttpExternalToolExecutor {
    /// Construct the host-side adapter. The service is deliberately constrained to a literal
    /// loopback HTTP endpoint: it is a composition-owned same-host sidecar, not a general-purpose
    /// outbound webhook. Inherited proxy variables and redirects are disabled as additional
    /// credential/SSRF boundaries.
    pub fn new(
        endpoint: impl Into<String>,
        bearer: Option<String>,
        timeout: Duration,
        capabilities: impl IntoIterator<Item = String>,
    ) -> Result<Self> {
        let endpoint = endpoint.into().parse::<reqwest::Url>().map_err(|error| {
            BrainError::Invalid(format!("external tool executor URL is invalid: {error}"))
        })?;
        if endpoint.scheme() != "http"
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(BrainError::Invalid(
                "external tool executor must be an http:// loopback URL without credentials, query, or fragment"
                    .into(),
            ));
        }
        let loopback = endpoint
            .host_str()
            // `url` retains brackets in `host_str()` for an IPv6 literal. `IpAddr` expects the
            // bare address, so normalize only that syntactic wrapper before the exact loopback
            // classification.
            .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !loopback {
            return Err(BrainError::Invalid(
                "external tool executor host must be a literal loopback address".into(),
            ));
        }
        if bearer.as_ref().is_some_and(|token| {
            reqwest::header::HeaderValue::try_from(format!("Bearer {token}")).is_err()
        }) {
            return Err(BrainError::Invalid(
                "external tool executor token is not a valid HTTP bearer value".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .build()
            .map_err(|error| {
                BrainError::Invalid(format!("external tool executor client: {error}"))
            })?;
        Ok(Self {
            client,
            endpoint,
            bearer,
            timeout,
            capabilities: capabilities.into_iter().collect(),
        })
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
        let body = serde_json::to_vec(&request).map_err(|error| {
            BrainError::Protocol(format!("external tool executor request: {error}"))
        })?;
        if body.len() > brain_protocol::MAX_EXTERNAL_TOOL_REQUEST_BYTES {
            return Err(BrainError::Protocol(format!(
                "external tool executor request exceeds {} bytes",
                brain_protocol::MAX_EXTERNAL_TOOL_REQUEST_BYTES
            )));
        }
        let mut send = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(bearer) = &self.bearer {
            send = send.bearer_auth(bearer);
        }
        let execute = async move {
            let response = send.send().await.map_err(|error| {
                BrainError::Transport(format!("external tool executor: {error}"))
            })?;
            let status = response.status();
            if response.content_length().is_some_and(|bytes| {
                bytes > brain_protocol::MAX_EXTERNAL_TOOL_RESPONSE_BYTES as u64
            }) {
                return Err(BrainError::Protocol(format!(
                    "external tool executor response exceeds {} bytes",
                    brain_protocol::MAX_EXTERNAL_TOOL_RESPONSE_BYTES
                )));
            }
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| {
                    BrainError::Transport(format!("external tool executor body: {error}"))
                })?;
                if body.len().saturating_add(chunk.len())
                    > brain_protocol::MAX_EXTERNAL_TOOL_RESPONSE_BYTES
                {
                    return Err(BrainError::Protocol(format!(
                        "external tool executor response exceeds {} bytes",
                        brain_protocol::MAX_EXTERNAL_TOOL_RESPONSE_BYTES
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            if !status.is_success() {
                let preview = String::from_utf8_lossy(&body);
                return Err(BrainError::Transport(format!(
                    "external tool executor returned {status}: {}",
                    preview.chars().take(512).collect::<String>()
                )));
            }
            serde_json::from_slice(&body).map_err(|error| {
                BrainError::Protocol(format!("external tool executor response: {error}"))
            })
        };
        tokio::select! {
            result = tokio::time::timeout(self.timeout, execute) => result
                .map_err(|_| BrainError::Transport("external tool executor timed out".into()))?,
            () = cancel.cancelled() => Err(BrainError::Cancelled),
        }
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
        )
        .unwrap();
        assert!(executor.supports("example.lookup.v1"));
        assert!(!executor.supports("lookup"));
        assert!(!executor.supports("example.delete.v1"));
    }

    #[test]
    fn endpoint_is_literal_loopback_http() {
        for endpoint in [
            "https://127.0.0.1:8080/tools",
            "http://localhost:8080/tools",
            "http://10.0.0.1:8080/tools",
            "http://user:secret@127.0.0.1:8080/tools",
        ] {
            assert!(
                HttpExternalToolExecutor::new(
                    endpoint,
                    None,
                    Duration::from_secs(1),
                    std::iter::empty(),
                )
                .is_err()
            );
        }
        assert!(
            HttpExternalToolExecutor::new(
                "http://[::1]:8080/tools",
                None,
                Duration::from_secs(1),
                std::iter::empty(),
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn oversized_request_is_rejected_before_network_dispatch() {
        let executor = HttpExternalToolExecutor::new(
            "http://127.0.0.1:1/tools",
            None,
            Duration::from_secs(1),
            ["example.lookup.v1".to_string()],
        )
        .unwrap();
        let request = serde_json::from_value(serde_json::json!({
            "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0",
            "turn_id": "trn_01HZX8Y2K3M4N5P6Q7R8S9U1",
            "agent_id": "root",
            "call_id": "call_01HZX8Y2K3M4N5P6Q7R8S9W3",
            "name": "lookup",
            "input": null,
            "context": {
                "oversized": "x".repeat(brain_protocol::MAX_EXTERNAL_TOOL_REQUEST_BYTES),
            },
        }))
        .unwrap();
        let error = executor
            .call("example.lookup.v1", request, CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, BrainError::Protocol(_)));
        assert!(error.to_string().contains("request exceeds"));
    }
}
