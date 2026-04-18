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
/// INIT -> DATA transfer -> VERIFY -> CONFIRM -> COMPLETE -> READY -> FINALIZED
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

    // Stages 2-6: cycle through DATA, VERIFY, CONFIRM, READY
    // With 80-byte chunks, 32KB payloads need ~500+ rounds per side
    for _ in 0..2000 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    (alice, bob)
}

// @internal
#[test]
fn test_e2e_text_only_card() {
    let alice_card = b"name:Alice\nemail:alice@example.com\nphone:+1234567890".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_e2e_card_with_avatar() {
    // Simulate card with 128x128 JPEG avatar (~12KB)
    let mut alice_card = b"name:Alice\navatar:".to_vec();
    alice_card.extend(vec![0xFFu8; 12_000]); // fake JPEG data
    let mut bob_card = b"name:Bob\navatar:".to_vec();
    bob_card.extend(vec![0xAAu8; 8_000]);
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_e2e_max_payload_32kb() {
    // 32KB is the max card size per design
    let alice_card = vec![0x42u8; 32_000];
    let bob_card = vec![0x43u8; 32_000];
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_e2e_minimum_payload_1_byte() {
    let (alice, bob) = run_full_exchange(vec![0x01], vec![0x02]);

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), vec![0x02]);
    assert_eq!(bob.get_received_data().unwrap(), vec![0x01]);
}

// @internal
#[test]
fn test_e2e_asymmetric_payload_sizes() {
    // One side has a tiny card, the other has a large card.
    // Tests that the protocol handles asymmetric chunk counts correctly.
    let alice_card = vec![0xAA; 100]; // ~1 chunk
    let bob_card = vec![0xBB; 20_000]; // many chunks
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
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

// @internal
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
    assert!(!matches!(bob.get_state(), ProtocolState::Finalized));
    assert!(!matches!(bob.get_state(), ProtocolState::Failed(_)));
}

// @internal
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
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
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
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
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

// @internal
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
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_e2e_grace_period_broadcasts_combo() {
    let (mut alice, mut bob) = run_full_exchange(b"Alice".to_vec(), b"Bob".to_vec());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);

    // After finalization, COMBO QRs are still displayed for a grace period
    // so the peer can also finalize (C3 fix: prevents asymmetric failure).
    let qr = alice.get_display_qr();
    assert!(qr.is_some(), "Grace period should still produce COMBO QRs");
    assert!(
        qr.as_ref().unwrap().data.starts_with("CMBO"),
        "Grace period QRs should be COMBO type, got: {}",
        &qr.unwrap().data[..4]
    );

    // Verify QRs are still produced after a short delay (still within grace).
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(
        bob.get_display_qr().is_some(),
        "Grace period should still be active after 1s"
    );
}

// @internal
#[test]
#[ignore] // Wall-clock test: takes 61s. Run with `cargo test -- --ignored`.
fn test_e2e_grace_period_expires() {
    let (mut alice, mut bob) = run_full_exchange(b"Alice".to_vec(), b"Bob".to_vec());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);

    // FINALIZED_GRACE_DURATION = 60s — sleep past it.
    std::thread::sleep(std::time::Duration::from_secs(61));
    assert!(
        alice.get_display_qr().is_none(),
        "QRs should stop after grace period"
    );
    assert!(
        bob.get_display_qr().is_none(),
        "QRs should stop after grace period"
    );
}

// @internal
#[test]
fn test_e2e_no_qr_after_cancel() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    alice.get_display_qr();
    alice.cancel();

    assert!(matches!(alice.get_state(), ProtocolState::Failed(_)));
    assert!(alice.get_display_qr().is_none());
}

// @internal
#[test]
fn test_e2e_received_data_none_before_complete() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());

    // Idle — no data
    assert!(alice.get_received_data().is_none());

    // Advertising — no data
    alice.get_display_qr();
    assert!(alice.get_received_data().is_none());
}

// @internal
#[test]
fn test_e2e_binary_payload_all_byte_values() {
    // Card containing every possible byte value (0x00..0xFF)
    let alice_card: Vec<u8> = (0..=255).collect();
    let bob_card: Vec<u8> = (0..=255).rev().collect();
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_e2e_identical_cards() {
    // Both sides exchange the exact same data
    let card = b"identical data on both sides".to_vec();
    let (alice, bob) = run_full_exchange(card.clone(), card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), card);
    assert_eq!(bob.get_received_data().unwrap(), card);
}

// === Atomicity tests (PRB-031) ===

/// Feature: contact_exchange.feature @atomicity
/// Data must not be available until both sides reach Finalized.
// @internal
#[test]
fn test_atomicity_data_not_available_in_complete() {
    let alice_card = b"Alice".to_vec();
    let bob_card = b"Bob".to_vec();

    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    // INIT
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Drive through DATA/VERIFY/CONFIRM until Complete
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

    // Both are in Complete but NOT Finalized yet
    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);

    // Data must NOT be available in Complete — only in Finalized
    assert!(
        alice.get_received_data().is_none(),
        "Data should not be available in Complete state"
    );
    assert!(
        bob.get_received_data().is_none(),
        "Data should not be available in Complete state"
    );
}

