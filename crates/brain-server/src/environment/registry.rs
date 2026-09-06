//! Every operation on a session's Environments: resolved to the adapter for its
//! driver, journaled before it is sent, and its outcome recorded after.

use std::sync::Arc;

use brain::{CreatingSession, Session, SessionStore};
use brain_protocol::{
    Driver, Environment, EnvironmentCallResult, EnvironmentName, EnvironmentOperation,
    EnvironmentReceipt, EnvironmentRequest, SessionConfig, codes,
};

use super::{
    BrainEnvironment, EnvironmentAdapter, HostEnvironment, HttpEnvironmentAdapter, Services,
};

pub struct EnvironmentRegistry {
    brain: Arc<BrainEnvironment>,
    hosts: HostEnvironment,
    http: Arc<HttpEnvironmentAdapter>,
}

impl EnvironmentRegistry {
    pub fn new(
        brain: Arc<BrainEnvironment>,
        hosts: HostEnvironment,
        http: Arc<HttpEnvironmentAdapter>,
    ) -> Self {
        Self { brain, hosts, http }
    }

    /// The registered hosts: the host env's registration, command stream, and results.
    pub fn hosts(&self) -> &HostEnvironment {
        &self.hosts
    }

    fn adapter(&self, driver: &Driver) -> &dyn EnvironmentAdapter {
        match driver {
            Driver::Brain {} => &*self.brain,
            Driver::Host { .. } => &self.hosts,
            Driver::Http { .. } => &*self.http,
        }
    }

    /// Sets up one Environment as part of session admission: its own configuration and
    /// the needs of everything placed in it, recorded before they are sent.
    pub async fn setup(
        &self,
        creation: &mut CreatingSession,
        environment: &Environment,
        needs: Vec<String>,
    ) -> Result<(), brain::Error> {
        let kind = codes::event::call::ENVIRONMENT_SETUP;
        let request = EnvironmentRequest::Setup {
            configuration: environment.configuration.clone(),
            needs,
        };
        let sequence = creation.record(
            &format!("{kind}_started"),
            serde_json::json!({"environment": environment.name, "request": request}),
        )?;
        let operation = EnvironmentOperation {
            sequence,
            environment: environment.name.clone(),
            session_id: creation.session_id().clone(),
            request,
        };
        let sent = self
            .adapter(&environment.driver)
            .execute(environment, &operation, Services::None)
            .await;
        match sent.and_then(|receipt| terminal(receipt, "setup")) {
            Ok(receipt) => creation.record_call_ended(kind, sequence, &receipt),
            Err(error) => {
                creation.record_call_failed(kind, sequence, &error)?;
                if matches!(error, brain::Error::Ambiguous(_)) {
                    creation.record(
                        codes::event::ENVIRONMENT_UNREACHABLE,
                        serde_json::json!({"environment": environment.name, "sequence": sequence}),
                    )?;
                }
                Err(error)
            }
        }
    }

    /// One operation on a session's behalf whose record the session already holds: a
    /// Tool invoke or cancel, or a turn.
    pub async fn execute(
        &self,
        environment: &Environment,
        operation: &EnvironmentOperation,
        services: Services<'_>,
    ) -> Result<EnvironmentReceipt, brain::Error> {
        self.adapter(&environment.driver)
            .execute(environment, operation, services)
            .await
    }

    pub async fn call(
        &self,
        session: &Session,
        store: &dyn SessionStore,
        name: &EnvironmentName,
        method: String,
        input: serde_json::Value,
    ) -> Result<EnvironmentCallResult, brain::Error> {
        let config = brain::session_config(store)?;
        let environment = config.environment(name).ok_or_else(|| {
            brain::Error::NotFound(format!("Environment `{name}` is not one of this session's"))
        })?;
        let kind = codes::event::call::ENVIRONMENT_CALL;
        let request = EnvironmentRequest::Call {
            name: method,
            input,
        };
        let sequence = session
            .record(
                &format!("{kind}_started"),
                serde_json::json!({"environment": name, "request": request}),
            )
            .await?;
        let operation = EnvironmentOperation {
            sequence,
            environment: name.clone(),
            session_id: session.id().clone(),
            request,
        };
        let sent = self
            .adapter(&environment.driver)
            .execute(environment, &operation, Services::None)
            .await;
        match sent.and_then(|receipt| terminal(receipt, "call")) {
            Ok(EnvironmentReceipt::Result { output }) => {
                session.record_call_ended(kind, sequence, &output).await?;
                Ok(EnvironmentCallResult { output })
            }
            Ok(_) => {
                let error = brain::Error::Executor(
                    "Environment returned a nonterminal call receipt".into(),
                );
                session.record_call_failed(kind, sequence, &error).await?;
                Err(error)
            }
            Err(error) => {
                session.record_call_failed(kind, sequence, &error).await?;
                if matches!(error, brain::Error::Ambiguous(_)) {
                    session
                        .record(
                            codes::event::ENVIRONMENT_UNREACHABLE,
                            serde_json::json!({"environment": name, "sequence": sequence}),
                        )
                        .await?;
                }
                Err(error)
            }
        }
    }

