use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use brain::{Kernel, KernelError, LoopExecutor};
use brain_http::BrainApi;
use brain_loophost::WorkerPool;
use brain_protocol::{
    AdmissionStatus, AgentloopAdmission, AgentloopDigest, ApiError, CreateSessionRequest,
    EventPage, MessageRequest, Session, SessionId, SessionList,
};
use tokio::sync::Mutex;

use crate::{EnvironmentRegistry, LocalSessionOwnership, SessionOwnership};

pub struct ServerResources {
    pub kernel: Kernel,
    pub loops: Arc<WorkerPool>,
    pub environments: Arc<EnvironmentRegistry>,
}

#[derive(Clone)]
pub struct ServerApi {
    resources: Arc<ServerResources>,
    idempotency: Arc<Mutex<()>>,
    session_locks: Arc<Mutex<HashMap<SessionId, Arc<Mutex<()>>>>>,
    ownership: Arc<dyn SessionOwnership>,
}

impl ServerApi {
    pub fn new(resources: ServerResources) -> Self {
        Self {
            resources: Arc::new(resources),
            idempotency: Arc::new(Mutex::new(())),
            session_locks: Arc::new(Mutex::new(HashMap::new())),
            ownership: Arc::new(LocalSessionOwnership),
        }
    }

    pub fn with_ownership(
        resources: ServerResources,
        ownership: Arc<dyn SessionOwnership>,
    ) -> Self {
        Self {
            resources: Arc::new(resources),
            idempotency: Arc::new(Mutex::new(())),
            session_locks: Arc::new(Mutex::new(HashMap::new())),
            ownership,
        }
    }

    async fn session_lock(&self, session_id: &SessionId) -> Arc<Mutex<()>> {
        self.session_locks
            .lock()
            .await
            .entry(session_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn replay<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, ApiError> {
        serde_json::from_value(value).map_err(|error| internal(error.to_string()))
    }
}

#[async_trait]
impl BrainApi for ServerApi {
    async fn admit_agentloop(
        &self,
        idempotency_key: String,
        package: Vec<u8>,
    ) -> Result<AgentloopAdmission, ApiError> {
        let _guard = self.idempotency.lock().await;
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get("admit_agentloop", &idempotency_key, &package)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let digest = self
            .resources
            .loops
            .admit(package.clone())
            .await
            .map_err(loop_error)?;
        let admission = AgentloopAdmission {
            digest,
            status: AdmissionStatus::Admitted,
            error: None,
        };
        self.resources
            .kernel
            .idempotency_put(
                "admit_agentloop",
                &idempotency_key,
                &package,
                &serde_json::to_value(&admission).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(admission)
    }

    async fn get_agentloop(&self, digest: AgentloopDigest) -> Result<AgentloopAdmission, ApiError> {
        if !valid_digest(digest.as_str()) {
            return Err(ApiError::invalid_request(
                "Agentloop digest must be 64 lowercase hexadecimal characters",
            ));
        }
        if !self
            .resources
            .loops
            .status(&digest)
            .await
            .map_err(loop_error)?
        {
            return Err(not_found("Agentloop is not admitted"));
        }
        Ok(AgentloopAdmission {
            digest,
            status: AdmissionStatus::Admitted,
            error: None,
        })
    }

    async fn create_session(
        &self,
        idempotency_key: String,
        request: CreateSessionRequest,
    ) -> Result<Session, ApiError> {
        let _guard = self.idempotency.lock().await;
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get("create_session", &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        if !self
            .resources
            .loops
            .status(&request.agentloop_digest)
            .await
            .map_err(loop_error)?
        {
            return Err(ApiError::invalid_request(
                "session Agentloop has not been admitted",
            ));
        }
        let creation = self
            .resources
            .kernel
            .begin_session(&request)
            .map_err(api_error)?;
        let session_id = creation.session_id().clone();
        if let Err(error) = self.ownership.claim_new(creation.session_id()).await {
            creation
                .fail("session_ownership_failed", &error.to_string())
                .map_err(api_error)?;
            return Err(api_error(error));
        }
        let prepared = self
            .resources
            .environments
            .prepare_session(creation, request.clone())
            .await;
        let (handle, _) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.ownership
                    .release(&session_id)
                    .await
                    .map_err(api_error)?;
                return Err(api_error(error));
            }
        };
        let session = self
            .resources
            .kernel
            .session(handle.id())
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
                "create_session",
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn get_session(&self, session_id: SessionId) -> Result<Session, ApiError> {
        self.resources
            .kernel
            .session(&session_id)
            .map_err(api_error)
    }

    async fn list_sessions(&self) -> Result<SessionList, ApiError> {
        Ok(SessionList {
            sessions: self.resources.kernel.sessions().map_err(api_error)?,
        })
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        idempotency_key: String,
        request: MessageRequest,
    ) -> Result<Session, ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id).await;
        let _guard = lock.lock().await;
        let scope = format!("session:{session_id}:message");
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let session = self
            .resources
            .kernel
            .handle(&session_id)
            .map_err(api_error)?
            .message(request.clone())
            .await
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
                &scope,
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn events(
        &self,
        session_id: SessionId,
        after: Option<u64>,
    ) -> Result<EventPage, ApiError> {
        self.resources
            .kernel
            .events(&session_id, after.unwrap_or(0), 1_000)
            .map_err(api_error)
    }

    async fn cancel_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let request = (session_id.clone(), "cancel");
        let scope = format!("session:{session_id}:cancel");
        let _guard = self.idempotency.lock().await;
        if self
            .resources
            .kernel
            .idempotency_get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.resources
            .kernel
            .handle(&session_id)
            .map_err(api_error)?
            .cancel()
            .await
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)
    }

