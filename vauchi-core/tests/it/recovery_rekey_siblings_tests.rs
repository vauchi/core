// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A rekey one of the contact's devices took reaches its other devices
//! (vauchi/private#796, D25): the device that accepted journals the move
//! or fold with the new key and shared key, and a sibling applying it
//! holds the same contact, then runs the handshake with the new identity.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::Vauchi;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::sync::device_sync::{ContactRekeySyncData, SyncItem};

use crate::common::device_sync::{journal_for_tablet, link_tablet};
use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{meet, signing_pk};
use crate::common::recovered_phone::{World, recovered};

/// Alice's phone, restored, and the offer it makes Dave.
fn offered() -> (World, Vec<u8>) {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut phone = Vauchi::in_memory_with_clock(clock).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let blob = world.phone.rekey_offer_deliveries().unwrap()[0]
        .blob
        .clone();
    (world, blob)
}

/// Dave's tablet, holding the contacts `ids` as Dave's phone holds them.
fn tablet_holding(dave: &Vauchi, ids: &[&str]) -> Vauchi {
    let tablet = create_vauchi_with_identity("Dave, tablet");
    for id in ids {
        let contact: Contact = dave.get_contact(id).unwrap().unwrap();
        tablet.add_contact(contact).unwrap();
    }
    tablet
}

fn rekeys(journal: &[SyncItem]) -> Vec<&ContactRekeySyncData> {
    journal
        .iter()
        .filter_map(|item| match item {
            SyncItem::ContactRekeyed { rekey, .. } => Some(rekey),
            _ => None,
        })
        .collect()
}

// @internal
#[test]
fn the_device_that_took_the_rekey_tells_its_siblings_the_new_key_and_shared_key() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let (registry, tablet_id) = link_tablet(dave, [61u8; 32]);

    dave.take_rekey_offer(&blob).unwrap();

    let new_pk = signing_pk(&world.phone);
    let moved = dave.get_contact(&hex::encode(new_pk)).unwrap().unwrap();
    let journal = journal_for_tablet(dave, registry, &tablet_id);
    let told = rekeys(&journal);
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].old_contact_id, old_id);
    assert_eq!(told[0].new_public_key.as_bytes(), &new_pk);
    assert_eq!(&told[0].shared_key, moved.shared_key().unwrap().as_bytes());
}

// @internal
#[test]
fn a_sibling_applying_the_rekey_holds_the_contact_under_the_new_key() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let tablet = tablet_holding(dave, &[&old_id]);
    let (registry, tablet_id) = link_tablet(dave, [62u8; 32]);
    dave.take_rekey_offer(&blob).unwrap();
    let journal = journal_for_tablet(dave, registry, &tablet_id);

    tablet.apply_sync_items(journal).unwrap();

    let new_id = hex::encode(signing_pk(&world.phone));
    let at_phone = dave.get_contact(&new_id).unwrap().unwrap();
    let at_tablet = tablet.get_contact(&new_id).unwrap().unwrap();
    assert!(tablet.get_contact(&old_id).unwrap().is_none());
    assert_eq!(at_tablet.public_key(), at_phone.public_key());
    assert_eq!(
        at_tablet.shared_key().unwrap().as_bytes(),
        at_phone.shared_key().unwrap().as_bytes()
    );
    assert_eq!(
        tablet.queue_registry_pushes().unwrap(),
        1,
        "the tablet starts its own handshake with the new identity"
    );
}

// @internal
#[test]
fn a_sibling_folds_the_old_contact_into_the_one_met_in_person() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let (_, phone_at_dave) = meet(&world.phone, dave, true);
    let tablet = tablet_holding(dave, &[&old_id, &phone_at_dave]);
    let met = dave.get_contact(&phone_at_dave).unwrap().unwrap();
    let (registry, tablet_id) = link_tablet(dave, [63u8; 32]);
    dave.take_rekey_offer(&blob).unwrap();
    let journal = journal_for_tablet(dave, registry, &tablet_id);

    tablet.apply_sync_items(journal).unwrap();

    assert!(tablet.get_contact(&old_id).unwrap().is_none());
    let folded = tablet.get_contact(&phone_at_dave).unwrap().unwrap();
    assert_eq!(
        folded.shared_key().unwrap().as_bytes(),
        met.shared_key().unwrap().as_bytes(),
        "the meeting's relationship stays"
    );
}

// @internal
#[test]
fn a_rekey_naming_a_contact_the_sibling_does_not_hold_changes_nothing() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let tablet = create_vauchi_with_identity("Dave, tablet");
    let (registry, tablet_id) = link_tablet(dave, [64u8; 32]);
    dave.take_rekey_offer(&blob).unwrap();
    let journal = journal_for_tablet(dave, registry, &tablet_id);

    tablet.apply_sync_items(journal).unwrap();

    assert!(tablet.list_contacts().unwrap().is_empty());
}

// @internal
#[test]
fn a_rekey_naming_the_old_key_as_the_new_one_changes_nothing() {
    let (world, _) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let before = dave.get_contact(&old_id).unwrap().unwrap();
    let old_pk = *before.public_key().unwrap();

    let applied = dave.apply_sync_items(vec![SyncItem::ContactRekeyed {
        rekey: ContactRekeySyncData {
            old_contact_id: old_id.clone(),
            new_public_key: old_pk.into(),
            shared_key: [9u8; 32],
        },
        timestamp: 1,
    }]);

    assert_eq!(applied.unwrap(), 0);
    let after = dave.get_contact(&old_id).unwrap().unwrap();
    assert_eq!(
        after.shared_key().unwrap().as_bytes(),
        before.shared_key().unwrap().as_bytes()
    );
}

// @internal
#[test]
fn the_rekey_item_keeps_its_shared_key_out_of_debug_output() {
    let rekey = ContactRekeySyncData {
        old_contact_id: "old".into(),
        new_public_key: [3u8; 32].into(),
        shared_key: [201u8; 32],
    };

    let shown = format!("{rekey:?}");

    assert!(shown.contains("old"));
    assert!(!shown.contains("201"), "{shown}");
}
