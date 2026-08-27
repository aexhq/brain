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
}
