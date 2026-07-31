// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `ContactStore::has_contacts` — the O(1) existence check
//! backing the sync-chrome contact gate
//! (`problems/2026-07-31-sync-chip-before-first-contact`).
//! Filter semantics must match `list_contacts`: soft-deleted and
//! archived contacts do not count.

use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn make_contact(name: &str) -> Contact {
    let public_key = [name.as_bytes()[0]; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

// @internal
#[test]
fn has_contacts_is_false_on_empty_store() {
    let vauchi = Vauchi::in_memory().unwrap();
    assert!(!vauchi.storage().contacts().has_contacts().unwrap());
}

// @internal
#[test]
fn has_contacts_is_true_with_an_active_contact() {
    let vauchi = Vauchi::in_memory().unwrap();
    vauchi.add_contact(make_contact("Bob")).unwrap();
    assert!(vauchi.storage().contacts().has_contacts().unwrap());
}

// @internal
#[test]
fn has_contacts_ignores_archived_contacts() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let contact = make_contact("Carol");
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    vauchi.archive_contact(&id).unwrap();
    assert!(
        !vauchi.storage().contacts().has_contacts().unwrap(),
        "an archived-only store must report no contacts"
    );
    vauchi.unarchive_contact(&id).unwrap();
    assert!(vauchi.storage().contacts().has_contacts().unwrap());
}

// @internal
#[test]
fn has_contacts_ignores_soft_deleted_contacts() {
    let vauchi = Vauchi::in_memory().unwrap();
    let contact = make_contact("Dave");
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    vauchi.remove_contact(&id).unwrap();
    assert!(
        !vauchi.storage().contacts().has_contacts().unwrap(),
        "a soft-deleted-only store must report no contacts"
    );
}
