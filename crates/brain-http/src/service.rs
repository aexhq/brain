use async_trait::async_trait;
use brain_protocol::{ApiError, CreateSessionRequest, EventPage, MessageRequest, Session, SessionId};

#[async_trait]
pub trait BrainApi: Clone + Send + Sync + 'static {
    async fn create_session(&self, idempotency_key: String, request: CreateSessionRequest) -> Result<Session, ApiError>;
    async fn get_session(&self, session_id: SessionId) -> Result<Session, ApiError>;
    async fn send_message(&self, session_id: SessionId, idempotency_key: String, request: MessageRequest) -> Result<Session, ApiError>;
    async fn events(&self, session_id: SessionId, after: Option<u64>) -> Result<EventPage, ApiError>;
    async fn live(&self) -> bool;
    async fn ready(&self) -> bool;
}
