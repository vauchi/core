// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the pure escrow-client request/response mapping
//! (`network::escrow_client`, ADR-049 Phase 1 T1).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use proptest::prelude::*;
use vauchi_core::network::escrow_client::{EscrowOutcome, escrow_outcome, escrow_request};
use vauchi_core::platform::Command;
use vauchi_protocol::escrow::{EscrowMessage, EscrowResponse, MAX_SLOTS_PER_GATE};

fn gate() -> Vec<u8> {
    vec![0xABu8; 32]
}
fn slot() -> Vec<u8> {
    vec![0xCDu8; 32]
}

// ── escrow_request: Command → EscrowMessage ──────────────────────────

// @internal
#[test]
fn deposit_command_maps_to_put_with_hex_hashes_and_base64_blob() {
    let card = vec![0x01u8, 0x02, 0x03];
    let msg = escrow_request(&Command::RelayEscrowDeposit {
        gate_hash: gate(),
        slot_hash: slot(),
        encrypted_card: card.clone(),
        ttl_seconds: 3600,
    })
    .expect("deposit maps to a message");
    match msg {
        EscrowMessage::Put {
            gate_hash,
            slot_hash,
            blob,
            ttl_seconds,
        } => {
            assert_eq!(gate_hash, hex::encode(gate()));
            assert_eq!(slot_hash, hex::encode(slot()));
            assert_eq!(ttl_seconds, 3600);
            assert_eq!(
                URL_SAFE_NO_PAD.decode(&blob).expect("blob is valid base64"),
                card
            );
        }
        other => panic!("expected Put, got {other:?}"),
    }
}

// @internal
#[test]
fn check_command_maps_to_count() {
    let msg = escrow_request(&Command::RelayEscrowCheck {
        gate_hash: gate(),
        suggested_interval_ms: 500,
    })
    .expect("check maps to a message");
    assert_eq!(
        msg,
        EscrowMessage::Count {
            gate_hash: hex::encode(gate()),
        }
    );
}

// @internal
#[test]
fn retrieve_command_maps_to_get() {
    let msg = escrow_request(&Command::RelayEscrowRetrieve {
        gate_hash: gate(),
        slot_hash: slot(),
    })
    .expect("retrieve maps to a message");
    assert_eq!(
        msg,
        EscrowMessage::Get {
            gate_hash: hex::encode(gate()),
            slot_hash: hex::encode(slot()),
        }
    );
}

// @internal
#[test]
fn non_escrow_command_maps_to_none() {
    assert_eq!(escrow_request(&Command::BleStopScanning), None);
    assert_eq!(escrow_request(&Command::ImagePickFromLibrary), None);
}

// ── escrow_outcome: EscrowResponse → EscrowOutcome ───────────────────

// @internal
#[test]
fn stored_and_already_exists_are_deposited() {
    assert_eq!(
        escrow_outcome(&EscrowResponse::Stored),
        EscrowOutcome::Deposited
    );
    assert_eq!(
        escrow_outcome(&EscrowResponse::AlreadyExists),
        EscrowOutcome::Deposited
    );
}

// @internal
#[test]
fn full_gate_is_ready_partial_is_pending() {
    assert_eq!(
        escrow_outcome(&EscrowResponse::Count {
            count: MAX_SLOTS_PER_GATE
        }),
        EscrowOutcome::Ready
    );
    assert_eq!(
        escrow_outcome(&EscrowResponse::Count { count: 1 }),
        EscrowOutcome::Pending
    );
    assert_eq!(
        escrow_outcome(&EscrowResponse::NotReady { count: 1 }),
        EscrowOutcome::Pending
    );
}

// @internal
#[test]
fn blob_response_round_trips_to_retrieved_bytes() {
    let bytes = vec![0xDEu8, 0xAD, 0xBE, 0xEF];
    let resp = EscrowResponse::Blob {
        blob: URL_SAFE_NO_PAD.encode(&bytes),
    };
    assert_eq!(escrow_outcome(&resp), EscrowOutcome::Retrieved(bytes));
}

// @internal
#[test]
fn malformed_blob_is_a_failure_not_a_silent_empty() {
    let resp = EscrowResponse::Blob {
        blob: "!!! not base64 !!!".to_string(),
    };
    match escrow_outcome(&resp) {
        EscrowOutcome::Failed(reason) => assert_eq!(reason, "malformed_blob"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

// @internal
#[test]
fn terminal_responses_map_to_specific_failure_reasons() {
    assert_eq!(
        escrow_outcome(&EscrowResponse::GateFull),
        EscrowOutcome::Failed("gate_full".to_string())
    );
    assert_eq!(
        escrow_outcome(&EscrowResponse::BlobTooLarge),
        EscrowOutcome::Failed("blob_too_large".to_string())
    );
    assert_eq!(
        escrow_outcome(&EscrowResponse::NotFound),
        EscrowOutcome::Failed("not_found".to_string())
    );
}

// ── property: any deposit blob round-trips through the wire ──────────

proptest! {
    // @internal
    #[test]
    fn deposit_blob_round_trips_for_arbitrary_card(
        card in proptest::collection::vec(any::<u8>(), 0..2048),
    ) {
        let msg = escrow_request(&Command::RelayEscrowDeposit {
            gate_hash: gate(),
            slot_hash: slot(),
            encrypted_card: card.clone(),
            ttl_seconds: 600,
        })
        .expect("deposit maps to a message");
        if let EscrowMessage::Put { blob, gate_hash, slot_hash, .. } = msg {
            prop_assert_eq!(URL_SAFE_NO_PAD.decode(&blob).unwrap(), card);
            prop_assert_eq!(hex::decode(&gate_hash).unwrap(), gate());
            prop_assert_eq!(hex::decode(&slot_hash).unwrap(), slot());
        } else {
            prop_assert!(false, "expected Put");
        }
    }
}
