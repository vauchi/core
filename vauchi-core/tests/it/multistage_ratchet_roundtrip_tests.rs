// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! RED reproduction: multi-stage Exchanged contacts cannot message.
//!
//! Feature file: features/contact_exchange.feature @multi-stage
//!
//! `core/vauchi-platform/src/multistage_exchange.rs::persist_finalized_contact`
//! initialises the Double Ratchet by calling `initialize_initiator` on BOTH
//! peers with `transport_key` as the secret and the peer's Ed25519 identity key
//! (from the deserialised payload) as the X25519 `their_dh_public` — the same
//! two-defect pattern fixed for the QR/BLE/NFC path. This drives two real
//! `MultiStageSession`s to `Finalized`, reproduces that sequence using the
//! symmetric `transport_key`, and asserts a message round-trips. It FAILS today.
//!
//! The fix (Option A in the problem record) keys the ratchet off the transport
//! ephemeral keys the session already holds (`peer_ephemeral` + our retained
//! ephemeral), with the role derived deterministically from the identity keys —
//! preserving the property that `transport_key` compromise (it is backed up and
//! synced) does not by itself reveal the ratchet root.

use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
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

    // Reproduce the production save+ratchet sequence
    // (multistage_exchange.rs::persist_finalized_contact): both peers
    // initialize_initiator, with a peer-identity-shaped key as their_dh_public.
    let synthetic_peer_identity = [7u8; 32];
    let mut alice_ratchet = DoubleRatchetState::initialize_initiator(
        &SymmetricKey::from_bytes(alice_tk),
        synthetic_peer_identity,
    )
    .expect("alice ratchet init");
    let mut bob_ratchet = DoubleRatchetState::initialize_initiator(
        &SymmetricKey::from_bytes(bob_tk),
        synthetic_peer_identity,
    )
    .expect("bob ratchet init");

    let msg = b"Hello from Alice over multi-stage";
    let ct = alice_ratchet.encrypt(msg).expect("alice encrypts");
    let pt = bob_ratchet
        .decrypt(&ct)
        .expect("bob must decrypt alice's first message");
    assert_eq!(pt, msg, "multi-stage round-trip must preserve plaintext");
}
