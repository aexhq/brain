//! Brain's native Environment adapter; guest code runs in managed worker processes.
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

use crate::{
    Access, HostCall, LoopError, NativeEnvironment, NativeToolInput, TurnBridge, WorkerPool,
    Workspace, network_covers,
};
use async_trait::async_trait;
use brain::environment::ExecutionServices;
use brain_protocol::{
    Environment, EnvironmentOperation, EnvironmentReceipt, EnvironmentRequest, SessionId, ToolId,
    TurnError,
};

use brain::environment::{EnvironmentAdapter, Services, unsupported};

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
    /// One retained `/workspace` per session and Environment name.
    workspaces: PathBuf,
}

/// The brain env's own options.
#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Configuration {
    secrets: Vec<String>,
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
        environment: &brain_protocol::EnvironmentName,
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
            self.grant(session, environment, need, &mut granted)?;
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
        environment: &brain_protocol::EnvironmentName,
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
                        let path = self
                            .workspaces
                            .join(session.as_str())
                            .join(environment.as_str());
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

    async fn run(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Arc<dyn ExecutionServices>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        let EnvironmentRequest::Execute {
            implementation,
            needs,
            input,
            deadline_ms,
            ..
        } = &operation.request
        else {
            unreachable!()
        };
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Component {
            #[serde(rename = "type")]
            kind: String,
            entrypoint: String,
            id: String,
            #[serde(default)]
            configuration: serde_json::Value,
        }
        let component: Component = serde_json::from_value(implementation.clone())
            .map_err(|e| brain::Error::InvalidState(e.to_string()))?;
        if component.kind != "brain_component" {
            return Ok(unsupported("run this implementation"));
        }
        if !sha256_valid(&component.id) {
            return Err(brain::Error::InvalidState(
                "Component id must be a lowercase SHA-256 digest".into(),
            ));
        }
        let grants = self
            .grants(
                &operation.session_id,
                &operation.environment,
                &Self::configuration(environment)?,
                needs,
            )
            .await?;
        let bridge = ServicesBridge(services);
        let result = match component.entrypoint.as_str() {
            "turn" => {
                let input = serde_json::from_value(input.clone())
                    .map_err(|e| brain::Error::InvalidState(e.to_string()))?;
                self.pool
                    .turn(
                        brain_protocol::AgentloopId::new(component.id),
                        grants,
                        input,
                        &bridge,
                    )
                    .await
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|e| LoopError::Failed(e.to_string()))
                    })
            }
            "run" => {
                self.pool
                    .tool(
                        ToolId::new(component.id),
                        grants,
                        NativeToolInput {
                            input: input.clone(),
                            configuration: component.configuration,
                            deadline_at_ms: wall_clock_ms().saturating_add(*deadline_ms),
                        },
                        &bridge,
                    )
                    .await
            }
            _ => return Ok(unsupported("run this entrypoint")),
        };
        match result {
            Ok(output) => Ok(EnvironmentReceipt::Result { output }),
            Err(LoopError::Turn(error)) => Ok(EnvironmentReceipt::Failure {
                details: None,
                code: error.code,
                message: error.message,
                retryable: error.retryable,
            }),
            Err(LoopError::Overloaded) => Err(brain::Error::Overloaded(
                "the brain env is at capacity".into(),
            )),
            Err(LoopError::Failed(message)) => Err(brain::Error::Ambiguous(message)),
        }
    }
}

