//! The host env: whatever process registered as a host, a browser tab, a Node process,
//! a server, reached over the command stream it holds open. A Tool placed here is a
//! function that process holds; Brain sends it the call and waits for the result.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use brain::environment::ExecutionServices;
use brain_protocol::{
    ApiError, Driver, Environment, EnvironmentName, EnvironmentOperation, EnvironmentReceipt,
    EnvironmentRequest, HostCommand, HostEvent, HostEventAck, HostId, HostOperation,
    HostRegistration, HostResult, Outcome, SessionId,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::{mpsc, oneshot};

use brain::environment::{EnvironmentAdapter, Services, unsupported};

#[derive(Clone)]
pub struct HostEnvironment {
    inner: Arc<Mutex<State>>,
    limits: crate::ServerLimits,
}

struct State {
    closed: bool,
    log: File,
    hosts: HashMap<HostId, Host>,
}

struct Host {
    /// The sessions placed in this host. A registration with sessions never expires.
    sessions: HashSet<(SessionId, EnvironmentName)>,
    token: [u8; 32],
    disconnected_at: Instant,
    connection: u64,
    commands: Option<mpsc::Sender<HostCommand>>,
    disconnect: Option<oneshot::Sender<()>>,
    pending: HashMap<(SessionId, u64), PendingCall>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RegistrationRecord {
    Registered {
        host_id: HostId,
        token: [u8; 32],
    },
    Removed {
        host_id: HostId,
    },
    Bound {
        environment: EnvironmentName,
        session_id: SessionId,
        host_id: HostId,
    },
    Released {
        environment: EnvironmentName,
        session_id: SessionId,
    },
}

fn registered(token: [u8; 32]) -> Host {
    Host {
        token,
        sessions: HashSet::new(),
        disconnected_at: Instant::now(),
        connection: 0,
        commands: None,
        disconnect: None,
        pending: HashMap::new(),
    }
}

struct PendingCall {
    outcome: oneshot::Sender<Outcome>,
    events: mpsc::Sender<PendingEvent>,
}

struct PendingEvent {
    kind: String,
    data: serde_json::Value,
    reply: oneshot::Sender<Result<u64, String>>,
}

impl HostEnvironment {
    pub fn open(path: &Path, limits: &crate::ServerLimits) -> Result<Self, brain::Error> {
        let (log, records) = crate::persistence::open_log::<RegistrationRecord>(path)?;
        let mut hosts = HashMap::new();
        for record in records {
            match record {
                RegistrationRecord::Registered { host_id, token } => {
                    hosts.insert(host_id, registered(token));
                }
                RegistrationRecord::Removed { host_id } => {
                    hosts.remove(&host_id);
                }
                RegistrationRecord::Bound {
                    session_id,
                    environment,
                    host_id,
                } => {
                    hosts
                        .get_mut(&host_id)
                        .ok_or_else(|| {
                            brain::Error::Journal(
                                "a session is bound to a host that was never registered".into(),
                            )
                        })?
                        .sessions
                        .insert((session_id, environment));
                }
                RegistrationRecord::Released {
                    session_id,
                    environment,
                } => {
                    for host in hosts.values_mut() {
                        host.sessions
                            .remove(&(session_id.clone(), environment.clone()));
                    }
                }
            }
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(State {
                log,
                hosts,
                closed: false,
            })),
            limits: limits.clone(),
        })
    }

    fn bind_session(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
        host_id: &HostId,
    ) -> Result<(), brain::Error> {
        let mut state = self.lock().map_err(api_error)?;
        let host = state
            .hosts
            .get(host_id)
            .ok_or_else(|| brain::Error::NotFound("host registration is missing".into()))?;
        if host
            .sessions
            .contains(&(session_id.clone(), environment.clone()))
        {
            return Ok(());
        }
        crate::persistence::append(
            &mut state.log,
            &RegistrationRecord::Bound {
                environment: environment.clone(),
                session_id: session_id.clone(),
                host_id: host_id.clone(),
            },
        )?;
        state
            .hosts
            .get_mut(host_id)
            .expect("registration checked")
            .sessions
            .insert((session_id.clone(), environment.clone()));
        Ok(())
    }

    fn release_session(
        &self,
        session_id: &SessionId,
        environment: &EnvironmentName,
    ) -> Result<(), brain::Error> {
        let mut state = self.lock().map_err(api_error)?;
        if state.hosts.values().any(|host| {
            host.sessions
                .contains(&(session_id.clone(), environment.clone()))
        }) {
            crate::persistence::append(
                &mut state.log,
                &RegistrationRecord::Released {
                    environment: environment.clone(),
                    session_id: session_id.clone(),
                },
            )?;
        }
        for host in state.hosts.values_mut() {
            host.sessions
                .remove(&(session_id.clone(), environment.clone()));
        }
        Ok(())
    }

    pub fn register(&self) -> Result<HostRegistration, ApiError> {
        let host_id = HostId::new(brain::random_id("host"));
        let token = brain::random_id("bht");
        let mut state = self.lock()?;
        let expired = state
            .hosts
            .iter()
            .filter(|(_, host)| {
                host.sessions.is_empty()
                    && host.commands.is_none()
                    && host.disconnected_at.elapsed() >= self.limits.host_unconnected()
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            crate::persistence::append(
                &mut state.log,
                &RegistrationRecord::Removed {
                    host_id: id.clone(),
                },
            )
            .map_err(|error| ApiError::internal(error.to_string()))?;
            state.hosts.remove(&id);
        }
        if state.hosts.len() >= crate::limits::ceiling(self.limits.max_hosts) {
            return Err(ApiError::overloaded("host table is full"));
        }
        crate::persistence::append(
            &mut state.log,
            &RegistrationRecord::Registered {
                host_id: host_id.clone(),
                token: digest(&token),
            },
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
        state
            .hosts
            .insert(host_id.clone(), registered(digest(&token)));
        Ok(HostRegistration { host_id, token })
    }

    pub fn is_connected(&self, host_id: &HostId) -> Result<bool, ApiError> {
        Ok(self.lock()?.hosts.get(host_id).is_some_and(|host| {
            host.commands
                .as_ref()
                .is_some_and(|sender| !sender.is_closed())
        }))
    }

    pub fn connect(
        &self,
        host_id: &HostId,
        token: &str,
    ) -> Result<brain_http::HostConnection, ApiError> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(ApiError::overloaded("Brain has drained its active work"));
        }
        let host = authorized(&mut state, host_id, token)?;
        let (sender, receiver) = mpsc::channel(self.limits.max_host_commands.max(1));
        let (disconnect, displaced) = oneshot::channel();
        if let Some(previous) = host.disconnect.replace(disconnect) {
            let _ = previous.send(());
        }
        host.connection = host.connection.saturating_add(1);
        let connection = host.connection;
        host.commands = Some(sender);
        let hosts = self.clone();
        let closing_host = host_id.clone();
        Ok(brain_http::HostConnection {
            commands: receiver,
            displaced,
            on_close: Some(Box::new(move || {
                hosts.close_connection(&closing_host, connection);
            })),
        })
    }

    pub fn resolve(
        &self,
        host_id: &HostId,
        token: &str,
        result: HostResult,
    ) -> Result<(), ApiError> {
        let mut state = self.lock()?;
        let host = authorized(&mut state, host_id, token)?;
        let Some(pending) = host.pending.remove(&(result.session_id, result.sequence)) else {
            return Err(ApiError::conflict("the host command is no longer pending"));
        };
        pending
            .outcome
            .send(result.outcome)
            .map_err(|_| ApiError::conflict("the host command is no longer pending"))
    }

    pub async fn emit(
        &self,
        host_id: &HostId,
        token: &str,
        event: HostEvent,
    ) -> Result<HostEventAck, ApiError> {
        let events = {
            let mut state = self.lock()?;
            let host = authorized(&mut state, host_id, token)?;
            host.pending
                .get(&(event.session_id, event.sequence))
                .map(|pending| pending.events.clone())
                .ok_or_else(|| ApiError::conflict("the host command is no longer pending"))?
        };
        let (reply, answer) = oneshot::channel();
        events
            .try_send(PendingEvent {
                kind: event.event_type,
                data: event.data,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    ApiError::overloaded("host Event queue is full")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    ApiError::conflict("the host command is no longer pending")
                }
            })?;
        match answer.await {
            Ok(Ok(sequence)) => Ok(HostEventAck { sequence }),
            Ok(Err(message)) => Err(ApiError::invalid_request(message)),
            Err(_) => Err(ApiError::conflict("the host command is no longer pending")),
        }
    }

    async fn invoke(
        &self,
        host_id: &HostId,
        operation: &EnvironmentOperation,
        name: &str,
        input: &serde_json::Value,
        deadline_ms: u64,
        services: &dyn ExecutionServices,
    ) -> Result<Outcome, brain::Error> {
        let key = (operation.session_id.clone(), operation.sequence);
        let (result_sender, mut result_receiver) = oneshot::channel();
        let (event_sender, mut event_receiver) = mpsc::channel(8);
        let command = HostCommand {
            environment: operation.environment.clone(),
            session_id: operation.session_id.clone(),
            sequence: operation.sequence,
            deadline_at_ms: wall_clock_ms().saturating_add(deadline_ms),
            operation: HostOperation::InvokeTool {
                name: name.to_owned(),
                input: input.clone(),
            },
        };
        let command_sender = {
            let mut state = self
                .inner
                .lock()
                .map_err(|_| brain::Error::Executor("host table is poisoned".into()))?;
            let host = state
                .hosts
                .get_mut(host_id)
                .ok_or_else(|| brain::Error::Executor("the host does not exist".into()))?;
            let sender = host
                .commands
                .clone()
                .ok_or_else(|| brain::Error::Executor("the host is not connected".into()))?;
            host.pending.insert(
                key.clone(),
                PendingCall {
                    outcome: result_sender,
                    events: event_sender,
                },
            );
            sender
        };
        let mut pending = Pending {
            hosts: self.clone(),
            host_id: host_id.clone(),
            key,
        };
        command_sender
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    brain::Error::Overloaded("host command queue is full".into())
                }
                mpsc::error::TrySendError::Closed(_) => {
                    brain::Error::Executor("the host is not connected".into())
                }
            })?;
        let deadline = tokio::time::sleep(Duration::from_millis(deadline_ms));
        tokio::pin!(deadline);
        let result = loop {
            tokio::select! {
                biased;
                result = &mut result_receiver => break result.map_err(|_| {
                    brain::Error::Ambiguous("the host's result was lost after dispatch".into())
                }),
                () = command_sender.closed() => break Err(brain::Error::Ambiguous(
                    "the host disconnected after dispatch".into(),
                )),
                () = &mut deadline => break Err(brain::Error::Ambiguous(
                    "the Tool deadline elapsed after dispatch".into(),
                )),
                Some(event) = event_receiver.recv() => {
                    let answer = services.call("emit", serde_json::json!({"event_type": event.kind, "data": event.data})).await.and_then(|value| serde_json::from_value(value).map_err(|error| brain::Error::Executor(error.to_string()))).map_err(|error| error.to_string());
                    let _ = event.reply.send(answer);
                }
            }
        };
        pending.remove();
        result
    }

    fn cancel(
        &self,
        host_id: &HostId,
        operation: &EnvironmentOperation,
        target_sequence: u64,
    ) -> Result<(), brain::Error> {
        let sender = {
            let state = self
                .inner
                .lock()
                .map_err(|_| brain::Error::Executor("host table is poisoned".into()))?;
            state
                .hosts
                .get(host_id)
                .and_then(|host| host.commands.clone())
                .ok_or_else(|| brain::Error::Executor("the host is not connected".into()))?
        };
        let command = HostCommand {
            environment: operation.environment.clone(),
            session_id: operation.session_id.clone(),
            sequence: operation.sequence,
            deadline_at_ms: wall_clock_ms().saturating_add(5_000),
            operation: HostOperation::CancelTool { target_sequence },
        };
        sender.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                brain::Error::Overloaded("host command queue is full".into())
            }
            mpsc::error::TrySendError::Closed(_) => {
                brain::Error::Executor("the host is not connected".into())
            }
        })
    }

    fn remove_pending(&self, host_id: &HostId, key: &(SessionId, u64)) {
        if let Ok(mut state) = self.inner.lock()
            && let Some(host) = state.hosts.get_mut(host_id)
        {
            host.pending.remove(key);
        }
    }

    pub fn close(&self) {
        let mut state = self.inner.lock().expect("host state poisoned");
        state.closed = true;
        for host in state.hosts.values_mut() {
            host.commands = None;
            if let Some(disconnect) = host.disconnect.take() {
                let _ = disconnect.send(());
            }
        }
    }

    fn close_connection(&self, host_id: &HostId, connection: u64) {
        if let Ok(mut state) = self.inner.lock()
            && let Some(host) = state.hosts.get_mut(host_id)
            && host.connection == connection
        {
            host.commands = None;
            host.disconnect = None;
            host.disconnected_at = Instant::now();
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, ApiError> {
        self.inner
            .lock()
            .map_err(|_| ApiError::internal("host table is poisoned"))
    }
}

#[async_trait]
impl EnvironmentAdapter for HostEnvironment {
    async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let Driver::Host { host_id } = &environment.driver else {
            return Err(brain::Error::InvalidState(
                "the host env was handed an Environment it does not reach".into(),
            ));
        };
        match (&operation.request, services) {
            (EnvironmentRequest::Setup { .. }, _) => {
                if !self.is_connected(host_id).map_err(api_error)? {
                    return Err(brain::Error::Executor("the host is not connected".into()));
                }
                self.bind_session(&operation.session_id, &operation.environment, host_id)?;
                Ok(EnvironmentReceipt::Accepted)
            }
            (
                EnvironmentRequest::Execute {
                    implementation,
                    input,
                    deadline_ms,
                    ..
                },
                Some(services),
            ) => {
                #[derive(serde::Deserialize)]
                #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
                enum Implementation {
                    HostFunction { name: String },
                }
                let implementation: Implementation =
                    match serde_json::from_value(implementation.clone()) {
                        Ok(implementation) => implementation,
                        Err(_) => return Ok(unsupported("run this implementation")),
                    };
                let Implementation::HostFunction { name } = implementation;
                let outcome = self
                    .invoke(host_id, operation, &name, input, *deadline_ms, &*services)
                    .await?;
                Ok(match outcome {
                    Outcome::Ok { value } => EnvironmentReceipt::Result { output: value },
                    Outcome::Error { error } => EnvironmentReceipt::Failure {
                        code: error.code,
                        message: error.message,
                        retryable: error.retryable,
                        details: error.details,
                    },
                    Outcome::Unknown { message } => EnvironmentReceipt::Unknown { message },
                    Outcome::Timeout => EnvironmentReceipt::Failure {
                        code: "timeout".into(),
                        message: "host invocation timed out".into(),
                        retryable: false,
                        details: None,
                    },
                    Outcome::Cancelled => EnvironmentReceipt::Failure {
                        code: "cancelled".into(),
                        message: "host invocation cancelled".into(),
                        retryable: false,
                        details: None,
                    },
                })
            }
            (EnvironmentRequest::Execute { .. }, None) => Err(brain::Error::InvalidState(
                "the host env needs invocation services".into(),
            )),
            (EnvironmentRequest::Cancel { target_sequence }, _) => {
                self.cancel(host_id, operation, *target_sequence)?;
                Ok(EnvironmentReceipt::Accepted)
            }
            (EnvironmentRequest::Detach | EnvironmentRequest::Teardown, _) => {
                self.release_session(&operation.session_id, &operation.environment)?;
                Ok(EnvironmentReceipt::Accepted)
            }
            (EnvironmentRequest::Call { .. }, _) => Ok(unsupported("answer calls")),
        }
    }
}

