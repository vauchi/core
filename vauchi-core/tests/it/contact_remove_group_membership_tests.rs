// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Removing a contact must also take it out of every group (private#854).
//!
//! An exchanged contact's id is derived from its public key, so exchanging
//! with the same person again yields the same id. A membership that
//! survives `remove_contact` silently re-grants every field the group
//! shows, against ADR-054's default-closed rule.

use vauchi_core::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::SyncItem;

use crate::common::device_sync::{journal_for_tablet, link_tablet};

const DAVE_PK: [u8; 32] = [4u8; 32];

fn dave() -> Contact {
    Contact::from_exchange(
        DAVE_PK,
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    )
}

/// Alice with a Mobile field visible to "Family", and Dave in Family.
/// Returns (alice, dave's contact id, mobile field id, family group id).
fn alice_with_dave_in_family() -> (Vauchi, String, String, String) {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();
    alice
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 000 00 00",
            0,
        ))
        .unwrap();
    let mobile_id = alice
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == "Mobile")
        .unwrap()
        .id()
        .to_string();
    let family = alice.create_group("Family").unwrap();
    let family_id = family.id().to_string();
    alice
        .set_group_field_visibility(&family_id, &mobile_id, true)
        .unwrap();

    let contact = dave();
    let dave_id = contact.id().to_string();
    alice.add_contact(contact).unwrap();
    assert!(
        !alice
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "Mobile is hidden from a fresh contact"
    );
    alice.add_contact_to_group(&family_id, &dave_id).unwrap();
    assert!(
        alice
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "Family membership shows Mobile"
    );
    (alice, dave_id, mobile_id, family_id)
}

// @scenario: contacts_management :: Remove contact cleans up visibility rules
#[test]
fn removed_contact_leaves_every_group_so_a_re_exchange_starts_default_closed() {
    let (alice, dave_id, mobile_id, _family_id) = alice_with_dave_in_family();

    assert!(alice.remove_contact(&dave_id).unwrap());

    assert!(
        alice.get_groups_for_contact(&dave_id).unwrap().is_empty(),
        "a removed contact belongs to no group"
    );

    let again = dave();
    assert_eq!(again.id(), dave_id, "same identity, same contact id");
    alice.add_contact(again).unwrap();
    assert!(
        alice.get_groups_for_contact(&dave_id).unwrap().is_empty(),
        "re-exchange must not restore old memberships"
    );
    assert!(
        !alice
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "Mobile stays hidden until Alice grants it again"
    );
}

// @scenario: contacts_management :: Remove contact cleans up visibility rules
#[test]
fn removing_a_contact_journals_the_groups_it_left_for_linked_devices() {
    let (alice, dave_id, _mobile_id, family_id) = alice_with_dave_in_family();
    let (registry, tablet_id) = link_tablet(&alice, [7u8; 32]);

    alice.remove_contact(&dave_id).unwrap();

    let journal = journal_for_tablet(&alice, registry, &tablet_id);
    let family_after: Vec<_> = journal
        .iter()
        .filter_map(|item| match item {
            SyncItem::GroupChanged { group_data, .. } if group_data.id == family_id => {
                Some(group_data)
            }
            _ => None,
        })
        .collect();
    let last = family_after
        .last()
        .expect("removing a member journals the group for the tablet");
    assert!(
        !last.contact_ids.contains(&dave_id),
        "the journaled Family state no longer lists Dave"
    );
}

// @scenario: contacts_management :: Remove contact cleans up visibility rules
#[test]
fn a_linked_device_applying_contact_removed_drops_the_group_membership_too() {
    let (tablet, dave_id, mobile_id, _family_id) = alice_with_dave_in_family();

    tablet
        .apply_sync_items(vec![SyncItem::ContactRemoved {
            contact_id: dave_id.clone(),
            timestamp: 1,
        }])
        .unwrap();

    assert!(
        tablet.get_groups_for_contact(&dave_id).unwrap().is_empty(),
        "the sibling device forgets the membership with the contact"
    );
    tablet.add_contact(dave()).unwrap();
    assert!(
        !tablet
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "a re-exchange on the sibling starts default-closed as well"
    );
}
