//! Canonical special-use address policy shared by Brain, Aex, and Hands.
//!
//! Hosted trusted-tier HTTP and sandbox connector policy is fail-closed: only ordinary public
//! unicast addresses pass. Keep additions data-only and cover them in [`SPECIAL_USE_FIXTURES`].
//! IPv4-mapped IPv6 addresses are classified as their embedded IPv4 address, so a mapped public
//! IPv4 address remains usable while mapped private/special-use space stays denied.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::hand::{NetworkCeiling, NetworkCeilingDestinationsItem};

/// Representative denied addresses and the stable reason returned by [`special_use_reason`].
pub const SPECIAL_USE_FIXTURES: &[(&str, &str)] = &[
    ("0.0.0.1", "this-network (0.0.0.0/8)"),
    ("10.0.0.1", "private (RFC1918)"),
    ("100.64.0.1", "carrier-grade NAT (RFC6598)"),
    ("127.0.0.1", "loopback"),
    ("169.254.169.254", "link-local (metadata service range)"),
    ("172.16.0.1", "private (RFC1918)"),
    ("192.0.0.170", "IETF protocol assignments (192.0.0.0/24)"),
    ("192.0.2.1", "documentation (TEST-NET-1)"),
    ("192.31.196.1", "AS112 service prefix"),
    ("192.52.193.1", "AMT relay anycast prefix"),
    ("192.88.99.1", "deprecated 6to4 relay anycast"),
    ("192.168.0.1", "private (RFC1918)"),
    ("192.175.48.1", "AS112 service prefix"),
    ("198.18.0.1", "benchmarking (198.18.0.0/15)"),
    ("198.51.100.1", "documentation (TEST-NET-2)"),
    ("203.0.113.1", "documentation (TEST-NET-3)"),
    ("224.0.0.1", "multicast"),
    ("255.255.255.255", "reserved/broadcast (240.0.0.0/4)"),
    ("::", "unspecified"),
    ("::1", "loopback"),
    ("::ffff:10.0.0.1", "private (RFC1918)"),
    ("64:ff9b::1", "IPv4/IPv6 translation (64:ff9b::/96)"),
    (
        "64:ff9b:1::1",
        "local-use IPv4/IPv6 translation (64:ff9b:1::/48)",
    ),
    ("100::1", "discard-only (100::/64)"),
    ("100:0:0:1::1", "dummy IPv6 prefix (100:0:0:1::/64)"),
    ("2001:2::1", "IETF protocol assignments (2001::/23)"),
    ("2001:db8::1", "documentation (2001:db8::/32)"),
    ("2002::1", "deprecated 6to4 (2002::/16)"),
    (
        "2620:4f:8000::1",
        "direct-delegation AS112 service prefix (2620:4f:8000::/48)",
    ),
    ("3fff::1", "documentation (3fff::/20)"),
    (
        "4000::1",
        "outside allocated IPv6 global unicast (2000::/3)",
    ),
    ("5f00::1", "SRv6 SIDs (5f00::/16)"),
    ("fc00::1", "unique-local (fc00::/7)"),
    ("fe80::1", "link-local"),
    ("fec0::1", "deprecated site-local (fec0::/10)"),
    ("ff02::1", "multicast"),
];

/// Representative public-unicast addresses that must remain allowed.
pub const PUBLIC_UNICAST_FIXTURES: &[&str] = &[
    "1.1.1.1",
    "8.8.8.8",
    "93.184.216.34",
    "2606:4700:4700::1111",
    "2001:4860:4860::8888",
];

/// Return a stable reason when `ip` is special-use/non-public, otherwise `None`.
pub fn special_use_reason(ip: &IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(ip) => ipv4_reason(*ip),
        IpAddr::V6(ip) => ipv6_reason(*ip),
    }
}

/// True only for ordinary public-unicast addresses.
pub fn is_public_unicast(ip: &IpAddr) -> bool {
    special_use_reason(ip).is_none()
}

/// Return whether `requested` is a well-formed semantic narrowing of `sealed`.
///
/// A malformed destination is never considered a subset. TLS host allowlists support a single
/// leading `*.` label; a wildcard covers subdomains, not the suffix itself. TCP destinations are
/// IPv4 CIDRs, and both CIDR containment and port-set containment are required. `public` covers
/// only ordinary public destinations, so an allowlist containing a special-use CIDR is not a
/// narrowing of it. This helper is the canonical Brain/Hands policy lattice; adapters must not
/// compare serialized destination objects for equality.
pub fn network_ceiling_is_subset(requested: &NetworkCeiling, sealed: &NetworkCeiling) -> bool {
    if !network_ceiling_is_well_formed(requested) || !network_ceiling_is_well_formed(sealed) {
        return false;
    }
    match (requested, sealed) {
        (NetworkCeiling::None, _) => true,
        (NetworkCeiling::Public, NetworkCeiling::Public) => true,
        (NetworkCeiling::Allowlist(requested), NetworkCeiling::Public) => {
            requested.iter().all(destination_is_public)
        }
        (NetworkCeiling::Allowlist(requested), NetworkCeiling::Allowlist(sealed)) => {
            requested.iter().all(|destination| {
                sealed
                    .iter()
                    .any(|limit| destination_is_subset(destination, limit))
            })
        }
        _ => false,
    }
}

