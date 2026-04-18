// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::exchange::multistage::base45;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

/// Helper: run the full exchange protocol between two sessions until both complete.
/// Returns (alice_state, bob_state) after at most `max_iters` iterations.
fn run_exchange(
    alice_data: Vec<u8>,
    bob_data: Vec<u8>,
    max_iters: usize,
) -> (MultiStageSession, MultiStageSession) {
    let mut alice = MultiStageSession::new(alice_data);
    let mut bob = MultiStageSession::new(bob_data);

    // Stage 1: Both display INIT QRs and scan each other
    let ai = alice.get_display_qr().expect("alice INIT");
    let bi = bob.get_display_qr().expect("bob INIT");
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Stages 2-4: Exchange DATA, VERIFY, CONFIRM
    for _ in 0..max_iters {
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

// --- Property-based tests ---

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Full exchange roundtrip with random payloads from 1 to 20000 bytes.
    /// Exercises INIT -> DATA -> VRFY -> CONF -> COMPLETE with asymmetric sizes.
// @internal
    #[test]
    fn test_exchange_roundtrip_any_payload(
        alice_data in prop::collection::vec(any::<u8>(), 1..20000),
        bob_data in prop::collection::vec(any::<u8>(), 1..20000),
    ) {
        let alice_data_clone = alice_data.clone();
        let bob_data_clone = bob_data.clone();

        let (alice, bob) = run_exchange(alice_data, bob_data, 1500);

        prop_assert!(matches!(alice.get_state(), ProtocolState::Finalized),
            "Alice did not reach Complete, got: {:?}", alice.get_state());
        prop_assert!(matches!(bob.get_state(), ProtocolState::Finalized),
            "Bob did not reach Complete, got: {:?}", bob.get_state());
        prop_assert_eq!(alice.get_received_data().unwrap(), bob_data_clone);
        prop_assert_eq!(bob.get_received_data().unwrap(), alice_data_clone);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Base45 encode/decode roundtrip for arbitrary byte sequences.
// @internal
    #[test]
    fn test_base45_roundtrip(data in prop::collection::vec(any::<u8>(), 0..1000)) {
        let encoded = base45::encode(&data);
        let decoded = base45::decode(&encoded).unwrap();
        prop_assert_eq!(decoded, data);
    }
}

// --- Regression tests from proptest counterexamples (CC-09) ---

/// Regression: asymmetric payload sizes caused deadlock before the fix.
/// The side with fewer chunks would finish first, reach Verifying, and stop
/// sending DATA QRs with ACK bitmaps, leaving the other side stuck in
/// Transferring forever.
///
/// Counterexample: alice=500 bytes (2 chunks), bob=19000 bytes (41 chunks).
// @internal
#[test]
fn regression_asymmetric_payload_deadlock() {
    let alice_data = vec![0xAA; 500];
    let bob_data = vec![0xBB; 19_000];

    let (alice, bob) = run_exchange(alice_data.clone(), bob_data.clone(), 1000);

    assert!(
        matches!(alice.get_state(), ProtocolState::Finalized),
        "Alice stuck in {:?}",
        alice.get_state()
    );
    assert!(
        matches!(bob.get_state(), ProtocolState::Finalized),
        "Bob stuck in {:?}",
        bob.get_state()
    );
    assert_eq!(alice.get_received_data().unwrap(), bob_data);
    assert_eq!(bob.get_received_data().unwrap(), alice_data);
}

/// Regression: extreme asymmetry — 1-byte vs maximum payload.
// @internal
#[test]
fn regression_extreme_asymmetry() {
    let alice_data = vec![0x42; 1];
    let bob_data = vec![0xFF; 19_999];

    let (alice, bob) = run_exchange(alice_data.clone(), bob_data.clone(), 1000);

    assert!(
        matches!(alice.get_state(), ProtocolState::Finalized),
        "Alice stuck in {:?}",
        alice.get_state()
    );
    assert!(
        matches!(bob.get_state(), ProtocolState::Finalized),
        "Bob stuck in {:?}",
        bob.get_state()
    );
    assert_eq!(alice.get_received_data().unwrap(), bob_data);
    assert_eq!(bob.get_received_data().unwrap(), alice_data);
}
