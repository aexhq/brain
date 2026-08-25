use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use brain::session::{Brain, BrainConfig};
use brain_server::api::{AppState, router};
use futures_util::stream::poll_fn;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use tokio::sync::Notify;
use tower::ServiceExt as _;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "brain-api-admission-{label}-{}-{}",
            std::process::id(),
            brain::wall_ms()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request(body: Body, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(body).expect("request")
}

#[tokio::test]
async fn unauthenticated_create_never_polls_the_body() {
    let temp = TempDir::new("unauthenticated");
    let brain = Brain::in_memory_test(
        temp.0.clone(),
        BrainConfig::default(),
        brain::provider::fake::unscripted_factory(),
    )
    .unwrap();
    let app = router(AppState {
        brain,
        token: "operator-token".into(),
        tenancy: brain_server::api::Tenancy::Implicit("local".into()),
    });
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let body = Body::from_stream(poll_fn(move |_| {
        observed.fetch_add(1, Ordering::Relaxed);
        Poll::Ready(Some(Ok::<Bytes, Infallible>(Bytes::from_static(b"{}"))))
    }));

    let response = app.oneshot(request(body, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(polls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_tenanted_composition_refuses_requests_without_the_tenant_header() {
    let temp = TempDir::new("require-tenant");
    let brain = Brain::in_memory_test(
        temp.0.clone(),
        BrainConfig::default(),
        brain::provider::fake::unscripted_factory(),
    )
    .unwrap();
    let app = router(AppState {
        brain,
        token: "operator-token".into(),
        tenancy: brain_server::api::Tenancy::Required,
    });
    // Authenticated but header-less: booked to no tenant, refused — never tenant "local".
    let response = app
        .oneshot(request(Body::from("{}"), Some("operator-token")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn scoped_customer_ingress_does_not_require_a_tenant_header() {
    let temp = TempDir::new("scoped-customer-ingress");
    let brain = Brain::in_memory_test(
        temp.0.clone(),
        BrainConfig::default(),
        brain::provider::fake::unscripted_factory(),
    )
    .unwrap();
    let app = router(AppState {
        brain,
        token: "operator-token".into(),
        tenancy: brain_server::api::Tenancy::Required,
    });

    let gateway = Request::builder()
        .method("POST")
        .uri("/internal/v1/customer-environment/gateway")
        .header(header::AUTHORIZATION, "Bearer operator-token")
        .header("x-brain-connection-id", "connection-1")
        .header("x-brain-request-id", "request-1")
        .header("x-brain-route-key", "$connect")
        .header("x-brain-source-ip", "192.0.2.10")
        .header(
            header::SEC_WEBSOCKET_PROTOCOL,
            "environment-grant.valid-token",
        )
        .body(Body::empty())
        .unwrap();
    let gateway_response = app.clone().oneshot(gateway).await.unwrap();
    assert_eq!(gateway_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let observation = Request::builder()
        .method("POST")
        .uri("/internal/v1/customer-environment/observations/grant-1")
        .header(header::AUTHORIZATION, "Bearer operator-token")
        .header("x-brain-observation-grant", "observation-token")
        .body(Body::from("{}"))
        .unwrap();
    let observation_response = app.oneshot(observation).await.unwrap();
    assert_eq!(
        observation_response.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn saturated_create_admission_rejects_before_polling_another_body() {
    let temp = TempDir::new("saturated");
    let brain = Brain::in_memory_test(
        temp.0.clone(),
        BrainConfig {
            max_concurrent_creates: 1,
            ..BrainConfig::default()
        },
        brain::provider::fake::unscripted_factory(),
    )
    .unwrap();
    let app = router(AppState {
        brain,
        token: "operator-token".into(),
        tenancy: brain_server::api::Tenancy::Implicit("local".into()),
    });

    let first_polled = Arc::new(Notify::new());
    let signal = first_polled.clone();
    let first_body = Body::from_stream(poll_fn(move |_| {
        signal.notify_one();
        Poll::Pending::<Option<Result<Bytes, Infallible>>>
    }));
    let first_app = app.clone();
    let first = tokio::spawn(async move {
        first_app
            .oneshot(request(first_body, Some("operator-token")))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), first_polled.notified())
        .await
        .expect("first request reached body extraction while holding create admission");

    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let second_body = Body::from_stream(poll_fn(move |_| {
        observed.fetch_add(1, Ordering::Relaxed);
        Poll::Ready(Some(Ok::<Bytes, Infallible>(Bytes::from_static(b"{}"))))
    }));
    let response = app
        .oneshot(request(second_body, Some("operator-token")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(polls.load(Ordering::Relaxed), 0);
    first.abort();
}
