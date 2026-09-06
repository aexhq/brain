//! The brain env: Brain's own Environment, hosted in this process.
//!
//! A Tool placed here is a Component admitted through `POST /v1/tools`; the Agentloop
//! is one admitted through `POST /v1/agentloops`. Each runs in a fresh Wasmtime store
//! with exactly the grants its needs name, bounded by the deployment's allow-lists:
//! `file:///workspace` is the session's directory, `file:///scratch` lives for one
//! invocation, both read-only unless the need says `?access=write`; an `https:` or
//! `http:` origin, exact or `scheme://*.domain`, opens `wasi:http` to it. Secrets are the
//! Environment's own option, `{ "secrets": [names] }`, read from the process environment
//! and mounted under `/secrets`. Nothing is installed: `pkg:` is refused.

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use brain::{ToolServices, TurnServices};
use brain_loophost::{
    Access, HostCall, LoopError, NativeEnvironment, NativeToolInput, TurnBridge, WorkerPool,
    Workspace, network_covers,
};
use brain_protocol::{
    Environment, EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, Outcome,
    OutcomeError, SessionId, ToolId, TurnError,
};

use super::{EnvironmentAdapter, Services, adapter::unsupported};

/// The deployment's ceiling on what the brain env may grant.
#[derive(Clone, Debug, Default)]
pub struct NativePolicy {
    /// Origins, exact or `scheme://*.domain`.
    pub network: HashSet<String>,
    /// Process environment variable names.
    pub secrets: HashSet<String>,
    /// `scratch`, `workspace`, or both.
    pub filesystem: HashSet<String>,
}

pub struct BrainEnvironment {
    pool: Arc<WorkerPool>,
    policy: NativePolicy,
    /// Every session's `/workspace`, one directory per session.
    workspaces: PathBuf,
}

/// The brain env's own options.
#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Configuration {
    secrets: Vec<String>,
}

/// A Tool's implementation as the brain env reads it.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Implementation {
    BrainComponent {
        id: ToolId,
        #[serde(default)]
        configuration: serde_json::Value,
    },
}

impl BrainEnvironment {
    pub fn new(pool: Arc<WorkerPool>, policy: NativePolicy, workspaces: PathBuf) -> Self {
        Self {
            pool,
            policy,
            workspaces,
        }
    }

    fn configuration(environment: &Environment) -> Result<Configuration, brain::Error> {
        if environment.configuration.is_null() {
            return Ok(Configuration::default());
        }
        serde_json::from_value(environment.configuration.clone()).map_err(|error| {
            brain::Error::InvalidState(format!("brain env configuration: {error}"))
        })
    }

    /// The grants one invocation receives: its needs, bounded by the policy, plus the
    /// Environment's secrets.
    async fn grants(
        &self,
        session: &SessionId,
        configuration: &Configuration,
        needs: &[String],
    ) -> Result<NativeEnvironment, brain::Error> {
        let mut granted = NativeEnvironment::default();
        for name in &configuration.secrets {
            if !self.policy.secrets.contains(name) {
                return Err(refused(format!(
                    "secret `{name}` is not granted by this server"
                )));
            }
            let value = std::env::var(name)
                .map_err(|_| refused(format!("secret `{name}` is not configured")))?;
            granted.secrets.insert(name.clone(), value);
        }
        for need in needs {
            self.grant(session, need, &mut granted)?;
        }
        if let Some(workspace) = &granted.workspace {
            tokio::fs::create_dir_all(&workspace.path)
                .await
                .map_err(|error| brain::Error::Executor(error.to_string()))?;
        }
        Ok(granted)
    }

