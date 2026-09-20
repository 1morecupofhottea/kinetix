//! Shared destination-address policy.
//!
//! A single source of truth for which IP literals a Kinetix outbound path is
//! allowed to reach. Both the provider SSRF guard and the plugin host-mediated
//! HTTP guard call into this module so the two can never drift apart.

/// Whether an IP literal falls in a blocked private/link-local/metadata range
/// (NFR-3.9). Covers loopback, RFC1918, link-local, unspecified, broadcast,
/// multicast, `0.0.0.0/8`, CGNAT (`100.64/10`), and benchmarking (`198.18/15`)
/// IPv4 ranges, plus loopback, unspecified, multicast, ULA (`fc00::/7`), and
/// link-local (`fe80::/10`) IPv6 ranges. IPv4-mapped IPv6 addresses are
/// unwrapped and re-checked as IPv4.
pub fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            let shared_address_space = octets[0] == 100 && (64..=127).contains(&octets[1]);
            let benchmarking = octets[0] == 198 && matches!(octets[1], 18 | 19);
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || octets[0] == 0
                || shared_address_space
                || benchmarking
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ip(std::net::IpAddr::V4(mapped));
            }
            let first = v6.segments()[0];
            let unique_local = first & 0xfe00 == 0xfc00;
            let link_local = first & 0xffc0 == 0xfe80;
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || unique_local
                || link_local
        }
    }
}

/// Whether a hostname names a local/internal destination without resolving DNS.
/// Catches the literal names (`localhost`, `.localhost`, `.internal`,
/// `metadata.google.internal`) and any IP literal, so a destination cannot dodge
/// the address policy by spelling it as a name.
pub fn is_blocked_host(host: &str) -> bool {
    let lower = host.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".internal") {
        return true;
    }
    if lower == "metadata.google.internal" {
        return true;
    }
    if let Ok(ip) = lower.parse::<std::net::IpAddr>() {
        return is_blocked_ip(ip);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse::<Ipv4Addr>().unwrap())
    }
    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(s.parse::<Ipv6Addr>().unwrap())
    }

    #[test]
    fn blocks_private_and_reserved_ranges() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "100.64.0.1",
            "198.18.0.1",
        ] {
            assert!(is_blocked_ip(v4(ip)), "{ip} should be blocked");
        }
        for ip in [
            "::1",
            "::",
            "ff02::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_blocked_ip(v6(ip)), "{ip} should be blocked");
        }
    }

    #[test]
    fn allows_public_ranges() {
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(!is_blocked_ip(v4(ip)), "{ip} should be allowed");
        }
        for ip in ["2606:4700:4700::1111", "::ffff:8.8.8.8"] {
            assert!(!is_blocked_ip(v6(ip)), "{ip} should be allowed");
        }
    }

    #[test]
    fn blocks_local_names_and_literals() {
        assert!(is_blocked_host("localhost"));
        assert!(is_blocked_host("db.internal"));
        assert!(is_blocked_host("metadata.google.internal"));
        assert!(is_blocked_host("127.0.0.1"));
        assert!(!is_blocked_host("api.example.com"));
        assert!(!is_blocked_host("8.8.8.8"));
    }
}
