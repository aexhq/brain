use async_trait::async_trait;
use brain_protocol::SessionId;

#[async_trait]
pub trait SessionOwnership: Send + Sync + 'static {
    async fn claim_new(&self, session_id: &SessionId) -> Result<(), brain::Error>;
    async fn authorize_mutation(&self, session_id: &SessionId) -> Result<(), brain::Error>;
    async fn release(&self, session_id: &SessionId) -> Result<(), brain::Error>;
}

pub struct LocalSessionOwnership;

#[async_trait]
impl SessionOwnership for LocalSessionOwnership {
    async fn claim_new(&self, _session_id: &SessionId) -> Result<(), brain::Error> {
        Ok(())
    }

    async fn authorize_mutation(&self, _session_id: &SessionId) -> Result<(), brain::Error> {
        Ok(())
    }

    async fn release(&self, _session_id: &SessionId) -> Result<(), brain::Error> {
        Ok(())
    }
}
