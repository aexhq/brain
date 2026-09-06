mod actor;
mod config;
mod services;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use brain_protocol::codes::{self, Failure};
use brain_protocol::{
    Driver, Message, MessageRequest, SessionConfig, SessionId, SessionStatus, SessionSummary,
};
use rand::RngCore;
use tokio::sync::{mpsc, oneshot};

use crate::{
    Error,
    journal::{AppendRecord, JournalEntry, SessionRow, SessionStore, SessionUpdate},
};
use actor::{SessionActor, SessionCommand, failure_of, failure_payload};

pub use actor::LAST_ACTIVATION_KEY;
pub use config::SessionRuntime;
pub use services::TurnServices;

/// One running session: a task that drives its turns, and this handle to it.
///
/// A session owns nothing across sessions. It is given its store, its executors, and its
/// limits, it numbers its own records, and it journals everything it does. Which sessions
/// exist, which are running, and what to do with them after a restart is the host's.
#[derive(Clone)]
pub struct Session {
    session_id: SessionId,
    sender: mpsc::Sender<SessionCommand>,
    cancelled: Arc<AtomicBool>,
}

/// A session between its first record and its admission. The host sets up the
/// Environments the request named, journalling each step here, and then admits the
/// session with [`CreatingSession::complete`].
pub struct CreatingSession {
    store: Arc<dyn SessionStore>,
    config: Arc<SessionRuntime>,
    row: SessionRow,
}

/// The configuration a session was admitted with, read from its row.
pub fn session_config(store: &dyn SessionStore) -> Result<SessionConfig, Error> {
    let row = store.session_row()?;
    serde_json::from_value(row.configuration).map_err(|error| Error::Journal(error.to_string()))
}

impl Session {
    /// Writes a session's first record and hands back the creation to finish.
    ///
    /// Admitting the configuration bounds its authority before anything is journalled:
    /// the contract is checked here and again at `complete`, so a session can only ever
    /// do what it was granted.
    pub fn begin(
        store: Arc<dyn SessionStore>,
        config: Arc<SessionRuntime>,
        request: &SessionConfig,
        transcript: &[Message],
    ) -> Result<CreatingSession, Error> {
        validate_session_contract(request)?;
        // The creation record is the session's own genesis and comes first; a transcript
        // the caller carries forward is what happened before it, and follows.
        store.append_sync(
            &[AppendRecord::new(
                codes::event::SESSION_CREATION_STARTED,
                serde_json::to_value(request).map_err(json_error)?,
            )],
            SessionUpdate {
                status: Some(SessionStatus::Creating),
                configuration: None,
            },
        )?;
        let mut row = store.session_row()?;
        if !transcript.is_empty() {
            row.through_sequence = store.append_journal_sync(&[JournalEntry::TranscriptDelta {
                keep: 0,
                append: transcript.to_vec(),
            }])?;
        }
        Ok(CreatingSession { store, config, row })
    }

    /// Starts a session that is already in the store: its transcript and kv fold out
    /// of its journal, and nothing is replayed into the loop.
    pub fn open(store: Arc<dyn SessionStore>, config: Arc<SessionRuntime>) -> Result<Self, Error> {
        let row = store.session_row()?;
        Self::spawn(store, config, row)
    }

    fn spawn(
        store: Arc<dyn SessionStore>,
        config: Arc<SessionRuntime>,
        row: SessionRow,
    ) -> Result<Self, Error> {
        let session_id = row.session_id.clone();
        let (sender, receiver) = mpsc::channel(8);
        let cancelled = Arc::new(AtomicBool::new(false));
        let actor = SessionActor::new(row, store, config, receiver, cancelled.clone())?;
        tokio::spawn(actor.run());
        Ok(Self {
            session_id,
            sender,
            cancelled,
        })
    }

    pub fn id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn validate_message(request: &MessageRequest) -> Result<(), Error> {
        if request.input.message.is_empty() {
            return Err(Error::InvalidState("message cannot be empty".into()));
        }
        Ok(())
    }

