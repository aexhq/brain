use async_trait::async_trait;
use brain_protocol::{
    ENVIRONMENT_CONTRACT, EnvironmentBinding, EnvironmentCommand, EnvironmentOperation,
    EnvironmentReceipt, EnvironmentRequest, EnvironmentResponse,
};

const MAX_ENVIRONMENT_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[async_trait]
pub trait EnvironmentAdapter: Send + Sync + 'static {
    async fn send(
        &self,
        endpoint: &str,
        binding: &EnvironmentBinding,
        operation: &EnvironmentOperation<EnvironmentRequest>,
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
        operation: &EnvironmentOperation<EnvironmentRequest>,
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
        let response = request.send().await.map_err(|error| {
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
        let body = response
            .bytes()
            .await
            .map_err(|error| brain::Error::Ambiguous(error.to_string()))?;
        if body.len() > MAX_ENVIRONMENT_RESPONSE_BYTES {
            return Err(brain::Error::Ambiguous(
                "Environment response exceeds 2 MiB".into(),
            ));
        }
        if !status.is_success() {
            return Err(brain::Error::Executor(format!(
                "Environment returned {status}: {}",
                String::from_utf8_lossy(&body[..body.len().min(16 * 1024)])
            )));
        }
        let response: EnvironmentResponse = serde_json::from_slice(&body).map_err(|error| {
            brain::Error::Ambiguous(format!("Environment terminal receipt is invalid: {error}"))
        })?;
        if response.contract != ENVIRONMENT_CONTRACT || response.sequence != operation.sequence {
            return Err(brain::Error::InvalidState(
                "Environment response correlation does not match the operation".into(),
            ));
        }
        Ok(response.receipt)
    }
}
