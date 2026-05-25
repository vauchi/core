// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression: multi-stage Exchanged contacts can message.
//!
//! Feature file: features/contact_exchange.feature @multi-stage
//!
//! Before the fix, `multistage_exchange.rs::persist_finalized_contact`
//! initialised the Double Ratchet with both peers as `initialize_initiator`
//! and the Ed25519 identity key as the X25519 `their_dh_public` — a silently
//! broken channel (same defects as the QR/BLE/NFC path).
//!
//! Option A (problem record `2026-05-25-in-person-exchange-ratchet-broken`):
//! `MultiStageSession::build_exchange_ratchet` derives the role
//! deterministically (smaller identity key = initiator) and keys the ratchet
//! off the transport ephemeral keys the session already holds — `peer_ephemeral`
//! (initiator) and our retained ephemeral (responder) — with `transport_key` as
//! the root seed. The root therefore depends on a fresh ephemeral DH, not
//! `transport_key` alone, so a leaked backup/synced `transport_key` does not by
//! itself reveal the ratchet. This drives two real `MultiStageSession`s to
//! `Finalized`, builds both ratchets via the seam, and round-trips both ways.

use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

/// Drives two sessions through a full multi-stage exchange to `Finalized`.
fn drive_to_finalized(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
) -> (MultiStageSession, MultiStageSession) {
    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

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

    assert!(
        matches!(alice.get_state(), ProtocolState::Finalized),
        "alice finalized"
    );
    assert!(
        matches!(bob.get_state(), ProtocolState::Finalized),
        "bob finalized"
    );
    (alice, bob)
}

// @scenario: contact_exchange :: Multi-stage exchanged contacts can message each other
#[test]
fn multistage_in_person_ratchet_round_trips() {
    let (alice, bob) = drive_to_finalized(
        b"Alice's contact card".to_vec(),
        b"Bob's contact card".to_vec(),
    );

    let alice_tk = alice.get_transport_key().expect("alice transport key");
    let bob_tk = bob.get_transport_key().expect("bob transport key");
    assert_eq!(alice_tk, bob_tk, "transport key must be symmetric");

    // The seam takes the identity keys for the deterministic role decision;
    // the protocol layer supplies them from the exchanged payloads. Synthetic
    // here — only their ordering matters.
    let alice_id = [1u8; 32];
    let bob_id = [2u8; 32];

    let (alice_ratchet, alice_is_initiator) = alice
        .build_exchange_ratchet(&alice_id, &bob_id)
        .expect("alice ratchet builds");
    let (bob_ratchet, bob_is_initiator) = bob
        .build_exchange_ratchet(&bob_id, &alice_id)
        .expect("bob ratchet builds");

    assert_ne!(
        alice_is_initiator, bob_is_initiator,
        "exactly one side must be the initiator"
    );

    // The responder has no sending chain until it receives the initiator's
    // first message, so the initiator speaks first.
    let (mut initiator, mut responder) = if alice_is_initiator {
        (alice_ratchet, bob_ratchet)
    } else {
        (bob_ratchet, alice_ratchet)
    };

    let msg = b"Hello over multi-stage";
    let ct = initiator.encrypt(msg).expect("initiator encrypts");
    let pt = responder
        .decrypt(&ct)
        .expect("responder must decrypt the initiator's first message");
    assert_eq!(pt, msg, "initiator->responder plaintext must survive");

    let reply = b"Reply over multi-stage";
    let ct = responder.encrypt(reply).expect("responder encrypts reply");
    let pt = initiator
        .decrypt(&ct)
        .expect("initiator must decrypt the responder's reply");
    assert_eq!(pt, reply, "responder->initiator plaintext must survive");
}
