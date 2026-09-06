use async_trait::async_trait;
use brain_protocol::{
    AgentloopAdmission, AgentloopId, ApiError, CreateSessionRequest, EnvironmentCallRequest,
    EnvironmentCallResult, EnvironmentName, EventPage, HostCommand, HostEvent, HostEventAck,
    HostId, HostRegistration, HostResult, LiveEvent, MessageRequest, SessionId, SessionList,
    SessionSummary, ToolAdmission, TurnAnswer, TurnCall,
};

pub struct HostConnection {
    pub commands: tokio::sync::mpsc::Receiver<HostCommand>,
    pub displaced: tokio::sync::oneshot::Receiver<()>,
    pub on_close: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl Drop for HostConnection {
    fn drop(&mut self) {
        if let Some(on_close) = self.on_close.take() {
            on_close();
        }
    }
}

#[async_trait]
pub trait BrainApi: Clone + Send + Sync + 'static {
    async fn register_host(&self) -> Result<HostRegistration, ApiError>;
    async fn connect_host(
        &self,
        host_id: HostId,
        token: String,
    ) -> Result<HostConnection, ApiError>;
    async fn resolve_host(
        &self,
        host_id: HostId,
        token: String,
        result: HostResult,
    ) -> Result<(), ApiError>;
    async fn emit_host_event(
        &self,
        host_id: HostId,
        token: String,
        event: HostEvent,
    ) -> Result<HostEventAck, ApiError>;
    /// One call on an open turn's routes by the Environment running that turn, opened
    /// by the token minted for it.
    async fn turn_call(
        &self,
        session_id: SessionId,
        sequence: u64,
        token: String,
        call: TurnCall,
    ) -> Result<TurnAnswer, ApiError>;
    async fn admit_agentloop(
        &self,
        idempotency_key: String,
        package: Vec<u8>,
    ) -> Result<AgentloopAdmission, ApiError>;
    async fn admit_tool(
        &self,
        idempotency_key: String,
        component: Vec<u8>,
    ) -> Result<ToolAdmission, ApiError>;
    async fn get_agentloop(&self, id: AgentloopId) -> Result<AgentloopAdmission, ApiError>;
    async fn create_session(
        &self,
        idempotency_key: String,
        request: CreateSessionRequest,
    ) -> Result<SessionSummary, ApiError>;
    async fn get_session(&self, session_id: SessionId) -> Result<SessionSummary, ApiError>;
    async fn transcript(
        &self,
        session_id: SessionId,
    ) -> Result<brain_protocol::SessionTranscript, ApiError>;
    async fn list_sessions(&self) -> Result<SessionList, ApiError>;
    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<SessionSummary, ApiError>;
    async fn call_environment(
        &self,
        session_id: SessionId,
        environment: EnvironmentName,
        name: String,
        idempotency_key: String,
        request: EnvironmentCallRequest,
    ) -> Result<EnvironmentCallResult, ApiError>;
    async fn events(
        &self,
        session_id: SessionId,
        after: Option<u64>,
    ) -> Result<EventPage, ApiError>;
    /// Every record appended from now on, for the selected session.
    ///
    /// Opened *before* the page a stream starts with, so a record appended between the
    /// two arrives here instead of being lost in the gap; the caller drops what the page
    /// already carried, by sequence. Falling behind loses records rather than holding up
    /// a turn — the journal is the record, and `after` reads it back.
    fn subscribe(
        &self,
        session_id: &SessionId,
    ) -> tokio::sync::broadcast::Receiver<(SessionId, LiveEvent)>;
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
