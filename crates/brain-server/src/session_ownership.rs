use async_trait::async_trait;
use brain::KernelError;
use brain_protocol::SessionId;

#[async_trait]
pub trait SessionOwnership: Send + Sync + 'static {
    async fn claim_new(&self, session_id: &SessionId) -> Result<(), KernelError>;
    async fn authorize_mutation(&self, session_id: &SessionId) -> Result<(), KernelError>;
    async fn release(&self, session_id: &SessionId) -> Result<(), KernelError>;
}

pub struct LocalSessionOwnership;

#[async_trait]
impl SessionOwnership for LocalSessionOwnership {
    async fn claim_new(&self, _session_id: &SessionId) -> Result<(), KernelError> {
        Ok(())
    }

    async fn authorize_mutation(&self, _session_id: &SessionId) -> Result<(), KernelError> {
        Ok(())
    }

    async fn release(&self, _session_id: &SessionId) -> Result<(), KernelError> {
        Ok(())
    }
}
