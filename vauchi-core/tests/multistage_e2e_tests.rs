// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end exchange tests for the multi-stage QR protocol.
//!
//! These tests simulate real device exchange flows including edge cases
//! like abort, duplicate scans, invalid data, and various payload sizes.

use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

/// Helper: run a full exchange between two sessions.
///
/// Drives both sessions through the complete protocol lifecycle:
/// INIT -> DATA transfer -> VERIFY -> CONFIRM -> COMPLETE
fn run_full_exchange(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
) -> (MultiStageSession, MultiStageSession) {
    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    // Stage 1: INIT — both advertise, then scan each other
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Stages 2-4: cycle through DATA, VERIFY, CONFIRM
    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Complete)
            && matches!(bob.get_state(), ProtocolState::Complete)
        {
            break;
        }
    }

    (alice, bob)
}

#[test]
fn test_e2e_text_only_card() {
    let alice_card = b"name:Alice\nemail:alice@example.com\nphone:+1234567890".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_card_with_avatar() {
    // Simulate card with 128x128 JPEG avatar (~12KB)
    let mut alice_card = b"name:Alice\navatar:".to_vec();
    alice_card.extend(vec![0xFFu8; 12_000]); // fake JPEG data
    let mut bob_card = b"name:Bob\navatar:".to_vec();
    bob_card.extend(vec![0xAAu8; 8_000]);
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_max_payload_32kb() {
    // 32KB is the max card size per design
    let alice_card = vec![0x42u8; 32_000];
    let bob_card = vec![0x43u8; 32_000];
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_minimum_payload_1_byte() {
    let (alice, bob) = run_full_exchange(vec![0x01], vec![0x02]);

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), vec![0x02]);
    assert_eq!(bob.get_received_data().unwrap(), vec![0x01]);
}

#[test]
fn test_e2e_asymmetric_payload_sizes() {
    // One side has a tiny card, the other has a large card.
    // Tests that the protocol handles asymmetric chunk counts correctly.
    let alice_card = vec![0xAA; 100]; // ~1 chunk
    let bob_card = vec![0xBB; 20_000]; // many chunks
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_abort_mid_transfer() {
    let alice_card = vec![0xAA; 15_000];
    let bob_card = vec![0xBB; 15_000];

    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    // Stage 1
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Partial Stage 2 — only exchange a few chunks
    for _ in 0..3 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
    }

    // Abort
    alice.cancel();
    bob.cancel();

    assert!(matches!(alice.get_state(), ProtocolState::Failed(_)));
    assert!(matches!(bob.get_state(), ProtocolState::Failed(_)));
    // cancel() calls clear_sensitive() which sets received_data = None
    assert!(alice.get_received_data().is_none());
    assert!(bob.get_received_data().is_none());
}

#[test]
fn test_e2e_one_side_cancel_other_unaffected() {
    let alice_card = vec![0xAA; 5_000];
    let bob_card = vec![0xBB; 5_000];

    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    // Stage 1
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Alice cancels immediately after INIT exchange
    alice.cancel();
    assert!(matches!(alice.get_state(), ProtocolState::Failed(_)));

    // Bob is still in Transferring — unaware of Alice's cancellation.
    // Bob's state should not be Complete or Failed (no partner signals).
    assert!(!matches!(bob.get_state(), ProtocolState::Complete));
    assert!(!matches!(bob.get_state(), ProtocolState::Failed(_)));
}

#[test]
fn test_e2e_duplicate_init_scans_idempotent() {
    let alice_card = b"Alice".to_vec();
    let bob_card = b"Bob".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();

    // Scan the same INIT QR multiple times (simulates confirmation-frame dedup failure)
    alice.process_scanned_qr(&bi.data);
    // Second scan while already in Transferring — handle_init rejects non-Advertising
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);
    bob.process_scanned_qr(&ai.data);

    // Should still work — complete the exchange
    for _ in 0..100 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Complete)
            && matches!(bob.get_state(), ProtocolState::Complete)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_duplicate_data_scans_idempotent() {
    let alice_card = b"Alice card data".to_vec();
    let bob_card = b"Bob card data".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Exchange with duplicate DATA scans each round
    for _ in 0..100 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
            bob.process_scanned_qr(&aq.data); // duplicate DATA
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
            alice.process_scanned_qr(&bq.data); // duplicate DATA
        }
        if matches!(alice.get_state(), ProtocolState::Complete)
            && matches!(bob.get_state(), ProtocolState::Complete)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_invalid_qr_rejected_gracefully() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    alice.get_display_qr(); // start advertising

    // Feed garbage QR data — should not crash or corrupt state
    let state = alice.process_scanned_qr("totally invalid data");
    // parse_qr returns Err => state unchanged (Advertising)
    assert_eq!(state, ProtocolState::Advertising);

    let state2 = alice.process_scanned_qr("INIT|bad|data");
    // Malformed INIT fields => parse_qr returns Err => state unchanged
    assert_eq!(state2, ProtocolState::Advertising);

    // Empty string
    let state3 = alice.process_scanned_qr("");
    assert_eq!(state3, ProtocolState::Advertising);

    // Session should still be functional — can proceed with a valid exchange
    assert_eq!(alice.get_state(), ProtocolState::Advertising);
}

#[test]
fn test_e2e_invalid_qr_during_transfer_ignored() {
    let alice_card = b"Alice".to_vec();
    let bob_card = b"Bob".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Feed garbage during transfer — should not corrupt state
    let state = alice.process_scanned_qr("garbage data mid-transfer");
    assert!(matches!(state, ProtocolState::Transferring { .. }));

    // Complete the exchange normally
    for _ in 0..100 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Complete)
            && matches!(bob.get_state(), ProtocolState::Complete)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_no_qr_after_complete() {
    let (mut alice, mut bob) = run_full_exchange(b"Alice".to_vec(), b"Bob".to_vec());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);

    // After completion, get_display_qr returns None
    assert!(alice.get_display_qr().is_none());
    assert!(bob.get_display_qr().is_none());
}

#[test]
fn test_e2e_no_qr_after_cancel() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    alice.get_display_qr();
    alice.cancel();

    assert!(matches!(alice.get_state(), ProtocolState::Failed(_)));
    assert!(alice.get_display_qr().is_none());
}

#[test]
fn test_e2e_received_data_none_before_complete() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());

    // Idle — no data
    assert!(alice.get_received_data().is_none());

    // Advertising — no data
    alice.get_display_qr();
    assert!(alice.get_received_data().is_none());
}

#[test]
fn test_e2e_binary_payload_all_byte_values() {
    // Card containing every possible byte value (0x00..0xFF)
    let alice_card: Vec<u8> = (0..=255).collect();
    let bob_card: Vec<u8> = (0..=255).rev().collect();
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

#[test]
fn test_e2e_identical_cards() {
    // Both sides exchange the exact same data
    let card = b"identical data on both sides".to_vec();
    let (alice, bob) = run_full_exchange(card.clone(), card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);
    assert_eq!(alice.get_received_data().unwrap(), card);
    assert_eq!(bob.get_received_data().unwrap(), card);
}
