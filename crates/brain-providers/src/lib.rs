//! Live provider transport: the guarded outbound HTTP client and one HTTP provider per
//! dialect, delegating request building and stream decoding to the pure codecs in
//! `brain::provider`. Nothing here renders or interprets model content.

use std::sync::Arc;
use std::time::Duration;

use brain::config::{Dialect, ProviderKey, SealedPrefix};
use brain::message::Message;
use brain::outbound::OutboundPolicy;
use brain::provider::{ModelRequest, Provider, ProviderEvent};
use brain::session::ProviderFactory;
use brain::{BrainError, Result};
use futures_util::stream::BoxStream;

pub mod external;

pub use external::HttpExternalToolExecutor;

/// Policy plus the one shared guarded client. reqwest clients are cheap to clone (an `Arc`
/// inside), so per-call use clones this.
#[derive(Clone)]
pub struct Outbound {
    client: reqwest::Client,
    policy: OutboundPolicy,
}

impl std::fmt::Debug for Outbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outbound")
            .field("allow_private", &self.policy.allow_private())
            .finish()
    }
}

impl Outbound {
    pub fn new(allow_private: bool) -> Self {
        let mut b = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(4);
        if !allow_private {
            b = b.dns_resolver(GuardingResolver);
        }
        Outbound {
            client: b.build().expect("outbound http client"),
            policy: OutboundPolicy::new(allow_private),
        }
    }

    pub fn policy(&self) -> OutboundPolicy {
        self.policy
    }

    /// The guarded client. Callers MUST run the policy check on the target first -- the
    /// resolver cannot judge literal-IP hosts or schemes.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn check_url(&self, url: &str) -> Result<url::Url> {
        self.policy.check_url(url)
    }
}

/// DNS resolution with the deny table applied INSIDE the client. System resolver via
/// `tokio::net::lookup_host`; any denied address fails the whole lookup.
struct GuardingResolver;

impl reqwest::dns::Resolve for GuardingResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            // Port 0 is a placeholder; reqwest substitutes the URL's real port.
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("dns {host}: {e}").into()
                })?
                .collect();
            if addrs.is_empty() {
                return Err(format!("dns {host}: no addresses").into());
            }
            for a in &addrs {
                if let Some(reason) = brain_protocol::network::special_use_reason(&a.ip()) {
                    return Err(format!(
                        "dns {host}: resolves to {} which is {reason} (SSRF guard)",
                        a.ip()
                    )
                    .into());
                }
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Shared streaming path for every dialect: send, check status, decode SSE
/// incrementally, hand each frame to the dialect decoder.
async fn http_stream(
    req: ModelRequest,
    outbound: &Outbound,
    decode: fn(Option<&str>, &str) -> Result<Vec<ProviderEvent>>,
) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
    use futures_util::StreamExt;

    let url = outbound.check_url(&req.url)?;
    let mut rb = outbound.client().post(url).body(req.body);
    for (k, v) in &req.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| BrainError::Transport(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        // Delta-seconds form only; the HTTP-date form falls back to the kernel's own backoff.
        let retry_after_ms = resp
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(|seconds| seconds.saturating_mul(1_000));
        let mut stream = resp.bytes_stream();
        let mut body = Vec::with_capacity(2048);
        while body.len() < 2048 {
            let Some(chunk) = stream.next().await else {
                break;
            };
            let chunk = chunk.map_err(|error| BrainError::Transport(error.to_string()))?;
            let take = (2048 - body.len()).min(chunk.len());
            body.extend_from_slice(&chunk[..take]);
            if take < chunk.len() {
                break;
            }
        }
        return Err(BrainError::ProviderStatus {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&body).into_owned(),
            retry_after_ms,
        });
    }

    let mut dec = brain::provider::sse::SseDecoder::default();
    let mut bytes = resp.bytes_stream();
    let s = async_stream::stream! {
        loop {
            match bytes.next().await {
                Some(Ok(chunk)) => match dec.feed(&chunk) {
                    Ok(frames) => {
                        for f in frames {
                            match decode(f.event.as_deref(), &f.data) {
                                Ok(evs) => { for e in evs { yield Ok(e); } }
                                Err(e) => { yield Err(e); return; }
                            }
                        }
                    }
                    Err(e) => { yield Err(e); return; }
                },
                Some(Err(e)) => { yield Err(BrainError::Transport(e.to_string())); return; }
                None => {
                    if dec.pending() > 0 {
                        // Early EOF mid-frame. Reported, never swallowed: a
                        // truncated stream that looks complete is how a partial
                        // assistant message becomes history.
                        yield Err(BrainError::Protocol(format!(
                            "provider stream ended with {} bytes of an incomplete SSE frame",
                            dec.pending()
                        )));
                    }
                    return;
                }
            }
        }
    };
    Ok(Box::pin(s))
}

