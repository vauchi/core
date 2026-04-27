// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Endpoint-level smoke tests for `HttpTransport` request construction.
//!
//! These pin that each public relay endpoint is callable, marshals its
//! arguments into a V2 request struct, hands it to `post_action`, and
//! surfaces a `NetworkError` when the relay is unreachable. They cover
//! the per-method request-construction code paths in `http_transport.rs`
//! that were 0% covered before — the post-network response-handling
//! branches still need a mock relay (see follow-up note in the
//! coverage-gap problem record).

#![cfg(feature = "network-http")]

use vauchi_core::network::NetworkError;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};

fn unreachable_transport() -> HttpTransport {
    // Port 1 is reserved (TCPMUX) — connect attempts fail fast everywhere.
    HttpTransport::new(HttpTransportConfig::for_testing("http://127.0.0.1:1", 50))
}

fn assert_is_network_error<T: std::fmt::Debug>(r: Result<T, NetworkError>, action: &str) {
    // We accept any network-layer failure shape. The point of these tests
    // is request construction reaching the network call, not the exact
    // shape of failure when there is no relay listening.
    match r {
        Err(_) => {}
        Ok(v) => panic!("{action}: unexpectedly succeeded against unreachable relay: {v:?}"),
    }
}

// @internal
#[test]
fn acknowledge_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.acknowledge("recipient-1", "blob-abc");
    assert_is_network_error(r, "acknowledge");
}

// @internal
#[test]
fn purge_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.purge(
        "recipient-1",
        "AAAA…public_key",
        "token-xyz",
        "sig-bytes-b64",
        1_700_000_000,
    );
    assert_is_network_error(r, "purge");
}

// @internal
#[test]
fn recovery_store_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.recovery_store("deadbeef".repeat(8).as_str(), "cHJvb2YK");
    assert_is_network_error(r, "recovery_store");
}

// @internal
#[test]
fn guardian_store_constructs_request_and_fails_at_network() {
    use vauchi_protocol::v2::V2GuardianEntry;

    let t = unreachable_transport();
    let entries = vec![V2GuardianEntry {
        data: "ZW50cnktYg==".to_string(),
    }];
    let r = t.guardian_store("guardian-hash-1", entries);
    assert_is_network_error(r, "guardian_store");
}

// @internal
#[test]
fn guardian_query_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.guardian_query("guardian-hash-2");
    assert_is_network_error(r, "guardian_query");
}

// @internal
#[test]
fn guardian_delete_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.guardian_delete("guardian-hash-3");
    assert_is_network_error(r, "guardian_delete");
}

// @internal
#[test]
fn exchange_offer_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.exchange_offer("cGF5bG9hZA==", Some(300));
    assert_is_network_error(r, "exchange_offer");
}

// @internal
#[test]
fn exchange_claim_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.exchange_claim("123456", "cmVzcG9uc2U=");
    assert_is_network_error(r, "exchange_claim");
}

// @internal
#[test]
fn exchange_complete_constructs_request_and_fails_at_network() {
    let t = unreachable_transport();
    let r = t.exchange_complete("123456");
    assert_is_network_error(r, "exchange_complete");
}

// @internal
#[test]
fn empty_argument_strings_still_marshal_to_request() {
    // Sanity: empty-string args don't panic during marshaling — the
    // relay would reject them, but the client must hand a well-formed
    // request to the network layer.
    let t = unreachable_transport();
    let r = t.acknowledge("", "");
    assert_is_network_error(r, "acknowledge with empty args");
    let r = t.guardian_query("");
    assert_is_network_error(r, "guardian_query with empty hash");
}