    fn grant(
        &self,
        session: &SessionId,
        need: &str,
        granted: &mut NativeEnvironment,
    ) -> Result<(), brain::Error> {
        let uri = url::Url::parse(need)
            .map_err(|error| refused(format!("need `{need}` is not a URI: {error}")))?;
        match uri.scheme() {
            "file" => {
                let access = file_access(&uri).ok_or_else(|| {
                    refused(format!(
                        "need `{need}` carries a query other than access=read or access=write"
                    ))
                })?;
                let root = uri.path().trim_end_matches('/');
                if uri.host_str().is_some_and(|host| !host.is_empty()) {
                    return Err(refused(format!(
                        "need `{need}` names a host; the brain env has file:///workspace and file:///scratch"
                    )));
                }
                match root {
                    "/workspace" => {
                        self.filesystem_granted("workspace", need)?;
                        let path = self.workspaces.join(session.as_str());
                        granted.workspace = Some(Workspace {
                            path: path.to_string_lossy().into_owned(),
                            access: widest(granted.workspace.as_ref().map(|w| w.access), access),
                        });
                    }
                    "/scratch" => {
                        self.filesystem_granted("scratch", need)?;
                        granted.scratch = Some(widest(granted.scratch, access));
                    }
                    _ => {
                        return Err(refused(format!(
                            "need `{need}` names a location the brain env does not have; it has file:///workspace and file:///scratch"
                        )));
                    }
                }
            }
            "https" | "http" => {
                let origin = network_origin(&uri).ok_or_else(|| {
                    refused(format!(
                        "need `{need}` must be an origin, `scheme://host[:port]`, with nothing after it"
                    ))
                })?;
                if !self
                    .policy
                    .network
                    .iter()
                    .any(|grant| network_covers(grant, &origin))
                {
                    return Err(refused(format!(
                        "need `{need}` is not within the network this server grants"
                    )));
                }
                granted.network_allow.push(origin);
            }
            "pkg" => {
                return Err(refused(format!(
                    "need `{need}` cannot be met: the brain env installs nothing, a Component brings its own code"
                )));
            }
            "wss" => {
                return Err(refused(format!(
                    "need `{need}` cannot be met: the brain env has no WebSocket, a Component reaches the network through wasi:http"
                )));
            }
            _ => {
                return Err(refused(format!(
                    "need `{need}` names a scheme the brain env does not honour"
                )));
            }
        }
        Ok(())
    }

    fn filesystem_granted(&self, root: &str, need: &str) -> Result<(), brain::Error> {
        if self.policy.filesystem.contains(root) {
            return Ok(());
        }
        Err(refused(format!(
            "need `{need}` asks for a filesystem root this server does not grant"
        )))
    }

    async fn invoke(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: &dyn ToolServices,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let EnvironmentRequest::Invoke {
            implementation,
            needs,
            input,
            deadline_ms,
            ..
        } = &operation.request
        else {
            unreachable!("invoke is called for invoke requests only");
        };
        let Some(implementation) = implementation else {
            return Err(brain::Error::InvalidState(
                "a Tool in the brain env carries a brain_component implementation".into(),
            ));
        };
        let Implementation::BrainComponent { id, configuration } =
            serde_json::from_value(implementation.clone()).map_err(|error| {
                brain::Error::InvalidState(format!(
                    "a Tool in the brain env carries a brain_component implementation: {error}"
                ))
            })?;
        if !self.pool.tool_status(&id).await.map_err(loop_error)? {
            return Err(brain::Error::InvalidState(format!(
                "Tool Component `{id}` has not been admitted"
            )));
        }
        let grants = self
            .grants(
                &operation.session_id,
                &Self::configuration(environment)?,
                needs,
            )
            .await?;
        let bridge = NativeToolBridge { services };
        let ran = self
            .pool
            .tool(
                id,
                grants,
                NativeToolInput {
                    input: input.clone(),
                    configuration,
                    deadline_at_ms: wall_clock_ms().saturating_add(*deadline_ms),
                },
                &bridge,
            )
            .await;
        match ran {
            Ok(value) => Ok(EnvironmentReceipt::Outcome {
                outcome: Outcome::Ok { value },
            }),
            Err(LoopError::Turn(error)) => Ok(EnvironmentReceipt::Outcome {
                outcome: Outcome::Error {
                    error: OutcomeError {
                        code: error.code,
                        message: error.message,
                        details: None,
                    },
                },
            }),
            Err(LoopError::Overloaded) => Err(brain::Error::Overloaded(
                "the brain env is at capacity".into(),
            )),
            Err(LoopError::Failed(message)) => Err(brain::Error::Ambiguous(message)),
        }
    }

    async fn turn(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: &Arc<dyn TurnServices>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let EnvironmentRequest::Turn { id, needs, input } = &operation.request else {
            unreachable!("turn is called for turn requests only");
        };
        if !self.pool.status(id).await.map_err(loop_error)? {
            return Err(brain::Error::InvalidState(format!(
                "Agentloop `{id}` has not been admitted"
            )));
        }
        let grants = self
            .grants(
                &operation.session_id,
                &Self::configuration(environment)?,
                needs,
            )
            .await?;
        let bridge = ServicesBridge(services.clone());
        match self
            .pool
            .turn(id.clone(), grants, (**input).clone(), &bridge)
            .await
        {
            Ok(output) => Ok(EnvironmentReceipt::Turned { output }),
            Err(LoopError::Turn(error)) => Ok(EnvironmentReceipt::Failure {
                code: error.code,
                message: error.message,
                retryable: error.retryable,
            }),
            Err(LoopError::Overloaded) => Err(brain::Error::Overloaded(
                "the brain env is at capacity".into(),
            )),
            Err(LoopError::Failed(message)) => Err(brain::Error::Executor(message)),
        }
    }
}

