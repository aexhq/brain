use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use brain_protocol::{
    ApiError, HostCommand, HostEvent, HostEventAck, HostId, HostOperation, HostRegistration,
    HostResult, Outcome, SessionId, ToolCancellation, ToolDispatch,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::{mpsc, oneshot};

const COMMAND_CAPACITY: usize = 128;
const MAX_HOSTS: usize = 4_096;
const UNCONNECTED_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct ResidentHosts {
    inner: Arc<Mutex<State>>,
}

struct State {
    log: File,
    hosts: HashMap<HostId, Host>,
}

struct Host {
    sessions: HashSet<SessionId>,
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
        session_id: SessionId,
        host_ids: Vec<HostId>,
    },
    Released {
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

impl ResidentHosts {
    pub fn open(path: &Path) -> Result<Self, brain::Error> {
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
                    host_ids,
                } => {
                    for id in host_ids {
                        hosts
                            .get_mut(&id)
                            .ok_or_else(|| {
                                brain::Error::Journal("resident binding has no registration".into())
                            })?
                            .sessions
                            .insert(session_id.clone());
                    }
                }
                RegistrationRecord::Released { session_id } => {
                    for host in hosts.values_mut() {
                        host.sessions.remove(&session_id);
                    }
                }
            }
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(State { log, hosts })),
        })
    }

    pub fn bind_session(
        &self,
        session_id: &SessionId,
        host_ids: &[HostId],
    ) -> Result<(), ApiError> {
        let mut state = self.lock()?;
        for id in host_ids {
            if !state.hosts.contains_key(id) {
                return Err(ApiError::invalid_request(
                    "resident host registration is missing",
                ));
            }
        }
        if host_ids
            .iter()
            .any(|id| !state.hosts[id].sessions.contains(session_id))
        {
            crate::persistence::append(
                &mut state.log,
                &RegistrationRecord::Bound {
                    session_id: session_id.clone(),
                    host_ids: host_ids.to_vec(),
                },
            )
            .map_err(|error| ApiError::internal(error.to_string()))?;
        }
        for id in host_ids {
            state
                .hosts
                .get_mut(id)
                .expect("registration checked")
                .sessions
                .insert(session_id.clone());
        }
        Ok(())
    }

    pub fn release_session(&self, session_id: &SessionId) -> Result<(), ApiError> {
        let mut state = self.lock()?;
        if state
            .hosts
            .values()
            .any(|host| host.sessions.contains(session_id))
        {
            crate::persistence::append(
                &mut state.log,
                &RegistrationRecord::Released {
                    session_id: session_id.clone(),
                },
            )
            .map_err(|error| ApiError::internal(error.to_string()))?;
        }
        for host in state.hosts.values_mut() {
            host.sessions.remove(session_id);
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
                    && host.disconnected_at.elapsed() >= UNCONNECTED_TTL
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
        if state.hosts.len() >= MAX_HOSTS {
            return Err(ApiError::overloaded("resident host table is full"));
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
        let host = authorized(&mut state, host_id, token)?;
        let (sender, receiver) = mpsc::channel(COMMAND_CAPACITY);
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
            return Err(ApiError::conflict(
                "the resident command is no longer pending",
            ));
        };
        pending
            .outcome
            .send(result.outcome)
            .map_err(|_| ApiError::conflict("the resident command is no longer pending"))
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
                .ok_or_else(|| ApiError::conflict("the resident command is no longer pending"))?
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
                    ApiError::overloaded("resident Event queue is full")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    ApiError::conflict("the resident command is no longer pending")
                }
            })?;
        match answer.await {
            Ok(Ok(sequence)) => Ok(HostEventAck { sequence }),
            Ok(Err(message)) => Err(ApiError::invalid_request(message)),
            Err(_) => Err(ApiError::conflict(
                "the resident command is no longer pending",
            )),
        }
    }

    pub async fn execute(
        &self,
        dispatch: ToolDispatch,
        services: &dyn brain::ToolServices,
    ) -> Result<Outcome, brain::Error> {
        let host_id = dispatch.binding.host_id.clone().ok_or_else(|| {
            brain::Error::InvalidState("resident Tool has no registered host".into())
        })?;
        let key = (dispatch.session_id.clone(), dispatch.sequence);
        let (result_sender, mut result_receiver) = oneshot::channel();
        let (event_sender, mut event_receiver) = mpsc::channel(8);
        let command = HostCommand {
            session_id: dispatch.session_id,
            sequence: dispatch.sequence,
            deadline_at_ms: wall_clock_ms().saturating_add(dispatch.deadline_ms),
            operation: HostOperation::InvokeTool {
                invocation: dispatch.invocation,
            },
        };
        let command_sender = {
            let mut state = self
                .inner
                .lock()
                .map_err(|_| brain::Error::Executor("resident host table is poisoned".into()))?;
            let host = state.hosts.get_mut(&host_id).ok_or_else(|| {
                brain::Error::Executor("resident Tool host does not exist".into())
            })?;
            let sender = host.commands.clone().ok_or_else(|| {
                brain::Error::Executor("resident Tool host is not connected".into())
            })?;
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
            host_id,
            key,
        };
        command_sender
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    brain::Error::Overloaded("resident host command queue is full".into())
                }
                mpsc::error::TrySendError::Closed(_) => {
                    brain::Error::Executor("resident Tool host is not connected".into())
                }
            })?;
        let deadline = tokio::time::sleep(std::time::Duration::from_millis(dispatch.deadline_ms));
        tokio::pin!(deadline);
        let result = loop {
            tokio::select! {
                biased;
                result = &mut result_receiver => break result.map_err(|_| {
                    brain::Error::Ambiguous("resident Tool result was lost after dispatch".into())
                }),
                () = command_sender.closed() => break Err(brain::Error::Ambiguous(
                    "resident Tool host disconnected after dispatch".into(),
                )),
                () = &mut deadline => break Err(brain::Error::Ambiguous(
                    "resident Tool deadline elapsed after dispatch".into(),
                )),
                Some(event) = event_receiver.recv() => {
                    let answer = services.emit(event.kind, event.data).await.map_err(|error| error.to_string());
                    let _ = event.reply.send(answer);
                }
            }
        };
        pending.remove();
        result
    }

    pub async fn cancel(&self, cancellation: ToolCancellation) -> Result<(), brain::Error> {
        let host_id = cancellation.binding.host_id.ok_or_else(|| {
            brain::Error::InvalidState("resident Tool has no registered host".into())
        })?;
        let sender = {
            let state = self
                .inner
                .lock()
                .map_err(|_| brain::Error::Executor("resident host table is poisoned".into()))?;
            state
                .hosts
                .get(&host_id)
                .and_then(|host| host.commands.clone())
                .ok_or_else(|| {
                    brain::Error::Executor("resident Tool host is not connected".into())
                })?
        };
        let command = HostCommand {
            session_id: cancellation.session_id,
            sequence: cancellation.sequence,
            deadline_at_ms: wall_clock_ms().saturating_add(5_000),
            operation: HostOperation::CancelTool {
                target_sequence: cancellation.target_sequence,
            },
        };
        sender.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                brain::Error::Overloaded("resident host command queue is full".into())
            }
            mpsc::error::TrySendError::Closed(_) => {
                brain::Error::Executor("resident Tool host is not connected".into())
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
            .map_err(|_| ApiError::internal("resident host table is poisoned"))
    }
}

