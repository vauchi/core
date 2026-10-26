// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The resync request (vauchi/private#691, maintainer 2026-10-26): a client
//! whose catch-up started behind the relay's retention asks each contact
//! to repropagate its visible card now. Ratchet-sealed like any update;
//! the receiver honors it at most once per contact per day and refuses it
//! from a blocked contact.

use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{
    DeltaError, PAYLOAD_VERSION_RESYNC_REQUEST, VersionedPayload, encode_resync_request,
};
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;

/// Alice as the responder, with Bob's initiator ratchet held by the test.
fn alice_receiving_from_bob() -> (Vauchi, String, DoubleRatchetState) {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let bob_contact = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        ContactCard::new("Bob"),
        shared_secret.clone(),
        0,
    );
    let bob_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();
    alice
        .create_ratchet_as_responder(
            &bob_id,
            &shared_secret,
            X3DHKeyPair::from_bytes(*bob_dh.secret_bytes()),
        )
        .unwrap();
    let bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    (alice, bob_id, bob_ratchet)
}

fn sealed(bob_ratchet: &mut DoubleRatchetState, payload: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&bob_ratchet.encrypt(payload).unwrap()).unwrap()
}

fn pending(wb: &Vauchi) -> usize {
    wb.storage().pending().count_all_pending_updates().unwrap()
}

// @internal
#[test]
fn resync_request_codec() {
    let encoded = encode_resync_request();
    assert_eq!(encoded, vec![PAYLOAD_VERSION_RESYNC_REQUEST]);
    assert!(matches!(
        VersionedPayload::decode(&encoded),
        Ok(VersionedPayload::ResyncRequest)
    ));
    assert!(matches!(
        VersionedPayload::decode(&[0x0b]),
        Err(DeltaError::UnknownPayloadVersion(0x0b))
    ));
}

// @scenario: message_delivery :: A client away longer than retention asks its contacts for their cards
#[test]
fn a_resync_request_is_received_as_such_and_advances_the_ratchet() {
    let (alice, bob_id, mut bob_ratchet) = alice_receiving_from_bob();

    let request = sealed(&mut bob_ratchet, &encode_resync_request());
    let outcome = process_single_card_update(
        alice.identity().unwrap(),
        alice.storage(),
        &bob_id,
        &request,
    );

    assert!(
        matches!(outcome, Ok(ReceiveOutcome::ResyncRequest)),
        "got {outcome:?}"
    );
    let (ratchet, _) = alice
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_id)
        .unwrap()
        .expect("Alice's ratchet for Bob");
    assert_eq!(ratchet.skipped_keys_count(), 0);
}

// @scenario: message_delivery :: A client away longer than retention asks its contacts for their cards
#[test]
fn a_resync_request_from_a_blocked_contact_is_refused() {
    let (alice, bob_id, mut bob_ratchet) = alice_receiving_from_bob();
    alice.block_contact(&bob_id).unwrap();

    let request = sealed(&mut bob_ratchet, &encode_resync_request());
    let outcome = process_single_card_update(
        alice.identity().unwrap(),
        alice.storage(),
        &bob_id,
        &request,
    );

    assert!(
        matches!(outcome, Err(CardUpdateError::ContactBlocked)),
        "got {outcome:?}"
    );
}

// @scenario: message_delivery :: A client away longer than retention asks its contacts for their cards
#[test]
fn honoring_a_request_queues_one_repropagation_per_day_and_contact() {
    let (alice, bob_id, mut bob_ratchet) = alice_receiving_from_bob();
    let field = ContactField::new(FieldType::Phone, "Phone", "+41 79 000 00 00", 0);
    let field_id = field.id().to_string();
    alice.add_own_field(field).unwrap();
    alice.set_own_field_public(&field_id).unwrap();
    // The request itself is Bob's first message: it gives Alice, the
    // responder, her sending chain.
    let request = sealed(&mut bob_ratchet, &encode_resync_request());
    process_single_card_update(
        alice.identity().unwrap(),
        alice.storage(),
        &bob_id,
        &request,
    )
    .unwrap();
    alice.run_owed_repropagation().unwrap();
    let before = pending(&alice);

    assert!(alice.honor_resync_request(&bob_id).unwrap());
    assert_eq!(
        pending(&alice),
        before + 1,
        "the visible card is sent again"
    );

    assert!(
        !alice.honor_resync_request(&bob_id).unwrap(),
        "a second request the same day is not honored"
    );
    assert_eq!(pending(&alice), before + 1);
}

// @scenario: message_delivery :: A client away longer than retention asks its contacts for their cards
#[test]
fn requesting_a_resync_queues_one_request_per_ratcheted_contact() {
    // Alice as the initiator: she can send Bob a message first.
    let alice = create_vauchi_with_identity("Alice");
    let bob = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let bob_id = bob.id().to_string();
    alice.add_contact(bob).unwrap();
    let dh = X3DHKeyPair::generate();
    alice
        .create_ratchet_as_initiator(&bob_id, &SymmetricKey::generate(), *dh.public_key())
        .unwrap();
    let unratcheted = Contact::from_exchange(
        [9; 32],
        ContactCard::new("No session yet"),
        SymmetricKey::generate(),
        0,
    );
    alice.add_contact(unratcheted).unwrap();
    let before = pending(&alice);

    let requested = alice.request_card_resync().unwrap();

    assert_eq!(requested, 1);
    assert_eq!(pending(&alice), before + 1);
    let queued = alice
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].update_type, "card_delta");
}