#[async_trait]
impl EnvironmentAdapter for BrainEnvironment {
    async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        match (&operation.request, services) {
            (EnvironmentRequest::Setup { needs, .. }, _) => {
                self.grants(
                    &operation.session_id,
                    &operation.environment,
                    &Self::configuration(environment)?,
                    needs,
                )
                .await?;
                Ok(EnvironmentReceipt::Accepted)
            }
            (EnvironmentRequest::Execute { .. }, Some(services)) => {
                self.run(environment, operation, services).await
            }
            (EnvironmentRequest::Execute { .. }, None) => Err(brain::Error::InvalidState(
                "the brain env needs invocation services".into(),
            )),
            (EnvironmentRequest::Cancel { .. } | EnvironmentRequest::Detach, _) => {
                Ok(EnvironmentReceipt::Accepted)
            }
            (EnvironmentRequest::Teardown, _) => {
                let workspace = self
                    .workspaces
                    .join(operation.session_id.as_str())
                    .join(operation.environment.as_str());
                match tokio::fs::remove_dir_all(workspace).await {
                    Ok(()) => Ok(EnvironmentReceipt::Accepted),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Ok(EnvironmentReceipt::Accepted)
                    }
                    Err(e) => Err(brain::Error::Executor(e.to_string())),
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

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

struct ServicesBridge(Arc<dyn ExecutionServices>);

#[async_trait]
impl TurnBridge for ServicesBridge {
    async fn call(&self, call: HostCall) -> Result<String, TurnError> {
        let parse =
            |text: &str| serde_json::from_str(text).map_err(|e| bridge_error("invalid_request", e));
        let (method, input) = match call {
            HostCall::SetTranscript { messages_json } => ("set_transcript", parse(&messages_json)?),
            HostCall::SetKv { key, value_json } => (
                "set_kv",
                serde_json::json!({"key": key, "value": parse(&value_json)?}),
            ),
            HostCall::Events { after } => ("events", serde_json::json!(after)),
            HostCall::Model { request_json } => ("model", parse(&request_json)?),
            HostCall::Dispatch { calls_json } => ("dispatch", parse(&calls_json)?),
            HostCall::Emit { kind, payload_json } => (
                "emit",
                serde_json::json!({"event_type": kind, "data": parse(&payload_json)?}),
            ),
            HostCall::Telemetry { record_json } => ("telemetry", parse(&record_json)?),
        };
        if !self.0.methods().contains(&method) {
            return Err(TurnError::new(
                "unsupported_host_call",
                "execution service is not granted",
            ));
        }
        let output = self.0.call(method, input).await.map_err(turn_error)?;
        serde_json::to_string(&output).map_err(|e| bridge_error("internal", e))
    }
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn can_dispatch(&self) -> bool {
        self.0.methods().contains(&"dispatch")
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

fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_digest_is_exactly_sixty_four_lowercase_hex_characters() {
        assert!(sha256_valid(&"a".repeat(64)));
        assert!(sha256_valid(&"0123456789abcdef".repeat(4)));
        assert!(!sha256_valid(&"a".repeat(63)));
        assert!(!sha256_valid(&"a".repeat(65)));
        assert!(!sha256_valid(&"A".repeat(64)));
        assert!(!sha256_valid(&"g".repeat(64)));
        assert!(!sha256_valid(""));
    }

    use super::*;

    fn environment(policy: NativePolicy, root: &std::path::Path) -> BrainEnvironment {
        BrainEnvironment::new(
            Arc::new(WorkerPool::new(
                "unused",
                root.join("run"),
                root.join("agentloops"),
                Default::default(),
                std::num::NonZeroUsize::new(1).unwrap(),
            )),
            policy,
            root.join("native-workspaces"),
        )
    }

    fn session() -> SessionId {
        SessionId::new("ses_grants")
    }

    #[tokio::test]
    async fn workspaces_survive_detach_and_teardown_removes_only_the_named_environment() {
        let root = tempfile::tempdir().unwrap();
        let brain = environment(
            NativePolicy {
                filesystem: HashSet::from(["workspace".into()]),
                ..Default::default()
            },
            root.path(),
        );
        let mut entries = Vec::new();
        for name in ["loop", "tools"] {
            let entry = Environment {
                name: brain_protocol::EnvironmentName::new(name),
                driver: brain_protocol::Driver::Brain {},
                configuration: serde_json::json!({}),
            };
            let operation = EnvironmentOperation {
                session_id: session(),
                environment: entry.name.clone(),
                sequence: 1,
                request: EnvironmentRequest::Setup {
                    configuration: serde_json::json!({}),
                    needs: vec!["file:///workspace?access=write".into()],
                },
            };
            brain.execute(&entry, &operation, None).await.unwrap();
            let path = root
                .path()
                .join("native-workspaces")
                .join(session().as_str())
                .join(name)
                .join("kept");
            std::fs::write(&path, name).unwrap();
            brain
                .execute(
                    &entry,
                    &EnvironmentOperation {
                        request: EnvironmentRequest::Detach,
                        ..operation.clone()
                    },
                    None,
                )
                .await
                .unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), name);
            entries.push((entry, operation, path));
        }
        let (entry, operation, path) = &entries[0];
        brain
            .execute(
                entry,
                &EnvironmentOperation {
                    request: EnvironmentRequest::Teardown,
                    ..operation.clone()
                },
                None,
            )
            .await
            .unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&entries[1].2).unwrap(), "tools");
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
                &brain_protocol::EnvironmentName::new("brain"),
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
                .grants(
                    &session(),
                    &brain_protocol::EnvironmentName::new("brain"),
                    &Configuration::default(),
                    &[refused.into()],
                )
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
                    &brain_protocol::EnvironmentName::new("brain"),
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
                    &brain_protocol::EnvironmentName::new("brain"),
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