fn network_ceiling_is_well_formed(ceiling: &NetworkCeiling) -> bool {
    match ceiling {
        NetworkCeiling::None | NetworkCeiling::Public => true,
        NetworkCeiling::Allowlist(destinations) => {
            !destinations.is_empty()
                && destinations.len() <= 128
                && destinations.iter().all(destination_is_well_formed)
        }
    }
}

fn destination_is_well_formed(destination: &NetworkCeilingDestinationsItem) -> bool {
    match destination {
        NetworkCeilingDestinationsItem::Tls { host, ports } => {
            ports[0].as_u64() == Some(443) && normalized_host_pattern(host).is_some()
        }
        NetworkCeilingDestinationsItem::Tcp { cidr, ports } => {
            parse_ipv4_cidr(cidr).is_some()
                && !ports.is_empty()
                && ports.len() <= 32
                && ports.iter().all(|port| port.get() <= 65_535)
                && ports
                    .iter()
                    .enumerate()
                    .all(|(index, port)| ports[..index].iter().all(|prior| prior != port))
        }
    }
}

fn destination_is_public(destination: &NetworkCeilingDestinationsItem) -> bool {
    match destination {
        NetworkCeilingDestinationsItem::Tls { host, .. } => {
            let Some(host) = normalized_host_pattern(host) else {
                return false;
            };
            let literal = host.strip_prefix("*.").unwrap_or(&host);
            literal
                .parse::<IpAddr>()
                .map_or(true, |address| is_public_unicast(&address))
        }
        NetworkCeilingDestinationsItem::Tcp { cidr, .. } => parse_ipv4_cidr(cidr)
            .is_some_and(|(network, prefix)| ipv4_cidr_is_public(network, prefix)),
    }
}

fn destination_is_subset(
    requested: &NetworkCeilingDestinationsItem,
    sealed: &NetworkCeilingDestinationsItem,
) -> bool {
    match (requested, sealed) {
        (
            NetworkCeilingDestinationsItem::Tls {
                host: requested, ..
            },
            NetworkCeilingDestinationsItem::Tls { host: sealed, .. },
        ) => host_pattern_is_subset(requested, sealed),
        (
            NetworkCeilingDestinationsItem::Tcp {
                cidr: requested_cidr,
                ports: requested_ports,
            },
            NetworkCeilingDestinationsItem::Tcp {
                cidr: sealed_cidr,
                ports: sealed_ports,
            },
        ) => {
            let Some((requested_network, requested_prefix)) = parse_ipv4_cidr(requested_cidr)
            else {
                return false;
            };
            let Some((sealed_network, sealed_prefix)) = parse_ipv4_cidr(sealed_cidr) else {
                return false;
            };
            requested_prefix >= sealed_prefix
                && prefix_match_u32(requested_network, sealed_network, sealed_prefix)
                && requested_ports
                    .iter()
                    .all(|requested| sealed_ports.contains(requested))
        }
        _ => false,
    }
}

fn normalized_host_pattern(host: &str) -> Option<String> {
    if host.is_empty() || host.len() > 253 || !host.is_ascii() || host.ends_with('.') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let suffix = host.strip_prefix("*.").unwrap_or(&host);
    if suffix.contains('*')
        || suffix.parse::<IpAddr>().is_ok() && host.starts_with("*.")
        || suffix.len() > 253
        || suffix.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return None;
    }
    Some(host)
}