    async fn end_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<Session, ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id).await;
        let _guard = lock.lock().await;
        let request = (session_id.clone(), "end");
        let scope = format!("session:{session_id}:end");
        if let Some(saved) = self
            .resources
            .kernel
            .idempotency_get(&scope, &idempotency_key, &request)
            .map_err(api_error)?
        {
            return Self::replay(saved);
        }
        let sealed = self
            .resources
            .kernel
            .sealed_config(&session_id)
            .map_err(api_error)?;
        self.resources
            .environments
            .release_session(&self.resources.kernel, &session_id, &sealed)
            .await
            .map_err(api_error)?;
        let session = self
            .resources
            .kernel
            .end_after_lifecycle(&session_id)
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(
                &scope,
                &idempotency_key,
                &request,
                &serde_json::to_value(&session).map_err(|error| internal(error.to_string()))?,
            )
            .map_err(api_error)?;
        Ok(session)
    }

    async fn delete_session(
        &self,
        session_id: SessionId,
        idempotency_key: String,
    ) -> Result<(), ApiError> {
        self.ownership
            .authorize_mutation(&session_id)
            .await
            .map_err(api_error)?;
        let lock = self.session_lock(&session_id).await;
        let _guard = lock.lock().await;
        let request = (session_id.clone(), "delete");
        let scope = format!("session:{session_id}:delete");
        let _idempotency = self.idempotency.lock().await;
        if self
            .resources
            .kernel
            .idempotency_get::<_>(&scope, &idempotency_key, &request)
            .map_err(api_error)?
            .is_some()
        {
            return Ok(());
        }
        self.resources
            .kernel
            .delete_ended(&session_id)
            .map_err(api_error)?;
        self.resources
            .kernel
            .idempotency_put(&scope, &idempotency_key, &request, &serde_json::json!({}))
            .map_err(api_error)?;
        self.session_locks.lock().await.remove(&session_id);
        self.ownership
            .release(&session_id)
            .await
            .map_err(api_error)?;
        Ok(())
    }

    async fn live(&self) -> bool {
        true
    }

    async fn ready(&self) -> bool {
        self.resources.loops.ready().await.is_ok()
    }
}

pub struct WorkerLoopExecutor(pub Arc<WorkerPool>);

#[async_trait]
impl LoopExecutor for WorkerLoopExecutor {
    async fn activate(
        &self,
        agentloop: &AgentloopDigest,
        input: brain_protocol::ActivationInput,
    ) -> Result<brain_protocol::ActivationOutput, KernelError> {
        self.0
            .activate(agentloop.clone(), input)
            .await
            .map_err(KernelError::Executor)
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn loop_error(message: String) -> ApiError {
    if message.contains("queue is full") {
        ApiError {
            code: "overloaded".into(),
            message,
            retryable: true,
            details: None,
        }
    } else {
        ApiError::invalid_request(message)
    }
}

fn api_error(error: KernelError) -> ApiError {
    let message = error.to_string();
    match error {
        KernelError::InvalidState(_) if message.contains("not found") => not_found(message),
        KernelError::InvalidState(_) if message.contains("idempotency key") => ApiError {
            code: "conflict".into(),
            message,
            retryable: false,
            details: None,
        },
        KernelError::InvalidState(_) => ApiError::invalid_request(message),
        KernelError::Ambiguous(_) => ApiError {
            code: "ambiguous".into(),
            message,
            retryable: false,
            details: None,
        },
        KernelError::Executor(_) => ApiError {
            code: "executor_failed".into(),
            message,
            retryable: true,
            details: None,
        },
        KernelError::Journal(_) => internal(message),
    }
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "not_found".into(),
        message: message.into(),
        retryable: false,
        details: None,
    }
}

fn internal(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "internal".into(),
        message: message.into(),
        retryable: false,
        details: None,
    }
}