    /// Runs one turn and returns when it is finished.
    pub async fn message(&self, request: MessageRequest) -> Result<SessionSummary, Error> {
        Self::validate_message(&request)?;
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::Message { request, reply })
            .await
            .map_err(|_| stopped())?;
        response.await.map_err(|_| stopped())?
    }

    pub async fn cancel(&self) -> Result<(), Error> {
        self.cancelled.store(true, Ordering::Release);
        match self.sender.try_send(SessionCommand::Cancel) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(stopped()),
        }
    }

    pub async fn end(&self) -> Result<SessionSummary, Error> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::End { reply })
            .await
            .map_err(|_| stopped())?;
        response.await.map_err(|_| stopped())?
    }

    /// Journals an effect the host is about to perform on the session's behalf outside a
    /// turn, such as calling one of its environments. Returns the record's sequence,
    /// which names the operation; the host records what came of it with
    /// [`Session::record_call_ended`] or [`Session::record_call_failed`]. Refused while a
    /// turn is running.
    pub async fn record_call_started<T: serde::Serialize>(
        &self,
        kind: &str,
        request: &T,
    ) -> Result<u64, Error> {
        self.append(AppendRecord::new(
            format!("{kind}_started"),
            serde_json::json!({"request": request}),
        ))
        .await
    }

    pub async fn record_call_ended<T: serde::Serialize>(
        &self,
        kind: &str,
        sequence: u64,
        result: &T,
    ) -> Result<(), Error> {
        self.append(AppendRecord::new(
            format!("{kind}_ended"),
            serde_json::json!({"sequence": sequence, "result": result}),
        ))
        .await
        .map(|_| ())
    }

    pub async fn record_call_failed(
        &self,
        kind: &str,
        sequence: u64,
        error: &Error,
    ) -> Result<(), Error> {
        self.append(failed_record(kind, sequence, error)?)
            .await
            .map(|_| ())
    }

    /// Writes a record on the host's behalf between turns: something the host did to the
    /// session, such as suspending or resuming it. Refused while a turn is running.
    pub async fn record(&self, kind: &str, payload: serde_json::Value) -> Result<u64, Error> {
        self.append(AppendRecord::new(kind, payload)).await
    }

    async fn append(&self, record: AppendRecord) -> Result<u64, Error> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(SessionCommand::Append { record, reply })
            .await
            .map_err(|_| stopped())?;
        response.await.map_err(|_| stopped())?
    }
}

impl CreatingSession {
    pub fn session_id(&self) -> &SessionId {
        &self.row.session_id
    }

    /// Journals an effect performed while the session is being created, before its
    /// actor exists. Returns the record's sequence, which names the operation.
    pub fn record_call_started<T: serde::Serialize>(
        &mut self,
        kind: &str,
        request: &T,
    ) -> Result<u64, Error> {
        self.append(AppendRecord::new(
            format!("{kind}_started"),
            serde_json::json!({"request": request}),
        ))
    }

    pub fn record_call_ended<T: serde::Serialize>(
        &mut self,
        kind: &str,
        sequence: u64,
        result: &T,
    ) -> Result<(), Error> {
        self.append(AppendRecord::new(
            format!("{kind}_ended"),
            serde_json::json!({"sequence": sequence, "result": result}),
        ))
        .map(|_| ())
    }

    pub fn record_call_failed(
        &mut self,
        kind: &str,
        sequence: u64,
        error: &Error,
    ) -> Result<(), Error> {
        self.append(failed_record(kind, sequence, error)?)
            .map(|_| ())
    }

    pub fn record(&mut self, kind: &str, payload: serde_json::Value) -> Result<u64, Error> {
        self.append(AppendRecord::new(kind, payload))
    }

    fn append(&mut self, record: AppendRecord) -> Result<u64, Error> {
        let saved = self
            .store
            .append_sync(&[record], SessionUpdate::default())?;
        self.row.through_sequence = saved
            .last()
            .map_or(self.row.through_sequence, |record| record.sequence);
        Ok(self.row.through_sequence)
    }