fn host_pattern_is_subset(requested: &str, sealed: &str) -> bool {
    let Some(requested) = normalized_host_pattern(requested) else {
        return false;
    };
    let Some(sealed) = normalized_host_pattern(sealed) else {
        return false;
    };
    let requested_wildcard = requested.strip_prefix("*.");
    let sealed_wildcard = sealed.strip_prefix("*.");
    match (requested_wildcard, sealed_wildcard) {
        (None, None) => requested == sealed,
        (Some(_), None) => false,
        (None, Some(sealed_suffix)) => {
            requested.len() > sealed_suffix.len()
                && requested.ends_with(sealed_suffix)
                && requested.as_bytes()[requested.len() - sealed_suffix.len() - 1] == b'.'
        }
        (Some(requested_suffix), Some(sealed_suffix)) => {
            requested_suffix == sealed_suffix
                || (requested_suffix.len() > sealed_suffix.len()
                    && requested_suffix.ends_with(sealed_suffix)
                    && requested_suffix.as_bytes()
                        [requested_suffix.len() - sealed_suffix.len() - 1]
                        == b'.')
        }
    }
}

fn parse_ipv4_cidr(cidr: &str) -> Option<(u32, u8)> {
    let (address, prefix) = cidr.split_once('/')?;
    let address: Ipv4Addr = address.parse().ok()?;
    let prefix: u8 = prefix.parse().ok()?;
    if prefix > 32 {
        return None;
    }
    let value = u32::from(address);
    let network = value & prefix_mask_u32(prefix);
    (value == network).then_some((network, prefix))
}

fn ipv4_cidr_is_public(network: u32, prefix: u8) -> bool {
    let requested_mask = prefix_mask_u32(prefix);
    let requested_last = network | !requested_mask;
    IPV4_SPECIAL_USE
        .iter()
        .all(|&(special, special_prefix, _)| {
            let special_mask = prefix_mask_u32(special_prefix);
            let special_first = special & special_mask;
            let special_last = special_first | !special_mask;
            requested_last < special_first || special_last < network
        })
}

fn ipv4_reason(ip: Ipv4Addr) -> Option<&'static str> {
    let value = u32::from(ip);
    for &(network, prefix, reason) in IPV4_SPECIAL_USE {
        if prefix_match_u32(value, network, prefix) {
            return Some(reason);
        }
    }
    None
}

fn ipv6_reason(ip: Ipv6Addr) -> Option<&'static str> {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_reason(mapped);
    }
    let value = u128::from(ip);
    for &(network, prefix, reason) in IPV6_SPECIAL_USE {
        if prefix_match_u128(value, network, prefix) {
            return Some(reason);
        }
    }
    // Ordinary allocated IPv6 global unicast currently lives in 2000::/3. Subtracting only
    // today's named special-use prefixes would fail open when an unallocated/reserved range is
    // presented, so require the positive allocation boundary as well.
    (!prefix_match_u128(value, 0x2000_0000_0000_0000_0000_0000_0000_0000, 3))
        .then_some("outside allocated IPv6 global unicast (2000::/3)")
}

const IPV4_SPECIAL_USE: &[(u32, u8, &str)] = &[
    (0x0000_0000, 8, "this-network (0.0.0.0/8)"),
    (0x0a00_0000, 8, "private (RFC1918)"),
    (0x6440_0000, 10, "carrier-grade NAT (RFC6598)"),
    (0x7f00_0000, 8, "loopback"),
    (0xa9fe_0000, 16, "link-local (metadata service range)"),
    (0xac10_0000, 12, "private (RFC1918)"),
    (0xc000_0000, 24, "IETF protocol assignments (192.0.0.0/24)"),
    (0xc000_0200, 24, "documentation (TEST-NET-1)"),
    (0xc01f_c400, 24, "AS112 service prefix"),
    (0xc034_c100, 24, "AMT relay anycast prefix"),
    (0xc058_6300, 24, "deprecated 6to4 relay anycast"),
    (0xc0a8_0000, 16, "private (RFC1918)"),
    (0xc0af_3000, 24, "AS112 service prefix"),
    (0xc612_0000, 15, "benchmarking (198.18.0.0/15)"),
    (0xc633_6400, 24, "documentation (TEST-NET-2)"),
    (0xcb00_7100, 24, "documentation (TEST-NET-3)"),
    (0xe000_0000, 4, "multicast"),
    (0xf000_0000, 4, "reserved/broadcast (240.0.0.0/4)"),
];

