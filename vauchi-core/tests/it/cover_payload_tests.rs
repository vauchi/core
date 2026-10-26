// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Cover: what a duress session sends in place of a withheld card update
//! (ADR-032, vauchi/private#811). The receiver treats it as a received
//! message — the ratchet advances and is saved — and otherwise as nothing.

use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::sync::delta::{
    CardDelta, CekWrappedPayload, DeltaError, PAYLOAD_VERSION_COVER, VersionedPayload,
    encode_cover_payload,
};
use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi,
    crypto::cek::ContentEncryptionKey, crypto::ratchet::DoubleRatchetState, exchange::X3DHKeyPair,
};

/// Alice as the responder, Bob's initiator ratchet held by the test.
fn alice_receiving_from_bob() -> (Vauchi, String, Identity, DoubleRatchetState) {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();
    let bob_identity = Identity::create("Bob", 0);
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let bob_contact = Contact::from_exchange(
        *bob_identity.signing_public_key(),
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
    (alice, bob_id, bob_identity, bob_ratchet)
}

fn sealed(bob_ratchet: &mut DoubleRatchetState, payload: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&bob_ratchet.encrypt(payload).unwrap()).unwrap()
}

fn sealed_update(
    bob_identity: &Identity,
    bob_ratchet: &mut DoubleRatchetState,
    alice_pk: &[u8; 32],
) -> Vec<u8> {
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "work",
            "bob@work.com",
            0,
        ))
        .unwrap();
    let mut delta = CardDelta::compute(&old_card, &new_card, 0);
    delta.set_version(1);
    delta.sign(bob_identity, alice_pk);
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext: cek.encrypt(&delta_bytes).unwrap(),
        signature: delta.signature,
        nonce: delta.nonce,
    };
    sealed(bob_ratchet, &VersionedPayload::encode_cek(&wrapped))
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn cover_advances_the_ratchet_and_changes_nothing() {
    let (alice, bob_id, bob_identity, mut bob_ratchet) = alice_receiving_from_bob();
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    let cover = sealed(&mut bob_ratchet, &encode_cover_payload(&[0x5a; 200]));
    let outcome =
        process_single_card_update(alice.identity().unwrap(), alice.storage(), &bob_id, &cover);

    assert!(
        matches!(outcome, Ok(ReceiveOutcome::Cover)),
        "got {outcome:?}"
    );
    let bob_at_alice = alice.get_contact(&bob_id).unwrap().unwrap();
    assert_eq!(bob_at_alice.card().display_name(), "Bob");
    assert!(bob_at_alice.card().fields().is_empty());

    // The next real update decrypts on the saved chain, with no key skipped.
    let update = sealed_update(&bob_identity, &mut bob_ratchet, &alice_pk);
    let outcome =
        process_single_card_update(alice.identity().unwrap(), alice.storage(), &bob_id, &update);
    assert!(
        matches!(outcome, Ok(ReceiveOutcome::CardDelta)),
        "got {outcome:?}"
    );
    let (ratchet, _) = alice
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_id)
        .unwrap()
        .expect("Alice's ratchet for Bob");
    assert_eq!(ratchet.skipped_keys_count(), 0);
    let bob_at_alice = alice.get_contact(&bob_id).unwrap().unwrap();
    assert_eq!(bob_at_alice.card().fields().len(), 1);
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn cover_from_a_blocked_contact_is_refused_like_an_update() {
    let (alice, bob_id, _bob_identity, mut bob_ratchet) = alice_receiving_from_bob();
    alice.block_contact(&bob_id).unwrap();

    let cover = sealed(&mut bob_ratchet, &encode_cover_payload(&[0x5a; 200]));
    let outcome =
        process_single_card_update(alice.identity().unwrap(), alice.storage(), &bob_id, &cover);

    assert!(
        matches!(outcome, Err(CardUpdateError::ContactBlocked)),
        "got {outcome:?}"
    );
}

// @internal
#[test]
fn cover_payload_codec() {
    let encoded = encode_cover_payload(&[7; 16]);
    assert_eq!(encoded.len(), 17);
    assert_eq!(encoded[0], PAYLOAD_VERSION_COVER);
    assert!(matches!(
        VersionedPayload::decode(&encoded),
        Ok(VersionedPayload::Cover)
    ));
    assert!(matches!(
        VersionedPayload::decode(&encode_cover_payload(&[])),
        Ok(VersionedPayload::Cover)
    ));

    assert!(matches!(
        VersionedPayload::decode(&[0x0b, 1, 2, 3]),
        Err(DeltaError::UnknownPayloadVersion(0x0b))
    ));
    assert!(matches!(
        VersionedPayload::decode(&[]),
        Err(DeltaError::PayloadTooShort)
    ));
}
