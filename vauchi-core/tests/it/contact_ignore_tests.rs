// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ignore is a silent, one-way, reversible de-prioritisation (ADR-072):
//! a local per-contact flag that mirrors archive's shape without
//! archive's list removal or catch-up.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{Identity, Storage};

fn exchanged_contact(name: &str) -> Contact {
    let identity = Identity::create(name, 0);
    Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn new_contact_is_not_ignored() {
    let contact = exchanged_contact("Bob");
    assert!(!contact.is_ignored());
    assert_eq!(contact.ignored_at(), None);
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignore_sets_flag_and_exact_timestamp() {
    let mut contact = exchanged_contact("Bob");
    contact.ignore(1_700_000_000);
    assert!(contact.is_ignored());
    assert_eq!(contact.ignored_at(), Some(1_700_000_000));
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn unignore_clears_flag_and_timestamp() {
    let mut contact = exchanged_contact("Bob");
    contact.ignore(1_700_000_000);
    contact.unignore();
    assert!(!contact.is_ignored());
    assert_eq!(contact.ignored_at(), None);
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignore_leaves_archive_and_block_state_untouched() {
    let mut contact = exchanged_contact("Bob");
    contact.ignore(1_700_000_000);
    assert!(!contact.is_archived(), "ignore must not archive");
    assert!(!contact.is_blocked(), "ignore must not block");
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn storage_round_trips_ignored_flag_and_timestamp() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = exchanged_contact("Bob");
    contact.ignore(1_700_000_000);
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(contact.id())
        .unwrap()
        .unwrap();
    assert!(loaded.is_ignored());
    assert_eq!(loaded.ignored_at(), Some(1_700_000_000));
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignored_contact_stays_in_list_contacts() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = exchanged_contact("Bob");
    contact.ignore(1_700_000_000);
    storage.contacts().save_contact(&contact).unwrap();

    let listed = storage.contacts().list_contacts().unwrap();
    assert_eq!(listed.len(), 1, "ignore must not remove from the list");
    assert!(listed[0].is_ignored(), "the listed row must carry the flag");
}
