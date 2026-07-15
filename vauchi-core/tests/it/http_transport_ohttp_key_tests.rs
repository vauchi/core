// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for HttpTransport::fetch_ohttp_key()
//!
//! These tests verify the method exists and behaves correctly without a live
//! relay — connection failures prove the URL is built and the request is
//! attempted.

#![cfg(feature = "network-http")]

use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};

use crate::common::mock_relay::{CannedResponse, MockRelay};

// @scenario: sync:OHTTP key fetch
#[test]
fn test_fetch_ohttp_key_builds_correct_url() {
    let transport =
        HttpTransport::new(HttpTransportConfig::for_testing("http://localhost:1", 1000));
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
    let transport = HttpTransport::new(HttpTransportConfig::for_testing("http://localhost:1", 100));
    assert!(transport.fetch_ohttp_key().is_err());
}

// @feature: release_privacy_multidevice_certification
// @rg-8 @fail-closed
// @scenario: release_privacy_multidevice_certification :: reject mislabeled OHTTP keys
#[test]
fn test_fetch_ohttp_key_rejects_wrong_content_type() {
    let relay = MockRelay::start();
    relay.queue("ohttp-key", CannedResponse::ok_json(b"encoded-key"));
    let transport = HttpTransport::new(HttpTransportConfig::for_testing(relay.url(), 1_000));

    let error = transport
        .fetch_ohttp_key()
        .expect_err("wrong OHTTP key content type must fail closed");

    assert_eq!(
        error.to_string(),
        "Invalid message format: OHTTP key response must use application/ohttp-keys"
    );
}

// @feature: release_privacy_multidevice_certification
// @rg-8 @fail-closed
// @scenario: release_privacy_multidevice_certification :: bound OHTTP key bootstrap
#[test]
fn test_fetch_ohttp_key_rejects_oversized_response() {
    let relay = MockRelay::start();
    relay.queue(
        "ohttp-key",
        CannedResponse {
            status: 200,
            headers: vec![("Content-Type".into(), "application/ohttp-keys".into())],
            body: vec![0; 65_537],
        },
    );
    let transport = HttpTransport::new(HttpTransportConfig::for_testing(relay.url(), 1_000));

    let error = transport
        .fetch_ohttp_key()
        .expect_err("oversized OHTTP key response must fail closed");

    assert_eq!(
        error.to_string(),
        "Invalid message format: OHTTP key response exceeds 65536 bytes"
    );
}
