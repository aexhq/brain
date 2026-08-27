use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self { code: "invalid_request".into(), message: message.into(), retryable: false, details: None }
    }
}