#[async_trait]
impl EnvironmentAdapter for BrainEnvironment {
    async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services<'_>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        match (&operation.request, services) {
            (EnvironmentRequest::Setup { needs, .. }, _) => {
                // Refuse at setup what cannot be honoured, naming the need, so a session
                // that could never run its Tools fails at create rather than at the first
                // call.
                self.grants(
                    &operation.session_id,
                    &Self::configuration(environment)?,
                    needs,
                )
                .await?;
                Ok(EnvironmentReceipt::Accepted)
            }
            (EnvironmentRequest::Invoke { .. }, Services::Tool(services)) => {
                self.invoke(environment, operation, services).await
            }
            (EnvironmentRequest::Turn { .. }, Services::Turn(services)) => {
                self.turn(environment, operation, services).await
            }
            (EnvironmentRequest::Invoke { .. } | EnvironmentRequest::Turn { .. }, _) => {
                Err(brain::Error::InvalidState(
                    "the brain env needs the call's services to run it".into(),
                ))
            }
            // The deadline kills an overdue call on the calling side; there is nothing
            // else to reach into a fresh store with.
            (EnvironmentRequest::Cancel { .. } | EnvironmentRequest::Detach, _) => {
                Ok(EnvironmentReceipt::Accepted)
            }
            (EnvironmentRequest::Teardown, _) => {
                let workspace = self.workspaces.join(operation.session_id.as_str());
                match tokio::fs::remove_dir_all(workspace).await {
                    Ok(()) => Ok(EnvironmentReceipt::Accepted),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Ok(EnvironmentReceipt::Accepted)
                    }
                    Err(error) => Err(brain::Error::Executor(error.to_string())),
                }
            }
            (EnvironmentRequest::Call { .. }, _) => Ok(unsupported("answer calls")),
        }
    }
}

/// The access a `file:` need asks for: read unless it says `?access=write`; `None`
/// for any other query.
fn file_access(uri: &url::Url) -> Option<Access> {
    let mut access = Access::Read;
    for (key, value) in uri.query_pairs() {
        match (&*key, &*value) {
            ("access", "read") => {}
            ("access", "write") => access = Access::Write,
            _ => return None,
        }
    }
    Some(access)
}

fn widest(current: Option<Access>, requested: Access) -> Access {
    match (current, requested) {
        (Some(Access::Write), _) | (_, Access::Write) => Access::Write,
        _ => Access::Read,
    }
}

/// `scheme://host[:port]`, lowercased, for a need that is exactly an origin. A host of
/// `*.domain` is a family of hosts.
fn network_origin(uri: &url::Url) -> Option<String> {
    let host = uri.host_str()?;
    if !uri.username().is_empty()
        || uri.password().is_some()
        || !matches!(uri.path(), "" | "/")
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return None;
    }
    let port = uri
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Some(format!(
        "{}://{}{port}",
        uri.scheme(),
        host.to_ascii_lowercase()
    ))
}

fn refused(message: String) -> brain::Error {
    brain::Error::InvalidState(message)
}

