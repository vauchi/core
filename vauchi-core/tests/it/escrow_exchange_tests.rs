// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `Vauchi::escrow_exchange` — the core-owned relay escrow
//! round-trip primitive (ADR-049 Phase 1 T2a). Drives a real `Vauchi`
//! (no OHTTP key → direct path) against the in-process `MockRelay`. Each
//! test gets an isolated temp storage dir so parallel runs don't collide
//! on the default `./vauchi_data` path.

#![cfg(feature = "network-http")]

use vauchi_core::VauchiError;
use vauchi_core::api::vauchi::VauchiBuilder;
use vauchi_protocol::escrow::{EscrowMessage, EscrowResponse};

use crate::common::mock_relay::{CannedResponse, MockRelay};

fn vauchi_pointing_at(mock: &MockRelay) -> (vauchi_core::Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("vauchi.db");
    let mut wb = VauchiBuilder::new()
        .relay_url(mock.url())
        .storage_path(db_path.to_str().expect("utf-8 path"))
        .build()
        .expect("build vauchi");
    wb.create_identity("Alice").expect("create identity");
    (wb, dir)
}

fn canned(resp: &EscrowResponse) -> CannedResponse {
    CannedResponse::ok_json(serde_json::to_vec(resp).expect("serialize EscrowResponse"))
}

#[test]
fn escrow_exchange_round_trips_a_put_over_the_relay_transport() {
    let mock = MockRelay::start();
    mock.queue("escrow", canned(&EscrowResponse::Stored));

    let (wb, _dir) = vauchi_pointing_at(&mock);
    let resp = wb
        .escrow_exchange(&EscrowMessage::Put {
            gate_hash: "aa".repeat(32),
            slot_hash: "bb".repeat(32),
            blob: "Zm9v".to_string(),
            ttl_seconds: 600,
        })
        .expect("escrow exchange succeeds");

    assert_eq!(resp, EscrowResponse::Stored);

    let req = mock.last_received();
    assert_eq!(req.path, "/v2/escrow");
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("body is JSON");
    assert_eq!(body["escrow_action"], "Put");
    assert_eq!(body["gate_hash"], "aa".repeat(32));
}

#[test]
fn escrow_exchange_parses_a_blob_retrieve() {
    let mock = MockRelay::start();
    mock.queue(
        "escrow",
        canned(&EscrowResponse::Blob {
            blob: "Zm9vYmFy".to_string(),
        }),
    );

    let (wb, _dir) = vauchi_pointing_at(&mock);
    let resp = wb
        .escrow_exchange(&EscrowMessage::Get {
            gate_hash: "cc".repeat(32),
            slot_hash: "dd".repeat(32),
        })
        .expect("escrow exchange succeeds");

    assert_eq!(
        resp,
        EscrowResponse::Blob {
            blob: "Zm9vYmFy".to_string()
        }
    );
}

#[test]
fn escrow_exchange_maps_relay_error_to_vauchi_network_error() {
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

    let (wb, _dir) = vauchi_pointing_at(&mock);
    let err = wb
        .escrow_exchange(&EscrowMessage::Count {
            gate_hash: "ee".repeat(32),
        })
        .expect_err("relay error must surface as Err");

    assert!(
        matches!(err, VauchiError::Network(_)),
        "expected VauchiError::Network, got {err:?}"
    );
}
