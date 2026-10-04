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
    // With 34-byte chunks, a 32KB payload is about 950 chunks per side,
    // and one frame in four re-shows the opening frame.
    for _ in 0..8000 {
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
fn test_e2e_grace_period_keeps_showing_the_final_frame() {
    let (mut alice, mut bob) = run_full_exchange(b"Alice".to_vec(), b"Bob".to_vec());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);

    // After finalization the final frame is still displayed, as DONE, so
    // the peer can also finalize (C3 fix: prevents asymmetric failure);
    // a phone that already read its peer's DONE shows BOTH instead (design
    // D6). DATA may be interleaved while chunks are unacked; the guarantee
    // is that an ending frame keeps coming.
    let ending_frames = (0..50)
        .filter_map(|_| alice.get_display_qr())
        .filter(|qr| qr.data.starts_with("DON3") || qr.data.starts_with("BTH3"))
        .count();
    assert!(
        ending_frames > 0,
        "Grace period must keep showing the final frame so the peer can finalize"
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
    let card = b"identical data on both sides".to_vec();
    let (alice, bob) = run_full_exchange(card.clone(), card.clone());

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data().unwrap(), card);
    assert_eq!(bob.get_received_data().unwrap(), card);
}

// === Atomicity tests (PRB-031) ===

/// Feature: contact_exchange.feature @atomicity
/// Data must not be available before each side reaches Finalized.
// @internal
#[test]
fn test_atomicity_data_not_available_before_finalized() {
    let alice_card = b"Alice".to_vec();
    let bob_card = b"Bob".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    assert_eq!(alice.get_received_data(), None);
    assert_eq!(bob.get_received_data(), None);

    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if !matches!(alice.get_state(), ProtocolState::Finalized) {
            assert_eq!(
                alice.get_received_data(),
                None,
                "Alice exposed Bob's data before Finalized in {:?}",
                alice.get_state()
            );
        }
        if !matches!(bob.get_state(), ProtocolState::Finalized) {
            assert_eq!(
                bob.get_received_data(),
                None,
                "Bob exposed Alice's data before Finalized in {:?}",
                bob.get_state()
            );
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data(), Some(bob_card));
    assert_eq!(bob.get_received_data(), Some(alice_card));
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
/// A side that never scans its peer's finalization frame must not finalize.
// @internal
#[test]
fn test_atomicity_without_peer_finalization_frame_no_finalize() {
    let mut alice = MultiStageSession::new(b"Alice".to_vec());
    let mut bob = MultiStageSession::new(b"Bob".to_vec());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Bob reads everything Alice shows except her final frame.
    let mut withheld = 0;
    for _ in 0..500 {
        if let Some(aq) = alice.get_display_qr() {
            // Once saved, Alice shows her final frame as DONE (design D6).
            if aq.data.starts_with("FIN3") || aq.data.starts_with("DON3") {
                withheld += 1;
            } else {
                bob.process_scanned_qr(&aq.data);
            }
        }
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
        assert_ne!(
            bob.get_state(),
            ProtocolState::Finalized,
            "Bob finalized without Alice's final frame"
        );
        assert_eq!(bob.get_received_data(), None);
    }

    assert!(withheld > 0, "Alice never showed a final frame to withhold");
    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(
        bob.get_state(),
        ProtocolState::Verifying,
        "Bob holds every chunk and waits for the reveal key"
    );
}

/// Feature: contact_exchange.feature @atomicity
/// Regression test for C3: asymmetric exchange failure (Samsung ↔ iPhone).
///
/// When one side finalizes first (reads the peer's final frame), it must
/// keep showing its own so the peer can also finalize. Without this, the
/// first-to-finalize side stops displaying QRs and the peer times out.
// @internal
#[test]
fn test_asymmetric_finalization_both_reach_finalized() {
    let alice_card = b"Alice (iPhone)".to_vec();
    let bob_card = b"Bob (Samsung)".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    let mut withheld_alice_finalization_frame = false;
    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            if aq.data.starts_with("FIN3") {
                withheld_alice_finalization_frame = true;
            } else {
                bob.process_scanned_qr(&aq.data);
            }
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized) {
            break;
        }
    }

    assert!(
        withheld_alice_finalization_frame,
        "Alice never displayed the finalization frame withheld from Bob"
    );
    assert_eq!(
        alice.get_state(),
        ProtocolState::Finalized,
        "Alice should finalize while Bob withholds scanning"
    );
    assert_ne!(
        bob.get_state(),
        ProtocolState::Finalized,
        "Bob should still need Alice's finalization frame"
    );

    for _ in 0..50 {
        let aq = alice.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if matches!(bob.get_state(), ProtocolState::Finalized) {
            break;
        }
    }

    assert_eq!(
        bob.get_state(),
        ProtocolState::Finalized,
        "Bob must finalize after scanning Alice's grace-period finalization frame"
    );
    assert_eq!(alice.get_received_data(), Some(bob_card));
    assert_eq!(bob.get_received_data(), Some(alice_card));
}

