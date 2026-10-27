// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The contact list, groups and contact removal, through the public API.

use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey, Vauchi};

/// Adds an exchanged contact named `name` and returns its id.
fn add_contact(vauchi: &Vauchi, name: &str) -> String {
    let peer = Identity::create(name, 0);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    id
}

fn contact_names(vauchi: &Vauchi) -> Vec<String> {
    let mut names: Vec<String> = vauchi
        .list_contacts()
        .unwrap()
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    names.sort();
    names
}

fn member_names(vauchi: &Vauchi, group_id: &str) -> Vec<String> {
    vauchi
        .get_group_members(group_id)
        .unwrap()
        .iter()
        .map(|c| c.display_name().to_string())
        .collect()
}

// @scenario: contacts_management :: View contacts list
#[test]
fn every_added_contact_is_listed() {
    let vauchi = create_vauchi_with_identity("Alice");
    for name in ["Bob", "Carol", "Dave"] {
        add_contact(&vauchi, name);
    }

    assert_eq!(contact_names(&vauchi), ["Bob", "Carol", "Dave"]);
}

// @scenario: contacts_management :: Create a contact group
#[test]
fn a_created_group_exists_and_starts_empty() {
    let vauchi = create_vauchi_with_identity("Alice");

    let family = vauchi.create_group("Family").unwrap();

    let names: Vec<String> = vauchi
        .list_groups()
        .unwrap()
        .iter()
        .map(|g| g.name().to_string())
        .collect();
    assert!(names.contains(&"Family".to_string()), "{names:?}");
    assert_eq!(member_names(&vauchi, family.id()), Vec::<String>::new());
}

// @scenario: contacts_management :: Add contact to group
#[test]
fn a_contact_added_to_a_group_is_a_member_and_still_listed() {
    let vauchi = create_vauchi_with_identity("Alice");
    let family = vauchi.create_group("Family").unwrap();
    let bob = add_contact(&vauchi, "Bob");

    vauchi.add_contact_to_group(family.id(), &bob).unwrap();

    assert_eq!(member_names(&vauchi, family.id()), ["Bob"]);
    assert_eq!(contact_names(&vauchi), ["Bob"]);
}

// @scenario: contacts_management :: Remove a contact
#[test]
fn a_removed_contact_is_gone() {
    let vauchi = create_vauchi_with_identity("Alice");
    let dave = add_contact(&vauchi, "Dave");
    add_contact(&vauchi, "Bob");

    assert!(vauchi.remove_contact(&dave).unwrap());

    assert!(vauchi.get_contact(&dave).unwrap().is_none());
    assert_eq!(contact_names(&vauchi), ["Bob"]);
}

// @scenario: privacy_compliance :: Delete specific contacts
#[test]
fn a_forgotten_contact_is_gone_until_a_later_exchange_brings_them_back() {
    let vauchi = create_vauchi_with_identity("Alice");
    let bob = Identity::create("Bob", 0);
    let exchange_with_bob = || {
        Contact::from_exchange(
            *bob.signing_public_key(),
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
            0,
        )
    };
    let bob_id = exchange_with_bob().id().to_string();
    vauchi.add_contact(exchange_with_bob()).unwrap();

    assert!(vauchi.remove_contact(&bob_id).unwrap());

    assert!(vauchi.get_contact(&bob_id).unwrap().is_none());
    vauchi.add_contact(exchange_with_bob()).unwrap();
    assert_eq!(contact_names(&vauchi), ["Bob"]);
}