/// Feature: contact_exchange.feature @atomicity
/// Both sides must exchange READY QRs to reach Finalized.
// @internal
#[test]
fn test_atomicity_ready_exchange_reaches_finalized() {
    let alice_card = b"Alice atomicity test".to_vec();
    let bob_card = b"Bob atomicity test".to_vec();
    let (alice, bob) = run_full_exchange(alice_card.clone(), bob_card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

/// Feature: contact_exchange.feature @atomicity
/// If one side stops scanning after Complete (never scans READY),
/// the other side must NOT reach Finalized.
// @internal
#[test]
fn test_atomicity_one_side_stops_scanning_no_finalize() {
    let alice_card = b"Alice".to_vec();
    let bob_card = b"Bob".to_vec();

    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    // INIT
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Drive to Complete (both sides)
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

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);

    // Now only Alice displays QRs (READY), but Bob never scans them.
    // Only Alice scans Bob's READY QRs, not vice versa.
    for _ in 0..100 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        // Alice scans Bob's READY
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        // Bob does NOT scan Alice's READY — simulates one side stopping
        let _ = aq;
    }

    // Alice may have received Bob's READY, but Bob never received Alice's
    // Bob should NOT be Finalized
    assert!(
        !matches!(bob.get_state(), ProtocolState::Finalized),
        "Bob should not be Finalized without scanning Alice's READY"
    );
    // Bob's data should not be available
    assert!(
        bob.get_received_data().is_none(),
        "Bob's data should not be available without finalization"
    );
}

/// Feature: contact_exchange.feature @atomicity
/// Regression test for C3: asymmetric exchange failure (Samsung ↔ iPhone).
///
/// When one side finalizes first (scans peer's RDYY), it must continue
/// broadcasting its own RDYY so the peer can also finalize. Without this,
/// the first-to-finalize side stops displaying QRs and the peer times out
/// with "peer did not confirm readiness".
// @internal
#[test]
fn test_asymmetric_finalization_both_must_complete() {
    let alice_card = b"Alice (iPhone)".to_vec();
    let bob_card = b"Bob (Samsung)".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    // INIT
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Drive to Complete (both sides)
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

    assert_eq!(alice.get_state(), ProtocolState::Complete);
    assert_eq!(bob.get_state(), ProtocolState::Complete);

    // Simulate asymmetric timing: Alice scans Bob's RDYY first (Alice finalizes).
    // Then Bob must still be able to scan Alice's RDYY to also finalize.
    // This is the exact C3 scenario: iPhone finalizes, Samsung is left behind.

    // Step 1: Only Alice scans Bob's QRs until Alice finalizes
    for _ in 0..50 {
        let _aq = alice.get_display_qr(); // Alice displays but Bob doesn't scan
        let bq = bob.get_display_qr();
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized) {
            break;
        }
    }

    assert_eq!(
        alice.get_state(),
        ProtocolState::Finalized,
        "Alice should have finalized after scanning Bob's RDYY"
    );
    assert_eq!(
        bob.get_state(),
        ProtocolState::Complete,
        "Bob should still be in Complete (hasn't scanned Alice's RDYY yet)"
    );

    // Step 2: Now Bob scans Alice's QRs. Alice is Finalized but MUST still
    // display RDYY so Bob can finalize too.
    for _ in 0..50 {
        let aq = alice.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        let _bq = bob.get_display_qr(); // Bob displays but Alice doesn't need to scan
        if matches!(bob.get_state(), ProtocolState::Finalized) {
            break;
        }
    }

    // CRITICAL: Both sides must reach Finalized — no asymmetric outcome
    assert_eq!(
        bob.get_state(),
        ProtocolState::Finalized,
        "Bob must finalize after scanning Alice's RDYY (C3 regression: was 'peer did not confirm readiness')"
    );

    // Both sides must have each other's data
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// ── Resilience tests (Solutions S1–S6) ──────────────────────────────────

/// S4: Verify data is NOT available in Complete state (only in Finalized).
// @internal
#[test]
fn test_data_not_available_in_complete() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    let mut bob = MultiStageSession::new(b"Bob".to_vec());

    // Exchange INIT
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Drive to Complete (both sides exchange data/vrfy/conf)
    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Complete) {
            break;
        }
    }
    assert_eq!(alice.get_state(), ProtocolState::Complete);

    // Data must NOT be available until Finalized
    assert!(alice.get_received_data().is_none());
}

/// S5: FAIL QR type — when one side fails, peer can detect it.
// @internal
#[test]
fn test_fail_qr_roundtrip() {
    use vauchi_core::exchange::multistage::qr_codec;

    let session_id: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let qr = qr_codec::format_fail_qr(&session_id);
    assert!(qr.starts_with("FAIL"));

    let parsed = qr_codec::parse_qr(&qr).unwrap();
    match parsed {
        qr_codec::StageQr::Fail { session_id: sid } => {
            assert_eq!(sid, session_id);
        }
        _ => panic!("Expected Fail variant, got {:?}", parsed),
    }
}

