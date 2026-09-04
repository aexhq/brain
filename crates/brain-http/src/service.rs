use async_trait::async_trait;
use brain_protocol::{
    AgentloopAdmission, AgentloopIdentity, ApiError, CreateSessionRequest, EnvironmentCallRequest,
    EnvironmentCallResult, EnvironmentId, EventPage, LiveEvent, MessageRequest, Outcome, SessionId,
    SessionList, SessionSummary,
};

#[async_trait]
pub trait BrainApi: Clone + Send + Sync + 'static {
    async fn admit_agentloop(
        &self,
        idempotency_key: String,
        package: Vec<u8>,
    ) -> Result<AgentloopAdmission, ApiError>;
    async fn get_agentloop(
        &self,
        digest: AgentloopIdentity,
    ) -> Result<AgentloopAdmission, ApiError>;
    async fn create_session(
        &self,
        idempotency_key: String,
        request: CreateSessionRequest,
    ) -> Result<SessionSummary, ApiError>;
    async fn get_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError>;
    async fn list_sessions(&self) -> Result<SessionList, ApiError>;
    async fn create_environment(
        &self,
        idempotency_key: String,
        request: brain_protocol::CreateEnvironmentRequest,
    ) -> Result<brain_protocol::EnvironmentSummary, ApiError>;
    async fn get_environment(
        &self,
        environment_id: EnvironmentId,
    ) -> Result<brain_protocol::EnvironmentSummary, ApiError>;
    async fn list_environments(&self) -> Result<brain_protocol::EnvironmentList, ApiError>;
    /// Refused while a session is still attached.
    async fn delete_environment(
        &self,
        environment_id: EnvironmentId,
        idempotency_key: String,
    ) -> Result<(), ApiError>;
    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<SessionSummary, ApiError>;
    async fn call_environment(
        &self,
        session_id: SessionId,
        environment_id: EnvironmentId,
        name: String,
        idempotency_key: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError>;
    async fn events(
        &self,
        session_id: SessionId,
        after: Option<u64>,
    ) -> Result<EventPage, ApiError>;
    /// Every record appended from now on, for every session.
    ///
    /// Opened *before* the page a stream starts with, so a record appended between the
    /// two arrives here instead of being lost in the gap; the caller drops what the page
    /// already carried, by sequence. Falling behind loses records rather than holding up
    /// a turn — the journal is the record, and `after` reads it back.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(SessionId, LiveEvent)>;
    /// The session's share key: the scoped credential that authorizes the serve feed
    /// and the tool-results endpoint for this session, and nothing else. Deterministic
    /// for the session's life, so it can be recomputed on every request.
    fn share_key(&self, session_id: &SessionId) -> String;
    /// Names of the session's client-hosted tools — the vocabulary the serve feed
    /// accepts in its `tools` filter.
    async fn client_tool_names(&self, session_id: SessionId) -> Result<Vec<String>, ApiError>;
    /// Answers a parked client-hosted tool call with its outcome. The call is named by
    /// the sequence of its `tool_call_started` record.
    async fn resolve_tool_call(
        &self,
        session_id: SessionId,
        sequence: u64,
        idempotency_key: String,
        outcome: Outcome,
    ) -> Result<(), ApiError>;
    async fn cancel_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError>;
    async fn end_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<SessionSummary, ApiError>;
    async fn delete_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError>;
    async fn live(&self) -> bool;
    async fn ready(&self) -> bool;
}
