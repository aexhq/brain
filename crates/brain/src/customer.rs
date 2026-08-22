//! Transport-neutral customer-app Hand ingress and delivery seams.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::adapter::CallOutcome;
use crate::{BrainError, Result};

pub use brain_protocol::MAX_CUSTOMER_OBSERVATION_BYTES as MAX_CUSTOMER_HTTP_OBSERVATION_BYTES;
/// Leaves deterministic metadata headroom below API Gateway WebSocket's 32 KiB frame maximum.
pub use brain_protocol::MAX_CUSTOMER_WS_FRAME_BYTES;
pub use brain_protocol::{MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES, MAX_CUSTOMER_REGISTRATIONS};

pub const CUSTOMER_MAX_GRANTS_ENV: &str = "BRAIN_CUSTOMER_HAND_MAX_GRANTS";
pub const CUSTOMER_MAX_CONNECTIONS_ENV: &str = "BRAIN_CUSTOMER_HAND_MAX_CONNECTIONS";
pub const CUSTOMER_MAX_PENDING_OPERATIONS_ENV: &str = "BRAIN_CUSTOMER_HAND_MAX_PENDING_OPERATIONS";
pub const CUSTOMER_MAX_PENDING_TERMINAL_BYTES_ENV: &str =
    "BRAIN_CUSTOMER_HAND_MAX_PENDING_TERMINAL_BYTES";
pub const CUSTOMER_MAX_REGISTRATION_BYTES_ENV: &str = "BRAIN_CUSTOMER_HAND_MAX_REGISTRATION_BYTES";

pub const DEFAULT_MAX_CUSTOMER_GRANTS: usize = 4_096;
pub const DEFAULT_MAX_CUSTOMER_CONNECTIONS: usize = 1_024;
pub const DEFAULT_MAX_CUSTOMER_PENDING_OPERATIONS: usize = 256;
pub const DEFAULT_MAX_CUSTOMER_PENDING_TERMINAL_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_CUSTOMER_REGISTRATION_BYTES: usize = 64 * 1024 * 1024;
/// Capacity reserved before an offer can reach customer code. The retained terminal is delivered
/// through this exact bounded observation envelope, so admission can never fail after the effect.
pub const CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES: usize = MAX_CUSTOMER_HTTP_OBSERVATION_BYTES;
const DEFAULT_CUSTOMER_CONNECTION_IDLE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomerGatewayRoute {
    Connect,
    Disconnect,
    Message,
}