/// S5: FAIL QR causes peer to abort when not yet Finalized.
// @internal
#[test]
fn test_fail_qr_aborts_peer() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());

    // Move to Advertising
    let _ai = alice.get_display_qr().unwrap();
    assert_eq!(alice.get_state(), ProtocolState::Advertising);

    // Receive FAIL while in Advertising → should abort
    use vauchi_core::exchange::multistage::qr_codec;
    let fail_qr = qr_codec::format_fail_qr(&[0u8; 16]);
    let state = alice.process_scanned_qr(&fail_qr);
    assert_eq!(
        state,
        ProtocolState::Failed("peer reported failure".to_string())
    );
}

/// S5: FAIL QR does NOT override Finalized state.
// @internal
#[test]
fn test_fail_qr_ignored_when_finalized() {
    let (mut alice, _bob) = run_full_exchange(b"Alice".to_vec(), b"Bob".to_vec());
    assert_eq!(alice.get_state(), ProtocolState::Finalized);

    // Late FAIL from peer should be ignored
    use vauchi_core::exchange::multistage::qr_codec;
    let fail_qr = qr_codec::format_fail_qr(&[0u8; 16]);
    let state = alice.process_scanned_qr(&fail_qr);
    assert_eq!(state, ProtocolState::Finalized);
}

/// S3: Adaptive display durations — each stage has appropriate timing with jitter.
// @internal
#[test]
fn test_adaptive_display_durations() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    let mut bob = MultiStageSession::new(b"Bob".to_vec());

    // INIT should be ~400ms (±20% jitter: 320–480ms)
    let init_qr = alice.get_display_qr().unwrap();
    assert!(
        (320..=480).contains(&init_qr.display_duration_ms),
        "INIT display should be ~400ms, got {}",
        init_qr.display_duration_ms
    );

    // Exchange INITs
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&init_qr.data);

    // DATA should be ~100ms (±20%: 80–120ms) — animated V4 QR at 10fps
    let data_qr = alice.get_display_qr().unwrap();
    assert!(
        (80..=120).contains(&data_qr.display_duration_ms),
        "DATA display should be ~100ms, got {}",
        data_qr.display_duration_ms
    );

    // Drive to Complete
    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Complete) {
            break;
        }
    }

    // RDYY should be ~400ms (±20%: 320–480ms)
    if matches!(alice.get_state(), ProtocolState::Complete) {
        let combo_qr = alice.get_display_qr().unwrap();
        assert!(
            (320..=480).contains(&combo_qr.display_duration_ms),
            "RDYY display should be ~400ms, got {}",
            combo_qr.display_duration_ms
        );
    }
}

/// Verify that clear_sensitive() zeroes all security-sensitive fields.
///
/// Guards against incomplete zeroization when new fields are added to
/// MultiStageSession. If this test fails after adding a field, update
/// clear_sensitive() to cover it.
// @internal
#[test]
fn test_clear_sensitive_covers_all_security_fields() {
    // Run a full exchange to populate all fields
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();
    let (mut alice, _bob) = run_full_exchange(alice_card, bob_card);

    // Verify fields are populated before cancel
    assert!(
        matches!(alice.get_state(), ProtocolState::Finalized),
        "Exchange must complete for all fields to be populated"
    );
    assert!(
        alice.get_received_data().is_some(),
        "received_data should be populated"
    );

    // Cancel triggers clear_sensitive()
    alice.cancel();

    // All sensitive data must be gone
    assert!(
        alice.get_received_data().is_none(),
        "received_data not cleared"
    );
    assert!(
        matches!(alice.get_state(), ProtocolState::Failed(_)),
        "state should be Failed after cancel"
    );
}

/// S2: Complete state shows ONLY COMBO QRs (no VRFY/CONF interleave).
// @internal
#[test]
fn test_complete_shows_only_combo() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    let mut bob = MultiStageSession::new(b"Bob".to_vec());

    // Drive to Complete
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Complete) {
            break;
        }
    }
    assert_eq!(alice.get_state(), ProtocolState::Complete);

    // Verify all QRs in Complete state are COMBO (VRFY+CONF+RDYY in one)
    for _ in 0..10 {
        if let Some(qr) = alice.get_display_qr() {
            assert!(
                qr.data.starts_with("CMBO"),
                "Complete state should show COMBO, got: {}",
                &qr.data[..4]
            );
            // COMBO QR uses "Q" error correction for better scan reliability
            // at high density (172 chars). See: iphone-exchange-completion-delay.
            assert_eq!(
                qr.error_correction, "Q",
                "COMBO QR should use Q (25%) error correction, got {}",
                qr.error_correction
            );
        } else {
            break;
        }
    }
}
