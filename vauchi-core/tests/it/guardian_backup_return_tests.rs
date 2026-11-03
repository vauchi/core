// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian returns the backup to a recovering identity as its own
//! message kind, `0x0d` (vauchi/private#796 step 2a, D13), so the new
//! identity keeps it as returned and never as a designation (step 1d).

use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{
    PAYLOAD_VERSION_RETURNED_BACKUP_PIECE, VersionedPayload, encode_returned_backup_piece,
};
use vauchi_core::sync::guardian_piece::{GuardianDelivery, split_into_pieces};
use vauchi_core::{Storage, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;

/// The recovering identity, with the guardian's initiator ratchet held by
/// the test.
fn new_identity_receiving_from_guardian() -> (Vauchi, String, DoubleRatchetState) {
    let recovering = create_vauchi_with_identity("Alice, new phone");
    let guardian = create_vauchi_with_identity("Guardian");
    let guardian_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let guardian_contact = Contact::from_exchange(
        *guardian.identity().unwrap().signing_public_key(),
        ContactCard::new("Guardian"),
        shared_secret.clone(),
        0,
    );
    let guardian_id = guardian_contact.id().to_string();
    recovering.add_contact(guardian_contact).unwrap();
    recovering
        .create_ratchet_as_responder(
            &guardian_id,
            &shared_secret,
            X3DHKeyPair::from_bytes(*guardian_dh.secret_bytes()),
        )
        .unwrap();
    let guardian_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *guardian_dh.public_key())
            .unwrap();
    (recovering, guardian_id, guardian_ratchet)
}

fn returned() -> GuardianDelivery {
    GuardianDelivery::new(1_700_000_000, vec![4; 124], vec![8; 70_000]).unwrap()
}

// @internal
#[test]
fn a_returned_backup_piece_is_its_own_message_kind() {
    let piece = split_into_pieces([2; 16], &returned().encode())
        .unwrap()
        .remove(0);

    let encoded = encode_returned_backup_piece(&piece);

    assert_eq!(PAYLOAD_VERSION_RETURNED_BACKUP_PIECE, 0x0d);
    assert_eq!(encoded[0], 0x0d);
    match VersionedPayload::decode(&encoded) {
        Ok(VersionedPayload::ReturnedBackupPiece(decoded)) => assert_eq!(decoded, piece),
        other => panic!("got {other:?}"),
    }
}

// @internal
#[test]
fn a_returned_backup_is_kept_as_returned_and_never_as_a_designation() {
    let (recovering, guardian_id, mut guardian_ratchet) = new_identity_receiving_from_guardian();
    let delivery = returned();

    for piece in split_into_pieces([2; 16], &delivery.encode()).unwrap() {
        let message = serde_json::to_vec(
            &guardian_ratchet
                .encrypt(&encode_returned_backup_piece(&piece))
                .unwrap(),
        )
        .unwrap();
        let outcome = process_single_card_update(
            recovering.identity().unwrap(),
            recovering.storage(),
            &guardian_id,
            &message,
        );
        assert!(
            matches!(outcome, Ok(ReceiveOutcome::ReturnedBackupPiece(_))),
            "got {outcome:?}"
        );
    }

    assert_eq!(
        recovering
            .storage()
            .returned_backups()
            .kept(&guardian_id)
            .unwrap(),
        Some(delivery)
    );
    assert_eq!(
        recovering
            .storage()
            .guardian_backups()
            .kept(&guardian_id)
            .unwrap(),
        None
    );
}

// @internal
#[test]
fn a_returned_backup_survives_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    let contact = Contact::from_exchange(
        pk,
        ContactCard::new("Guardian"),
        SymmetricKey::generate(),
        0,
    );
    let guardian = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    let delivery = returned();
    let mut pieces = split_into_pieces([2; 16], &delivery.encode()).unwrap();
    let last = pieces.pop().unwrap();
    for piece in &pieces {
        storage
            .returned_backups()
            .keep_piece(&guardian, piece, 1)
            .unwrap();
    }

    storage.rekey(SymmetricKey::generate()).unwrap();

    storage
        .returned_backups()
        .keep_piece(&guardian, &last, 1)
        .unwrap();
    assert_eq!(
        storage.returned_backups().kept(&guardian).unwrap(),
        Some(delivery)
    );
}
