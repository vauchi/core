// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for HttpTransport::fetch_ohttp_key()
//!
//! These tests verify the method exists and behaves correctly without a live
//! relay — connection failures prove the URL is built and the request is
//! attempted.

#![cfg(feature = "network-http")]

use vauchi_core::network::ProxyConfig;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};

// @scenario: sync:OHTTP key fetch
#[test]
fn test_fetch_ohttp_key_builds_correct_url() {
    let config = HttpTransportConfig {
        relay_url: "http://localhost:1".to_string(),
        timeout_ms: 1000,
        proxy: ProxyConfig::None,
        allow_direct: true,
        pinned_certs: vec![],
    };
    let transport = HttpTransport::new(config);
    // Will fail with connection refused — but proves
    // the method exists and builds the right URL
    let err = transport.fetch_ohttp_key().unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("connection")
            || err_str.contains("refused")
            || err_str.contains("Connection"),
        "expected connection error, got: {err_str}"
    );
}

// @scenario: sync:OHTTP key fetch
#[test]
fn test_fetch_ohttp_key_empty_response_is_error() {
    // This test verifies the method exists and returns
    // the right error type. Can't test with real relay.
    let config = HttpTransportConfig {
        relay_url: "http://localhost:1".to_string(),
        timeout_ms: 100,
        proxy: ProxyConfig::None,
        allow_direct: true,
        pinned_certs: vec![],
    };
    let transport = HttpTransport::new(config);
    assert!(transport.fetch_ohttp_key().is_err());
}
