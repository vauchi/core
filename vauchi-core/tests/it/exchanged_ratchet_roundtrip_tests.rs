// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression: face-to-face (non-relay) exchanged contacts can message.
//!
//! Feature file: features/contact_exchange.feature @qr-mutual
//!
//! Before the fix, the production save sites for in-person Exchanged contacts —
//! `core/vauchi-app/src/ui/app_engine/routing.rs`,
//! `core/vauchi-platform/src/mobile_exchange.rs`, and
//! `core/vauchi-platform/src/multistage_exchange.rs` — initialised the Double
//! Ratchet by calling `initialize_initiator` on BOTH peers and passing
//! `contact.public_key()` (the Ed25519 *identity* key) as the X25519
//! `their_dh_public`. Two independent defects:
//!
//!   1. **Role:** two initiators never reconcile root keys. A correct pair is
//!      initiator + responder, where the responder's `our_dh` is the keypair
//!      whose public the initiator received as `their_dh_public`
//!      (`ratchet.rs` `dh_ratchet`).
//!   2. **Key:** the X3DH secret is derived against the X25519 *exchange* key
//!      (`session.rs` `handle_perform_key_agreement`), not the identity key.
//!
//! Either alone breaks the secure channel. The fix routes every save site
//! through `ExchangeSession::build_exchange_ratchet`, which derives the role
//! deterministically (smaller identity key = initiator) and keys the ratchet
//! off the retained X25519 exchange key. This test drives two real
//! `ExchangeSession`s through a full mutual-QR exchange to `Complete`, builds
//! both ratchets via that seam, and asserts a message round-trips both ways.

use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::{ContactCard, Identity};

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

    let alice_contact = alice_session
        .extract_contact()
        .expect("alice reached Complete");
    let bob_contact = bob_session.extract_contact().expect("bob reached Complete");

    // Both sides agree on the X3DH shared secret — that part was always correct.
    assert_eq!(
        alice_contact.shared_key().unwrap().as_bytes(),
        bob_contact.shared_key().unwrap().as_bytes(),
        "both sides must derive the same X3DH shared secret"
    );

    // Build both ratchets via the production seam. Role + key are derived
    // inside the seam — callers cannot pick the wrong ones.
    let (alice_ratchet, alice_is_initiator) = alice_session
        .build_exchange_ratchet(&alice_contact)
        .expect("alice ratchet builds");
    let (bob_ratchet, bob_is_initiator) = bob_session
        .build_exchange_ratchet(&bob_contact)
        .expect("bob ratchet builds");

    // Exactly one initiator and one responder — the deterministic role rule.
    assert_ne!(
        alice_is_initiator, bob_is_initiator,
        "exactly one side must be the initiator"
    );

    // The responder has no sending chain until it receives the initiator's
    // first message, so the initiator must speak first.
    let (mut initiator, mut responder) = if alice_is_initiator {
        (alice_ratchet, bob_ratchet)
    } else {
        (bob_ratchet, alice_ratchet)
    };

    // Initiator -> responder.
    let msg = b"Hello, this is the initiator";
    let ct = initiator.encrypt(msg).expect("initiator encrypts");
    let pt = responder
        .decrypt(&ct)
        .expect("responder must decrypt the initiator's first message");
    assert_eq!(pt, msg, "initiator->responder plaintext must survive");

    // Responder -> initiator.
    let reply = b"Reply from the responder";
    let ct = responder.encrypt(reply).expect("responder encrypts reply");
    let pt = initiator
        .decrypt(&ct)
        .expect("initiator must decrypt the responder's reply");
    assert_eq!(pt, reply, "responder->initiator plaintext must survive");
}