    /// Admits the session with what was granted and starts it.
    pub fn complete(mut self, config: SessionConfig) -> Result<Session, Error> {
        validate_session_contract(&config)?;
        let configuration = serde_json::to_value(&config).map_err(json_error)?;
        let saved = self.store.append_sync(
            &[AppendRecord::new(
                codes::event::SESSION_CREATION_ENDED,
                serde_json::json!({"configuration":config}),
            )],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                configuration: Some(&configuration),
            },
        )?;
        self.row.through_sequence = saved
            .last()
            .map_or(self.row.through_sequence, |record| record.sequence);
        self.row.status = SessionStatus::Idle;
        self.row.configuration = configuration;
        Session::spawn(self.store, self.config, self.row)
    }

    pub fn fail(mut self, code: &str, message: &str) -> Result<(), Error> {
        let saved = self.store.append_sync(
            &[AppendRecord::new(
                codes::event::SESSION_CREATION_FAILED,
                failure_payload(None, &Failure::new(code, message))?,
            )],
            SessionUpdate {
                status: Some(SessionStatus::Failed),
                configuration: None,
            },
        )?;
        self.row.through_sequence = saved
            .last()
            .map_or(self.row.through_sequence, |record| record.sequence);
        Ok(())
    }
}

/// The record of an effect that did not come back with a result. `ambiguous` says
/// whether it may have happened anyway.
fn failed_record(kind: &str, sequence: u64, error: &Error) -> Result<AppendRecord, Error> {
    Ok(AppendRecord::new(
        format!("{kind}_failed"),
        failure_payload(Some(sequence), &failure_of(error))?,
    ))
}

fn stopped() -> Error {
    Error::InvalidState("session actor stopped".into())
}

fn validate_session_contract(config: &SessionConfig) -> Result<(), Error> {
    if !sha256_valid(config.agentloop.id.as_str())
        || !identifier_valid(&config.model.provider)
        || config.model.name.is_empty()
        || config.model.name.chars().any(char::is_whitespace)
    {
        return Err(Error::InvalidState(
            "session request violates a contract identity rule".into(),
        ));
    }
    for tool in &config.tools {
        if !identifier_valid(&tool.name)
            || !tool.input_schema.is_object()
            || tool
                .output_schema
                .as_ref()
                .is_some_and(|value| !value.is_object())
        {
            return Err(Error::InvalidState(
                "Tool definition violates the session contract".into(),
            ));
        }
    }
    if config
        .environments
        .iter()
        .any(|environment| !identifier_valid(environment.name.as_str()))
    {
        return Err(Error::InvalidState(
            "Environment name is not an identifier".into(),
        ));
    }
    // Needs are handed to the Environment unread; Brain checks only that they are a
    // bounded list of distinct URIs, so a malformed one is refused here rather than
    // silently ignored there.
    let declared = config
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), &tool.needs))
        .chain(std::iter::once(("agentloop", &config.agentloop.needs)));
    for (subject, needs) in declared {
        if !needs_valid(needs) {
            return Err(Error::InvalidState(format!(
                "`{subject}` names an invalid or repeated need"
            )));
        }
    }
    // The server seals an Environment credential beside the model key; a configuration
    // still carrying one would write it into the journal.
    if config.environments.iter().any(|environment| {
        matches!(
            &environment.driver,
            Driver::Http {
                credential: Some(_),
                ..
            }
        )
    }) {
        return Err(Error::InvalidState(
            "an Environment credential must be sealed before the session is admitted".into(),
        ));
    }
    let mut names: Vec<&str> = config.tools.iter().map(|tool| tool.name.as_str()).collect();
    names.sort_unstable();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::InvalidState("Tool names must be unique".into()));
    }
    let environments: std::collections::HashSet<_> = config
        .environments
        .iter()
        .map(|environment| &environment.name)
        .collect();
    if environments.len() != config.environments.len() {
        return Err(Error::InvalidState(
            "Environment names must be unique".into(),
        ));
    }
    // The one placement rule: everything that runs names an Environment of this session.
    let placed = config
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), &tool.environment))
        .chain(std::iter::once((
            "agentloop",
            &config.agentloop.environment,
        )));
    for (subject, environment) in placed {
        if !environments.contains(environment) {
            return Err(Error::InvalidState(format!(
                "`{subject}` must name an Environment of this session; `{environment}` is not one"
            )));
        }
    }
    Ok(())
}

