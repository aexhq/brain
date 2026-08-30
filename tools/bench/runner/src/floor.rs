//! What the load generator costs when the subject costs nothing.
//!
//! The numbers under test are single-digit milliseconds. A client that needs half a
//! millisecond to notice a byte arrived is contributing a visible share of every sample,
//! and on a shared instance under load it contributes more. So the runner measures itself
//! against a server that does nothing, and refuses to publish any subject latency that
//! sits within `MARGIN` of that floor — at that point the number is mostly us.

use std::net::SocketAddr;
use std::time::Instant;

use anyhow::Result;
use axum::{Router, response::IntoResponse, routing::get};
use futures_util::StreamExt;

/// A subject sample must be at least this multiple of the generator floor to be
/// publishable. Five is not conservative: at 5x the floor still contributes a fifth of
/// the number, which is why the floor travels with every datapoint rather than being
/// checked once and forgotten.
pub const MARGIN: f64 = 5.0;

/// The null server: a complete SSE response with one data frame, written immediately.
/// Its shape mirrors what a subject's stream looks like so the client does the same
/// parsing work it will do for real.
async fn null_stream() -> impl IntoResponse {
    (
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
        ],
        "data: {\"delta\":\"x\"}\n\ndata: [DONE]\n\n",
    )
}

/// Measures the client's own time-to-first-byte against the null server.
pub async fn measure(samples: usize) -> Result<f64> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let app = Router::new().route("/null", get(null_stream));
        let _ = axum::serve(listener, app).await;
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(64)
        .build()?;
    let url = format!("http://{address}/null");

    let mut measured = Vec::with_capacity(samples);
    for index in 0..samples {
        let started = Instant::now();
        let response = client.get(&url).send().await?;
        let mut stream = response.bytes_stream();
        // First byte off the socket, which is the same event the subject drivers time.
        if stream.next().await.transpose()?.is_some() {
            let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
            // The first handful pay for connection setup, which a warm subject run does
            // not; discarding them measures the steady-state floor.
            if index >= samples / 10 {
                measured.push(elapsed);
            }
        }
    }
    server.abort();

    anyhow::ensure!(!measured.is_empty(), "floor check produced no samples");
    let summary = crate::stats::summarize(&mut measured);
    // The floor is the median, not the minimum: the minimum is the client's best case and
    // would set the bar too low to catch jitter that is actually present.
    summary.p50_ms.or(summary.max_ms).ok_or_else(|| {
        anyhow::anyhow!(
            "floor check needs at least {} samples",
            crate::stats::MIN_N_P50
        )
    })
}

/// Whether a measured latency stands clear of the generator's own cost.
pub fn clears(value_ms: f64, floor_ms: f64) -> bool {
    value_ms >= floor_ms * MARGIN
}

/// The note attached to a datapoint that does not clear the floor. It stays in the raw
/// results and is excluded from generated tables.
pub fn note(value_ms: f64, floor_ms: f64) -> String {
    format!(
        "{value_ms:.3} ms is within {MARGIN}x the load generator's own floor of \
         {floor_ms:.3} ms; this measures the client as much as the subject"
    )
}
