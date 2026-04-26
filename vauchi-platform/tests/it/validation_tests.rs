// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the G3 validation UniFFI exports.

use vauchi_platform::{
    mobile_is_valid_email, mobile_is_valid_pem_certificate, mobile_is_valid_phone,
    mobile_is_valid_relay_url, passcode_max_length, passcode_min_length, password_min_length,
    recovery_claim_min_input_length, recovery_public_key_hex_length,
};

// @internal
#[test]
fn passcode_constants_match_per_platform_defaults() {
    assert_eq!(passcode_min_length(), 4);
    assert_eq!(passcode_max_length(), 64);
    assert_eq!(password_min_length(), 8);
}

// @internal
#[test]
fn email_validator_handles_intranet_addresses() {
    assert!(mobile_is_valid_email("alice@example.com".into()));
    assert!(mobile_is_valid_email("alice@localhost".into()));
    assert!(!mobile_is_valid_email("not-an-email".into()));
    assert!(!mobile_is_valid_email("@nolocal.com".into()));
    assert!(!mobile_is_valid_email("nodomain@".into()));
}

// @internal
#[test]
fn phone_validator_strips_punctuation_then_counts_digits() {
    assert!(mobile_is_valid_phone("+1 (555) 123-4567".into()));
    assert!(!mobile_is_valid_phone("12".into()));
    assert!(!mobile_is_valid_phone("not-a-phone".into()));
}

// @internal
#[test]
fn relay_url_validator_rejects_non_loopback_http() {
    assert!(mobile_is_valid_relay_url("https://relay.vauchi.app".into()));
    assert!(mobile_is_valid_relay_url("http://localhost:8080".into()));
    assert!(!mobile_is_valid_relay_url("http://example.com".into()));
    assert!(!mobile_is_valid_relay_url("ftp://example.com".into()));
}

// @internal
#[test]
fn pem_validator_accepts_well_formed_certificate() {
    // The body content is opaque to the validator — it only checks
    // BEGIN/END markers. Real certs are validated downstream by the
    // rustls verifier; using opaque base64-shaped placeholder bytes
    // keeps this test independent of cert generation tooling.
    let pem = "-----BEGIN CERTIFICATE-----\nbase64body\n-----END CERTIFICATE-----";
    assert!(mobile_is_valid_pem_certificate(pem.into()));
}

// @internal
#[test]
fn pem_validator_trims_surrounding_whitespace() {
    let pem = "  \n\t-----BEGIN CERTIFICATE-----\nbody\n-----END CERTIFICATE-----  \n";
    assert!(mobile_is_valid_pem_certificate(pem.into()));
}

// @internal
#[test]
fn pem_validator_rejects_missing_begin_marker() {
    let pem = "body\n-----END CERTIFICATE-----";
    assert!(!mobile_is_valid_pem_certificate(pem.into()));
}

// @internal
#[test]
fn pem_validator_rejects_missing_end_marker() {
    let pem = "-----BEGIN CERTIFICATE-----\nbody";
    assert!(!mobile_is_valid_pem_certificate(pem.into()));
}

// @internal
#[test]
fn pem_validator_rejects_wrong_label() {
    // Only X.509 certificates are accepted; PRIVATE KEY and other PEM
    // labels are rejected so frontends can give the right user hint.
    let pem = "-----BEGIN PRIVATE KEY-----\nbody\n-----END PRIVATE KEY-----";
    assert!(!mobile_is_valid_pem_certificate(pem.into()));
}

// @internal
#[test]
fn pem_validator_rejects_empty_and_whitespace_only_input() {
    assert!(!mobile_is_valid_pem_certificate(String::new()));
    assert!(!mobile_is_valid_pem_certificate("   \n\t  ".into()));
}

// @internal
#[test]
fn pem_validator_rejects_garbage() {
    assert!(!mobile_is_valid_pem_certificate("not a certificate".into()));
}

// @internal
#[test]
fn recovery_public_key_hex_length_is_64() {
    // 32 bytes (Ed25519 public key) × 2 hex characters = 64.
    assert_eq!(recovery_public_key_hex_length(), 64);
}

// @internal
#[test]
fn recovery_claim_min_input_length_is_20() {
    // The 20-character heuristic that core's `recovery_help` engine
    // also uses to gate the "Verify Claim" button.
    assert_eq!(recovery_claim_min_input_length(), 20);
}
