//! Managed web tools. Search uses the operator-owned Serper credential; fetch uses the same
//! D14 guarded outbound client as MCP. User URLs are checked at every redirect hop and the
//! resolver pins the approved addresses to the connection.

use crate::adapter::CallOutcome;
use crate::config::ProviderKey;
use crate::outbound::Outbound;
use crate::{BrainError, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchInput {
    query: String,
    #[serde(default = "default_results")]
    num: usize,
    country: Option<String>,
    language: Option<String>,
}

fn default_results() -> usize {
    5
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FetchInput {
    url: String,
    max_chars: Option<usize>,
}

#[derive(Debug)]
pub struct WebRuntime {
    outbound: Outbound,
    search_endpoint: String,
    search_key: Option<ProviderKey>,
    timeout: Duration,
    max_result_bytes: usize,
    search_max_response_bytes: usize,
    fetch_max_response_bytes: usize,
    fetch_max_chars: usize,
}

impl WebRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        outbound: Outbound,
        search_endpoint: String,
        search_key: Option<ProviderKey>,
        timeout: Duration,
        max_result_bytes: usize,
        search_max_response_bytes: usize,
        fetch_max_response_bytes: usize,
        fetch_max_chars: usize,
    ) -> Self {
        Self {
            outbound,
            search_endpoint,
            search_key,
            timeout,
            max_result_bytes,
            search_max_response_bytes,
            fetch_max_response_bytes,
            fetch_max_chars,
        }
    }

    pub async fn call(&self, name: &str, input: &Value, cancel: &CancellationToken) -> CallOutcome {
        let started = Instant::now();
        let operation = async {
            match name {
                "web_search" => self.search(input).await,
                "web_fetch" => self.fetch(input).await,
                _ => Err(BrainError::UndeclaredTool { name: name.into() }),
            }
        };
        let result = tokio::select! {
            _ = cancel.cancelled() => {
                return outcome("cancelled", "web request cancelled", true, started, false);
            }
            result = tokio::time::timeout(self.timeout, operation) => result,
        };
        match result {
            Err(_) => outcome(
                "deadline_exceeded",
                format!("web request exceeded {} ms", self.timeout.as_millis()),
                true,
                started,
                false,
            ),
            Ok(Err(error)) => outcome("failed", error.to_string(), true, started, false),
            Ok(Ok(content)) => {
                let (content, truncated) = bound_tail(content, self.max_result_bytes);
                outcome("completed", content, false, started, truncated)
            }
        }
    }

    async fn search(&self, input: &Value) -> Result<String> {
        let input: SearchInput = serde_json::from_value(input.clone())
            .map_err(|error| BrainError::Invalid(format!("web_search input: {error}")))?;
        let query = input.query.trim();
        if query.is_empty() || query.chars().count() > 500 {
            return Err(BrainError::Invalid(
                "web_search.query must contain 1..=500 characters".into(),
            ));
        }
        if !(1..=10).contains(&input.num) {
            return Err(BrainError::Invalid(
                "web_search.num must be between 1 and 10".into(),
            ));
        }
        for (name, value, max) in [
            ("country", input.country.as_deref(), 8usize),
            ("language", input.language.as_deref(), 16usize),
        ] {
            if let Some(value) = value
                && (value.len() < 2 || value.len() > max)
            {
                return Err(BrainError::Invalid(format!(
                    "web_search.{name} must contain 2..={max} bytes"
                )));
            }
        }
        let key = self.search_key.as_ref().ok_or_else(|| {
            BrainError::Invalid("managed web search is not configured on this plane".into())
        })?;
        let endpoint = self.outbound.check_url(&self.search_endpoint)?;
        let mut body = json!({"q": query, "num": input.num});
        if let Some(country) = input.country {
            body["gl"] = country.into();
        }
        if let Some(language) = input.language {
            body["hl"] = language.into();
        }
        let response = self
            .outbound
            .client()
            .post(endpoint)
            .header("X-API-KEY", key.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|error| BrainError::Transport(format!("web search: {error}")))?;
        let status = response.status();
        let bytes = read_bounded(response, self.search_max_response_bytes).await?;
        if !status.is_success() {
            return Err(BrainError::ProviderStatus {
                status: status.as_u16(),
                body: safe_preview(&bytes, 512),
            });
        }
        let provider: Value = serde_json::from_slice(&bytes)
            .map_err(|error| BrainError::Protocol(format!("web search response: {error}")))?;
        let results: Vec<Value> = provider["organic"]
            .as_array()
            .into_iter()
            .flatten()
            .take(input.num)
            .filter_map(|item| {
                let title = item["title"].as_str()?;
                let url = item["link"].as_str()?;
                let mut result = json!({
                    "title": title,
                    "url": url,
                    "snippet": item["snippet"].as_str().unwrap_or("")
                });
                if let Some(date) = item["date"].as_str() {
                    result["date"] = date.into();
                }
                Some(result)
            })
            .collect();
        serde_json::to_string(&json!({"query": query, "results": results}))
            .map_err(BrainError::from)
    }

    async fn fetch(&self, input: &Value) -> Result<String> {
        let input: FetchInput = serde_json::from_value(input.clone())
            .map_err(|error| BrainError::Invalid(format!("web_fetch input: {error}")))?;
        let max_chars = input.max_chars.unwrap_or(self.fetch_max_chars);
        if max_chars == 0 || max_chars > self.fetch_max_chars {
            return Err(BrainError::Invalid(format!(
                "web_fetch.max_chars must be between 1 and {}",
                self.fetch_max_chars
            )));
        }
        let mut current = self.outbound.check_url(&input.url)?;
        let mut redirects = 0usize;
        let response = loop {
            let response = self
                .outbound
                .client()
                .get(current.clone())
                .header(
                    reqwest::header::ACCEPT,
                    "text/html, text/plain, application/json;q=0.9",
                )
                .send()
                .await
                .map_err(|error| BrainError::Transport(format!("web fetch: {error}")))?;
            if response.status().is_redirection() {
                if redirects >= 5 {
                    return Err(BrainError::Protocol(
                        "web fetch exceeded five redirects".into(),
                    ));
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| BrainError::Protocol("redirect has no valid Location".into()))?;
                let next = current
                    .join(location)
                    .map_err(|error| BrainError::Invalid(format!("redirect URL: {error}")))?;
                current = self.outbound.check_url(next.as_str())?;
                redirects += 1;
                continue;
            }
            break response;
        };
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("application/octet-stream")
            .trim()
            .to_ascii_lowercase();
        if !status.is_success() {
            return Err(BrainError::ProviderStatus {
                status: status.as_u16(),
                body: "web fetch returned a non-success status".into(),
            });
        }
        let supported = content_type == "text/html"
            || content_type == "text/plain"
            || content_type == "application/json"
            || content_type.ends_with("+json");
        if !supported {
            return Err(BrainError::Invalid(format!(
                "web_fetch content type {content_type} is not text, HTML, or JSON"
            )));
        }
        let bytes = read_bounded(response, self.fetch_max_response_bytes).await?;
        let decoded = String::from_utf8_lossy(&bytes);
        let mut text = if content_type == "text/html" {
            html2text::from_read(decoded.as_bytes(), 120)
                .map_err(|error| BrainError::Protocol(format!("HTML conversion: {error}")))?
        } else {
            decoded.into_owned()
        };
        let mut truncated = false;
        if text.chars().count() > max_chars {
            text = text.chars().take(max_chars).collect();
            truncated = true;
        }
        serde_json::to_string(&json!({
            "url": current.as_str(),
            "status": status.as_u16(),
            "content_type": content_type,
            "text": text,
            "truncated": truncated
        }))
        .map_err(BrainError::from)
    }
}

async fn read_bounded(response: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(BrainError::Invalid(format!(
            "web response exceeds the configured {max_bytes}-byte limit"
        )));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| BrainError::Transport(format!("web body: {error}")))?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(BrainError::Invalid(format!(
                "web response exceeds the configured {max_bytes}-byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn safe_preview(bytes: &[u8], max: usize) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(max)]).into_owned()
}

fn bound_tail(mut content: String, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content, false);
    }
    let marker = "[...web result truncated...]\n";
    let keep = max_bytes.saturating_sub(marker.len());
    let mut start = content.len().saturating_sub(keep);
    while start < content.len() && !content.is_char_boundary(start) {
        start += 1;
    }
    content = format!("{marker}{}", &content[start..]);
    (content, true)
}

fn outcome(
    kind: &str,
    content: impl Into<String>,
    is_error: bool,
    started: Instant,
    truncated: bool,
) -> CallOutcome {
    CallOutcome {
        outcome: kind.into(),
        content: content.into(),
        is_error,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
        terminal: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_tail_bound_is_safe() {
        let (text, truncated) = bound_tail("é".repeat(100), 64);
        assert!(truncated);
        assert!(text.is_char_boundary(text.len()));
        assert!(text.len() <= 64);
    }
}
