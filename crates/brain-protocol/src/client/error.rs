use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::codes::api;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ApiError {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub code: String,
    #[schemars(length(min = 1, max = 4096))]
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
            details: None,
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(api::INVALID_REQUEST, message, false)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(api::UNAUTHORIZED, message, false)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(api::NOT_FOUND, message, false)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(api::CONFLICT, message, false)
    }

    pub fn overloaded(message: impl Into<String>) -> Self {
        Self::new(api::OVERLOADED, message, true)
    }

    pub fn ambiguous(message: impl Into<String>) -> Self {
        Self::new(api::AMBIGUOUS, message, false)
    }

    pub fn executor_failed(message: impl Into<String>) -> Self {
        Self::new(api::EXECUTOR_FAILED, message, true)
    }

    pub fn model_provider_failed(message: impl Into<String>, retryable: bool) -> Self {
        Self::new(api::MODEL_PROVIDER_FAILED, message, retryable)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(api::INTERNAL, message, false)
    }
}
