use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use brain::api::{AppState, router};
use brain::session::{Brain, BrainConfig};
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
    let brain = Brain::in_memory_test(temp.0.clone(), BrainConfig::default()).unwrap();
    let app = router(AppState {
        brain,
        token: "operator-token".into(),
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
async fn saturated_create_admission_rejects_before_polling_another_body() {
    let temp = TempDir::new("saturated");
    let brain = Brain::in_memory_test(
        temp.0.clone(),
        BrainConfig {
            max_concurrent_creates: 1,
            ..BrainConfig::default()
        },
    )
    .unwrap();
    let app = router(AppState {
        brain,
        token: "operator-token".into(),
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
