// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian keeps the newest complete backup each contact sent it
//! (vauchi/private#796 step 1d, D7): pieces wait until every one is in, a
//! delivery that does not add up is dropped without touching the kept one,
//! at most two unfinished backups wait per contact, and a deleted contact
//! takes what it left behind with it.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::storage::KeptPiece;
use vauchi_core::sync::guardian_piece::{
    GuardianBackupPiece, GuardianDelivery, PieceError, split_into_pieces,
};
use vauchi_core::{Storage, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

const NOW: u64 = 1_700_000_000;

/// A guardian with one contact, the designator; returns its contact id.
fn guardian() -> (Vauchi, String) {
    let wb = create_vauchi_with_identity("Guardian");
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    let contact =
        Contact::from_exchange(pk, ContactCard::new("Alice"), SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    wb.storage().contacts().save_contact(&contact).unwrap();
    (wb, id)
}

fn delivery(created_at: u64, fill: u8) -> GuardianDelivery {
    GuardianDelivery::new(created_at, vec![fill; 124], vec![fill; 120_000]).unwrap()
}

fn pieces(backup: u8, delivery: &GuardianDelivery) -> Vec<GuardianBackupPiece> {
    split_into_pieces([backup; 16], &delivery.encode()).unwrap()
}

fn tampered(piece: &GuardianBackupPiece) -> GuardianBackupPiece {
    let mut bytes = piece.encode();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    GuardianBackupPiece::decode(&bytes).unwrap()
}

fn keep_all(wb: &Vauchi, designator: &str, pieces: Vec<GuardianBackupPiece>) -> KeptPiece {
    let mut last = KeptPiece::Waiting;
    for piece in pieces {
        last = wb
            .storage()
            .guardian_backups()
            .keep_piece(designator, &piece, NOW)
            .unwrap();
    }
    last
}

fn kept(wb: &Vauchi, designator: &str) -> Option<GuardianDelivery> {
    wb.storage().guardian_backups().kept(designator).unwrap()
}

fn unfinished(wb: &Vauchi, designator: &str) -> usize {
    wb.storage()
        .guardian_backups()
        .unfinished_backups(designator)
        .unwrap()
}

// @internal
#[test]
fn a_guardian_keeps_a_backup_once_every_piece_is_in_and_not_before() {
    let (wb, alice) = guardian();
    let sent = delivery(NOW, 1);
    let mut all = pieces(1, &sent);
    let last = all.pop().unwrap();

    assert_eq!(keep_all(&wb, &alice, all), KeptPiece::Waiting);
    assert_eq!(kept(&wb, &alice), None);
    assert_eq!(unfinished(&wb, &alice), 1);

    assert_eq!(keep_all(&wb, &alice, vec![last]), KeptPiece::Kept);
    assert_eq!(kept(&wb, &alice), Some(sent));
    assert_eq!(unfinished(&wb, &alice), 0);
}

// @internal
#[test]
fn a_newer_backup_replaces_the_kept_one_and_an_older_one_does_not() {
    let (wb, alice) = guardian();
    let middle = delivery(NOW + 100, 2);
    let older = delivery(NOW, 1);
    let newer = delivery(NOW + 200, 3);

    assert_eq!(keep_all(&wb, &alice, pieces(2, &middle)), KeptPiece::Kept);
    assert_eq!(keep_all(&wb, &alice, pieces(1, &older)), KeptPiece::Older);
    assert_eq!(kept(&wb, &alice), Some(middle));
    assert_eq!(keep_all(&wb, &alice, pieces(3, &newer)), KeptPiece::Kept);
    assert_eq!(kept(&wb, &alice), Some(newer));
    assert_eq!(unfinished(&wb, &alice), 0);
}

// @internal
#[test]
fn a_backup_that_misses_its_hash_is_dropped_and_the_kept_one_stays() {
    let (wb, alice) = guardian();
    let good = delivery(NOW, 1);
    keep_all(&wb, &alice, pieces(1, &good));
    let mut bad = pieces(2, &delivery(NOW + 1, 2));
    bad[1] = tampered(&bad[1]);

    assert_eq!(
        keep_all(&wb, &alice, bad),
        KeptPiece::Dropped(PieceError::HashMismatch)
    );
    assert_eq!(kept(&wb, &alice), Some(good));
    assert_eq!(unfinished(&wb, &alice), 0);
}

// @internal
#[test]
fn a_conflicting_copy_of_a_piece_drops_that_backup() {
    let (wb, alice) = guardian();
    let all = pieces(1, &delivery(NOW, 1));
    let store = wb.storage().guardian_backups();

    assert_eq!(
        store.keep_piece(&alice, &all[0], NOW).unwrap(),
        KeptPiece::Waiting
    );
    assert_eq!(
        store.keep_piece(&alice, &all[0], NOW).unwrap(),
        KeptPiece::Waiting
    );
    assert_eq!(
        store.keep_piece(&alice, &tampered(&all[0]), NOW).unwrap(),
        KeptPiece::Dropped(PieceError::ConflictingDuplicate)
    );
    assert_eq!(unfinished(&wb, &alice), 0);
}

// @internal
#[test]
fn a_third_unfinished_backup_evicts_the_oldest_and_the_others_still_complete() {
    let (wb, alice) = guardian();
    let first = pieces(1, &delivery(NOW, 1));
    let second = pieces(2, &delivery(NOW + 1, 2));
    let third = pieces(3, &delivery(NOW + 2, 3));
    let store = wb.storage().guardian_backups();
    store.keep_piece(&alice, &first[0], NOW).unwrap();
    store.keep_piece(&alice, &second[0], NOW + 1).unwrap();

    store.keep_piece(&alice, &third[0], NOW + 2).unwrap();

    assert_eq!(unfinished(&wb, &alice), 2);
    assert_eq!(
        keep_all(&wb, &alice, first[1..].to_vec()),
        KeptPiece::Waiting,
        "the oldest backup lost its first piece"
    );
    assert_eq!(keep_all(&wb, &alice, third[1..].to_vec()), KeptPiece::Kept);
}

/// Both tables are encrypted at rest, so a storage key rotation must carry
/// them over: the kept backup still reads, and an unfinished one still
/// completes.
// @internal
#[test]
fn a_kept_backup_and_waiting_pieces_survive_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    let contact =
        Contact::from_exchange(pk, ContactCard::new("Alice"), SymmetricKey::generate(), 0);
    let alice = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    let kept_before = delivery(NOW, 1);
    for piece in pieces(1, &kept_before) {
        storage
            .guardian_backups()
            .keep_piece(&alice, &piece, NOW)
            .unwrap();
    }
    let waiting = pieces(2, &delivery(NOW + 1, 2));
    storage
        .guardian_backups()
        .keep_piece(&alice, &waiting[0], NOW)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage.guardian_backups().kept(&alice).unwrap(),
        Some(kept_before)
    );
    let mut last = KeptPiece::Waiting;
    for piece in &waiting[1..] {
        last = storage
            .guardian_backups()
            .keep_piece(&alice, piece, NOW)
            .unwrap();
    }
    assert_eq!(last, KeptPiece::Kept);
}

