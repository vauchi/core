// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the G3 validation UniFFI exports.

use vauchi_platform::{
    mobile_is_valid_email, mobile_is_valid_phone, mobile_is_valid_relay_url, passcode_max_length,
    passcode_min_length, password_min_length,
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
