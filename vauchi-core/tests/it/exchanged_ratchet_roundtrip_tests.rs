// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! RED reproduction: face-to-face (non-relay) exchange ratchet wiring is broken.
//!
//! Feature file: features/contact_exchange.feature @qr-mutual
//!
//! The production save sites for in-person Exchanged contacts —
//! `core/vauchi-app/src/ui/app_engine/routing.rs:417`,
//! `core/vauchi-platform/src/mobile_exchange.rs:112`, and
//! `core/vauchi-platform/src/multistage_exchange.rs:1000` — all initialise the
//! Double Ratchet by calling `initialize_initiator` on BOTH peers and passing
//! `contact.public_key()` (the Ed25519 *identity* key) as the X25519
//! `their_dh_public`. Two independent defects:
//!
//!   1. **Role:** two initiators never reconcile root keys. A correct pair is
//!      initiator + responder, where the responder's `our_dh` is the keypair
//!      whose public the initiator received as `their_dh_public`
//!      (`ratchet.rs:406` `dh_ratchet`). With two initiators each side derives
//!      its root from `KDF(S, DH(own_fresh_dh, peer_key))` and there is no
//!      matching responder step.
//!   2. **Key:** the X3DH secret is computed against the X25519 *exchange* key
//!      (`session.rs:1329`), not the Ed25519 identity key. The ratchet's first
//!      DH must use the same exchange key.
//!
//! Either defect alone breaks the secure channel. This test drives two real
//! `ExchangeSession`s through a full mutual-QR exchange to `Complete`, then
//! reproduces the production save+ratchet sequence verbatim and asserts a
//! message round-trips both ways. It FAILS today — that is the proof of the
//! bug. The existing `exchange_e2e_tests` round-trip passes only because it
//! hand-rolls the correct `initialize_initiator`/`initialize_responder` pair
//! with a real X25519 DH key and a synthetic shared secret, bypassing every
//! production save site.

use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, ExchangeState, MockProximityVerifier};
use vauchi_core::{ContactCard, Identity};

/// X3DH outputs the production save sites have available once a session reaches
/// `Complete`: the contact id, the shared secret, and `contact.public_key()`.
struct CompletedSide {
    shared_key: SymmetricKey,
    /// What `contact.public_key()` returns — the Ed25519 identity key, exactly
    /// what the production save sites feed to the ratchet as `their_dh_public`.
    contact_public_key: [u8; 32],
}

fn completed_side(session: &ExchangeSession) -> CompletedSide {
    match session.state() {
        ExchangeState::Complete { contact } => CompletedSide {
            shared_key: contact
                .shared_key()
                .expect("exchange contact has shared key")
                .clone(),
            contact_public_key: *contact
                .public_key()
                .expect("exchange contact has public key"),
        },
        other => panic!("expected Complete, got {other:?}"),
    }
}

// @scenario: contact_exchange :: In-person exchanged contacts can message each other
#[test]
fn in_person_exchange_double_ratchet_round_trips() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);
    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Drive a full mutual-QR exchange to Complete on both sides (mirrors
    // `mutual_qr_exchange_tests::test_full_qr_exchange`).
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let alice = completed_side(&alice_session);
    let bob = completed_side(&bob_session);

    // Both sides agree on the X3DH shared secret — that part is correct.
    assert_eq!(
        alice.shared_key.as_bytes(),
        bob.shared_key.as_bytes(),
        "both sides must derive the same X3DH shared secret"
    );

    // Reproduce the production save+ratchet sequence verbatim:
    // `initialize_initiator` on BOTH sides, `contact.public_key()` (identity
    // key) as `their_dh_public`. Mirrors routing.rs:417 / mobile_exchange.rs:112
    // / multistage_exchange.rs:1000.
    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&alice.shared_key, alice.contact_public_key)
            .expect("alice ratchet init");
    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&bob.shared_key, bob.contact_public_key)
            .expect("bob ratchet init");

    // Alice -> Bob.
    let msg = b"Hello Bob, this is Alice";
    let ct = alice_ratchet.encrypt(msg).expect("alice encrypts");
    let pt = bob_ratchet
        .decrypt(&ct)
        .expect("bob must decrypt alice's first message");
    assert_eq!(pt, msg, "Alice->Bob plaintext must survive the ratchet");

    // Bob -> Alice.
    let reply = b"Hi Alice, got it";
    let ct = bob_ratchet.encrypt(reply).expect("bob encrypts");
    let pt = alice_ratchet
        .decrypt(&ct)
        .expect("alice must decrypt bob's reply");
    assert_eq!(pt, reply, "Bob->Alice plaintext must survive the ratchet");
}
