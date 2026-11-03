// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Restoring the guardian backup into the new identity (vauchi/private#796
//! step 3, D6): with one returned share of every group the new phone opens
//! the backup its guardians handed back; Alice's contacts, labels and own
//! card come back under the new identity, each restored exchanged contact
//! waits for its rekey, and a guardian she just met again keeps the new
//! relationship.

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{FAMILY_FIELD, PASSPHRASE, World, recovered};

/// The restore world with a plain new phone.
fn world(passphrase: Option<&str>, handing_back: usize) -> World {
    recovered(
        create_vauchi_with_identity("Alice, new phone"),
        passphrase,
        handing_back,
    )
}

// @internal
#[test]
fn the_new_identity_gets_back_the_contacts_labels_and_own_card_and_keeps_its_key() {
    let world = world(None, 3);
    let new_pk = signing_pk(&world.phone);

    let restored = world.phone.restore_from_returned_backups(None).unwrap();

    assert_eq!(restored, 1, "Dave; the guardians were met again");
    let dave = world.phone.get_contact(&world.dave_id).unwrap().unwrap();
    assert_eq!(dave.display_name(), "Dave");
    let family: Vec<_> = world
        .phone
        .list_groups()
        .unwrap()
        .into_iter()
        .filter(|group| group.name() == "Family")
        .collect();
    assert_eq!(family.len(), 1);
    assert!(family[0].contacts().contains(&world.dave_id));
    assert_eq!(
        world.phone.own_card().unwrap().unwrap().display_name(),
        "Alice"
    );
    assert_eq!(signing_pk(&world.phone), new_pk, "D6: a new identity");
}

// @internal
#[test]
fn a_guardian_met_again_keeps_the_new_relationship() {
    let world = world(None, 3);
    let met = world
        .phone
        .get_contact(&world.guardians_at_phone[0])
        .unwrap()
        .unwrap();

    world.phone.restore_from_returned_backups(None).unwrap();

    let after = world
        .phone
        .get_contact(&world.guardians_at_phone[0])
        .unwrap()
        .unwrap();
    assert_eq!(
        after.shared_key().map(|key| *key.as_bytes()),
        met.shared_key().map(|key| *key.as_bytes())
    );
    assert_eq!(after.display_name(), met.display_name());
}

// @internal
#[test]
fn each_restored_exchanged_contact_waits_for_its_rekey_and_a_guardian_met_again_does_not() {
    let world = world(None, 3);

    world.phone.restore_from_returned_backups(None).unwrap();

    assert_eq!(
        world.phone.rekey_pending_contacts().unwrap(),
        vec![world.dave_id.clone()]
    );
}

// @internal
#[test]
fn without_a_share_of_every_group_nothing_is_restored() {
    let world = world(None, 2);

    assert_eq!(world.phone.guardians_handed_back().unwrap(), 2);
    assert!(world.phone.restore_from_returned_backups(None).is_err());

    assert!(world.phone.get_contact(&world.dave_id).unwrap().is_none());
    assert!(world.phone.rekey_pending_contacts().unwrap().is_empty());
}

// @internal
#[test]
fn with_nothing_handed_back_nothing_is_restored() {
    let world = world(None, 0);

    assert_eq!(world.phone.guardians_handed_back().unwrap(), 0);
    assert!(!world.phone.returned_backup_needs_passphrase().unwrap());
    assert!(world.phone.restore_from_returned_backups(None).is_err());
}

// @internal
#[test]
fn a_backup_sealed_under_the_passphrase_restores_only_with_it() {
    let world = world(Some(PASSPHRASE), 3);

    assert!(world.phone.returned_backup_needs_passphrase().unwrap());
    assert!(world.phone.restore_from_returned_backups(None).is_err());
    assert!(
        world
            .phone
            .restore_from_returned_backups(Some("the owl flies at dusk"))
            .is_err()
    );
    assert!(world.phone.get_contact(&world.dave_id).unwrap().is_none());

    assert_eq!(
        world
            .phone
            .restore_from_returned_backups(Some(PASSPHRASE))
            .unwrap(),
        1
    );
}

// @internal
#[test]
fn restoring_again_restores_nothing_new() {
    let world = world(None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();

    assert_eq!(world.phone.restore_from_returned_backups(None).unwrap(), 0);
    assert_eq!(
        world.phone.rekey_pending_contacts().unwrap(),
        vec![world.dave_id.clone()]
    );
    assert_eq!(world.guardians.len(), 3);
}

// @internal
#[test]
fn a_restored_label_shows_the_fields_it_showed() {
    let world = world(None, 3);

    world.phone.restore_from_returned_backups(None).unwrap();

    let family = world
        .phone
        .list_groups()
        .unwrap()
        .into_iter()
        .find(|group| group.name() == "Family")
        .unwrap();
    assert_eq!(
        family.visible_fields().iter().collect::<Vec<_>>(),
        vec![FAMILY_FIELD]
    );
}
