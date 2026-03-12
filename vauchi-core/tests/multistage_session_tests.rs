// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

#[test]
fn test_new_session_is_idle() {
    let card = b"Alice's contact card".to_vec();
    let session = MultiStageSession::new(card);
    assert!(matches!(session.get_state(), ProtocolState::Idle));
}

#[test]
fn test_get_display_qr_starts_advertising() {
    let card = b"Alice's card".to_vec();
    let mut session = MultiStageSession::new(card);
    let qr = session.get_display_qr();
    assert!(qr.is_some());
    assert!(qr.unwrap().data.starts_with("INIT"));
    assert!(matches!(session.get_state(), ProtocolState::Advertising));
}

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

        if matches!(alice.get_state(), ProtocolState::Complete)
            && matches!(bob.get_state(), ProtocolState::Complete)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Complete));
    assert!(matches!(bob.get_state(), ProtocolState::Complete));

    let alice_received = alice.get_received_data().unwrap();
    let bob_received = bob.get_received_data().unwrap();
    assert_eq!(alice_received, bob_card);
    assert_eq!(bob_received, alice_card);
}

#[test]
fn test_cancel_session_clears_state() {
    let mut session = MultiStageSession::new(b"data".to_vec());
    session.get_display_qr();
    session.cancel();
    assert!(matches!(session.get_state(), ProtocolState::Failed(_)));
    assert!(session.get_received_data().is_none());
}

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
        if matches!(alice.get_state(), ProtocolState::Complete)
            && matches!(bob.get_state(), ProtocolState::Complete)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Complete));
    let alice_received = alice.get_received_data().unwrap();
    let bob_received = bob.get_received_data().unwrap();
    assert_eq!(alice_received, bob_card);
    assert_eq!(bob_received, alice_card);
}
