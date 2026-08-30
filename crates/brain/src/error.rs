#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("invalid session state: {0}")]
    InvalidState(String),
    #[error("executor failed: {0}")]
    Executor(String),
    #[error("journal failed: {0}")]
    Journal(String),
    #[error("operation outcome is ambiguous: {0}")]
    Ambiguous(String),
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
