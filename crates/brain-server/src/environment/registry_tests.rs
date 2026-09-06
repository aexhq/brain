use super::*;
use brain::SessionStore;
use brain_protocol::{EnvironmentReceipt, EnvironmentRequest};
use std::sync::Arc;
#[cfg(test)]
mod tests {
    use super::*;
    use brain_protocol::{Driver, Environment, EnvironmentName, codes};
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
        let registry = EnvironmentRegistry::new(Arc::new(HttpEnvironmentAdapter::new(
            reqwest::Client::new(),
            metadata,
            &Default::default(),
            Arc::new(crate::Executions::default()),
            "http://brain.example".into(),
        )));
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