/// A live dialect provider: the core codec for building requests and decoding events, this
/// crate's guarded client for the wire.
#[derive(Debug)]
struct HttpProvider {
    dialect: Dialect,
    decode: fn(Option<&str>, &str) -> Result<Vec<ProviderEvent>>,
    build: fn(&SealedPrefix, &[Message], &ProviderKey, &str) -> Result<ModelRequest>,
    outbound: Outbound,
}

#[async_trait::async_trait]
impl Provider for HttpProvider {
    fn dialect(&self) -> Dialect {
        self.dialect
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        (self.build)(prefix, history, key, base_url)
    }

    async fn stream(&self, req: ModelRequest) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        http_stream(req, &self.outbound, self.decode).await
    }
}

/// The live providers for every dialect, sharing one guarded client built from the
/// composition's outbound policy.
pub fn default_factory(allow_private: bool) -> ProviderFactory {
    let outbound = Outbound::new(allow_private);
    Arc::new(move |dialect| match dialect {
        Dialect::AnthropicMessages => Arc::new(HttpProvider {
            dialect,
            decode: brain::provider::anthropic::decode,
            build: brain::provider::anthropic::Anthropic::build_request,
            outbound: outbound.clone(),
        }),
        Dialect::OpenAiChat => Arc::new(HttpProvider {
            dialect,
            decode: brain::provider::openai::decode,
            build: brain::provider::openai::OpenAiChat::build_request,
            outbound: outbound.clone(),
        }),
    })
}

/// The host-owned executor for sealed external tools, derived from the composition's config.
/// Fails fast on a malformed endpoint instead of composing a Brain that cannot dispatch.
pub fn external_executor_from_cfg(
    cfg: &brain::session::BrainConfig,
) -> Result<Arc<dyn brain::adapter::ToolExecutor>> {
    Ok(match &cfg.external_executor_url {
        Some(endpoint) => Arc::new(HttpExternalToolExecutor::new(
            endpoint.clone(),
            cfg.external_executor_token
                .as_ref()
                .map(|token| token.expose().to_string()),
            cfg.external_call_timeout,
            cfg.external_executor_capabilities.iter().cloned(),
        )?),
        None => Arc::new(brain::adapter::DisabledToolExecutor),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn guarded_client_refuses_hostnames_resolving_internal() {
        // `localhost` resolves to loopback everywhere; the guarding resolver must fail the
        // connection attempt itself (error, not a response).
        let o = Outbound::new(false);
        let err = o
            .client()
            .get("https://localhost:1/nope")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .expect_err("localhost must not connect through the guarded client");
        let text = format!("{err:?}");
        assert!(
            text.contains("SSRF guard"),
            "failure must be attributed to the guard, got: {text}"
        );
    }

    #[test]
    fn default_factory_serves_every_dialect() {
        let factory = default_factory(false);
        for dialect in [Dialect::AnthropicMessages, Dialect::OpenAiChat] {
            assert_eq!(factory(dialect).dialect(), dialect);
        }
    }
}
