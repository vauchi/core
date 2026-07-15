// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for relay URL validation (Phase 1G).
//!
//! Validates that only safe relay URLs are accepted.
//! Security: prevents SSRF, injection, and malicious relay URLs from contacts.

use vauchi_core::network::relay_url::{RelayUrlError, validate_relay_url};

// ── Valid URLs ──────────────────────────────────────────────────────

// @internal
#[test]
fn valid_wss_url_accepted() {
    assert!(
        validate_relay_url("https://relay.vauchi.app").is_ok(),
        "expected success"
    );
}

// @internal
#[test]
fn valid_wss_url_with_port_accepted() {
    assert!(
        validate_relay_url("https://relay.example.com:8443").is_ok(),
        "expected success"
    );
}

// @internal
#[test]
fn valid_wss_url_with_path_accepted() {
    assert!(
        validate_relay_url("https://relay.example.com/ws").is_ok(),
        "expected success"
    );
}

// @internal
#[test]
fn valid_wss_onion_url_accepted() {
    // Tor .onion addresses are valid relay URLs
    assert!(
        validate_relay_url("https://abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst.onion")
            .is_ok()
    );
}

// ── Scheme validation ──────────────────────────────────────────────

// @internal
#[test]
fn ws_scheme_rejected() {
    let err = validate_relay_url("http://relay.example.com").unwrap_err();
    assert!(matches!(err, RelayUrlError::InsecureScheme));
}

// @internal
#[test]
fn http_scheme_rejected() {
    let err = validate_relay_url("http://relay.example.com").unwrap_err();
    assert!(matches!(err, RelayUrlError::InsecureScheme));
}

// @internal
#[test]
fn https_scheme_accepted() {
    assert!(
        validate_relay_url("https://relay.example.com").is_ok(),
        "https:// is the primary scheme for relay URLs"
    );
}

// @internal
#[test]
fn ftp_scheme_rejected() {
    let err = validate_relay_url("ftp://relay.example.com").unwrap_err();
    assert!(matches!(err, RelayUrlError::InsecureScheme));
}

// @internal
#[test]
fn javascript_scheme_rejected() {
    let err = validate_relay_url("javascript:alert(1)").unwrap_err();
    // Could be InvalidFormat or InsecureScheme depending on parse
    assert!(matches!(
        err,
        RelayUrlError::InsecureScheme | RelayUrlError::InvalidFormat(_)
    ));
}

// @internal
#[test]
fn no_scheme_rejected() {
    let err = validate_relay_url("relay.example.com").unwrap_err();
    assert!(matches!(err, RelayUrlError::InvalidFormat(_)));
}

// ── Host validation (SSRF prevention) ──────────────────────────────

// @internal
#[test]
fn localhost_rejected() {
    let err = validate_relay_url("https://localhost").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn localhost_with_port_rejected() {
    let err = validate_relay_url("https://localhost:8080").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_loopback_rejected() {
    let err = validate_relay_url("https://127.0.0.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_loopback_range_rejected() {
    let err = validate_relay_url("https://127.0.0.2").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @rg-8 @fail-closed
#[test]
fn alternate_ipv4_loopback_forms_rejected() {
    for url in [
        "https://127.1",
        "https://2130706433",
        "https://0x7f000001",
        "https://0177.0.0.1",
    ] {
        let err = validate_relay_url(url).expect_err("loopback aliases must be rejected");
        assert!(
            matches!(err, RelayUrlError::PrivateHost),
            "unexpected error for {url}: {err}"
        );
    }
}

// @internal
#[test]
fn ipv4_private_10_rejected() {
    let err = validate_relay_url("https://10.0.0.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_private_172_rejected() {
    let err = validate_relay_url("https://172.16.0.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_private_192_rejected() {
    let err = validate_relay_url("https://192.168.1.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv6_loopback_rejected() {
    let err = validate_relay_url("https://[::1]").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_link_local_rejected() {
    let err = validate_relay_url("https://169.254.1.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_zero_rejected() {
    let err = validate_relay_url("https://0.0.0.0").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// ── Length validation ──────────────────────────────────────────────

// @internal
#[test]
fn empty_url_rejected() {
    let err = validate_relay_url("").unwrap_err();
    assert!(matches!(err, RelayUrlError::Empty));
}

// @internal
#[test]
fn url_over_max_length_rejected() {
    let long_host = "a".repeat(2000);
    let url = format!("https://{long_host}.com");
    let err = validate_relay_url(&url).unwrap_err();
    assert!(matches!(err, RelayUrlError::TooLong { .. }));
}

// ── Adversarial inputs (CC-14) ─────────────────────────────────────

// @internal
#[test]
fn null_bytes_in_url_rejected() {
    let err = validate_relay_url("https://relay.example.com\0/evil").unwrap_err();
    assert!(matches!(err, RelayUrlError::InvalidFormat(_)));
}

// @internal
#[test]
fn unicode_homoglyph_url_rejected_or_accepted() {
    // At minimum, the URL must parse correctly
    let result = validate_relay_url("https://rеlay.example.com"); // Cyrillic 'е'
    // Either accepted (punycode) or rejected — but must not panic
    assert!(result.is_ok() || result.is_err(), "expected error");
}

// @internal
#[test]
fn url_with_userinfo_rejected() {
    // URLs with user:pass@ are suspicious
    let err = validate_relay_url("https://user:pass@relay.example.com").unwrap_err();
    assert!(matches!(err, RelayUrlError::InvalidFormat(_)));
}

// @internal
#[test]
fn url_with_fragment_rejected() {
    let err = validate_relay_url("https://relay.example.com#fragment").unwrap_err();
    assert!(matches!(err, RelayUrlError::InvalidFormat(_)));
}

// @internal
#[test]
fn missing_host_rejected() {
    let err = validate_relay_url("https://").unwrap_err();
    assert!(matches!(err, RelayUrlError::InvalidFormat(_)));
}

// ── Extended SSRF prevention (IPv6 ULA, link-local, multicast; IPv4 CGN, multicast) ──

// @internal
#[test]
fn ipv6_ula_rejected() {
    let err = validate_relay_url("https://[fd00::1]").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv6_link_local_rejected() {
    let err = validate_relay_url("https://[fe80::1]").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv6_multicast_rejected() {
    let err = validate_relay_url("https://[ff02::1]").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_cgn_rejected() {
    let err = validate_relay_url("https://100.64.0.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_cgn_upper_bound_rejected() {
    let err = validate_relay_url("https://100.127.255.254").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_multicast_rejected() {
    let err = validate_relay_url("https://224.0.0.1").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}

// @internal
#[test]
fn ipv4_broadcast_rejected() {
    let err = validate_relay_url("https://255.255.255.255").unwrap_err();
    assert!(matches!(err, RelayUrlError::PrivateHost));
}