    /// Detaches the session from every Environment it was set up in, last first. The
    /// Environments stay until the session is deleted.
    pub async fn release_session(
        &self,
        session: &Session,
        config: &SessionConfig,
        store: &dyn SessionStore,
    ) -> Result<(), brain::Error> {
        let kind = codes::event::call::ENVIRONMENT_DETACH;
        for environment in config.environments.iter().rev() {
            if let Some(attempt) = operation_outcome(store, &environment.name, kind)? {
                if attempt.outcome.is_none() {
                    session
                        .record_call_failed(
                            kind,
                            attempt.sequence,
                            &brain::Error::Ambiguous("detach result was not recorded".into()),
                        )
                        .await?;
                }
                continue;
            }
            let sequence = session
                .record(
                    &format!("{kind}_started"),
                    serde_json::json!({"environment": environment.name, "request": EnvironmentRequest::Detach}),
                )
                .await?;
            let operation = EnvironmentOperation {
                sequence,
                environment: environment.name.clone(),
                session_id: session.id().clone(),
                request: EnvironmentRequest::Detach,
            };
            let sent = self
                .adapter(&environment.driver)
                .execute(environment, &operation, Services::None)
                .await;
            match sent.and_then(|receipt| terminal(receipt, "detach")) {
                Ok(receipt) => session.record_call_ended(kind, sequence, &receipt).await?,
                Err(error) => {
                    session.record_call_failed(kind, sequence, &error).await?;
                    if matches!(error, brain::Error::Ambiguous(_)) {
                        session
                            .record(
                                codes::event::ENVIRONMENT_UNREACHABLE,
                                serde_json::json!({"environment": environment.name, "sequence": sequence}),
                            )
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Tears the Environment down for good. A teardown whose outcome was never recorded
    /// is failed as interrupted; a failed teardown is not retried unless `retry_failed`,
    /// so the session's records keep saying what happened until the caller decides.
    pub async fn close(
        &self,
        environment: &Environment,
        store: &dyn SessionStore,
        retry_failed: bool,
    ) -> Result<(), brain::Error> {
        let kind = codes::event::call::ENVIRONMENT_TEARDOWN;
        let previous = operation_outcome(store, &environment.name, kind)?;
        if previous
            .as_ref()
            .is_some_and(|attempt| attempt.outcome == Some(true))
        {
            return Ok(());
        }
        if let Some(attempt) = &previous
            && attempt.outcome.is_none()
        {
            store.append_sync(&[brain::AppendRecord::new(codes::event::ENVIRONMENT_TEARDOWN_FAILED,
                serde_json::json!({"sequence":attempt.sequence,"code":"interrupted","ambiguous":true,"message":"teardown result was not recorded"}))], brain::SessionUpdate::default())?;
        }
        if previous.is_some() && !retry_failed {
            return Err(brain::Error::Ambiguous(
                "Environment teardown needs an explicit retry".into(),
            ));
        }
        let saved = store.append_sync(
            &[brain::AppendRecord::new(
                codes::event::ENVIRONMENT_TEARDOWN_STARTED,
                serde_json::json!({"environment": environment.name, "request": EnvironmentRequest::Teardown}),
            )],
            brain::SessionUpdate::default(),
        )?;
        let sequence = saved[0].sequence;
        let operation = EnvironmentOperation {
            sequence,
            environment: environment.name.clone(),
            session_id: store.session_id().clone(),
            request: EnvironmentRequest::Teardown,
        };
        let result = self
            .adapter(&environment.driver)
            .execute(environment, &operation, Services::None)
            .await
            .and_then(|receipt| terminal(receipt, "teardown"));
        let (kind, payload) = match &result {
            Ok(receipt) => (
                codes::event::ENVIRONMENT_TEARDOWN_ENDED,
                serde_json::json!({"sequence": sequence, "result": receipt}),
            ),
            Err(error) => (
                codes::event::ENVIRONMENT_TEARDOWN_FAILED,
                serde_json::json!({"sequence": sequence, "code": error.code(), "message": error.to_string(), "ambiguous": matches!(error, brain::Error::Ambiguous(_))}),
            ),
        };
        store.append_sync(
            &[brain::AppendRecord::new(kind, payload)],
            brain::SessionUpdate::default(),
        )?;
        result?;
        store.append_sync(
            &[brain::AppendRecord::new(
                codes::event::ENVIRONMENT_CLOSED,
                serde_json::json!({"environment": environment.name}),
            )],
            brain::SessionUpdate::default(),
        )?;
        Ok(())
    }
}

struct Attempt {
    sequence: u64,
    outcome: Option<bool>,
}

/// The last attempt at `kind` on the named Environment, as the journal tells it.
fn operation_outcome(
    store: &dyn SessionStore,
    environment: &EnvironmentName,
    kind: &str,
) -> Result<Option<Attempt>, brain::Error> {
    let mut after = 0;
    let mut attempt: Option<Attempt> = None;
    loop {
        let records = store.records_after(after, 1000)?;
        if records.is_empty() {
            return Ok(attempt);
        }
        for record in records {
            after = record.sequence;
            if record.kind == format!("{kind}_started")
                && record
                    .payload
                    .get("environment")
                    .and_then(serde_json::Value::as_str)
                    == Some(environment.as_str())
            {
                attempt = Some(Attempt {
                    sequence: record.sequence,
                    outcome: None,
                });
            } else if let Some(attempt) = &mut attempt
                && record
                    .payload
                    .get("sequence")
                    .and_then(serde_json::Value::as_u64)
                    == Some(attempt.sequence)
            {
                if record.kind == format!("{kind}_ended") {
                    attempt.outcome = Some(true);
                }
                if record.kind == format!("{kind}_failed") {
                    attempt.outcome = Some(false);
                }
            }
        }
    }
}

/// A receipt that ended the operation, or the error it ended with. A failure receipt is
/// the environment saying the effect failed; an ambiguous one says it does not know; a
/// progress receipt where a terminal one was owed is a broken environment.
fn terminal(receipt: EnvironmentReceipt, what: &str) -> Result<EnvironmentReceipt, brain::Error> {
    match receipt {
        EnvironmentReceipt::Unknown { message } => Err(brain::Error::Ambiguous(message)),
        EnvironmentReceipt::Failure { message, .. } => Err(brain::Error::Executor(message)),
        EnvironmentReceipt::Progress { .. } => Err(brain::Error::Executor(format!(
            "Environment returned progress without a terminal {what} receipt"
        ))),
        receipt => Ok(receipt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An HTTP Environment that answers once it is asked twice, standing in for one
    /// whose first teardown was lost on the wire.
    async fn flaky_environment() -> (String, Arc<AtomicUsize>) {
        use axum::{Json, Router, routing::post};
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/v1/operations",
            post({
                let calls = calls.clone();
                move |Json(command): Json<brain_protocol::EnvironmentCommand>| {
                    let calls = calls.clone();
                    async move {
                        assert!(matches!(
                            command.operation.request,
                            EnvironmentRequest::Teardown
                        ));
                        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                            return Err(axum::http::StatusCode::BAD_GATEWAY);
                        }
                        Ok(Json(brain_protocol::EnvironmentResponse {
                            contract: brain_protocol::ENVIRONMENT_CONTRACT.into(),
                            sequence: command.operation.sequence,
                            receipt: EnvironmentReceipt::Accepted,
                        }))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), calls)
    }

    #[tokio::test]
    async fn teardown_is_journaled_and_failure_is_retained_without_automatic_retry() {
        let (url, calls) = flaky_environment().await;
        let root = tempfile::tempdir().unwrap();
        let (telemetry, _) = brain_telemetry::telemetry_channel();
        let store = brain::LocalSessionStore::create(
            &root.path().join("session"),
            brain_protocol::SessionId::new("ses_owner"),
            &serde_json::json!({}),
            brain::Writer::spawn(),
            Arc::new(brain::Feed::new(telemetry)),
        )
        .unwrap();
        let metadata =
            Arc::new(crate::metadata::ServerMetadata::open(&root.path().join("metadata")).unwrap());
        let registry = EnvironmentRegistry::new(
            Arc::new(BrainEnvironment::new(
                Arc::new(brain_loophost::WorkerPool::new(
                    "unused",
                    root.path().join("run"),
                    root.path().join("agentloops"),
                    Default::default(),
                )),
                Default::default(),
                root.path().join("native-workspaces"),
            )),
            HostEnvironment::open(&root.path().join("hosts.log")).unwrap(),
            Arc::new(HttpEnvironmentAdapter::new(
                reqwest::Client::new(),
                metadata,
                0,
            )),
        );
        let environment = Environment {
            name: EnvironmentName::new("sandbox"),
            driver: Driver::Http {
                url,
                credential: None,
            },
            configuration: serde_json::json!({}),
        };
        assert!(registry.close(&environment, &*store, false).await.is_err());
        assert!(registry.close(&environment, &*store, false).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            store
                .records_after(0, 100)
                .unwrap()
                .iter()
                .any(
                    |record| record.kind == codes::event::ENVIRONMENT_TEARDOWN_FAILED
                        && record.payload["ambiguous"] == true
                )
        );
        registry.close(&environment, &*store, true).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let kinds: Vec<String> = store
            .records_after(0, 100)
            .unwrap()
            .into_iter()
            .map(|record| record.kind)
            .collect();
        assert_eq!(kinds.last().unwrap(), codes::event::ENVIRONMENT_CLOSED);
        registry.close(&environment, &*store, true).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a closed Environment stays closed"
        );
    }
}
