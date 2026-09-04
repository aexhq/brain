use brain_protocol::codes::api;

/// What a session, its journal, or one of its executors can fail with.
///
/// Each variant is one API error code; the message is for people. A caller that needs to
/// branch matches the variant, never the text.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid session state: {0}")]
    InvalidState(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("overloaded: {0}")]
    Overloaded(String),
    #[error("executor failed: {0}")]
    Executor(String),
    #[error("journal failed: {0}")]
    Journal(String),
    #[error("operation outcome is ambiguous: {0}")]
    Ambiguous(String),
    /// The turn was cancelled, or timed out, while this was in flight.
    #[error("cancelled: {0}")]
    Cancelled(String),
    /// The turn asked for more than its budget allows.
    #[error("budget exceeded: {0}")]
    Budget(String),
    /// A model provider answered with a complete non-success response before
    /// anything streamed. Typed so the retry policy can branch on the status
    /// instead of parsing it back out of a message.
    #[error("model provider returned {status}: {body}")]
    ProviderStatus {
        status: u16,
        body: String,
        retry_after_ms: Option<u64>,
    },
}

impl Error {
    /// The API error code this failure surfaces as.
    pub fn code(&self) -> &'static str {
        match self {
            Error::InvalidState(_) => api::INVALID_REQUEST,
            Error::NotFound(_) => api::NOT_FOUND,
            Error::Conflict(_) => api::CONFLICT,
            Error::Overloaded(_) => api::OVERLOADED,
            Error::Executor(_) => api::EXECUTOR_FAILED,
            Error::Journal(_) => api::INTERNAL,
            Error::Ambiguous(_) => api::AMBIGUOUS,
            Error::Cancelled(_) => brain_protocol::codes::failure::CANCELLED,
            Error::Budget(_) => brain_protocol::codes::failure::DECISION_LIMIT,
            Error::ProviderStatus { .. } => api::MODEL_PROVIDER_FAILED,
        }
    }

    /// Whether repeating the whole operation can help.
    pub fn retryable(&self) -> bool {
        match self {
            Error::Overloaded(_) | Error::Executor(_) => true,
            // The executor already retried what was worth retrying in place; a
            // whole-turn retry can still help for transient statuses.
            Error::ProviderStatus { status, .. } => matches!(status, 408 | 429) || *status >= 500,
            _ => false,
        }
    }
}