const IPV6_SPECIAL_USE: &[(u128, u8, &str)] = &[
    (0, 128, "unspecified"),
    (1, 128, "loopback"),
    (
        0x0064_ff9b_0000_0000_0000_0000_0000_0000,
        96,
        "IPv4/IPv6 translation (64:ff9b::/96)",
    ),
    (
        0x0064_ff9b_0001_0000_0000_0000_0000_0000,
        48,
        "local-use IPv4/IPv6 translation (64:ff9b:1::/48)",
    ),
    (
        0x0100_0000_0000_0000_0000_0000_0000_0000,
        64,
        "discard-only (100::/64)",
    ),
    (
        0x0100_0000_0000_0001_0000_0000_0000_0000,
        64,
        "dummy IPv6 prefix (100:0:0:1::/64)",
    ),
    (
        0x2001_0000_0000_0000_0000_0000_0000_0000,
        23,
        "IETF protocol assignments (2001::/23)",
    ),
    (
        0x2001_0db8_0000_0000_0000_0000_0000_0000,
        32,
        "documentation (2001:db8::/32)",
    ),
    (
        0x2002_0000_0000_0000_0000_0000_0000_0000,
        16,
        "deprecated 6to4 (2002::/16)",
    ),
    (
        0x2620_004f_8000_0000_0000_0000_0000_0000,
        48,
        "direct-delegation AS112 service prefix (2620:4f:8000::/48)",
    ),
    (
        0x3fff_0000_0000_0000_0000_0000_0000_0000,
        20,
        "documentation (3fff::/20)",
    ),
    (
        0x5f00_0000_0000_0000_0000_0000_0000_0000,
        16,
        "SRv6 SIDs (5f00::/16)",
    ),
    (
        0xfc00_0000_0000_0000_0000_0000_0000_0000,
        7,
        "unique-local (fc00::/7)",
    ),
    (0xfe80_0000_0000_0000_0000_0000_0000_0000, 10, "link-local"),
    (
        0xfec0_0000_0000_0000_0000_0000_0000_0000,
        10,
        "deprecated site-local (fec0::/10)",
    ),
    (0xff00_0000_0000_0000_0000_0000_0000_0000, 8, "multicast"),
];

fn prefix_match_u32(value: u32, network: u32, prefix: u8) -> bool {
    let mask = prefix_mask_u32(prefix);
    value & mask == network & mask
}

fn prefix_mask_u32(prefix: u8) -> u32 {
    u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0)
}

fn prefix_match_u128(value: u128, network: u128, prefix: u8) -> bool {
    let mask = u128::MAX.checked_shl(u32::from(128 - prefix)).unwrap_or(0);
    value & mask == network & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ceiling(value: serde_json::Value) -> NetworkCeiling {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn every_special_use_fixture_is_denied_with_its_stable_reason() {
        for &(address, expected) in SPECIAL_USE_FIXTURES {
            let address: IpAddr = address.parse().unwrap();
            assert_eq!(special_use_reason(&address), Some(expected), "{address}");
        }
    }

    #[test]
    fn public_fixture_stays_public() {
        for &address in PUBLIC_UNICAST_FIXTURES {
            let address: IpAddr = address.parse().unwrap();
            assert!(is_public_unicast(&address), "{address}");
        }
    }

    #[test]
    fn mapped_public_ipv4_is_classified_as_public_ipv4() {
        let mapped: IpAddr = "::ffff:8.8.8.8".parse().unwrap();
        assert!(is_public_unicast(&mapped));
    }

    #[test]
    fn network_subset_uses_semantic_host_cidr_and_port_containment() {
        let sealed = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[
                {"protocol":"tls","host":"*.example.com","ports":[443]},
                {"protocol":"tcp","cidr":"8.8.8.0/24","ports":[80,443]}
            ]
        }));
        let narrowed = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[
                {"protocol":"tls","host":"api.example.com","ports":[443]},
                {"protocol":"tcp","cidr":"8.8.8.8/32","ports":[443]}
            ]
        }));
        assert!(network_ceiling_is_subset(&narrowed, &sealed));

        let root_host = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[{"protocol":"tls","host":"example.com","ports":[443]}]
        }));
        assert!(!network_ceiling_is_subset(&root_host, &sealed));

        let wider_port = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[{"protocol":"tcp","cidr":"8.8.8.8/32","ports":[22]}]
        }));
        assert!(!network_ceiling_is_subset(&wider_port, &sealed));
    }

    #[test]
    fn public_ceiling_does_not_cover_special_use_allowlists() {
        let public = ceiling(serde_json::json!({"kind":"public"}));
        let ordinary = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[{"protocol":"tcp","cidr":"8.8.8.0/24","ports":[443]}]
        }));
        let private = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[{"protocol":"tcp","cidr":"10.0.0.0/8","ports":[443]}]
        }));
        let malformed = ceiling(serde_json::json!({
            "kind":"allowlist",
            "destinations":[{"protocol":"tcp","cidr":"8.8.8.1/24","ports":[443]}]
        }));
        assert!(network_ceiling_is_subset(&ordinary, &public));
        assert!(!network_ceiling_is_subset(&private, &public));
        assert!(!network_ceiling_is_subset(&malformed, &public));
        assert!(network_ceiling_is_subset(&NetworkCeiling::None, &private));
    }
}