struct Pending {
    hosts: HostEnvironment,
    host_id: HostId,
    key: (SessionId, u64),
}

impl Pending {
    fn remove(&mut self) {
        self.hosts.remove_pending(&self.host_id, &self.key);
        self.key.1 = 0;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.key.1 != 0 {
            self.hosts.remove_pending(&self.host_id, &self.key);
        }
    }
}

fn authorized<'a>(
    state: &'a mut State,
    host_id: &HostId,
    token: &str,
) -> Result<&'a mut Host, ApiError> {
    let host = state
        .hosts
        .get_mut(host_id)
        .ok_or_else(|| ApiError::not_found("the host does not exist"))?;
    if host.token != digest(token) {
        return Err(ApiError::unauthorized("the host token is invalid"));
    }
    Ok(host)
}

fn api_error(error: ApiError) -> brain::Error {
    brain::Error::Executor(error.message)
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_protocol::EnvironmentName;

    fn test_hosts() -> HostEnvironment {
        HostEnvironment::open(
            &std::env::temp_dir()
                .join(format!("brain-hosts-{}", rand::random::<u64>()))
                .join("hosts.log"),
            &Default::default(),
        )
        .unwrap()
    }

    fn entry(host_id: HostId) -> Environment {
        Environment {
            name: EnvironmentName::new("app"),
            driver: Driver::Host { host_id },
            configuration: serde_json::json!({}),
        }
    }

    fn operation(request: EnvironmentRequest) -> EnvironmentOperation {
        EnvironmentOperation {
            sequence: 7,
            environment: EnvironmentName::new("app"),
            session_id: SessionId::new("ses_12345678901234567890"),
            request,
        }
    }

    fn invoke() -> EnvironmentOperation {
        operation(EnvironmentRequest::Execute {
            implementation: serde_json::json!({"type": "host_function", "name": "read_dom"}),
            callback: None,
            input: serde_json::json!({}),
            deadline_ms: 5_000,
        })
    }

    struct NoEvents;

    #[async_trait::async_trait]
    impl brain::ToolServices for NoEvents {
        async fn emit(&self, _: String, _: serde_json::Value) -> Result<u64, brain::Error> {
            Ok(8)
        }

        fn telemetry(&self, _: serde_json::Value) {}
    }

    #[tokio::test]
    async fn registration_survives_restart_and_placed_sessions_prevent_expiry() {
        let path = std::env::temp_dir()
            .join(format!("brain-hosts-{}", rand::random::<u64>()))
            .join("hosts.log");
        let hosts = HostEnvironment::open(&path, &Default::default()).unwrap();
        let registration = hosts.register().unwrap();
        drop(hosts);
        let hosts = HostEnvironment::open(&path, &Default::default()).unwrap();
        let connection = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        assert!(hosts.connect(&registration.host_id, "wrong token").is_err());
        let session = SessionId::new("ses_pinned");
        let setup = EnvironmentOperation {
            session_id: session.clone(),
            ..operation(EnvironmentRequest::Setup {
                configuration: serde_json::json!({}),
            })
        };
        hosts
            .execute(&entry(registration.host_id.clone()), &setup, None)
            .await
            .unwrap();
        drop(connection);
        drop(hosts);
        let hosts = HostEnvironment::open(&path, &Default::default()).unwrap();
        hosts
            .lock()
            .unwrap()
            .hosts
            .get_mut(&registration.host_id)
            .unwrap()
            .disconnected_at = Instant::now() - crate::ServerLimits::default().host_unconnected();
        hosts.register().unwrap();
        assert!(
            hosts
                .connect(&registration.host_id, &registration.token)
                .is_ok()
        );
        let detach = EnvironmentOperation {
            session_id: session,
            ..operation(EnvironmentRequest::Detach)
        };
        hosts
            .execute(&entry(registration.host_id.clone()), &detach, None)
            .await
            .unwrap();
        hosts
            .lock()
            .unwrap()
            .hosts
            .get_mut(&registration.host_id)
            .unwrap()
            .disconnected_at = Instant::now() - crate::ServerLimits::default().host_unconnected();
        hosts.register().unwrap();
        assert!(
            hosts
                .connect(&registration.host_id, &registration.token)
                .is_err()
        );
    }

    #[tokio::test]
    async fn setup_needs_a_connected_host() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let setup = operation(EnvironmentRequest::Setup {
            configuration: serde_json::json!({}),
        });
        assert!(
            hosts
                .execute(&entry(registration.host_id.clone()), &setup, None)
                .await
                .is_err()
        );
        let _connection = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        hosts
            .execute(&entry(registration.host_id), &setup, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_host_command_is_sent_once_and_resolved_by_session_sequence() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let mut connection = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        let executing = tokio::spawn({
            let hosts = hosts.clone();
            let entry = entry(registration.host_id.clone());
            async move {
                hosts
                    .execute(
                        &entry,
                        &invoke(),
                        Some(Arc::new(brain_sessions::SessionServices::Tool(Arc::new(
                            NoEvents,
                        )))),
                    )
                    .await
            }
        });
        let command = connection.commands.recv().await.unwrap();
        assert_eq!(command.sequence, 7);
        assert!(matches!(
            &command.operation,
            HostOperation::InvokeTool { name, .. } if name == "read_dom"
        ));
        assert!(connection.commands.try_recv().is_err());
        hosts
            .resolve(
                &registration.host_id,
                &registration.token,
                HostResult {
                    session_id: command.session_id,
                    sequence: command.sequence,
                    outcome: Outcome::Ok {
                        value: serde_json::json!({"ok": true}),
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            executing.await.unwrap().unwrap(),
            EnvironmentReceipt::Result { output: value } if value == serde_json::json!({"ok": true})
        ));
    }

    #[tokio::test]
    async fn dispatch_without_a_connected_host_fails_before_send() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let error = hosts
            .execute(
                &entry(registration.host_id),
                &invoke(),
                Some(Arc::new(brain_sessions::SessionServices::Tool(Arc::new(
                    NoEvents,
                )))),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, brain::Error::Executor(_)));
    }

    #[test]
    fn closing_a_connection_keeps_its_registration_reconnectable() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let connection = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        assert!(hosts.is_connected(&registration.host_id).unwrap());
        drop(connection);
        assert!(!hosts.is_connected(&registration.host_id).unwrap());
        let replacement = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        assert!(hosts.is_connected(&registration.host_id).unwrap());
        drop(replacement);
    }

    #[test]
    fn releasing_one_environment_preserves_its_sibling_host_binding() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let session = SessionId::new("ses_shared");
        let first = EnvironmentName::new("first");
        let second = EnvironmentName::new("second");
        hosts
            .bind_session(&session, &first, &registration.host_id)
            .unwrap();
        hosts
            .bind_session(&session, &second, &registration.host_id)
            .unwrap();
        hosts.release_session(&session, &first).unwrap();
        let state = hosts.lock().unwrap();
        let bindings = &state.hosts[&registration.host_id].sessions;
        assert!(!bindings.contains(&(session.clone(), first)));
        assert!(bindings.contains(&(session, second)));
    }

    #[tokio::test]
    async fn replacing_a_host_connection_makes_an_inflight_outcome_unknown() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let mut first = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        let executing = tokio::spawn({
            let hosts = hosts.clone();
            let entry = entry(registration.host_id.clone());
            async move {
                hosts
                    .execute(
                        &entry,
                        &invoke(),
                        Some(Arc::new(brain_sessions::SessionServices::Tool(Arc::new(
                            NoEvents,
                        )))),
                    )
                    .await
            }
        });
        first.commands.recv().await.unwrap();
        let _replacement = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        (&mut first.displaced).await.unwrap();
        drop(first);
        let error = executing.await.unwrap().unwrap_err();
        assert!(matches!(error, brain::Error::Ambiguous(_)));
    }
}
