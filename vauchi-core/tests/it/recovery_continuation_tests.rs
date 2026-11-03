// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in-person rekey (vauchi/private#796, D20): when Alice's new phone
//! and a contact have exchanged in person, before or after her restore,
//! her phone sends that contact a continuation over their ratchet, signed
//! with her old key from the restored backup. The contact's device folds
//! its old contact for her into the one the meeting made; nobody is asked,
//! and it works with the contact offline until they meet.

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::rekey::RecoveryContinuation;
use vauchi_core::{Contact, ContactCard, Identity, Storage, SymmetricKey};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{deliver, meet, signing_pk};
use crate::common::recovered_phone::recovered;

// @internal
#[test]
fn a_guardian_met_before_the_restore_folds_its_old_contact_into_the_new_one() {
    let world = recovered(create_vauchi_with_identity("Alice, new phone"), None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let guardian = world.guardians[0].vauchi();
    let alice_old_at_guardian = world.guardians[0].sharer_id().to_string();
    let phone_at_guardian = hex::encode(signing_pk(&world.phone));
    let met = guardian.get_contact(&phone_at_guardian).unwrap().unwrap();

    assert_eq!(world.phone.send_owed_recovery_continuations().unwrap(), 3);
    deliver(
        &world.phone,
        &world.guardians_at_phone[0],
        guardian,
        &phone_at_guardian,
    );

    assert!(
        guardian
            .get_contact(&alice_old_at_guardian)
            .unwrap()
            .is_none()
    );
    let folded = guardian.get_contact(&phone_at_guardian).unwrap().unwrap();
    assert_eq!(
        folded.shared_key().unwrap().as_bytes(),
        met.shared_key().unwrap().as_bytes()
    );
    assert!(
        guardian
            .storage()
            .guardian_backups()
            .kept(&phone_at_guardian)
            .unwrap()
            .is_some(),
        "the backup it keeps for Alice follows her"
    );
}

// @internal
#[test]
fn a_contact_met_after_the_restore_folds_and_gets_no_more_offers() {
    let world = recovered(create_vauchi_with_identity("Alice, new phone"), None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let dave = world.dave.vauchi();
    let alice_old_at_dave = world.dave.sharer_id().to_string();
    // The phone starts the exchange, so it can send at once; a phone that
    // cannot send yet owes the continuation (the guardian case above).
    let (phone_at_dave, dave_at_phone) = meet(dave, &world.phone, false);

    world.phone.send_owed_recovery_continuations().unwrap();
    deliver(&world.phone, &dave_at_phone, dave, &phone_at_dave);

    assert!(dave.get_contact(&alice_old_at_dave).unwrap().is_none());
    assert!(dave.get_contact(&phone_at_dave).unwrap().is_some());
    assert!(
        world
            .phone
            .rekey_offer_deliveries()
            .unwrap()
            .iter()
            .all(|delivery| delivery.contact_id != dave_at_phone)
    );
    assert!(
        !world
            .phone
            .rekey_pending_contacts()
            .unwrap()
            .contains(&dave_at_phone)
    );
}

// @internal
#[test]
fn a_contact_not_yet_met_is_owed_no_continuation() {
    let world = recovered(create_vauchi_with_identity("Alice, new phone"), None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();

    world.phone.send_owed_recovery_continuations().unwrap();

    assert!(
        world
            .phone
            .rekey_pending_contacts()
            .unwrap()
            .contains(&world.dave_id),
        "Dave still waits, for the remote offer"
    );
}

/// A storage with Alice's old contact and the contact a meeting with her
/// new phone made; returns the old key pair and both ids.
fn met_again() -> (Storage, SigningKeyPair, String, [u8; 32]) {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let old_keys = SigningKeyPair::generate();
    let old = Contact::from_exchange(
        *old_keys.public_key().as_bytes(),
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    let new_pk = *Identity::create("Alice, new phone", 0).signing_public_key();
    let new = Contact::from_exchange(
        new_pk,
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&old).unwrap();
    storage.contacts().save_contact(&new).unwrap();
    (storage, old_keys, old.id().to_string(), new_pk)
}

// @internal
#[test]
fn a_continuation_folds_only_when_the_old_key_signed_it_and_it_names_its_sender() {
    let (storage, old_keys, old_id, new_pk) = met_again();
    let new_id = hex::encode(new_pk);
    let other_pk = *Identity::create("Someone else", 0).signing_public_key();

    let forged = RecoveryContinuation::create(&SigningKeyPair::generate(), &new_pk);
    assert!(
        !storage
            .honour_recovery_continuation(&new_id, &forged)
            .unwrap()
    );
    let for_another = RecoveryContinuation::create(&old_keys, &other_pk);
    assert!(
        !storage
            .honour_recovery_continuation(&new_id, &for_another)
            .unwrap()
    );
    assert!(storage.contacts().load_contact(&old_id).unwrap().is_some());

    let honest = RecoveryContinuation::create(&old_keys, &new_pk);
    assert!(
        storage
            .honour_recovery_continuation(&new_id, &honest)
            .unwrap()
    );
    assert!(storage.contacts().load_contact(&old_id).unwrap().is_none());
}

// @internal
#[test]
fn a_continuation_for_a_blocked_contact_folds_nothing() {
    let (storage, old_keys, old_id, new_pk) = met_again();
    let mut old = storage.contacts().load_contact(&old_id).unwrap().unwrap();
    old.block();
    storage.contacts().save_contact(&old).unwrap();

    let continuation = RecoveryContinuation::create(&old_keys, &new_pk);

    assert!(
        !storage
            .honour_recovery_continuation(&hex::encode(new_pk), &continuation)
            .unwrap()
    );
    assert!(storage.contacts().load_contact(&old_id).unwrap().is_some());
}

// @internal
#[test]
fn a_continuation_round_trips_and_a_flipped_byte_breaks_it() {
    let old_keys = SigningKeyPair::generate();
    let new_pk = [5; 32];
    let continuation = RecoveryContinuation::create(&old_keys, &new_pk);

    let bytes = continuation.encode();
    assert_eq!(RecoveryContinuation::decode(&bytes).unwrap(), continuation);
    assert!(continuation.verify());
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 1;
        assert!(
            RecoveryContinuation::decode(&flipped).is_ok_and(|c| !c.verify()),
            "{at}"
        );
    }
    assert!(RecoveryContinuation::decode(&bytes[1..]).is_err());
}

// @internal
#[test]
fn the_old_identitys_seed_kept_at_restore_survives_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage.recovery().save_old_identity_seed(&[3; 32]).unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage
            .recovery()
            .load_old_identity_seed()
            .unwrap()
            .as_deref(),
        Some(&[3u8; 32])
    );
}
