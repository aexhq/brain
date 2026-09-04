//! The client side of the stdio bridge (`src/bin/bridge.rs`).
//!
//! Lines go in through `POST /stdin`; lines come out through the `/stdout` event feed.
//! Which lines, and what they mean, is the subject's own protocol and lives in its driver.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::broadcast;

use super::feed::{self, Feed};

pub struct Bridge {
    http: reqwest::Client,
    base_url: String,
    feed: Feed,
}

impl Bridge {
    /// Opens the stdout feed; returns once it is live.
    pub async fn connect(http: reqwest::Client, base_url: &str) -> Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_owned();
        let feed = Feed::open(&format!("{base_url}/stdout"))
            .await
            .with_context(|| format!("subscribing to the subject's stdout at {base_url}"))?;
        Ok(Self {
            http,
            base_url,
            feed,
        })
    }

    /// Writes one line to the subject's stdin.
    pub async fn send(&self, line: &Value) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/stdin", self.base_url))
            .body(line.to_string())
            .send()
            .await?;
        feed::ok(response, "writing to the subject's stdin").await?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Value>> {
        self.feed.subscribe()
    }
}