struct Pending {
    hosts: ResidentHosts,
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
        .ok_or_else(|| ApiError::not_found("resident host does not exist"))?;
    if host.token != digest(token) {
        return Err(ApiError::unauthorized("the resident host token is invalid"));
    }
    Ok(host)
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

    fn test_hosts() -> ResidentHosts {
        ResidentHosts::open(
            &std::env::temp_dir()
                .join(format!("brain-hosts-{}", rand::random::<u64>()))
                .join("hosts.log"),
        )
        .unwrap()
    }

    #[test]
    fn registration_survives_restart_and_session_references_prevent_expiry() {
        let path = std::env::temp_dir()
            .join(format!("brain-hosts-{}", rand::random::<u64>()))
            .join("hosts.log");
        let hosts = ResidentHosts::open(&path).unwrap();
        let registration = hosts.register().unwrap();
        drop(hosts);
        let hosts = ResidentHosts::open(&path).unwrap();
        assert!(
            hosts
                .connect(&registration.host_id, &registration.token)
                .is_ok()
        );
        assert!(hosts.connect(&registration.host_id, "wrong token").is_err());
        let session = SessionId::new("ses_pinned");
        hosts
            .bind_session(&session, std::slice::from_ref(&registration.host_id))
            .unwrap();
        drop(hosts);
        let hosts = ResidentHosts::open(&path).unwrap();
        hosts
            .lock()
            .unwrap()
            .hosts
            .get_mut(&registration.host_id)
            .unwrap()
            .disconnected_at = Instant::now() - UNCONNECTED_TTL;
        hosts.register().unwrap();
        assert!(
            hosts
                .connect(&registration.host_id, &registration.token)
                .is_ok()
        );
        hosts.release_session(&session).unwrap();
        hosts
            .lock()
            .unwrap()
            .hosts
            .get_mut(&registration.host_id)
            .unwrap()
            .disconnected_at = Instant::now() - UNCONNECTED_TTL;
        hosts.register().unwrap();
        assert!(
            hosts
                .connect(&registration.host_id, &registration.token)
                .is_err()
        );
    }
    use brain_protocol::{ToolBinding, ToolHosting, ToolInvocation};