/// Trusted facts supplied by a hosted/local gateway adapter after it authenticates the request.
#[derive(Clone, Serialize, Deserialize)]
pub struct CustomerGatewayInput {
    pub route: CustomerGatewayRoute,
    pub connection_id: String,
    pub request_id: String,
    pub route_key: String,
    pub source_ip: String,
    /// Connect carries the short-lived scoped grant from Sec-WebSocket-Protocol. Brain validates
    /// it and derives tenancy; the public API key never enters the socket URL or a register frame.
    pub subprotocol: Option<String>,
    /// Raw text frame. `$connect` and `$disconnect` normally omit it.
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerRegistration {
    pub registration: String,
    pub name: String,
    pub contract_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerConnection {
    pub tenant_id: String,
    pub client_id: String,
    pub process_id: String,
    pub connection_id: String,
    pub epoch: u64,
    pub registrations: Vec<CustomerRegistration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerOperationOffer {
    pub epoch: u64,
    pub operation_id: String,
    pub request_digest: String,
    pub session_id: String,
    pub registration: String,
    pub name: String,
    pub contract_digest: String,
    pub input: Value,
    pub deadline_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerCancelOffer {
    pub epoch: u64,
    pub operation_id: String,
    pub reason: String,
}

/// The exact small WebSocket vocabulary. Hosted adapters serialize this value rather than
/// duplicating Brain's command tags or frame bound.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomerCommand {
    Offer(CustomerOperationOffer),
    Cancel(CustomerCancelOffer),
    Ready {
        epoch: u64,
    },
    Registered {
        epoch: u64,
        batch_id: String,
    },
    Ack {
        epoch: u64,
        operation_id: String,
        request_digest: String,
        terminal_digest: String,
    },
    Heartbeat {
        epoch: u64,
        nonce: String,
    },
    Error {
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        batch_id: Option<String>,
    },
}

impl CustomerCommand {
    pub fn to_frame(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_CUSTOMER_WS_FRAME_BYTES {
            return Err(crate::BrainError::FileTooLarge {
                limit: MAX_CUSTOMER_WS_FRAME_BYTES,
            });
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone)]
pub struct CustomerDeliveryRequest {
    pub connection_id: String,
    pub command: CustomerCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomerDelivery {
    Delivered,
    /// The transport authoritatively reported that the recorded connection is gone (HTTP 410).
    Gone,
    /// Nothing was offered to the application process.
    Unavailable,
    /// The send may have reached the process; Brain must report unknown outcome.
    Unknown,
}

/// Receives authenticated connect/message/disconnect callbacks and durably advances epochs.
#[async_trait]
pub trait CustomerHandIngressPort: Send + Sync {
    async fn receive(&self, input: CustomerGatewayInput) -> Result<()>;
}

/// Sends offers through an injected gateway. Implementations must not select another connection.
#[async_trait]
pub trait CustomerHandDeliveryPort: Send + Sync {
    async fn send(&self, request: CustomerDeliveryRequest) -> Result<CustomerDelivery>;
}

#[derive(Debug, Clone)]
pub struct CustomerTransportConfig {
    pub websocket_url: String,
    pub observation_base_url: String,
    pub grant_ttl: Duration,
    pub observation_ttl: Duration,
    /// Process-wide bounds. Entries are never evicted to admit newer work; callers receive
    /// backpressure until an expiry or an exact terminal acknowledgement releases capacity.
    pub max_grants: usize,
    pub max_connections: usize,
    pub max_pending_operations: usize,
    /// Process-wide bytes reserved for retained terminal observation envelopes. One full
    /// observation allowance is reserved before each new operation can be offered.
    pub max_pending_terminal_bytes: usize,
    /// Process-wide serialized registration descriptor bytes across all live connections.
    pub max_registration_bytes: usize,
    pub connection_idle_ttl: Duration,
}

impl CustomerTransportConfig {
    pub fn new(
        websocket_url: impl Into<String>,
        observation_base_url: impl Into<String>,
    ) -> Result<Self> {
        let websocket_url = websocket_url.into();
        let observation_base_url = observation_base_url.into().trim_end_matches('/').to_owned();
        if !(websocket_url.starts_with("ws://") || websocket_url.starts_with("wss://")) {
            return Err(BrainError::Invalid(
                "customer Hand WebSocket URL must use ws or wss".into(),
            ));
        }
        if !(observation_base_url.starts_with("http://")
            || observation_base_url.starts_with("https://"))
        {
            return Err(BrainError::Invalid(
                "customer Hand observation base URL must use http or https".into(),
            ));
        }
        Ok(Self {
            websocket_url,
            observation_base_url,
            grant_ttl: Duration::from_secs(60),
            observation_ttl: Duration::from_secs(24 * 60 * 60),
            max_grants: customer_limit_from_env(
                CUSTOMER_MAX_GRANTS_ENV,
                DEFAULT_MAX_CUSTOMER_GRANTS,
                1,
                65_536,
            )?,
            max_connections: customer_limit_from_env(
                CUSTOMER_MAX_CONNECTIONS_ENV,
                DEFAULT_MAX_CUSTOMER_CONNECTIONS,
                1,
                16_384,
            )?,
            max_pending_operations: customer_limit_from_env(
                CUSTOMER_MAX_PENDING_OPERATIONS_ENV,
                DEFAULT_MAX_CUSTOMER_PENDING_OPERATIONS,
                1,
                4_096,
            )?,
            max_pending_terminal_bytes: customer_limit_from_env(
                CUSTOMER_MAX_PENDING_TERMINAL_BYTES_ENV,
                DEFAULT_MAX_CUSTOMER_PENDING_TERMINAL_BYTES,
                CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES,
                512 * 1024 * 1024,
            )?,
            max_registration_bytes: customer_limit_from_env(
                CUSTOMER_MAX_REGISTRATION_BYTES_ENV,
                DEFAULT_MAX_CUSTOMER_REGISTRATION_BYTES,
                MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES,
                512 * 1024 * 1024,
            )?,
            connection_idle_ttl: DEFAULT_CUSTOMER_CONNECTION_IDLE_TTL,
        })
    }
}

fn customer_limit_from_env(name: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let Some(raw) = std::env::var_os(name) else {
        return Ok(default);
    };
    let raw = raw.to_str().ok_or_else(|| {
        BrainError::Invalid(format!("{name} must contain an unsigned decimal integer"))
    })?;
    parse_customer_limit(name, Some(raw), default, min, max)
}

fn parse_customer_limit(
    name: &str,
    raw: Option<&str>,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let value = raw.parse::<usize>().map_err(|_| {
        BrainError::Invalid(format!("{name} must contain an unsigned decimal integer"))
    })?;
    if !(min..=max).contains(&value) {
        return Err(BrainError::Invalid(format!(
            "{name} must be between {min} and {max}"
        )));
    }
    Ok(value)
}

#[derive(Debug, Clone, Serialize)]
pub struct CustomerGrant {
    pub url: String,
    pub protocol: String,
    pub expires_at_ms: u64,
    /// Non-secret identifier used in the observation URL and ordinary access logs.
    pub grant_id: String,
    pub observation_url: String,
    /// Secret bearer credential. It never appears in a URL and Brain stores only its hash.
    pub observation_token: String,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CustomerClientFrame {
    Register {
        client_id: String,
        process_id: String,
        proof: String,
    },
    RegisterTools {
        epoch: u64,
        batch_id: String,
        proof: String,
        registrations: Vec<CustomerRegistration>,
    },
    Heartbeat {
        epoch: u64,
        nonce: String,
        proof: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CustomerObservation {
    Receipt {
        epoch: u64,
        operation_id: String,
        request_digest: String,
        replayed: bool,
    },
    Terminal {
        epoch: u64,
        operation_id: String,
        request_digest: String,
        ok: bool,
        output: Option<Value>,
        error: Option<String>,
    },
}

#[derive(Clone)]
struct GrantClaims {
    tenant_id: String,
    client_id: String,
    expires_at_ms: u64,
}

#[derive(Clone)]
struct PendingConnection {
    claims: GrantClaims,
    proof_hash: String,
}

struct ObservationGrant {
    tenant_id: String,
    client_id: String,
    token_hash: String,
    expires_at_ms: u64,
}

struct Connection {
    process_id: String,
    connection_id: String,
    epoch: u64,
    registrations: HashMap<String, CustomerRegistration>,
    registration_bytes: usize,
    local_sender: Option<mpsc::Sender<CustomerCommand>>,
    proof_hash: String,
    last_seen_ms: u64,
}

#[derive(Clone)]
enum PendingObservation {
    Waiting,
    Receipt,
    Terminal {
        ok: bool,
        output: Option<Value>,
        error: Option<String>,
        terminal_digest: String,
    },
}

struct PendingOperation {
    tenant_id: String,
    client_id: String,
    request_digest: String,
    process_id: String,
    deadline_at_ms: u64,
    terminal_reservation_bytes: usize,
    offered_epochs: HashSet<u64>,
    sender: watch::Sender<PendingObservation>,
}

/// Exact customer terminal identity retained until the session actor commits the corresponding
/// ToolResult. It contains no result payload and is safe to carry only inside the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerTerminalReceipt {
    pub operation_id: String,
    pub request_digest: String,
    pub terminal_digest: String,
    pub(crate) process_id: String,
}

pub struct CustomerExecution {
    pub outcome: CallOutcome,
    pub terminal_receipt: Option<CustomerTerminalReceipt>,
    /// True only when no delivery could have reached the sealed process and recovery may wait
    /// for that same process to reconnect without changing the operation digest.
    pub retryable_without_effect: bool,
}

/// Complete immutable offer identity selected before the ToolCall journal decision. The process
/// id is routing authority, not model input; recovery may re-offer only to this exact process.
#[derive(Debug, Clone)]
pub struct CustomerOperationIntent {
    pub tenant_id: String,
    pub client_id: String,
    pub process_id: String,
    pub session_id: String,
    pub operation_id: String,
    pub registration: String,
    pub name: String,
    pub contract_digest: String,
    pub input: Value,
    pub deadline_at_ms: u64,
    pub request_digest: String,
}

#[derive(Default)]
struct CoordinatorState {
    grants: HashMap<String, GrantClaims>,
    observation_grants: HashMap<String, ObservationGrant>,
    pending_connections: HashMap<String, PendingConnection>,
    connections: HashMap<(String, String), Connection>,
    connection_keys: HashMap<String, (String, String)>,
    local_senders: HashMap<String, mpsc::Sender<CustomerCommand>>,
    pending_operations: HashMap<String, PendingOperation>,
    pending_terminal_bytes: usize,
    registration_bytes: usize,
    next_epoch: u64,
}

impl CoordinatorState {
    fn remove_pending_operation(&mut self, operation_id: &str) -> Option<PendingOperation> {
        let pending = self.pending_operations.remove(operation_id)?;
        self.pending_terminal_bytes = self
            .pending_terminal_bytes
            .saturating_sub(pending.terminal_reservation_bytes);
        Some(pending)
    }

    fn remove_connection(&mut self, key: &(String, String)) -> Option<Connection> {
        let connection = self.connections.remove(key)?;
        self.registration_bytes = self
            .registration_bytes
            .saturating_sub(connection.registration_bytes);
        if self
            .connection_keys
            .get(&connection.connection_id)
            .is_some_and(|current| current == key)
        {
            self.connection_keys.remove(&connection.connection_id);
        }
        self.local_senders.remove(&connection.connection_id);
        Some(connection)
    }
}

/// Brain-owned customer-app state machine. A local socket uses the in-process delivery channel;
/// hosted composition injects API Gateway Management API through `external_delivery`.
pub struct CustomerCoordinator {
    config: CustomerTransportConfig,
    external_delivery: Option<Arc<dyn CustomerHandDeliveryPort>>,
    state: Mutex<CoordinatorState>,
}

impl CustomerCoordinator {
    pub fn new(
        config: CustomerTransportConfig,
        external_delivery: Option<Arc<dyn CustomerHandDeliveryPort>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            external_delivery,
            state: Mutex::new(CoordinatorState::default()),
        })
    }

    pub async fn grant(&self, tenant_id: &str, client_id: &str) -> Result<CustomerGrant> {
        validate_identifier("client id", client_id)?;
        let now = crate::wall_ms();
        let expires_at_ms = now.saturating_add(self.config.grant_ttl.as_millis() as u64);
        let observation_expires =
            now.saturating_add(self.config.observation_ttl.as_millis() as u64);
        let protocol = format!("aex-grant.{}", crate::mint_id("g", 32));
        let grant_id = crate::mint_id("grant", 24);
        let observation_token = crate::mint_id("obs", 40);
        let claims = GrantClaims {
            tenant_id: tenant_id.to_owned(),
            client_id: client_id.to_owned(),
            expires_at_ms,
        };
        let mut state = self.state.lock().await;
        self.prune(&mut state, now);
        if state.grants.len() >= self.config.max_grants
            || state.observation_grants.len() >= self.config.max_grants
        {
            return Err(BrainError::Overloaded);
        }
        state.grants.insert(protocol.clone(), claims);
        state.observation_grants.insert(
            grant_id.clone(),
            ObservationGrant {
                tenant_id: tenant_id.to_owned(),
                client_id: client_id.to_owned(),
                token_hash: secret_hash(&observation_token),
                expires_at_ms: observation_expires,
            },
        );
        Ok(CustomerGrant {
            url: self.config.websocket_url.clone(),
            protocol,
            expires_at_ms,
            grant_id: grant_id.clone(),
            observation_url: format!(
                "{}/v1/customer-hand/observations/{}",
                self.config.observation_base_url, grant_id
            ),
            observation_token,
        })
    }

    pub async fn bind_local_sender(
        &self,
        connection_id: &str,
        sender: mpsc::Sender<CustomerCommand>,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        self.prune(&mut state, crate::wall_ms());
        if let Some(key) = state.connection_keys.get(connection_id).cloned()
            && let Some(connection) = state.connections.get_mut(&key)
        {
            connection.local_sender = Some(sender);
        } else if state.pending_connections.contains_key(connection_id) {
            state.local_senders.insert(connection_id.to_owned(), sender);
        } else {
            return Err(BrainError::Invalid(
                "customer Hand local connection is not pending".into(),
            ));
        }
        Ok(())
    }

    pub async fn observation(
        &self,
        grant_id: &str,
        token: &str,
        observation: CustomerObservation,
    ) -> Result<()> {
        let observation_bytes = serde_json::to_vec(&observation)?.len();
        if observation_bytes > MAX_CUSTOMER_HTTP_OBSERVATION_BYTES {
            return Err(BrainError::FileTooLarge {
                limit: MAX_CUSTOMER_HTTP_OBSERVATION_BYTES,
            });
        }
        let now = crate::wall_ms();
        let mut state = self.state.lock().await;
        self.prune(&mut state, now);
        let Some(grant) = state.observation_grants.get(grant_id) else {
            return Err(BrainError::Invalid(
                "customer Hand observation grant is invalid or expired".into(),
            ));
        };
        if grant.token_hash != secret_hash(token) {
            return Err(BrainError::Invalid(
                "customer Hand observation grant is invalid or expired".into(),
            ));
        }
        let tenant_id = grant.tenant_id.clone();
        let client_id = grant.client_id.clone();
        let (epoch, operation_id, request_digest, value) = match observation {
            CustomerObservation::Receipt {
                epoch,
                operation_id,
                request_digest,
                ..
            } => (
                epoch,
                operation_id,
                request_digest,
                PendingObservation::Receipt,
            ),
            CustomerObservation::Terminal {
                epoch,
                operation_id,
                request_digest,
                ok,
                output,
                error,
            } => (
                epoch,
                operation_id,
                request_digest,
                PendingObservation::Terminal {
                    ok,
                    output,
                    error,
                    terminal_digest: String::new(),
                },
            ),
        };
        let Some(pending) = state.pending_operations.get_mut(&operation_id) else {
            return Ok(());
        };
        if pending.tenant_id != tenant_id
            || pending.client_id != client_id
            || pending.request_digest != request_digest
            || !pending.offered_epochs.contains(&epoch)
        {
            return Err(BrainError::Invalid(
                "customer Hand observation does not match its operation receipt".into(),
            ));
        }
        let process_id = pending.process_id.clone();
        match value {
            PendingObservation::Receipt => {
                if matches!(*pending.sender.borrow(), PendingObservation::Waiting) {
                    pending.sender.send_replace(PendingObservation::Receipt);
                }
            }
            PendingObservation::Terminal {
                ok, output, error, ..
            } => {
                let terminal_digest = terminal_digest(
                    &operation_id,
                    &request_digest,
                    ok,
                    output.as_ref(),
                    error.as_deref(),
                )?;
                let current = pending.sender.borrow().clone();
                if let PendingObservation::Terminal {
                    terminal_digest: current_digest,
                    ..
                } = current
                {
                    if current_digest != terminal_digest {
                        return Err(BrainError::Invalid(
                            "customer Hand supplied conflicting terminal facts".into(),
                        ));
                    }
                } else {
                    pending.sender.send_replace(PendingObservation::Terminal {
                        ok,
                        output,
                        error,
                        terminal_digest,
                    });
                }
            }
            PendingObservation::Waiting => unreachable!("waiting is coordinator-internal"),
        }
        if let Some(connection) = state.connections.get_mut(&(tenant_id, client_id))
            && connection.process_id == process_id
            && connection.epoch == epoch
        {
            connection.last_seen_ms = now;
        }
        Ok(())
    }

    /// Authenticate a scoped HTTPS observation grant before an HTTP adapter reads its body.
    /// `observation` validates it again under the same lock when applying the decoded command, so
    /// expiry or revocation between admission and application still fails closed.
    pub async fn authorize_observation(&self, grant_id: &str, token: &str) -> Result<()> {
        let now = crate::wall_ms();
        let mut state = self.state.lock().await;
        self.prune(&mut state, now);
        let valid = state
            .observation_grants
            .get(grant_id)
            .is_some_and(|grant| grant.token_hash == secret_hash(token));
        if !valid {
            return Err(BrainError::Invalid(
                "customer Hand observation grant is invalid or expired".into(),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_operation(
        &self,
        tenant_id: &str,
        client_id: &str,
        session_id: &str,
        operation_id: &str,
        registration: &str,
        name: &str,
        contract_digest: &str,
        input: Value,
        deadline_at_ms: u64,
    ) -> Result<CustomerOperationIntent> {
        let request_digest = customer_request_digest(
            session_id,
            operation_id,
            registration,
            name,
            contract_digest,
            &input,
            deadline_at_ms,
        )?;
        let mut state = self.state.lock().await;
        self.prune(&mut state, crate::wall_ms());
        let connection = state
            .connections
            .get(&(tenant_id.to_owned(), client_id.to_owned()))
            .ok_or_else(|| {
                BrainError::HandUnavailable("customer application is not connected".into())
            })?;
        let registered = connection.registrations.get(registration).ok_or_else(|| {
            BrainError::Invalid("customer Tool registration is unavailable".into())
        })?;
        if registered.name != name || registered.contract_digest != contract_digest {
            return Err(BrainError::Invalid(
                "customer Tool registration does not match the sealed contract".into(),
            ));
        }
        customer_offer(
            u64::MAX,
            operation_id,
            &request_digest,
            session_id,
            registration,
            name,
            contract_digest,
            input.clone(),
            deadline_at_ms,
        )
        .to_frame()?;
        Ok(CustomerOperationIntent {
            tenant_id: tenant_id.to_owned(),
            client_id: client_id.to_owned(),
            process_id: connection.process_id.clone(),
            session_id: session_id.to_owned(),
            operation_id: operation_id.to_owned(),
            registration: registration.to_owned(),
            name: name.to_owned(),
            contract_digest: contract_digest.to_owned(),
            input,
            deadline_at_ms,
            request_digest,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        tenant_id: &str,
        client_id: &str,
        submit_retries: u32,
        session_id: &str,
        operation_id: &str,
        registration: &str,
        name: &str,
        contract_digest: &str,
        input: Value,
        deadline_at_ms: u64,
        cancel: CancellationToken,
    ) -> CustomerExecution {
        match self
            .prepare_operation(
                tenant_id,
                client_id,
                session_id,
                operation_id,
                registration,
                name,
                contract_digest,
                input,
                deadline_at_ms,
            )
            .await
        {
            Ok(intent) => self.execute_prepared(intent, submit_retries, cancel).await,
            Err(BrainError::HandUnavailable(message)) => customer_execution(interrupted(message)),
            Err(BrainError::Overloaded) => customer_execution(interrupted(
                "customer Tool coordinator is at capacity; retry with backoff",
            )),
            Err(BrainError::FileTooLarge { .. }) => customer_execution(CallOutcome::failed(
                "customer Tool command exceeds the 24 KiB frame bound",
            )),
            Err(error) => customer_execution(CallOutcome::failed(error.to_string())),
        }
    }

    pub async fn execute_prepared(
        &self,
        intent: CustomerOperationIntent,
        submit_retries: u32,
        cancel: CancellationToken,
    ) -> CustomerExecution {
        let CustomerOperationIntent {
            tenant_id,
            client_id,
            process_id,
            session_id,
            operation_id,
            registration,
            name,
            contract_digest,
            input,
            deadline_at_ms,
            request_digest,
        } = intent;
        let rebuilt_digest = match customer_request_digest(
            &session_id,
            &operation_id,
            &registration,
            &name,
            &contract_digest,
            &input,
            deadline_at_ms,
        ) {
            Ok(digest) => digest,
            Err(error) => return customer_execution(CallOutcome::failed(error.to_string())),
        };
        if rebuilt_digest != request_digest {
            return customer_execution(CallOutcome::failed(
                "customer Tool prepared request digest does not match its exact envelope",
            ));
        }
        let (mut receiver, initial_connection_id, initial_epoch, newly_admitted) = {
            let mut state = self.state.lock().await;
            self.prune(&mut state, crate::wall_ms());
            let (connection_id, epoch) = {
                let Some(connection) = state
                    .connections
                    .get(&(tenant_id.clone(), client_id.clone()))
                    .filter(|connection| connection.process_id == process_id)
                else {
                    return retryable_customer_execution(
                        "sealed customer application process is not connected",
                    );
                };
                let Some(registered) = connection.registrations.get(&registration) else {
                    return retryable_customer_execution(
                        "sealed customer Tool registration is not currently available",
                    );
                };
                if registered.name != name || registered.contract_digest != contract_digest {
                    return customer_execution(CallOutcome::failed(
                        "customer Tool registration does not match the sealed contract",
                    ));
                }
                (connection.connection_id.clone(), connection.epoch)
            };
            let receiver = if let Some(pending) = state.pending_operations.get_mut(&operation_id) {
                if pending.request_digest != request_digest {
                    return customer_execution(CallOutcome::failed(
                        "operation_id was reused with a different request_digest",
                    ));
                }
                if pending.process_id != process_id {
                    return customer_execution(interrupted(
                        "customer operation cannot be reassigned to a replacement process",
                    ));
                }
                if pending.deadline_at_ms != deadline_at_ms {
                    return customer_execution(CallOutcome::failed(
                        "customer operation retry changed its sealed deadline",
                    ));
                }
                (pending.sender.subscribe(), false)
            } else {
                if state.pending_operations.len() >= self.config.max_pending_operations {
                    return customer_execution(interrupted(
                        "customer Tool coordinator is at capacity; retry with backoff",
                    ));
                }
                let Some(pending_terminal_bytes) = state
                    .pending_terminal_bytes
                    .checked_add(CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES)
                else {
                    return customer_execution(interrupted(
                        "customer Tool coordinator is at capacity; retry with backoff",
                    ));
                };
                if pending_terminal_bytes > self.config.max_pending_terminal_bytes {
                    return customer_execution(interrupted(
                        "customer Tool coordinator is at capacity; retry with backoff",
                    ));
                }
                let (sender, receiver) = watch::channel(PendingObservation::Waiting);
                state.pending_operations.insert(
                    operation_id.to_owned(),
                    PendingOperation {
                        tenant_id: tenant_id.to_owned(),
                        client_id: client_id.to_owned(),
                        request_digest: request_digest.clone(),
                        process_id: process_id.clone(),
                        deadline_at_ms,
                        terminal_reservation_bytes: CUSTOMER_PENDING_TERMINAL_RESERVATION_BYTES,
                        offered_epochs: HashSet::new(),
                        sender,
                    },
                );
                state.pending_terminal_bytes = pending_terminal_bytes;
                (receiver, true)
            };
            (receiver.0, connection_id, epoch, receiver.1)
        };
        let probe = customer_offer(
            initial_epoch,
            &operation_id,
            &request_digest,
            &session_id,
            &registration,
            &name,
            &contract_digest,
            input.clone(),
            deadline_at_ms,
        );
        if let Err(error) = probe.to_frame() {
            if newly_admitted {
                self.state
                    .lock()
                    .await
                    .remove_pending_operation(&operation_id);
            }
            return customer_execution(CallOutcome::failed(format!(
                "customer Tool command exceeds the 24 KiB frame bound: {error}"
            )));
        }
        let mut could_have_reached_process = false;
        let mut connection_id = initial_connection_id;
        for attempt in 0..=submit_retries {
            if !matches!(*receiver.borrow(), PendingObservation::Waiting) {
                break;
            }
            let route = {
                let mut state = self.state.lock().await;
                let current = state
                    .connections
                    .get(&(tenant_id.to_owned(), client_id.to_owned()))
                    .filter(|connection| connection.process_id == process_id)
                    .map(|connection| (connection.connection_id.clone(), connection.epoch));
                if let Some((current_connection_id, epoch)) = &current {
                    if let Some(pending) = state.pending_operations.get_mut(&operation_id) {
                        pending.offered_epochs.insert(*epoch);
                    }
                    connection_id = current_connection_id.clone();
                }
                current
            };
            let delivery = if let Some((current_connection_id, epoch)) = route {
                self.deliver(CustomerDeliveryRequest {
                    connection_id: current_connection_id.clone(),
                    command: customer_offer(
                        epoch,
                        &operation_id,
                        &request_digest,
                        &session_id,
                        &registration,
                        &name,
                        &contract_digest,
                        input.clone(),
                        deadline_at_ms,
                    ),
                })
                .await
            } else {
                Ok(CustomerDelivery::Unavailable)
            };
            match delivery {
                Ok(CustomerDelivery::Delivered | CustomerDelivery::Unknown) => {
                    could_have_reached_process = true;
                }
                Ok(CustomerDelivery::Gone) => {
                    self.remove_connection_if_exact(&connection_id, &process_id)
                        .await;
                }
                Ok(CustomerDelivery::Unavailable) | Err(_) => {}
            }
            if !matches!(*receiver.borrow(), PendingObservation::Waiting) {
                break;
            }
            let remaining = deadline_at_ms.saturating_sub(crate::wall_ms());
            if attempt < submit_retries && remaining > 0 {
                let wait = Duration::from_millis(remaining.min(250));
                let _ = tokio::time::timeout(wait, receiver.changed()).await;
            } else if could_have_reached_process && remaining > 0 {
                // A successful management-API send is not an execution receipt. Give the exact
                // process a short bounded window to publish one before classifying ambiguity.
                let wait = Duration::from_millis(remaining.min(250));
                let _ = tokio::time::timeout(wait, receiver.changed()).await;
            }
        }
        if matches!(*receiver.borrow(), PendingObservation::Waiting) {
            self.remove_waiting_operation(&operation_id, &request_digest)
                .await;
            return customer_execution(interrupted(if could_have_reached_process {
                "customer Tool execution outcome is unknown"
            } else {
                "customer application delivery failed before an execution receipt"
            }));
        }
        let wait_ms = deadline_at_ms.saturating_sub(crate::wall_ms()).max(1);
        let result: std::result::Result<PendingObservation, String> = tokio::select! {
            _ = cancel.cancelled() => Err("customer Tool was cancelled".to_owned()),
            result = tokio::time::timeout(Duration::from_millis(wait_ms), async {
                loop {
                    let current = receiver.borrow().clone();
                    if matches!(current, PendingObservation::Terminal { .. }) {
                        break Ok(current);
                    }
                    if receiver.changed().await.is_err() {
                        break Err("customer Tool observation channel closed".to_owned());
                    }
                }
            }) => result.unwrap_or_else(|_| Err("customer Tool deadline exceeded".to_owned())),
        };
        match result {
            Ok(PendingObservation::Terminal {
                ok: true,
                output,
                terminal_digest,
                ..
            }) => {
                let value = output.unwrap_or(Value::Null);
                CustomerExecution {
                    outcome: CallOutcome {
                        outcome: "completed".into(),
                        content: serde_json::to_string(&value).unwrap_or_else(|_| "null".into()),
                        value: Some(value),
                        is_error: false,
                        exit_code: None,
                        duration_ms: 0,
                        truncated: false,
                        terminal: None,
                    },
                    terminal_receipt: Some(CustomerTerminalReceipt {
                        operation_id: operation_id.to_owned(),
                        request_digest,
                        terminal_digest,
                        process_id,
                    }),
                    retryable_without_effect: false,
                }
            }
            Ok(PendingObservation::Terminal {
                ok: false,
                error,
                terminal_digest,
                ..
            }) => CustomerExecution {
                outcome: CallOutcome::failed(
                    error.unwrap_or_else(|| "customer Tool failed".into()),
                ),
                terminal_receipt: Some(CustomerTerminalReceipt {
                    operation_id: operation_id.to_owned(),
                    request_digest,
                    terminal_digest,
                    process_id,
                }),
                retryable_without_effect: false,
            },
            Ok(PendingObservation::Waiting | PendingObservation::Receipt) => {
                unreachable!("terminal wait returned a nonterminal observation")
            }
            Err(error) => customer_execution(interrupted(error)),
        }
    }

    /// Releases an exact retained customer terminal only after the caller has durably committed
    /// the corresponding ToolResult. Ambiguous delivery keeps the pending fact for retry.
    pub async fn acknowledge_terminal(&self, receipt: &CustomerTerminalReceipt) -> Result<()> {
        let (connection_id, epoch) = {
            let state = self.state.lock().await;
            let Some(pending) = state.pending_operations.get(&receipt.operation_id) else {
                return Ok(());
            };
            if pending.request_digest != receipt.request_digest
                || pending.process_id != receipt.process_id
                || !matches!(
                    &*pending.sender.borrow(),
                    PendingObservation::Terminal { terminal_digest, .. }
                        if terminal_digest == &receipt.terminal_digest
                )
            {
                return Err(BrainError::Invalid(
                    "customer terminal acknowledgement does not match the retained fact".into(),
                ));
            }
            let connection = state
                .connections
                .get(&(pending.tenant_id.clone(), pending.client_id.clone()))
                .filter(|connection| connection.process_id == pending.process_id)
                .ok_or_else(|| {
                    BrainError::HandUnavailable(
                        "customer process is not connected for terminal acknowledgement".into(),
                    )
                })?;
            (connection.connection_id.clone(), connection.epoch)
        };
        let delivery = self
            .deliver(CustomerDeliveryRequest {
                connection_id,
                command: CustomerCommand::Ack {
                    epoch,
                    operation_id: receipt.operation_id.clone(),
                    request_digest: receipt.request_digest.clone(),
                    terminal_digest: receipt.terminal_digest.clone(),
                },
            })
            .await?;
        if delivery != CustomerDelivery::Delivered {
            return Err(BrainError::HandUnavailable(
                "customer terminal acknowledgement delivery is ambiguous".into(),
            ));
        }
        let mut state = self.state.lock().await;
        if state
            .pending_operations
            .get(&receipt.operation_id)
            .is_some_and(|pending| {
                pending.request_digest == receipt.request_digest
                    && pending.process_id == receipt.process_id
                    && matches!(
                        &*pending.sender.borrow(),
                        PendingObservation::Terminal { terminal_digest, .. }
                            if terminal_digest == &receipt.terminal_digest
                    )
            })
        {
            state.remove_pending_operation(&receipt.operation_id);
        }
        Ok(())
    }

    /// Replays an exact ACK after Brain process loss. The durable session projection supplies
    /// tenancy/client routing and the sealed process id; no in-memory pending operation is
    /// required, and a replacement process is never selected.
    pub(crate) async fn acknowledge_durable_terminal(
        &self,
        tenant_id: &str,
        client_id: &str,
        receipt: &CustomerTerminalReceipt,
    ) -> Result<()> {
        let (connection_id, epoch) = {
            let mut state = self.state.lock().await;
            self.prune(&mut state, crate::wall_ms());
            let connection = state
                .connections
                .get(&(tenant_id.to_owned(), client_id.to_owned()))
                .filter(|connection| connection.process_id == receipt.process_id)
                .ok_or_else(|| {
                    BrainError::HandUnavailable(
                        "sealed customer process is not connected for terminal acknowledgement"
                            .into(),
                    )
                })?;
            (connection.connection_id.clone(), connection.epoch)
        };
        let delivery = self
            .deliver(CustomerDeliveryRequest {
                connection_id,
                command: CustomerCommand::Ack {
                    epoch,
                    operation_id: receipt.operation_id.clone(),
                    request_digest: receipt.request_digest.clone(),
                    terminal_digest: receipt.terminal_digest.clone(),
                },
            })
            .await?;
        if delivery == CustomerDelivery::Delivered {
            Ok(())
        } else {
            Err(BrainError::HandUnavailable(
                "customer terminal acknowledgement delivery is ambiguous".into(),
            ))
        }
    }

    async fn remove_waiting_operation(&self, operation_id: &str, request_digest: &str) {
        let mut state = self.state.lock().await;
        if state
            .pending_operations
            .get(operation_id)
            .is_some_and(|pending| {
                pending.request_digest == request_digest
                    && matches!(*pending.sender.borrow(), PendingObservation::Waiting)
            })
        {
            state.remove_pending_operation(operation_id);
        }
    }

    async fn remove_connection_if_exact(&self, connection_id: &str, process_id: &str) {
        let mut state = self.state.lock().await;
        let Some(key) = state.connection_keys.get(connection_id).cloned() else {
            return;
        };
        if state.connections.get(&key).is_some_and(|connection| {
            connection.connection_id == connection_id && connection.process_id == process_id
        }) {
            state.remove_connection(&key);
        }
    }

    fn prune(&self, state: &mut CoordinatorState, now: u64) {
        prune(
            state,
            now,
            self.config.connection_idle_ttl.as_millis() as u64,
        );
    }

    async fn deliver(&self, request: CustomerDeliveryRequest) -> Result<CustomerDelivery> {
        let local = {
            let state = self.state.lock().await;
            state
                .connection_keys
                .get(&request.connection_id)
                .and_then(|key| state.connections.get(key))
                .and_then(|connection| connection.local_sender.clone())
        };
        if let Some(sender) = local {
            return Ok(match sender.try_send(request.command) {
                Ok(()) => CustomerDelivery::Delivered,
                Err(mpsc::error::TrySendError::Full(_)) => CustomerDelivery::Unavailable,
                Err(mpsc::error::TrySendError::Closed(_)) => CustomerDelivery::Gone,
            });
        }
        match &self.external_delivery {
            Some(delivery) => delivery.send(request).await,
            None => Ok(CustomerDelivery::Unavailable),
        }
    }
}

#[async_trait]
impl CustomerHandIngressPort for CustomerCoordinator {
    async fn receive(&self, input: CustomerGatewayInput) -> Result<()> {
        match input.route {
            CustomerGatewayRoute::Connect => {
                let protocol = input.subprotocol.ok_or_else(|| {
                    BrainError::Invalid("customer Hand grant subprotocol is missing".into())
                })?;
                let mut state = self.state.lock().await;
                self.prune(&mut state, crate::wall_ms());
                let claims = state.grants.get(&protocol).cloned().ok_or_else(|| {
                    BrainError::Invalid("customer Hand grant is invalid or expired".into())
                })?;
                if state.pending_connections.len() >= self.config.max_connections {
                    return Err(BrainError::Overloaded);
                }
                if state.pending_connections.contains_key(&input.connection_id)
                    || state.connection_keys.contains_key(&input.connection_id)
                {
                    return Err(BrainError::Invalid(
                        "customer Hand connection id is already in use".into(),
                    ));
                }
                state.grants.remove(&protocol);
                state.pending_connections.insert(
                    input.connection_id,
                    PendingConnection {
                        claims,
                        proof_hash: secret_hash(&frame_proof(&protocol)),
                    },
                );
            }
            CustomerGatewayRoute::Disconnect => {
                // API Gateway authorizer identity is connect-only. A disconnect callback is an
                // untrusted advisory and cannot revoke a process or cancel assigned work. A new
                // proven connection takes over by epoch; stale delivery observes Gone/closed.
            }
            CustomerGatewayRoute::Message => {
                let body = input.body.ok_or_else(|| {
                    BrainError::Invalid("customer Hand frame body is missing".into())
                })?;
                if body.len() > MAX_CUSTOMER_WS_FRAME_BYTES {
                    return Err(BrainError::FileTooLarge {
                        limit: MAX_CUSTOMER_WS_FRAME_BYTES,
                    });
                }
                let frame: CustomerClientFrame = serde_json::from_str(&body)?;
                match frame {
                    CustomerClientFrame::Register {
                        client_id,
                        process_id,
                        proof,
                    } => {
                        validate_identifier("process id", &process_id)?;
                        let (epoch, command) = {
                            let mut state = self.state.lock().await;
                            let now = crate::wall_ms();
                            self.prune(&mut state, now);
                            let pending = state
                                .pending_connections
                                .get(&input.connection_id)
                                .cloned()
                                .ok_or_else(|| {
                                    BrainError::Invalid(
                                        "customer Hand connection has no grant".into(),
                                    )
                                })?;
                            if pending.proof_hash != secret_hash(&proof) {
                                return Err(BrainError::Invalid(
                                    "customer Hand frame proof is invalid".into(),
                                ));
                            }
                            let claims = pending.claims;
                            if claims.client_id != client_id || claims.expires_at_ms < now {
                                return Err(BrainError::Invalid(
                                    "customer Hand registration does not match its grant".into(),
                                ));
                            }
                            let key = (claims.tenant_id.clone(), claims.client_id.clone());
                            if !state.connections.contains_key(&key)
                                && state.connections.len() >= self.config.max_connections
                            {
                                return Err(BrainError::Overloaded);
                            }
                            state.pending_connections.remove(&input.connection_id);
                            state.next_epoch = state.next_epoch.saturating_add(1).max(1);
                            let epoch = state.next_epoch;
                            state.remove_connection(&key);
                            if let Some(old_key) =
                                state.connection_keys.get(&input.connection_id).cloned()
                                && old_key != key
                            {
                                state.remove_connection(&old_key);
                            }
                            state
                                .connection_keys
                                .insert(input.connection_id.clone(), key.clone());
                            let local_sender = state.local_senders.remove(&input.connection_id);
                            state.connections.insert(
                                key,
                                Connection {
                                    process_id,
                                    connection_id: input.connection_id.clone(),
                                    epoch,
                                    registrations: HashMap::new(),
                                    registration_bytes: 0,
                                    local_sender,
                                    proof_hash: pending.proof_hash,
                                    last_seen_ms: now,
                                },
                            );
                            (epoch, CustomerCommand::Ready { epoch })
                        };
                        let _ = epoch;
                        let delivery = self
                            .deliver(CustomerDeliveryRequest {
                                connection_id: input.connection_id,
                                command,
                            })
                            .await?;
                        if delivery != CustomerDelivery::Delivered {
                            return Err(BrainError::HandUnavailable(
                                "customer Hand ready frame was not delivered".into(),
                            ));
                        }
                    }
                    CustomerClientFrame::RegisterTools {
                        epoch,
                        batch_id,
                        proof,
                        registrations,
                    } => {
                        if registrations.len() > 128 {
                            return Err(BrainError::Invalid(
                                "customer Hand registration batch exceeds 128 tools".into(),
                            ));
                        }
                        {
                            let mut state = self.state.lock().await;
                            let now = crate::wall_ms();
                            self.prune(&mut state, now);
                            let key = state
                                .connection_keys
                                .get(&input.connection_id)
                                .cloned()
                                .ok_or_else(|| {
                                    BrainError::Invalid(
                                        "customer Hand connection is not registered".into(),
                                    )
                                })?;
                            let process_registration_bytes = state.registration_bytes;
                            let added_bytes = {
                                let connection =
                                    state.connections.get_mut(&key).ok_or_else(|| {
                                        BrainError::Invalid(
                                            "customer Hand connection is not current".into(),
                                        )
                                    })?;
                                if connection.proof_hash != secret_hash(&proof) {
                                    return Err(BrainError::Invalid(
                                        "customer Hand frame proof is invalid".into(),
                                    ));
                                }
                                if connection.epoch != epoch {
                                    return Err(BrainError::Fenced);
                                }
                                let mut additions = Vec::new();
                                let mut seen = HashMap::<String, CustomerRegistration>::new();
                                let mut added_bytes = 0usize;
                                for registration in registrations {
                                    validate_identifier(
                                        "registration",
                                        &registration.registration,
                                    )?;
                                    validate_identifier("customer Tool name", &registration.name)?;
                                    if !is_digest(&registration.contract_digest) {
                                        return Err(BrainError::Invalid(
                                            "customer Tool contract digest is invalid".into(),
                                        ));
                                    }
                                    let current = connection
                                        .registrations
                                        .get(&registration.registration)
                                        .or_else(|| seen.get(&registration.registration));
                                    if let Some(current) = current
                                        && (current.name != registration.name
                                            || current.contract_digest
                                                != registration.contract_digest)
                                    {
                                        return Err(BrainError::Invalid(format!(
                                            "customer Hand registration {} conflicts with its existing contract",
                                            registration.registration
                                        )));
                                    }
                                    if current.is_none() {
                                        added_bytes = added_bytes
                                            .checked_add(serde_json::to_vec(&registration)?.len())
                                            .ok_or(BrainError::Overloaded)?;
                                        seen.insert(
                                            registration.registration.clone(),
                                            registration.clone(),
                                        );
                                        additions.push(registration);
                                    }
                                }
                                if connection
                                    .registrations
                                    .len()
                                    .saturating_add(additions.len())
                                    > MAX_CUSTOMER_REGISTRATIONS
                                {
                                    return Err(BrainError::Invalid(format!(
                                        "customer Hand exceeds its {MAX_CUSTOMER_REGISTRATIONS} registration limit"
                                    )));
                                }
                                if connection.registration_bytes.saturating_add(added_bytes)
                                    > MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES
                                {
                                    return Err(BrainError::Invalid(format!(
                                        "customer Hand registration descriptors exceed {MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES} bytes"
                                    )));
                                }
                                let Some(next_process_bytes) =
                                    process_registration_bytes.checked_add(added_bytes)
                                else {
                                    return Err(BrainError::Overloaded);
                                };
                                if next_process_bytes > self.config.max_registration_bytes {
                                    return Err(BrainError::Overloaded);
                                }
                                for registration in additions {
                                    connection
                                        .registrations
                                        .insert(registration.registration.clone(), registration);
                                }
                                connection.registration_bytes = connection
                                    .registration_bytes
                                    .checked_add(added_bytes)
                                    .ok_or(BrainError::Overloaded)?;
                                connection.last_seen_ms = now;
                                added_bytes
                            };
                            state.registration_bytes = process_registration_bytes
                                .checked_add(added_bytes)
                                .ok_or(BrainError::Overloaded)?;
                        }
                        let delivery = self
                            .deliver(CustomerDeliveryRequest {
                                connection_id: input.connection_id,
                                command: CustomerCommand::Registered { epoch, batch_id },
                            })
                            .await?;
                        if delivery != CustomerDelivery::Delivered {
                            return Err(BrainError::HandUnavailable(
                                "customer Hand registration acknowledgement was not delivered"
                                    .into(),
                            ));
                        }
                    }
                    CustomerClientFrame::Heartbeat {
                        epoch,
                        nonce,
                        proof,
                    } => {
                        {
                            let mut state = self.state.lock().await;
                            let now = crate::wall_ms();
                            self.prune(&mut state, now);
                            let key = state
                                .connection_keys
                                .get(&input.connection_id)
                                .cloned()
                                .ok_or_else(|| {
                                    BrainError::Invalid(
                                        "customer Hand connection is not registered".into(),
                                    )
                                })?;
                            let connection = state.connections.get_mut(&key).ok_or_else(|| {
                                BrainError::Invalid(
                                    "customer Hand connection is not current".into(),
                                )
                            })?;
                            if connection.proof_hash != secret_hash(&proof) {
                                return Err(BrainError::Invalid(
                                    "customer Hand frame proof is invalid".into(),
                                ));
                            }
                            if connection.epoch != epoch {
                                return Err(BrainError::Fenced);
                            }
                            connection.last_seen_ms = now;
                        }
                        let delivery = self
                            .deliver(CustomerDeliveryRequest {
                                connection_id: input.connection_id,
                                command: CustomerCommand::Heartbeat { epoch, nonce },
                            })
                            .await?;
                        if delivery != CustomerDelivery::Delivered {
                            return Err(BrainError::HandUnavailable(
                                "customer Hand heartbeat acknowledgement was not delivered".into(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(BrainError::Invalid(format!(
            "{label} must contain 1 to 128 safe ASCII bytes"
        )));
    }
    Ok(())
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[allow(clippy::too_many_arguments)]
fn customer_offer(
    epoch: u64,
    operation_id: &str,
    request_digest: &str,
    session_id: &str,
    registration: &str,
    name: &str,
    contract_digest: &str,
    input: Value,
    deadline_at_ms: u64,
) -> CustomerCommand {
    CustomerCommand::Offer(CustomerOperationOffer {
        epoch,
        operation_id: operation_id.to_owned(),
        request_digest: request_digest.to_owned(),
        session_id: session_id.to_owned(),
        registration: registration.to_owned(),
        name: name.to_owned(),
        contract_digest: contract_digest.to_owned(),
        input,
        deadline_at_ms,
    })
}

fn terminal_digest(
    operation_id: &str,
    request_digest: &str,
    ok: bool,
    output: Option<&Value>,
    error: Option<&str>,
) -> Result<String> {
    let bytes = serde_jcs::to_vec(&serde_json::json!({
        "operation_id": operation_id,
        "request_digest": request_digest,
        "ok": ok,
        "output": if ok { output } else { None },
        "error": if ok { None } else { error },
    }))
    .map_err(|error| {
        BrainError::Protocol(format!("customer terminal canonicalization: {error}"))
    })?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[allow(clippy::too_many_arguments)]
fn customer_request_digest(
    session_id: &str,
    operation_id: &str,
    registration: &str,
    name: &str,
    contract_digest: &str,
    input: &Value,
    deadline_at_ms: u64,
) -> Result<String> {
    let bytes = serde_jcs::to_vec(&serde_json::json!({
        "session_id": session_id,
        "operation_id": operation_id,
        "registration": registration,
        "name": name,
        "contract_digest": contract_digest,
        "input": input,
        "deadline_at_ms": deadline_at_ms,
    }))
    .map_err(|error| BrainError::Protocol(format!("customer Tool request: {error}")))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn customer_execution(outcome: CallOutcome) -> CustomerExecution {
    CustomerExecution {
        outcome,
        terminal_receipt: None,
        retryable_without_effect: false,
    }
}

fn retryable_customer_execution(message: impl Into<String>) -> CustomerExecution {
    CustomerExecution {
        outcome: interrupted(message),
        terminal_receipt: None,
        retryable_without_effect: true,
    }
}

fn interrupted(message: impl Into<String>) -> CallOutcome {
    let mut outcome = CallOutcome::failed(message);
    outcome.outcome = "interrupted".into();
    outcome
}

fn secret_hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// Derived, connection-only proof. Knowing it cannot mint a second socket because the original
/// grant is consumed at `$connect`.
pub fn frame_proof(protocol: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"aex.customer-hand.frame-proof\0");
    digest.update(protocol.as_bytes());
    hex::encode(digest.finalize())
}

fn prune(state: &mut CoordinatorState, now: u64, connection_idle_ttl_ms: u64) {
    state.grants.retain(|_, claims| claims.expires_at_ms >= now);
    state
        .observation_grants
        .retain(|_, grant| grant.expires_at_ms >= now);
    state
        .pending_connections
        .retain(|_, pending| pending.claims.expires_at_ms >= now);

    let expired_connections = state
        .connections
        .iter()
        .filter(|(_, connection)| {
            connection
                .last_seen_ms
                .saturating_add(connection_idle_ttl_ms)
                < now
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in expired_connections {
        state.remove_connection(&key);
    }
    state.connection_keys.retain(|connection_id, key| {
        state
            .connections
            .get(key)
            .is_some_and(|connection| connection.connection_id == *connection_id)
    });
    state
        .local_senders
        .retain(|connection_id, _| state.pending_connections.contains_key(connection_id));
}

#[cfg(test)]
#[path = "customer_tests.rs"]
mod tests;
