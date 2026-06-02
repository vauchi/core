// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `HttpTransport::escrow` — the core-owned relay escrow send
//! wrapper (ADR-049 Phase 1 T1). Drives the in-process `MockRelay` over
//! the direct path; asserts the request envelope (escrow_action rename,
//! hex/base64 fields) and the parsed `EscrowResponse`, including the
//! relay-error path.

#![cfg(feature = "network-http")]

use vauchi_core::network::NetworkError;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};
use vauchi_protocol::escrow::{EscrowMessage, EscrowResponse};

use crate::common::mock_relay::{CannedResponse, MockRelay};

fn transport_pointing_at(mock: &MockRelay) -> HttpTransport {
    HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000))
}

fn canned(resp: &EscrowResponse) -> CannedResponse {
    CannedResponse::ok_json(serde_json::to_vec(resp).expect("serialize EscrowResponse"))
}

#[test]
fn put_sends_escrow_action_envelope_and_parses_stored() {
    let mock = MockRelay::start();
    mock.queue("escrow", canned(&EscrowResponse::Stored));

    let transport = transport_pointing_at(&mock);
    let result = transport
        .escrow(&EscrowMessage::Put {
            gate_hash: "aa".repeat(32),
            slot_hash: "bb".repeat(32),
            blob: "Zm9v".to_string(),
            ttl_seconds: 600,
        })
        .expect("escrow put succeeds");

    assert_eq!(result, EscrowResponse::Stored);

    let req = mock.last_received();
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/v2/escrow");
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("body is JSON");
    // The EscrowMessage serde tag `action` is renamed to `escrow_action`
    // so it does not shadow the relay's outer routing `action`.
    assert_eq!(body["escrow_action"], "Put");
    assert!(
        body.get("action").is_none(),
        "outer action must not leak the variant tag"
    );
    assert_eq!(body["gate_hash"], "aa".repeat(32));
    assert_eq!(body["slot_hash"], "bb".repeat(32));
    assert_eq!(body["blob"], "Zm9v");
    assert_eq!(body["ttl_seconds"], 600);
}

#[test]
fn get_parses_blob_response() {
    let mock = MockRelay::start();
    mock.queue(
        "escrow",
        canned(&EscrowResponse::Blob {
            blob: "Zm9vYmFy".to_string(),
        }),
    );

    let transport = transport_pointing_at(&mock);
    let result = transport
        .escrow(&EscrowMessage::Get {
            gate_hash: "cc".repeat(32),
            slot_hash: "dd".repeat(32),
        })
        .expect("escrow get succeeds");

    assert_eq!(
        result,
        EscrowResponse::Blob {
            blob: "Zm9vYmFy".to_string()
        }
    );
    let body: serde_json::Value =
        serde_json::from_slice(&mock.last_received().body).expect("body is JSON");
    assert_eq!(body["escrow_action"], "Get");
}

#[test]
fn count_parses_count_response() {
    let mock = MockRelay::start();
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 2 }));

    let transport = transport_pointing_at(&mock);
    let result = transport
        .escrow(&EscrowMessage::Count {
            gate_hash: "ee".repeat(32),
        })
        .expect("escrow count succeeds");

    assert_eq!(result, EscrowResponse::Count { count: 2 });
}

#[test]
fn relay_error_status_maps_to_network_error() {
    let mock = MockRelay::start();
    mock.queue(
        "escrow",
        CannedResponse::ok_json(
            serde_json::to_vec(&serde_json::json!({
                "status": "error",
                "error": "rate limit exceeded"
            }))
            .unwrap(),
        ),
    );

    let transport = transport_pointing_at(&mock);
    let err = transport
        .escrow(&EscrowMessage::Count {
            gate_hash: "ff".repeat(32),
        })
        .expect_err("relay error must surface as Err, not a parsed response");

    match err {
        NetworkError::InvalidMessage(detail) => assert_eq!(detail, "rate limit exceeded"),
        other => panic!("expected InvalidMessage, got {other:?}"),
    }
}