fn loop_error(error: LoopError) -> brain::Error {
    match error {
        LoopError::Overloaded => brain::Error::Overloaded(error.to_string()),
        LoopError::Turn(error) => brain::Error::Loop(error),
        LoopError::Failed(message) => brain::Error::Executor(message),
    }
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Brain's services as the worker's guest reaches them during a turn: JSON in, JSON
/// out, one call at a time.
struct ServicesBridge(Arc<dyn TurnServices>);

#[async_trait]
impl TurnBridge for ServicesBridge {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        let answer = match call {
            HostCall::Events { after } => {
                let page = self.0.events(after).await.map_err(turn_error)?;
                serde_json::to_string(&page).map_err(|error| bridge_error("internal", error))?
            }
            HostCall::Model { request_json } => {
                let request = serde_json::from_str(&request_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                let result = self.0.model(request).await.map_err(turn_error)?;
                serde_json::to_string(&result).map_err(|error| bridge_error("internal", error))?
            }
            HostCall::Dispatch { calls_json } => {
                let calls = serde_json::from_str(&calls_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                let results = self.0.dispatch(calls).await.map_err(turn_error)?;
                serde_json::to_string(&results).map_err(|error| bridge_error("internal", error))?
            }
            HostCall::Emit { kind, payload_json } => {
                let payload = serde_json::from_str(&payload_json)
                    .map_err(|error| bridge_error("invalid_request", error))?;
                self.0
                    .emit(kind, payload)
                    .await
                    .map_err(turn_error)?
                    .to_string()
            }
            HostCall::Telemetry { record_json } => {
                if let Ok(record) = serde_json::from_str(&record_json) {
                    self.0.telemetry(record);
                }
                String::new()
            }
        };
        Ok(answer)
    }

    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
}

/// What a Tool may reach during a call: its session's feed and telemetry.
struct NativeToolBridge<'a> {
    services: &'a dyn ToolServices,
}

#[async_trait]
impl TurnBridge for NativeToolBridge<'_> {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        match call {
            HostCall::Emit { kind, payload_json } => {
                let payload = serde_json::from_str(&payload_json)
                    .map_err(|error| TurnError::new("invalid_event", error.to_string()))?;
                self.services
                    .emit(kind, payload)
                    .await
                    .map(|sequence| sequence.to_string())
                    .map_err(|error| TurnError::new(error.code(), error.to_string()))
            }
            HostCall::Telemetry { record_json } => {
                if let Ok(record) = serde_json::from_str(&record_json) {
                    self.services.telemetry(record);
                }
                Ok(String::new())
            }
            _ => Err(TurnError::new(
                "unsupported_host_call",
                "a Tool may only emit Events or telemetry",
            )),
        }
    }

    fn cancelled(&self) -> bool {
        false
    }
}

fn turn_error(error: brain::Error) -> TurnError {
    TurnError {
        code: error.code().to_owned(),
        message: error.to_string(),
        retryable: error.retryable(),
    }
}

fn bridge_error(code: &str, error: impl std::fmt::Display) -> TurnError {
    TurnError::new(code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(policy: NativePolicy, root: &std::path::Path) -> BrainEnvironment {
        BrainEnvironment::new(
            Arc::new(WorkerPool::new(
                "unused",
                root.join("run"),
                root.join("agentloops"),
                Default::default(),
            )),
            policy,
            root.join("native-workspaces"),
        )
    }

    fn session() -> SessionId {
        SessionId::new("ses_grants")
    }

    #[tokio::test]
    async fn needs_become_grants_bounded_by_the_deployment_policy() {
        let root = tempfile::tempdir().unwrap();
        let brain = environment(
            NativePolicy {
                network: HashSet::from(["https://*.example.com".into()]),
                secrets: HashSet::new(),
                filesystem: HashSet::from(["workspace".into(), "scratch".into()]),
            },
            root.path(),
        );
        let granted = brain
            .grants(
                &session(),
                &Configuration::default(),
                &[
                    "file:///workspace".into(),
                    "file:///workspace?access=write".into(),
                    "file:///scratch".into(),
                    "https://API.example.com/".into(),
                ],
            )
            .await
            .unwrap();
        let workspace = granted.workspace.unwrap();
        assert_eq!(workspace.access, Access::Write);
        assert!(std::path::Path::new(&workspace.path).is_dir());
        assert_eq!(granted.scratch, Some(Access::Read));
        assert_eq!(granted.network_allow, vec!["https://api.example.com"]);

        for refused in [
            "https://elsewhere.net",
            "https://example.com",
            "https://api.example.com/path",
            "wss://api.example.com",
            "pkg:apt/ffmpeg",
            "file:///etc",
            "file://host/workspace",
            "file:///workspace?access=root",
            "mailto:someone@example.com",
        ] {
            let error = brain
                .grants(&session(), &Configuration::default(), &[refused.into()])
                .await
                .expect_err(refused)
                .to_string();
            assert!(error.contains(refused), "{error} must name {refused}");
        }
    }

    #[tokio::test]
    async fn the_policy_is_the_ceiling_on_filesystem_and_secrets() {
        let root = tempfile::tempdir().unwrap();
        let brain = environment(NativePolicy::default(), root.path());
        assert!(
            brain
                .grants(
                    &session(),
                    &Configuration::default(),
                    &["file:///scratch".into()]
                )
                .await
                .is_err()
        );
        assert!(
            brain
                .grants(
                    &session(),
                    &Configuration {
                        secrets: vec!["BRAIN_API_TOKEN".into()],
                    },
                    &[],
                )
                .await
                .is_err()
        );
        let entry = Environment {
            name: brain_protocol::EnvironmentName::new("brain"),
            driver: brain_protocol::Driver::Brain {},
            configuration: serde_json::json!({"network": {"allow": []}}),
        };
        assert!(
            BrainEnvironment::configuration(&entry).is_err(),
            "the brain env has no options but secrets"
        );
    }
}