    struct NoEvents;

    #[async_trait::async_trait]
    impl brain::ToolServices for NoEvents {
        async fn emit(&self, _: String, _: serde_json::Value) -> Result<u64, brain::Error> {
            Ok(8)
        }

        fn telemetry(&self, _: serde_json::Value) {}
    }

    fn dispatch(host_id: HostId) -> ToolDispatch {
        ToolDispatch {
            sequence: 7,
            session_id: SessionId::new("ses_12345678901234567890"),
            binding: ToolBinding {
                name: "read_dom".into(),
                environment_id: None,
                environment: None,
                attachment_id: None,
                host_id: Some(host_id),
                needs: Vec::new(),
                binding_names: Vec::new(),
                hosting: ToolHosting::Resident,
                implementation: None,
            },
            invocation: ToolInvocation {
                call_id: "call_1".into(),
                name: "read_dom".into(),
                input: serde_json::json!({}),
            },
            deadline_ms: 5_000,
        }
    }

    #[tokio::test]
    async fn a_resident_command_is_sent_once_and_resolved_by_session_sequence() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let mut connection = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        let executing = tokio::spawn({
            let hosts = hosts.clone();
            let dispatch = dispatch(registration.host_id.clone());
            async move { hosts.execute(dispatch, &NoEvents).await }
        });
        let command = connection.commands.recv().await.unwrap();
        assert_eq!(command.sequence, 7);
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
        assert_eq!(
            executing.await.unwrap().unwrap(),
            Outcome::Ok {
                value: serde_json::json!({"ok": true})
            }
        );
    }

    #[tokio::test]
    async fn dispatch_without_a_connected_host_fails_before_send() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let error = hosts
            .execute(dispatch(registration.host_id), &NoEvents)
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

    #[tokio::test]
    async fn replacing_a_host_connection_makes_an_inflight_outcome_unknown() {
        let hosts = test_hosts();
        let registration = hosts.register().unwrap();
        let mut first = hosts
            .connect(&registration.host_id, &registration.token)
            .unwrap();
        let executing = tokio::spawn({
            let hosts = hosts.clone();
            let dispatch = dispatch(registration.host_id.clone());
            async move { hosts.execute(dispatch, &NoEvents).await }
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
