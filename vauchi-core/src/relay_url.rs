// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay URL Validation
//!
//! Validates relay URLs to prevent SSRF, injection, and abuse from
//! malicious contact exchange payloads.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use thiserror::Error;
use url::{Host, Url};

/// Maximum allowed relay URL length in bytes.
const MAX_URL_LENGTH: usize = 1024;

/// Errors from relay URL validation.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelayUrlError {
    #[error("Relay URL is empty")]
    Empty,

    #[error("Relay URL exceeds maximum length of {max} bytes (got {actual})")]
    TooLong { max: usize, actual: usize },

    #[error("Relay URL must use https:// scheme")]
    InsecureScheme,

    #[error("Relay URL points to private/loopback host")]
    PrivateHost,

    #[error("Invalid relay URL format: {0}")]
    InvalidFormat(String),
}

/// Validates a relay URL for safety.
///
/// Checks:
/// - Non-empty and within length limit
/// - https:// scheme only (no http://, etc.)
/// - No private/loopback/link-local hosts (SSRF prevention)
/// - No userinfo (user:pass@)
/// - No fragment (#)
/// - No null bytes
pub fn validate_relay_url(url: &str) -> Result<(), RelayUrlError> {
    // Length checks
    if url.is_empty() {
        return Err(RelayUrlError::Empty);
    }
    if url.len() > MAX_URL_LENGTH {
        return Err(RelayUrlError::TooLong {
            max: MAX_URL_LENGTH,
            actual: url.len(),
        });
    }

    // Null byte check (before parsing to prevent C-string truncation attacks)
    if url.contains('\0') {
        return Err(RelayUrlError::InvalidFormat(
            "URL contains null bytes".to_string(),
        ));
    }

    let parsed =
        Url::parse(url).map_err(|error| RelayUrlError::InvalidFormat(error.to_string()))?;

    if parsed.scheme() != "https" {
        return Err(RelayUrlError::InsecureScheme);
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(RelayUrlError::InvalidFormat(
            "URL must not contain userinfo".to_string(),
        ));
    }

    if parsed.fragment().is_some() {
        return Err(RelayUrlError::InvalidFormat(
            "URL must not contain a fragment".to_string(),
        ));
    }

    let host = parsed
        .host()
        .ok_or_else(|| RelayUrlError::InvalidFormat("URL has no host".to_string()))?;
    check_host_not_private(host)?;

    Ok(())
}

/// Rejects localhost, loopback, private RFC1918, link-local, and zero addresses.
fn check_host_not_private(host: Host<&str>) -> Result<(), RelayUrlError> {
    match host {
        Host::Domain(domain) if domain == "localhost" || domain.ends_with(".localhost") => {
            return Err(RelayUrlError::PrivateHost);
        }
        Host::Ipv4(address) if is_private_ip(&IpAddr::V4(address)) => {
            return Err(RelayUrlError::PrivateHost);
        }
        Host::Ipv6(address) if is_private_ip(&IpAddr::V6(address)) => {
            return Err(RelayUrlError::PrivateHost);
        }
        _ => {}
    }

    Ok(())
}

/// Returns true if the IP address is private, loopback, link-local, or unspecified.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()           // 127.0.0.0/8
                || v4.is_private()     // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()  // 169.254.0.0/16
                || v4.is_unspecified() // 0.0.0.0
                || v4.is_broadcast()   // 255.255.255.255
                || v4.is_multicast()   // 224.0.0.0/4 (RFC 5771)
                || is_cgn_ip(v4) // 100.64.0.0/10 (RFC 6598 Carrier-Grade NAT)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()           // ::1
                || v6.is_unspecified() // ::
                || v6.is_multicast()   // ff00::/8 (RFC 3513)
                || is_ipv6_ula(v6)     // fc00::/7 (RFC 4193 Unique Local)
                || is_ipv6_link_local(v6) // fe80::/10 (RFC 4291)
                // IPv4-mapped private addresses (::ffff:x.x.x.x)
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                        || v4.is_multicast()
                        || is_cgn_ip(&v4)
                })
        }
    }
}

/// Returns true if IPv4 address is in the Carrier-Grade NAT range (100.64.0.0/10, RFC 6598).
fn is_cgn_ip(v4: &Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// Returns true if IPv6 address is Unique Local (fc00::/7, RFC 4193).
fn is_ipv6_ula(v6: &Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// Returns true if IPv6 address is link-local (fe80::/10, RFC 4291).
fn is_ipv6_link_local(v6: &Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

/// Converts a WebSocket relay URL to its HTTP equivalent.
///
/// - `https://` → `https://`
/// - `http://` → `http://`
// INLINE_TEST_REQUIRED: tests access private is_private_ip, is_cgn_ip, is_ipv6_ula helpers
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_private_ip_loopback_v4() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"127.0.0.2".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_rfc1918() {
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_public() {
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn is_private_ip_loopback_v6() {
        assert!(is_private_ip(&"::1".parse().unwrap()));
    }
}
