// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for network::anonymous (anonymous sender IDs)

use vauchi_core::network::anonymous::{
    compute_anonymous_id, current_epoch, resolve_sender, AnonymousSender,
};
use vauchi_core::{Contact, ContactCard, SymmetricKey};

fn make_contact_with_key(name: &str, key: SymmetricKey) -> Contact {
    let pk = *vauchi_core::SigningKeyPair::generate()
        .public_key()
        .as_bytes();
    Contact::from_exchange(pk, ContactCard::new(name), key)
}

#[test]
fn test_compute_deterministic() {
    let key = [0xABu8; 32];
    let epoch = 100;
    let id1 = compute_anonymous_id(&key, epoch);
    let id2 = compute_anonymous_id(&key, epoch);
    assert_eq!(id1, id2);
}

#[test]
fn test_different_epochs_different_ids() {
    let key = [0xABu8; 32];
    let id1 = compute_anonymous_id(&key, 100);
    let id2 = compute_anonymous_id(&key, 101);
    assert_ne!(id1, id2);
}

#[test]
fn test_different_keys_different_ids() {
    let key1 = [0xABu8; 32];
    let key2 = [0xCDu8; 32];
    let id1 = compute_anonymous_id(&key1, 100);
    let id2 = compute_anonymous_id(&key2, 100);
    assert_ne!(id1, id2);
}

#[test]
fn test_anonymous_sender_compute() {
    let key = [0x42u8; 32];
    let epoch = 500;
    let sender = AnonymousSender::compute(&key, epoch);
    assert_eq!(sender.epoch, epoch);
    assert_eq!(sender.anonymous_id, compute_anonymous_id(&key, epoch));
}

#[test]
fn test_anonymous_sender_for_current_epoch() {
    let key = [0x42u8; 32];
    let sender = AnonymousSender::for_current_epoch(&key);
    assert_eq!(sender.epoch, current_epoch());
}

#[test]
fn test_current_epoch_is_reasonable() {
    let epoch = current_epoch();
    // Should be > 0 (we're well past UNIX epoch)
    assert!(epoch > 0);
    // In 2026, epoch ~= 1768000000 / 3600 ~= 491111
    assert!(epoch > 400_000);
}

#[test]
fn test_resolve_sender_finds_match() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch);

    let result = resolve_sender(&contacts, &anon_id, epoch);
    assert!(result.is_some());
    assert_eq!(result.unwrap().display_name(), "Alice");
}

#[test]
fn test_resolve_sender_no_match() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key);
    let contacts = vec![contact];

    let wrong_id = [0u8; 32];
    let result = resolve_sender(&contacts, &wrong_id, 1000);
    assert!(result.is_none());
}

#[test]
fn test_resolve_sender_previous_epoch_tolerance() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Bob", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    // Compute ID for previous epoch
    let prev_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch - 1);

    // Should still resolve when checking current epoch (boundary tolerance)
    let result = resolve_sender(&contacts, &prev_id, epoch);
    assert!(result.is_some());
    assert_eq!(result.unwrap().display_name(), "Bob");
}

#[test]
fn test_resolve_sender_empty_contacts() {
    let contacts: Vec<Contact> = vec![];
    let anon_id = [0u8; 32];
    let result = resolve_sender(&contacts, &anon_id, 1000);
    assert!(result.is_none());
}

#[test]
fn test_resolve_sender_epoch_zero() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), 0);
    let result = resolve_sender(&contacts, &anon_id, 0);
    assert!(result.is_some());
}

#[test]
fn test_resolve_sender_multiple_contacts() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let contacts = vec![
        make_contact_with_key("Alice", key1),
        make_contact_with_key("Bob", key2),
    ];

    let epoch = 500;
    let bob_id = compute_anonymous_id(contacts[1].shared_key().as_bytes(), epoch);

    let result = resolve_sender(&contacts, &bob_id, epoch);
    assert!(result.is_some());
    assert_eq!(result.unwrap().display_name(), "Bob");
}