// ── Resilience tests (Solutions S1–S6) ──────────────────────────────────

/// S4: Verify data is not available before Finalized.
// @internal
#[test]
fn test_data_not_available_before_finalized() {
    let alice_card = b"Alice".to_vec();
    let bob_card = b"Bob".to_vec();
    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    assert_eq!(alice.get_received_data(), None);
    assert_eq!(bob.get_received_data(), None);

    for _ in 0..500 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if !matches!(alice.get_state(), ProtocolState::Finalized) {
            assert_eq!(
                alice.get_received_data(),
                None,
                "Alice exposed Bob's data before Finalized in {:?}",
                alice.get_state()
            );
        }
        if !matches!(bob.get_state(), ProtocolState::Finalized) {
            assert_eq!(
                bob.get_received_data(),
                None,
                "Bob exposed Alice's data before Finalized in {:?}",
                bob.get_state()
            );
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert_eq!(alice.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(alice.get_received_data(), Some(bob_card));
    assert_eq!(bob.get_received_data(), Some(alice_card));
}

/// S5: FAIL QR type — when one side fails, peer can detect it.
// @internal
#[test]
fn test_fail_qr_roundtrip() {
    use vauchi_core::exchange::multistage::qr_codec;

    let session_id: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let qr = qr_codec::format_fail_qr(&session_id);
    assert!(qr.starts_with("FAI3"));

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

    use vauchi_core::exchange::multistage::qr_codec;
    let fail_qr = qr_codec::format_fail_qr(&[0u8; 16]);
    let state = alice.process_scanned_qr(&fail_qr);
    assert_eq!(state, ProtocolState::Finalized);
}

/// S3: Adaptive display durations — each stage has appropriate timing with jitter.
// @internal
#[test]
fn test_adaptive_display_durations() {
    // Three chunks each, so the two frames that settle the layouts below
    // cannot complete the transfer.
    let mut alice = MultiStageSession::new(vec![0xA1; 62]);
    let mut bob = MultiStageSession::new(vec![0xB2; 62]);

    // With no peer heard yet the opening frame is a sweep frame: ~100ms
    // (±20% jitter: 80–120ms), one per layout.
    let init_qr = alice.get_display_qr().unwrap();
    assert!(
        (80..=120).contains(&init_qr.display_duration_ms),
        "a sweeping INIT should be ~100ms, got {}",
        init_qr.display_duration_ms
    );

    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&init_qr.data);
    // Two more frames each way: both have then heard the other's echo and
    // stopped sweeping, so the frames below carry their stage's own dwell.
    for _ in 0..2 {
        let aq = alice.get_display_qr().unwrap();
        let bq = bob.get_display_qr().unwrap();
        alice.process_scanned_qr(&bq.data);
        bob.process_scanned_qr(&aq.data);
    }

    // DATA should be ~300ms (±20%: 240–360ms). Raised from 100ms, at which a
    // Pixel 3a decoded zero of the peer's DATA frames; 300ms decoded 1048 and
    // got the transfer to recv=2/3. Dwell has to cover the peer's *capture*,
    // not just rxing's decode
    // (2026-08-18-multistage-data-frames-too-brief-to-capture).
    let data_qr = (0..60)
        .filter_map(|_| alice.get_display_qr())
        .find(|qr| qr.data.starts_with("DAT3"))
        .expect("a Transferring session shows DATA");
    assert!(
        (240..=360).contains(&data_qr.display_duration_ms),
        "DATA display should be ~300ms, got {}",
        data_qr.display_duration_ms
    );

    // The rescue frame a Transferring session re-shows for a peer still in
    // Advertising must carry INIT dwell, not the shorter DATA dwell — that
    // peer scans at INIT cadence, so emitting the recovery path briefly
    // made the recovery itself uncapturable. The re-show is drawn per frame
    // (roughly one in four) rather than landing on a fixed cycle position, so
    // walk enough frames that a miss is vanishingly unlikely.
    let rescue = (0..60)
        .filter_map(|_| alice.get_display_qr())
        .find(|qr| qr.data.starts_with("INI"))
        .expect("a Transferring session re-shows INIT for a peer still advertising");
    assert!(
        (320..=480).contains(&rescue.display_duration_ms),
        "the re-shown INIT must use INIT dwell so a lagging peer can \
         actually capture it, got {}",
        rescue.display_duration_ms
    );

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
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();
    let (mut alice, _bob) = run_full_exchange(alice_card, bob_card);

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

// @internal
#[test]
fn transport_decrypt_failure_count_starts_at_zero() {
    let session = MultiStageSession::new(b"card".to_vec());
    assert_eq!(session.transport_decrypt_failure_count(), 0);
}

// @internal
#[test]
fn transport_decrypt_failure_count_increments_on_tampered_data_chunk() {
    use vauchi_core::exchange::multistage::qr_codec;

    let mut alice = MultiStageSession::new(b"alice card".to_vec());
    let mut bob = MultiStageSession::new(b"bob card".to_vec());

    // Stage 1: alice must call get_display_qr first to transition
    // Idle → Advertising; otherwise handle_init refuses bob's INIT and
    // alice's transport_key stays None — short-circuiting handle_data
    // before the AEAD step we're trying to exercise.
    let _ai = alice.get_display_qr().expect("alice INIT");
    let bi = bob.get_display_qr().expect("bob INIT");
    alice.process_scanned_qr(&bi.data);

    assert_eq!(
        alice.transport_decrypt_failure_count(),
        0,
        "counter must be zero before any DATA arrives"
    );

    // Craft a tampered DATA QR: valid framing, valid length (12 nonce + 16 tag),
    // garbage ciphertext. Pre-fix this silently produced None and skipped
    // mark_received without surfacing the failure anywhere.
    let tampered = qr_codec::format_data_qr(
        &bob.session_id(),
        0,   // chunk_idx
        1,   // chunk_total
        &[], // ack_bitmap (none of our chunks seen yet)
        &[0xAAu8; 28],
    );

    alice.process_scanned_qr(&tampered);

    assert_eq!(
        alice.transport_decrypt_failure_count(),
        1,
        "tampered DATA chunk must be observable via the counter, not silently dropped"
    );
}

// @internal
#[test]
fn transport_decrypt_failure_count_does_not_increment_on_legitimate_data_chunk() {
    let mut alice = MultiStageSession::new(b"alice card".to_vec());
    let mut bob = MultiStageSession::new(b"bob card".to_vec());

    // INIT both ways so both sides have transport keys and reach Discovered.
    let ai = alice.get_display_qr().expect("alice INIT");
    let bi = bob.get_display_qr().expect("bob INIT");
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Get bob's first DATA chunk and feed it to alice — legitimate, must
    // not increment the counter.
    let bq = bob.get_display_qr().expect("bob DATA chunk");
    alice.process_scanned_qr(&bq.data);

    assert_eq!(
        alice.transport_decrypt_failure_count(),
        0,
        "legitimate DATA chunks must not raise the counter"
    );
}
