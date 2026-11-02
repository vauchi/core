// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian backup piece travels as its own ratchet-sealed message kind,
//! `0x0b` (vauchi/private#796, decision D9): received as such from a
//! contact, refused from a blocked one, refused when malformed.

use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{
    DeltaError, PAYLOAD_VERSION_GUARDIAN_BACKUP_PIECE, VersionedPayload,
    encode_guardian_backup_piece,
};
use vauchi_core::sync::guardian_piece::{GuardianBackupPiece, split_into_pieces};
use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;

/// Bob as the guardian, with Alice's initiator ratchet held by the test.
fn bob_receiving_from_alice() -> (Vauchi, String, DoubleRatchetState) {
    let bob = create_vauchi_with_identity("Bob");
    let alice = create_vauchi_with_identity("Alice");
    let alice_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let alice_contact = Contact::from_exchange(
        *alice.identity().unwrap().signing_public_key(),
        ContactCard::new("Alice"),
        shared_secret.clone(),
        0,
    );
    let alice_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();
    bob.create_ratchet_as_responder(
        &alice_id,
        &shared_secret,
        X3DHKeyPair::from_bytes(*alice_dh.secret_bytes()),
    )
    .unwrap();
    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *alice_dh.public_key()).unwrap();
    (bob, alice_id, alice_ratchet)
}

fn sealed(ratchet: &mut DoubleRatchetState, payload: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&ratchet.encrypt(payload).unwrap()).unwrap()
}

fn a_piece() -> GuardianBackupPiece {
    split_into_pieces([5; 16], &[7; 60_000]).unwrap().remove(1)
}

// @internal
#[test]
fn a_guardian_backup_piece_encodes_under_its_own_version_byte_and_decodes_back() {
    let piece = a_piece();

    let encoded = encode_guardian_backup_piece(&piece);

    assert_eq!(encoded[0], PAYLOAD_VERSION_GUARDIAN_BACKUP_PIECE);
    assert_eq!(PAYLOAD_VERSION_GUARDIAN_BACKUP_PIECE, 0x0b);
    match VersionedPayload::decode(&encoded) {
        Ok(VersionedPayload::GuardianBackupPiece(decoded)) => assert_eq!(decoded, piece),
        other => panic!("got {other:?}"),
    }
}

// @internal
#[test]
fn a_malformed_guardian_backup_piece_is_refused_at_decode() {
    let mut encoded = encode_guardian_backup_piece(&a_piece());
    encoded.truncate(40);

    assert!(matches!(
        VersionedPayload::decode(&encoded),
        Err(DeltaError::InvalidPayload(_))
    ));
}

// @internal
#[test]
fn a_guardian_backup_piece_from_a_contact_is_received_as_such_and_advances_the_ratchet() {
    let (bob, alice_id, mut alice_ratchet) = bob_receiving_from_alice();
    let piece = a_piece();

    let message = sealed(&mut alice_ratchet, &encode_guardian_backup_piece(&piece));
    let outcome =
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &message);

    match outcome {
        Ok(ReceiveOutcome::GuardianBackupPiece(received)) => assert_eq!(received, piece),
        other => panic!("got {other:?}"),
    }
    let (ratchet, _) = bob
        .storage()
        .ratchets()
        .load_ratchet_state(&alice_id)
        .unwrap()
        .expect("Bob's ratchet for Alice");
    assert_eq!(ratchet.skipped_keys_count(), 0);
}

// @internal
#[test]
fn a_guardian_backup_piece_from_a_blocked_contact_is_refused() {
    let (bob, alice_id, mut alice_ratchet) = bob_receiving_from_alice();
    bob.block_contact(&alice_id).unwrap();

    let message = sealed(
        &mut alice_ratchet,
        &encode_guardian_backup_piece(&a_piece()),
    );
    let outcome =
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &message);

    assert!(
        matches!(outcome, Err(CardUpdateError::ContactBlocked)),
        "got {outcome:?}"
    );
}