// @internal
#[test]
fn a_deleted_contact_takes_its_kept_backup_with_it() {
    let (wb, alice) = guardian();
    keep_all(&wb, &alice, pieces(1, &delivery(NOW, 1)));

    assert!(wb.remove_contact(&alice).unwrap());

    assert_eq!(kept(&wb, &alice), None);
}

// @internal
#[test]
fn designating_three_guardians_leaves_each_keeping_its_share_and_the_same_backup() {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let mut guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }

    let deliveries: Vec<GuardianDelivery> = guardians
        .iter_mut()
        .map(|guardian| {
            receive(&alice, guardian);
            kept(guardian.vauchi(), guardian.sharer_id()).expect("a kept backup")
        })
        .collect();

    assert!(
        deliveries
            .iter()
            .all(|d| d.backup() == deliveries[0].backup())
    );
    assert_ne!(deliveries[0].sealed_share(), deliveries[1].sealed_share());
    let new_pk = *SigningKeyPair::generate().public_key().as_bytes();
    for (guardian, delivery) in guardians.iter().zip(&deliveries) {
        let resealed = guardian
            .vauchi()
            .respond_to_recovery(delivery.sealed_share(), &new_pk)
            .expect("each guardian opens its own share");
        assert_eq!(resealed.len(), 124, "a share re-sealed to the new key");
    }
}
