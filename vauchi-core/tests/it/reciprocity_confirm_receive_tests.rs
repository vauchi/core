// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core receive-side of the P3 relay-sync reciprocity confirmation (Slice A).
//!
//! A signed `ReciprocityConfirmPayload` (`VersionedPayload` 0x03) arriving over
//! the card-update envelope is Ed25519-verified (sender + recipient binding)
//! and surfaced as `ReceiveOutcome::ReciprocityConfirm`, carrying the peer's
//! token for the app-layer match against `expected_their_token` → Confirmed.
//! Mirrors the safety-alert receive path. Design:
//! `2026-07-04-reciprocity-p3-relay-sync-plan.md`.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{ReciprocityConfirmPayload, VersionedPayload};

/// Two mutual contacts with ratchets stored: Bob is the initiator (can
/// encrypt/send), Alice the responder (can decrypt/receive). Mirrors the
/// safety-alert suite; kept local so this suite is self-contained.
fn setup_two_party() -> (vauchi_core::Vauchi, vauchi_core::Vauchi, String, String) {
    let alice_wb = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared_secret = SymmetricKey::generate();

    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        shared_secret.clone(),
        0,
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    let alice_dh = X3DHKeyPair::generate();
    let bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *alice_dh.public_key()).unwrap();
    let alice_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, alice_dh);

    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, false)
        .unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, true)
        .unwrap();

    (alice_wb, bob_wb, bob_contact_id, alice_contact_id)
}

/// Build the confirmation blob Bob sends Alice: a signed
/// `ReciprocityConfirmPayload` (0x03) encrypted with Bob's ratchet — the same
/// envelope as a card update on the wire. `recipient_pk` is signed over, so a
/// wrong value makes Alice's verify fail (redirect defense).
fn create_reciprocity_blob(
    bob_wb: &vauchi_core::Vauchi,
    recipient_pk: &[u8; 32],
    alice_contact_id: &str,
    token: [u8; 32],
) -> Vec<u8> {
    let bob_identity = bob_wb.identity().unwrap();
    let payload = ReciprocityConfirmPayload::new(token, bob_identity, recipient_pk);
    let encoded = VersionedPayload::encode_reciprocity(&payload);

    let (mut bob_ratchet, is_init) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(alice_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&encoded).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(alice_contact_id, &bob_ratchet, is_init)
        .unwrap();
    serde_json::to_vec(&ratchet_msg).unwrap()
}

// CC-03: a valid signed confirmation is verified + surfaced with its token.
// @scenario: reciprocity :: relay-sync confirmation is verified and surfaced
#[test]
fn received_reciprocity_confirm_is_verified_and_surfaced() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let token = [7u8; 32];

    let blob = create_reciprocity_blob(&bob_wb, &alice_pk, &alice_contact_id, token);

    let outcome = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &blob,
    )
    .expect("a signed confirmation must be received, not dropped");

    match outcome {
        ReceiveOutcome::ReciprocityConfirm {
            sender_id,
            token: got,
        } => {
            assert_eq!(
                sender_id, bob_contact_id,
                "attributed to the sending contact"
            );
            assert_eq!(got, token, "the peer's token is surfaced verbatim");
        }
        other => panic!("expected ReciprocityConfirm, got {other:?}"),
    }
}

// CC-14: a confirmation signed for a DIFFERENT recipient must be rejected —
// the signature binds the recipient, so a redirected/replayed-to-us payload
// fails verification (never surfaced, never confirmable).
// @scenario: reciprocity :: wrong-recipient confirmation is rejected
#[test]
fn wrong_recipient_reciprocity_confirm_is_rejected() {
    let (alice_wb, bob_wb, bob_contact_id, alice_contact_id) = setup_two_party();
    let wrong_recipient = [0xAAu8; 32];

    let blob = create_reciprocity_blob(&bob_wb, &wrong_recipient, &alice_contact_id, [7u8; 32]);

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &blob,
    );

    assert!(
        matches!(result, Err(CardUpdateError::SignatureInvalid)),
        "a confirmation bound to another recipient must fail verification, got {result:?}"
    );
}
