//! A server-sent event stream that several waiters can read at once.
//!
//! Three subjects answer over a stream that is not tied to one request: the stdio bridge
//! carries every line a child writes, and OpenCode publishes every bus event on one
//! `/event` endpoint. A driver that opened a fresh stream per turn would put its own
//! subscribe latency inside the number, and one that read the stream inline could not
//! also be awaiting the HTTP call that provoked the events. So the stream is opened once,
//! parsed off in the background, and fanned out: each timed operation subscribes before it
//! sends, and reads until it has seen what it was waiting for.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::sync::broadcast;

pub struct Feed {
    sender: broadcast::Sender<Arc<Value>>,
    task: tokio::task::JoinHandle<()>,
}

impl Feed {
    /// Opens `url` and returns once the server has sent its first bytes, so the
    /// subscription is live before the caller sends anything whose answer it wants.
    pub async fn open(url: &str) -> Result<Self> {
        // Its own client, without the request timeout the drivers put on their turns: a
        // total-time budget on a stream that is meant to stay open would close it mid-run.
        let http = reqwest::Client::builder().no_proxy().build()?;
        let response = http
            .get(url)
            .header("accept", "text/event-stream")
            .send()
            .await
            .with_context(|| format!("opening the event stream at {url}"))?;
        let response = ok(response, "opening an event stream").await?;
        let mut chunks = response.bytes_stream();
        let first = chunks
            .next()
            .await
            .context("the event stream closed before it opened")??;
        let mut pending = String::from_utf8_lossy(&first).into_owned();

        let debug = std::env::var_os("BENCH_DEBUG_EVENTS").is_some();
        let (sender, _) = broadcast::channel(1 << 16);
        let publisher = sender.clone();
        let task = tokio::spawn(async move {
            loop {
                for frame in sse_frames(&mut pending) {
                    if debug {
                        eprintln!("event: {frame}");
                    }
                    let _ = publisher.send(Arc::new(frame));
                }
                match chunks.next().await {
                    Some(Ok(chunk)) => pending.push_str(&String::from_utf8_lossy(&chunk)),
                    Some(Err(error)) => {
                        let _ = publisher
                            .send(Arc::new(json!({ "$feed_error": error.to_string() })));
                        break;
                    }
                    None => {
                        let _ = publisher.send(Arc::new(json!({ "$feed_closed": true })));
                        break;
                    }
                }
            }
        });
        Ok(Self { sender, task })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Value>> {
        self.sender.subscribe()
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The next event, or why there will not be one.
///
/// A lagged waiter is an error rather than a skip: it means an answer may already have
/// gone by, and waiting on for it would hang the probe with no reason attached.
pub async fn next(
    receiver: &mut broadcast::Receiver<Arc<Value>>,
    deadline: tokio::time::Instant,
) -> Result<Arc<Value>> {
    let received = tokio::time::timeout_at(deadline, receiver.recv())
        .await
        .context("timed out waiting on the event feed")?;
    match received {
        Ok(value) => {
            if let Some(missed) = value.get("$lagged") {
                anyhow::bail!(
                    "the event feed lagged by {missed} frames; the client fell behind the subject"
                )
            }
            if let Some(error) = value.get("$feed_error") {
                anyhow::bail!("the event feed failed: {error}")
            }
            if value.get("$feed_closed").is_some() {
                anyhow::bail!("the event feed closed")
            }
            Ok(value)
        }
        Err(broadcast::error::RecvError::Lagged(missed)) => {
            anyhow::bail!("this waiter fell {missed} frames behind the event feed")
        }
        Err(broadcast::error::RecvError::Closed) => anyhow::bail!("the event feed closed"),
    }
}

/// Reads until an event satisfies `wanted`, and returns it.
pub async fn wait_for(
    receiver: &mut broadcast::Receiver<Arc<Value>>,
    timeout: Duration,
    mut wanted: impl FnMut(&Value) -> bool,
) -> Result<Arc<Value>> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let event = next(receiver, deadline).await?;
        if wanted(&event) {
            return Ok(event);
        }
    }
}

/// Pulls whole frames out of an SSE buffer, leaving any partial frame behind.
///
/// `data:` payloads are parsed as JSON; a non-JSON payload is passed on under `$text` so
/// a driver can still see it. A `lagged` event from the bridge becomes `$lagged`.
fn sse_frames(pending: &mut String) -> Vec<Value> {
    let mut frames = Vec::new();
    while let Some(end) = pending.find("\n\n") {
        let block = pending[..end].to_owned();
        pending.drain(..end + 2);
        let mut event = None;
        let mut data = String::new();
        for line in block.lines() {
            if let Some(name) = line.strip_prefix("event:") {
                event = Some(name.trim().to_owned());
            } else if let Some(payload) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(payload.strip_prefix(' ').unwrap_or(payload));
            }
        }
        if event.as_deref() == Some("lagged") {
            frames.push(json!({ "$lagged": data }));
            continue;
        }
        if data.is_empty() {
            continue;
        }
        frames.push(serde_json::from_str(&data).unwrap_or_else(|_| json!({ "$text": data })));
    }
    frames
}

/// `error_for_status` throws the body away, and the body is where the subject says what
/// went wrong.
pub async fn ok(response: reqwest::Response, doing: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("<body unreadable: {error}>"));
    anyhow::bail!("{doing}: {status}: {body}")
}

/// The HTTP client the feed-based drivers share the shape of: no proxy, a pool matched to
/// Brain's, and a per-request timeout that bounds a hung turn without bounding the feed.
pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(512)
        .timeout(Duration::from_secs(60))
        .build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_parsed_and_partial_ones_kept() {
        let mut pending =
            String::from("data: {\"a\":1}\n\nevent: lagged\ndata: 3\n\ndata: {\"b\"");
        let frames = sse_frames(&mut pending);
        assert_eq!(frames, vec![json!({"a": 1}), json!({"$lagged": "3"})]);
        assert_eq!(pending, "data: {\"b\"");
        pending.push_str(":2}\n\n");
        assert_eq!(sse_frames(&mut pending), vec![json!({"b": 2})]);
    }

    #[test]
    fn comments_and_empty_frames_are_skipped() {
        let mut pending = String::from(": connected\n\n\n\ndata: plain\n\n");
        assert_eq!(sse_frames(&mut pending), vec![json!({"$text": "plain"})]);
    }
}
