// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `Vauchi::escrow_exchange` — the core-owned relay escrow
//! round-trip primitive (ADR-049 Phase 1 T2a). Drives a real `Vauchi`
//! through the explicit testing-only direct seam against the in-process
//! `MockRelay`. Production action transports remain fail-closed. Each test
//! gets an isolated temp storage dir so parallel runs don't collide.

#![cfg(feature = "network-http")]

use vauchi_core::api::VauchiConfig;
use vauchi_core::api::vauchi::VauchiBuilder;
use vauchi_core::{Command, Event, VauchiError};
use vauchi_protocol::escrow::{EscrowMessage, EscrowResponse};

use crate::common::mock_relay::{CannedResponse, MockRelay};

fn vauchi_pointing_at(mock: &MockRelay) -> (vauchi_core::Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("vauchi.db");
    let mut config = VauchiConfig::with_storage_path(&db_path).with_relay_url(mock.url());
    config.ohttp.allow_direct = true;
    let mut wb = VauchiBuilder::new()
        .config(config)
        .build()
        .expect("build vauchi");
    wb.create_identity("Alice").expect("create identity");
    (wb, dir)
}

fn canned(resp: &EscrowResponse) -> CannedResponse {
    CannedResponse::ok_json(serde_json::to_vec(resp).expect("serialize EscrowResponse"))
}

// @internal
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

// @internal
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

// @internal
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

// ── run_escrow_command: Command -> relay -> Event ────────────────────

// @internal
#[test]
fn run_escrow_command_deposit_returns_no_event() {
    let mock = MockRelay::start();
    mock.queue("escrow", canned(&EscrowResponse::Stored));
    let (wb, _dir) = vauchi_pointing_at(&mock);

    let event = wb.run_escrow_command(&Command::RelayEscrowDeposit {
        gate_hash: vec![0xABu8; 32],
        slot_hash: vec![0xCDu8; 32],
        encrypted_card: vec![1, 2, 3],
        ttl_seconds: 600,
    });
    assert_eq!(event, None, "a stored deposit produces no machine event");
}

// @internal
#[test]
fn run_escrow_command_check_full_gate_yields_ready_with_gate_hash() {
    let mock = MockRelay::start();
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 2 }));
    let (wb, _dir) = vauchi_pointing_at(&mock);

    let event = wb.run_escrow_command(&Command::RelayEscrowCheck {
        gate_hash: vec![0xABu8; 32],
        suggested_interval_ms: 0,
    });
    assert_eq!(
        event,
        Some(Event::RelayEscrowReady {
            gate_hash: vec![0xABu8; 32]
        })
    );
}

// @internal
#[test]
fn run_escrow_command_check_partial_gate_yields_no_event() {
    let mock = MockRelay::start();
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 1 }));
    let (wb, _dir) = vauchi_pointing_at(&mock);

    let event = wb.run_escrow_command(&Command::RelayEscrowCheck {
        gate_hash: vec![0xABu8; 32],
        suggested_interval_ms: 0,
    });
    assert_eq!(event, None, "a partially-filled gate keeps polling");
}

// @internal
#[test]
fn run_escrow_command_retrieve_yields_blob_received_with_decoded_bytes() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let mock = MockRelay::start();
    mock.queue(
        "escrow",
        canned(&EscrowResponse::Blob {
            blob: URL_SAFE_NO_PAD.encode(b"peer-card-bytes"),
        }),
    );
    let (wb, _dir) = vauchi_pointing_at(&mock);

    let event = wb.run_escrow_command(&Command::RelayEscrowRetrieve {
        gate_hash: vec![0xABu8; 32],
        slot_hash: vec![0xCDu8; 32],
    });
    assert_eq!(
        event,
        Some(Event::RelayEscrowBlobReceived {
            gate_hash: vec![0xABu8; 32],
            blob: b"peer-card-bytes".to_vec(),
        })
    );
}

// @internal
#[test]
fn run_escrow_command_ignores_non_escrow_commands() {
    let mock = MockRelay::start();
    let (wb, _dir) = vauchi_pointing_at(&mock);
    assert_eq!(wb.run_escrow_command(&Command::BleStopScanning), None);
}
