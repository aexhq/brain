use async_trait::async_trait;
use brain_protocol::{
    ENVIRONMENT_CONTRACT, EnvironmentBinding, EnvironmentCommand, EnvironmentOperation,
    EnvironmentReceipt, EnvironmentResponse,
};

const MAX_ENVIRONMENT_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[async_trait]
pub trait EnvironmentAdapter: Send + Sync + 'static {
    async fn send(
        &self,
        endpoint: &str,
        binding: &EnvironmentBinding,
        operation: &EnvironmentOperation,
    ) -> Result<EnvironmentReceipt, brain::Error>;
}

pub struct HttpEnvironmentAdapter {
    client: reqwest::Client,
    bearer_token: Option<String>,
}

impl HttpEnvironmentAdapter {
    pub fn new(client: reqwest::Client, bearer_token: Option<String>) -> Self {
        Self {
            client,
            bearer_token,
        }
    }
}

#[async_trait]
impl EnvironmentAdapter for HttpEnvironmentAdapter {
    async fn send(
        &self,
        endpoint: &str,
        binding: &EnvironmentBinding,
        operation: &EnvironmentOperation,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let command = EnvironmentCommand {
            contract: ENVIRONMENT_CONTRACT.into(),
            binding: binding.clone(),
            operation: operation.clone(),
        };
        let mut request = self
            .client
            .post(format!("{}/v1/operations", endpoint.trim_end_matches('/')))
            .json(&command);
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token);
        }
        let mut response = request.send().await.map_err(|error| {
            brain::Error::Ambiguous(format!("Environment transport outcome is unknown: {error}"))
        })?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ENVIRONMENT_RESPONSE_BYTES as u64)
        {
            return Err(brain::Error::Ambiguous(
                "Environment response exceeds 2 MiB".into(),
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| brain::Error::Ambiguous(error.to_string()))?
        {
            if chunk.len() > MAX_ENVIRONMENT_RESPONSE_BYTES - body.len() {
                return Err(brain::Error::Ambiguous(
                    "Environment response exceeds 2 MiB".into(),
                ));
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
            let body = vec![b'x'; MAX_ENVIRONMENT_RESPONSE_BYTES + 1];
            socket
                .write_all(format!("{:x}\r\n", body.len()).as_bytes())
                .await
                .unwrap();
            socket.write_all(&body).await.unwrap();
            socket.write_all(b"\r\n").await.unwrap();
            std::future::pending::<()>().await;
        });
        let environment_id = brain_protocol::EnvironmentId::new("env_large");
        let binding = EnvironmentBinding {
            environment_id: environment_id.clone(),
            directory_generation: 1,
        };
        let operation = EnvironmentOperation {
            sequence: 1,
            environment_id,
            session_id: brain_protocol::SessionId::new("ses_test"),
            attachment_id: None,
            request: brain_protocol::EnvironmentRequest::Teardown,
        };
        let adapter = HttpEnvironmentAdapter::new(reqwest::Client::new(), None);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            adapter.send(&format!("http://{address}"), &binding, &operation),
        )
        .await;
        server.abort();
        assert!(
            matches!(result.unwrap(), Err(brain::Error::Ambiguous(message)) if message.contains("exceeds 2 MiB"))
        );
    }
}