fn identifier_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// At most 64 distinct URIs, each with a scheme and no whitespace or control characters.
fn needs_valid(needs: &[String]) -> bool {
    needs.len() <= 64
        && needs.iter().all(|need| uri_shaped(need))
        && needs
            .iter()
            .enumerate()
            .all(|(index, need)| !needs[..index].contains(need))
}

fn uri_shaped(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once(':') else {
        return false;
    };
    let scheme_valid = scheme
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'));
    scheme_valid
        && !rest.is_empty()
        && value.len() <= 2_048
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

pub fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    format!("{prefix}_{}", hex::encode(bytes))
}

fn json_error(error: serde_json::Error) -> Error {
    Error::InvalidState(error.to_string())
}

#[cfg(test)]
mod tests {
    use brain_protocol::{
        AgentloopId, AgentloopRef, Environment, EnvironmentName, ModelBinding, Tool,
    };

    use super::*;

    fn digest() -> String {
        "a".repeat(64)
    }

    fn tool() -> Tool {
        Tool {
            name: "search".into(),
            description: "search the workspace".into(),
            input_schema: serde_json::json!({"type":"object"}),
            output_schema: None,
            environment: EnvironmentName::new("workspace"),
            needs: Vec::new(),
            implementation: Some(serde_json::json!({"kind": "test"})),
        }
    }

    fn environment(name: &str, driver: Driver) -> Environment {
        Environment {
            name: EnvironmentName::new(name),
            driver,
            configuration: serde_json::json!({}),
        }
    }

    /// The smallest configuration with one Tool: an Agentloop and the Tool, both placed
    /// in one Environment.
    fn config() -> SessionConfig {
        SessionConfig {
            agentloop: AgentloopRef {
                id: AgentloopId::new(digest()),
                configuration: serde_json::json!({}),
                environment: EnvironmentName::new("workspace"),
                needs: Vec::new(),
            },
            model: ModelBinding {
                provider: "vercel-ai-gateway".into(),
                name: "openai/test".into(),
            },
            system: "test".into(),
            response_format: None,
            tools: vec![tool()],
            environments: vec![environment("workspace", Driver::Brain {})],
            idle_ttl_ms: None,
        }
    }

    /// A rejection case: the name of the breach, the smallest edit that commits it, and
    /// the bound that must reject it.
    type Breach = (&'static str, fn(&mut SessionConfig), &'static str);

    /// Each case names the bound it breaches, so a case that starts passing for some
    /// other reason fails rather than quietly stops testing what it was written for.
    fn assert_rejected(case: &str, config: &SessionConfig, bound: &str) {
        let error = validate_session_contract(config)
            .expect_err(&format!("a configuration with {case} must be rejected"));
        let message = error.to_string();
        assert!(
            message.contains(bound),
            "a configuration with {case} must be rejected by the {bound:?} bound, not by {message:?}"
        );
    }

    #[test]
    fn a_configuration_within_every_bound_is_admitted() {
        validate_session_contract(&config()).unwrap();
        let mut placed_elsewhere = config();
        placed_elsewhere.environments.push(environment(
            "sandbox",
            Driver::Http {
                url: "https://sandbox.example".into(),
                credential: None,
            },
        ));
        placed_elsewhere.tools[0].environment = EnvironmentName::new("sandbox");
        placed_elsewhere.tools[0].needs = vec![
            "file:///workspace?access=write".into(),
            "pkg:pypi/numpy".into(),
        ];
        validate_session_contract(&placed_elsewhere).unwrap();
    }

