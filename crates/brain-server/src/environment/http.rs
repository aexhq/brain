//! An Environment reached over HTTP at the URL its entry names, with the credential
//! the session sealed for it.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use brain_protocol::{
    Driver, ENVIRONMENT_CONTRACT, Environment, EnvironmentCommand, EnvironmentOperation,
    EnvironmentReceipt, EnvironmentRequest, EnvironmentResponse,
};

use super::{EnvironmentAdapter, Services};
use crate::CredentialStore;

pub struct HttpEnvironmentAdapter {
    client: reqwest::Client,
    credentials: Arc<dyn CredentialStore>,
    /// How long a turn may run before the session cancels it: the one operation that
    /// outlives the client's ordinary request timeout. `None` means no bound.
    max_turn: Option<Duration>,
    max_response_bytes: usize,
}

impl HttpEnvironmentAdapter {
    pub fn new(
        client: reqwest::Client,
        credentials: Arc<dyn CredentialStore>,
        limits: &brain::Limits,
        server_limits: &crate::ServerLimits,
    ) -> Self {
        Self {
            client,
            credentials,
            max_turn: limits.max_turn(),
            max_response_bytes: crate::limits::ceiling(
                server_limits.max_environment_response_bytes,
            ),
        }
    }
}

/// Where an HTTP Environment may live: HTTPS, or plain HTTP on a literal loopback
/// address, without credentials, query, or fragment in the URL itself.
pub fn validate_url(url: &str) -> Result<(), brain::Error> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| brain::Error::InvalidState(format!("Environment url {url:?}: {error}")))?;
    let loopback_http = parsed.scheme() == "http"
        && parsed
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback());
    if !(parsed.scheme() == "https" || loopback_http)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(brain::Error::InvalidState(format!(
            "Environment url {url:?} must use HTTPS or literal loopback HTTP and cannot contain credentials, query, or fragment"
        )));
    }
    Ok(())
}

#[async_trait]
impl EnvironmentAdapter for HttpEnvironmentAdapter {
    async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services<'_>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let Driver::Http { url, .. } = &environment.driver else {
            return Err(brain::Error::InvalidState(
                "the HTTP adapter was handed an Environment it does not reach".into(),
            ));
        };
        let credential = self
            .credentials
            .environment(&operation.session_id, &environment.name)?;
        let command = EnvironmentCommand {
            contract: ENVIRONMENT_CONTRACT.into(),
            operation: operation.clone(),
        };
        let mut request = self
            .client
            .post(format!("{}/v1/operations", url.trim_end_matches('/')))
            .json(&command);
        if let Some(credential) = &credential {
            request = request.bearer_auth(credential.as_str());
        }
        // A turn runs for as long as the loop needs, under the session's wall-time bound
        // rather than the client's; a cancellation ends the wait here, and the callbacks
        // the loop makes after it fail with `cancelled`, which is how it learns to stop.
        let turn = match (&operation.request, &services) {
            (EnvironmentRequest::Turn { .. }, Services::Turn(services)) => {
                Some(Arc::clone(services))
            }
            _ => None,
        };
        if turn.is_some() {
            request = request.timeout(
                self.max_turn
                    .unwrap_or(Duration::from_secs(10 * 365 * 24 * 60 * 60)),
            );
        }
        let sent = request.send();
        let cancelled = async {
            match turn {
                Some(services) => {
                    while !services.cancelled() {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
                None => std::future::pending::<()>().await,
            }
        };
        let mut response = tokio::select! {
            sent = sent => sent.map_err(|error| {
                brain::Error::Ambiguous(format!("Environment transport outcome is unknown: {error}"))
            })?,
            () = cancelled => return Err(brain::Error::Cancelled("turn cancelled".into())),
        };
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_bytes as u64)
        {
            return Err(brain::Error::Ambiguous(format!(
                "Environment response exceeds {} bytes",
                self.max_response_bytes
            )));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| brain::Error::Ambiguous(error.to_string()))?
        {
            if chunk.len() > self.max_response_bytes - body.len() {
                return Err(brain::Error::Ambiguous(format!(
                    "Environment response exceeds {} bytes",
                    self.max_response_bytes
                )));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(brain::Error::Ambiguous(format!(
                "Environment returned {status}: {}",
                String::from_utf8_lossy(&body[..body.len().min(16 * 1024)])
            )));
        }
        let response: EnvironmentResponse = serde_json::from_slice(&body).map_err(|error| {
            brain::Error::Ambiguous(format!("Environment terminal receipt is invalid: {error}"))
        })?;
        if response.contract != ENVIRONMENT_CONTRACT || response.sequence != operation.sequence {
            return Err(brain::Error::Ambiguous(
                "Environment response correlation does not match the operation".into(),
            ));
        }
        Ok(response.receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn an_environment_url_is_https_or_loopback_http_and_nothing_more() {
        validate_url("https://sandbox.example/base").unwrap();
        validate_url("http://127.0.0.1:8090").unwrap();
        validate_url("http://[::1]:8090/").unwrap();
        for wrong in [
            "http://sandbox.example",
            "https://user:pass@sandbox.example",
            "https://sandbox.example/?token=x",
            "https://sandbox.example/#frag",
            "ftp://sandbox.example",
            "not a url",
        ] {
            assert!(validate_url(wrong).is_err(), "{wrong} must be refused");
        }
    }

    #[tokio::test]
    async fn oversized_chunked_response_is_rejected_before_waiting_for_eof() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            let body = vec![b'x'; 1024 + 1];
            socket
                .write_all(format!("{:x}\r\n", body.len()).as_bytes())
                .await
                .unwrap();
            socket.write_all(&body).await.unwrap();
            socket.write_all(b"\r\n").await.unwrap();
            std::future::pending::<()>().await;
        });
        let environment = Environment {
            name: brain_protocol::EnvironmentName::new("large"),
            driver: Driver::Http {
                url: format!("http://{address}"),
                credential: None,
            },
            configuration: serde_json::json!({}),
        };
        let operation = EnvironmentOperation {
            sequence: 1,
            environment: environment.name.clone(),
            session_id: brain_protocol::SessionId::new("ses_test"),
            request: EnvironmentRequest::Teardown,
        };
        let directory = tempfile::tempdir().unwrap();
        let credentials =
            Arc::new(crate::metadata::ServerMetadata::open(directory.path()).unwrap());
        let adapter = HttpEnvironmentAdapter::new(
            reqwest::Client::new(),
            credentials,
            &brain::Limits {
                max_turn_secs: 0,
                ..Default::default()
            },
            &crate::ServerLimits {
                max_environment_response_bytes: 1024,
                ..Default::default()
            },
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            adapter.execute(&environment, &operation, Services::None),
        )
        .await;
        server.abort();
        assert!(
            matches!(result.unwrap(), Err(brain::Error::Ambiguous(message)) if message.contains("exceeds 1024 bytes"))
        );
    }
}
