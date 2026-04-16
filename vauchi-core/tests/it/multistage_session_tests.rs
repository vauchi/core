// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

// @internal
#[test]
fn test_new_session_is_idle() {
    let card = b"Alice's contact card".to_vec();
    let session = MultiStageSession::new(card);
    assert!(matches!(session.get_state(), ProtocolState::Idle));
}

// @internal
#[test]
fn test_get_display_qr_starts_advertising() {
    let card = b"Alice's card".to_vec();
    let mut session = MultiStageSession::new(card);
    let qr = session.get_display_qr();
    let qr = qr.expect("expected Some");
    assert!(qr.data.starts_with("INIT"));
    assert!(matches!(session.get_state(), ProtocolState::Advertising));
}

// @internal
#[test]
fn test_full_exchange_two_sessions() {
    let alice_card = b"Alice's full contact card with avatar data".to_vec();
    let bob_card = b"Bob's full contact card with avatar data".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    // Stage 1: Both display INIT QRs
    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    assert!(alice_init.data.starts_with("INIT"));
    assert!(bob_init.data.starts_with("INIT"));

    // Both scan each other's INIT
    let alice_state = alice.process_scanned_qr(&bob_init.data);
    let bob_state = bob.process_scanned_qr(&alice_init.data);
    assert!(matches!(
        alice_state,
        ProtocolState::Discovered | ProtocolState::Transferring { .. }
    ));
    assert!(matches!(
        bob_state,
        ProtocolState::Discovered | ProtocolState::Transferring { .. }
    ));

    // Stage 2: Exchange DATA chunks until both complete
    for _ in 0..100 {
        let alice_qr = alice.get_display_qr();
        let bob_qr = bob.get_display_qr();

        if let Some(aq) = &alice_qr {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bob_qr {
            alice.process_scanned_qr(&bq.data);
        }

        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    assert!(matches!(bob.get_state(), ProtocolState::Finalized));

    let alice_received = alice.get_received_data().unwrap();
    let bob_received = bob.get_received_data().unwrap();
    assert_eq!(alice_received, bob_card);
    assert_eq!(bob_received, alice_card);
}

// @internal
#[test]
fn test_full_exchange_with_relay_metadata() {
    let alice_card = b"Alice's card for relay test".to_vec();
    let bob_card = b"Bob's card for relay test".to_vec();

    let mut alice = MultiStageSession::new_with_relay(
        alice_card.clone(),
        Some("https://alice-relay.example.com".to_string()),
        Some([0xAA; 32]),
    );
    let mut bob = MultiStageSession::new_with_relay(
        bob_card.clone(),
        Some("https://bob-relay.example.com".to_string()),
        Some([0xBB; 32]),
    );

    // Stage 1: Both display INIT QRs
    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();

    // Both scan each other's INIT
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

    // Exchange until complete
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

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    assert!(matches!(bob.get_state(), ProtocolState::Finalized));

    // Verify relay metadata was exchanged
    assert_eq!(
        alice.peer_relay_url(),
        Some("https://bob-relay.example.com")
    );
    assert_eq!(alice.peer_relay_noise_pubkey(), Some([0xBB; 32]));

    assert_eq!(
        bob.peer_relay_url(),
        Some("https://alice-relay.example.com")
    );
    assert_eq!(bob.peer_relay_noise_pubkey(), Some([0xAA; 32]));

    // Card data should still be correct
    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_exchange_without_relay_metadata() {
    let alice_card = b"Alice no relay".to_vec();
    let bob_card = b"Bob no relay".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

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

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    assert!(alice.peer_relay_url().is_none());
    assert!(alice.peer_relay_noise_pubkey().is_none());
    assert!(bob.peer_relay_url().is_none());
    assert!(bob.peer_relay_noise_pubkey().is_none());
}

// @internal
#[test]
fn test_cancel_session_clears_state() {
    let mut session = MultiStageSession::new(b"data".to_vec());
    session.get_display_qr();
    session.cancel();
    assert!(matches!(session.get_state(), ProtocolState::Failed(_)));
    assert!(session.get_received_data().is_none());
}

// @internal
#[test]
fn test_exchange_with_large_payload() {
    let alice_card = vec![0xAA; 15_000];
    let bob_card = vec![0xBB; 15_000];

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

    for _ in 0..200 {
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

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    let alice_received = alice.get_received_data().unwrap();
    let bob_received = bob.get_received_data().unwrap();
    assert_eq!(alice_received, bob_card);
    assert_eq!(bob_received, alice_card);
}