    /// Authority is fixed at create, so every bound below is the difference between a
    /// session that can only do what it was granted and one that cannot be reasoned
    /// about at all. Each case is the smallest edit that breaches one bound.
    #[test]
    fn every_bound_rejects_a_configuration_that_breaches_it() {
        let cases: Vec<Breach> = vec![
            (
                "an Agentloop id of the wrong length",
                |request| request.agentloop.id = AgentloopId::new("a".repeat(63)),
                "identity rule",
            ),
            (
                "an Agentloop id that is not hex",
                |request| request.agentloop.id = AgentloopId::new("g".repeat(64)),
                "identity rule",
            ),
            (
                "an empty model provider",
                |request| request.model.provider = String::new(),
                "identity rule",
            ),
            (
                "a model provider holding a path traversal",
                |request| request.model.provider = "gateway/../root".into(),
                "identity rule",
            ),
            (
                "an empty model name",
                |request| request.model.name = String::new(),
                "identity rule",
            ),
            (
                "a model name holding whitespace",
                |request| request.model.name = "gpt 5".into(),
                "identity rule",
            ),
            (
                "a Tool name that is not an identifier",
                |request| request.tools[0].name = "../escape".into(),
                "Tool definition violates",
            ),
            (
                "a Tool input schema that is not an object",
                |request| request.tools[0].input_schema = serde_json::json!("string"),
                "Tool definition violates",
            ),
            (
                "a Tool output schema that is not an object",
                |request| request.tools[0].output_schema = Some(serde_json::json!([])),
                "Tool definition violates",
            ),
            (
                "an Environment name that is not an identifier",
                |request| {
                    request.environments[0].name = EnvironmentName::new("../escape");
                    request.tools[0].environment = EnvironmentName::new("../escape");
                    request.agentloop.environment = EnvironmentName::new("../escape");
                },
                "Environment name",
            ),
            (
                "a Tool need that is not a URI",
                |request| request.tools[0].needs = vec!["../fs".into()],
                "invalid or repeated need",
            ),
            (
                "a Tool need repeated",
                |request| {
                    request.tools[0].needs = vec!["pkg:apt/ffmpeg".into(), "pkg:apt/ffmpeg".into()];
                },
                "invalid or repeated need",
            ),
            (
                "an Agentloop need with whitespace",
                |request| request.agentloop.needs = vec!["https://api.example.com /".into()],
                "invalid or repeated need",
            ),
            (
                "an Environment credential left unsealed",
                |request| {
                    request.environments.push(environment(
                        "sandbox",
                        Driver::Http {
                            url: "https://sandbox.example".into(),
                            credential: Some("s3cret".into()),
                        },
                    ));
                },
                "sealed",
            ),
            (
                "two Tools sharing one name",
                |request| request.tools.push(tool()),
                "Tool names must be unique",
            ),
            (
                "two Environments sharing one name",
                |request| {
                    let environment = request.environments[0].clone();
                    request.environments.push(environment);
                },
                "Environment names must be unique",
            ),
            (
                "a Tool naming an Environment the session does not have",
                |request| request.tools[0].environment = EnvironmentName::new("elsewhere"),
                "must name an Environment of this session",
            ),
            (
                "an Agentloop naming an Environment the session does not have",
                |request| request.agentloop.environment = EnvironmentName::new("elsewhere"),
                "must name an Environment of this session",
            ),
        ];
        for (case, breach, bound) in cases {
            let mut request = config();
            breach(&mut request);
            assert_rejected(case, &request, bound);
        }
    }

    #[test]
    fn an_identifier_admits_only_the_characters_the_contract_names() {
        assert!(identifier_valid("a"));
        assert!(identifier_valid("workspace.tool_1:read-only"));
        assert!(identifier_valid(&"a".repeat(128)));
        assert!(!identifier_valid(""));
        assert!(!identifier_valid(&"a".repeat(129)));
        assert!(!identifier_valid(".leading"));
        assert!(!identifier_valid("-leading"));
        assert!(!identifier_valid("has space"));
        assert!(!identifier_valid("has/slash"));
        assert!(!identifier_valid("../escape"));
        assert!(!identifier_valid("na\u{ef}ve"));
    }

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

    #[test]
    fn a_need_is_any_uri_and_nothing_else() {
        for valid in [
            "pkg:apt/ffmpeg",
            "pkg:pypi/numpy@2.1.0",
            "https://api.example.com",
            "https://*.example.com",
            "wss://stream.example.com",
            "file:///workspace?access=write",
            "aws:iam",
        ] {
            assert!(uri_shaped(valid), "{valid}");
        }
        for invalid in [
            "",
            "fs",
            "../fs",
            "https://a b",
            "1pkg:x",
            "pkg:",
            ":x",
            "a\tb:c",
        ] {
            assert!(!uri_shaped(invalid), "{invalid}");
        }
    }
}
