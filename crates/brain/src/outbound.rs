//! The `Outbound` seam: every Brain-originated request to a user-controlled URL goes
//! through here. Provider gateways are its current tenant.
//!
//! The threat is SSRF from the trusted tier: the brain sits next to its own metadata service,
//! its journal and its control plane, and a custom provider URL is attacker-chosen. Two
//! layers, both mandatory in guarded mode:
//!
//! 1. **URL pre-check** ([`check_url`]) -- scheme must be https, no userinfo, and a literal-IP
//!    host is judged against the deny table directly (a literal IP never reaches a DNS
//!    resolver, so a resolver-only guard would wave `http://169.254.169.254/` straight
//!    through).
//! 2. **Guarding resolver** ([`GuardingResolver`]) -- DNS resolution happens INSIDE the
//!    reqwest client, and the connection uses exactly the addresses the guard approved, so
//!    there is no resolve-then-connect gap for DNS rebinding to slip through. If ANY resolved
//!    address is denied the whole resolution fails: a hostname that is half-public,
//!    half-internal is an attack, not a configuration.
//!
//! Redirects are disabled outright -- a JSON-RPC endpoint that answers 3xx is not something we
//! follow into an unguarded location.
//!
//! `allow_private` is a COMPOSITION choice, not a runtime branch: local mode composes a
//! permissive policy (a developer's provider gateway may live on `127.0.0.1`), the AWS composition
//! refuses to start with one (`brain-aws` fails fast). Never let a local-mode convenience
//! weaken a production invariant (AGENTS.md).

use crate::{BrainError, Result};
use std::net::IpAddr;
use std::time::Duration;

/// Policy + the one shared client. Built once per [`crate::session::Brain`]; reqwest clients
/// are cheap to clone (an `Arc` inside), so per-session and per-call use clones this.
#[derive(Clone)]
pub struct Outbound {
    client: reqwest::Client,
    allow_private: bool,
}

impl std::fmt::Debug for Outbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outbound")
            .field("allow_private", &self.allow_private)
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
            allow_private,
        }
    }

    pub fn allow_private(&self) -> bool {
        self.allow_private
    }

    /// The guarded client. Callers MUST run [`Outbound::check_url`] on the target first --
    /// the resolver cannot judge literal-IP hosts or schemes.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Validates a user-supplied outbound URL against the policy. Returns the parsed URL so
    /// callers never re-parse what was checked.
    pub fn check_url(&self, url: &str) -> Result<reqwest::Url> {
        let parsed = reqwest::Url::parse(url)
            .map_err(|_| BrainError::Invalid("outbound URL is invalid".into()))?;
        let target = redacted_target(&parsed);
        match parsed.scheme() {
            "https" => {}
            "http" if self.allow_private => {}
            s => {
                return Err(BrainError::Invalid(format!(
                    "outbound target {target}: scheme {s:?} is not allowed (https required)"
                )));
            }
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(BrainError::Invalid(format!(
                "outbound target {target}: userinfo is not allowed"
            )));
        }
        if parsed.fragment().is_some() {
            return Err(BrainError::Invalid(format!(
                "outbound target {target}: fragments are not allowed"
            )));
        }
        let Some(host) = parsed.host_str() else {
            return Err(BrainError::Invalid("outbound URL has no host".into()));
        };
        if !self.allow_private {
            // A literal-IP host never reaches the DNS resolver, so it is judged here. The
            // url crate canonicalises exotic v4 spellings (`https://2130706433/` parses as
            // `127.0.0.1`), so parsing the canonical host string catches those too. v6
            // literals carry brackets in `host_str`.
            let bare = host.trim_start_matches('[').trim_end_matches(']');
            if let Ok(ip) = bare.parse::<IpAddr>()
                && let Some(reason) = brain_protocol::network::special_use_reason(&ip)
            {
                return Err(BrainError::Invalid(format!(
                    "outbound target {target}: address is {reason} (SSRF guard)"
                )));
            }
        }
        Ok(parsed)
    }
}

/// Why an address is refused, or `None` if it is a plain public unicast address. The table is
/// deny-by-class: loopback, RFC1918, link-local (the AWS metadata service lives at
/// 169.254.169.254), CGN, unspecified, multicast/broadcast, ULA, and v4-mapped v6 (judged as
/// the mapped v4 -- `::ffff:10.0.0.1` is 10.0.0.1 wearing a coat).
pub fn deny_reason(ip: &IpAddr) -> Option<&'static str> {
    brain_protocol::network::special_use_reason(ip)
}

fn redacted_target(url: &reqwest::Url) -> String {
    let host = url.host_str().unwrap_or("<missing-host>");
    match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn deny_table_refuses_every_internal_class() {
        for &(addr, why) in brain_protocol::network::SPECIAL_USE_FIXTURES {
            assert!(
                deny_reason(&ip(addr)).is_some(),
                "{addr} ({why}) must be denied"
            );
        }
    }

    #[test]
    fn deny_table_passes_public_unicast() {
        for &addr in brain_protocol::network::PUBLIC_UNICAST_FIXTURES {
            assert!(deny_reason(&ip(addr)).is_none(), "{addr} must be allowed");
        }
    }

    #[test]
    fn guarded_url_check_refuses_literal_internal_ips_and_bad_schemes() {
        let o = Outbound::new(false);
        for bad in [
            "http://example.com/provider",          // scheme
            "https://user:pw@example.com/provider", // userinfo
            "https://127.0.0.1/provider",           // literal loopback
            "https://169.254.169.254/latest/meta",  // literal metadata
            "https://[::1]/provider",               // literal v6 loopback
            "https://[::ffff:10.0.0.1]/provider",   // v4-mapped literal
            "https://192.168.0.10:8443/provider",   // literal rfc1918 with port
            "ftp://example.com/x",                  // scheme
        ] {
            assert!(o.check_url(bad).is_err(), "{bad} must be refused");
        }
        assert!(o.check_url("https://provider.example.com/v1").is_ok());
    }

    #[test]
    fn rejected_urls_never_echo_query_credentials() {
        let secret = "query-secret-sentinel";
        let error = Outbound::new(false)
            .check_url(&format!("https://127.0.0.1/path?token={secret}#fragment"))
            .unwrap_err()
            .to_string();
        assert!(!error.contains(secret));
        assert!(!error.contains("/path"));
    }

    #[test]
    fn permissive_policy_allows_loopback_and_http() {
        let o = Outbound::new(true);
        assert!(o.check_url("http://127.0.0.1:3001/v1").is_ok());
        assert!(o.check_url("https://[::1]:8443/v1").is_ok());
        assert!(
            o.check_url("https://user:pw@localhost/v1").is_err(),
            "userinfo stays refused even permissively"
        );
    }

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
}
