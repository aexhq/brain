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

/// The URL/IP policy half of the outbound seam: pure judgement, no client. The guarded HTTP
/// client that enforces it during DNS resolution lives in `brain-providers`.
#[derive(Clone, Copy, Debug)]
pub struct OutboundPolicy {
    allow_private: bool,
}

impl OutboundPolicy {
    pub fn new(allow_private: bool) -> Self {
        OutboundPolicy { allow_private }
    }

    pub fn allow_private(&self) -> bool {
        self.allow_private
    }

    /// Validates a user-supplied outbound URL against the policy. Returns the parsed URL so
    /// callers never re-parse what was checked.
    pub fn check_url(&self, url: &str) -> Result<url::Url> {
        let parsed = url::Url::parse(url)
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

fn redacted_target(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("<missing-host>");
    match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

/// The external tool executor endpoint grammar: literal-loopback http URL, no credentials,
/// query, or fragment. Pure so config validation needs no HTTP client; the executor in
/// `brain-providers` enforces the same rule at construction.
pub fn validate_external_executor_url(endpoint: &str) -> Result<url::Url> {
    let endpoint = endpoint.parse::<url::Url>().map_err(|error| {
        BrainError::Invalid(format!("external tool executor URL is invalid: {error}"))
    })?;
    if endpoint.scheme() != "http"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(BrainError::Invalid(
            "external tool executor must be an http:// loopback URL without credentials, query, or fragment"
                .into(),
        ));
    }
    let loopback = endpoint
        .host_str()
        // `url` retains brackets in `host_str()` for an IPv6 literal. `IpAddr` expects the
        // bare address, so normalize only that syntactic wrapper before the exact loopback
        // classification.
        .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !loopback {
        return Err(BrainError::Invalid(
            "external tool executor host must be a literal loopback address".into(),
        ));
    }
    Ok(endpoint)
}

/// A bearer credential must be a valid HTTP header token payload: visible ASCII only —
/// no control bytes, no line breaks, nothing an intermediary could reinterpret as framing.
pub fn validate_bearer_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| (b'!'..=b'~').contains(&byte))
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
        let o = OutboundPolicy::new(false);
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
        let error = OutboundPolicy::new(false)
            .check_url(&format!("https://127.0.0.1/path?token={secret}#fragment"))
            .unwrap_err()
            .to_string();
        assert!(!error.contains(secret));
        assert!(!error.contains("/path"));
    }

    #[test]
    fn permissive_policy_allows_loopback_and_http() {
        let o = OutboundPolicy::new(true);
        assert!(o.check_url("http://127.0.0.1:3001/v1").is_ok());
        assert!(o.check_url("https://[::1]:8443/v1").is_ok());
        assert!(
            o.check_url("https://user:pw@localhost/v1").is_err(),
            "userinfo stays refused even permissively"
        );
    }
}
