// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for CEK-aware contact storage (card encrypted with CEK at rest).
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Card updates use per-contact content encryption key"
//!   - "Contact display name is protected by crypto-shredding"
//!   - "Crypto-shredding renders card unreadable without key"

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::storage::Storage;

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).expect("in-memory storage")
}

fn make_contact(name: &str) -> Contact {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "test@example.com",
        0,
    ))
    .unwrap();
    let pk = [0x42u8; 32];
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key, 0)
}

fn make_contact_with_cek(name: &str) -> Contact {
    let mut contact = make_contact(name);
    let cek = ContentEncryptionKey::generate();
    contact.set_cek(cek);
    contact
}

// === Contact Struct CEK Field ===

// @internal
#[test]
fn test_contact_cek_none_by_default() {
    let contact = make_contact("Alice");
    assert!(contact.cek().is_none());
}

// @internal
#[test]
fn test_contact_set_and_get_cek() {
    let mut contact = make_contact("Alice");
    let cek = ContentEncryptionKey::generate();
    let cek_bytes = cek.to_bytes();

    contact.set_cek(cek);

    contact.cek().expect("expected Some");
    assert_eq!(contact.cek().unwrap().to_bytes(), cek_bytes);
}

// @internal
#[test]
fn test_contact_clear_cek() {
    let mut contact = make_contact_with_cek("Alice");
    contact.cek().expect("expected Some");

    contact.clear_cek();
    assert!(contact.cek().is_none());
}

// === CEK-Aware Save/Load Round-trip ===

// @internal
#[test]
fn test_save_load_contact_with_cek_roundtrip() {
    let storage = test_storage();
    let contact = make_contact_with_cek("Alice Smith");
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Alice Smith");
    assert_eq!(loaded.card().display_name(), "Alice Smith");
    assert!(loaded.cek().is_some(), "CEK should be loaded with contact");
}

// @internal
#[test]
fn test_legacy_contact_still_loads_without_cek() {
    let storage = test_storage();
    let contact = make_contact("Bob Legacy");
    assert!(contact.cek().is_none());

    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Bob Legacy");
    assert!(loaded.cek().is_none(), "Legacy contact should have no CEK");
}

// @internal
#[test]
fn test_list_contacts_includes_cek_contacts() {
    let storage = test_storage();
    let contact = make_contact_with_cek("CEK Contact");
    storage.save_contact(&contact).unwrap();

    let all = storage.list_contacts().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].display_name(), "CEK Contact");
    all[0].cek().expect("expected Some");
}

// === CEK-Protected Display Name ===

// @internal
#[test]
fn test_cek_contact_display_name_empty_in_db() {
    let storage = test_storage();
    let contact = make_contact_with_cek("Secret Name");
    storage.save_contact(&contact).unwrap();

    // Read display_name column directly — should be empty for CEK contacts
    // (no plaintext personal data in DB; name is inside CEK-encrypted card)
    let display_name: String = storage
        .connection()
        .query_row(
            "SELECT display_name FROM contacts WHERE id = ?1",
            rusqlite::params![contact.id()],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        display_name.is_empty(),
        "CEK contacts should have empty display_name in DB, got: {}",
        display_name
    );
}

// @internal
#[test]
fn test_legacy_contact_has_plaintext_display_name() {
    let storage = test_storage();
    let contact = make_contact("Public Name");
    storage.save_contact(&contact).unwrap();

    let display_name: String = storage
        .connection()
        .query_row(
            "SELECT display_name FROM contacts WHERE id = ?1",
            rusqlite::params![contact.id()],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(display_name, "Public Name");
}

// === Card Encrypted With CEK (not storage key) ===

// @internal
#[test]
fn test_cek_contact_card_not_decryptable_with_storage_key_alone() {
    let storage = test_storage();
    let contact = make_contact_with_cek("Alice CEK");
    storage.save_contact(&contact).unwrap();

    // Read card_encrypted column and try to decrypt with storage key
    let card_encrypted: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT card_encrypted FROM contacts WHERE id = ?1",
            rusqlite::params![contact.id()],
            |row| row.get(0),
        )
        .unwrap();

    // Card is encrypted with CEK, not storage key. Attempting storage key decrypt should fail.
    let result = vauchi_core::crypto::decrypt(storage.key(), &card_encrypted);
    assert!(
        result.is_err(),
        "CEK-encrypted card should not be decryptable with storage key"
    );
}

// === Crypto-Shredding: CEK Deletion Makes Card Unreadable ===

// @internal
#[test]
fn test_crypto_shred_makes_card_unreadable() {
    let storage = test_storage();
    let contact = make_contact_with_cek("Carol Shredded");
    storage.save_contact(&contact).unwrap();

    // Verify card is readable before shredding
    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Carol Shredded");

    // Crypto-shred: delete the CEK
    storage.delete_contact_cek(contact.id()).unwrap();

    // After shredding, loading the contact should fail (card is encrypted with CEK that's gone)
    let result = storage.load_contact(contact.id());
    assert!(
        result.is_err() || result.unwrap().is_none(),
        "Contact should be unloadable after crypto-shredding"
    );
}

// === Search Works With CEK Contacts ===

// @internal
#[test]
fn test_search_contacts_finds_cek_protected_name() {
    let storage = test_storage();

    // CEK contact — display_name is NULL in DB but available after decryption
    let contact = make_contact_with_cek("Alice Encrypted");
    storage.save_contact(&contact).unwrap();

    // Legacy contact with different public key to avoid ID collision
    let pk2 = [0x43u8; 32];
    let card2 = ContactCard::new("Bob Plaintext");
    let legacy = Contact::from_exchange(pk2, card2, SymmetricKey::generate(), 0);
    storage.save_contact(&legacy).unwrap();

    // Search should find Alice even though display_name is NULL in DB
    let results = storage.search_contacts("Alice").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].display_name(), "Alice Encrypted");

    // Search should find Bob via legacy path
    let results = storage.search_contacts("Bob").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].display_name(), "Bob Plaintext");

    // Empty search returns all
    let all = storage.search_contacts("").unwrap();
    assert_eq!(all.len(), 2);
}

// === CEK Rotation Replaces At-Rest Encryption ===

// @internal
#[test]
fn test_save_contact_with_rotated_cek() {
    let storage = test_storage();
    let mut contact = make_contact_with_cek("Dave Rotated");
    storage.save_contact(&contact).unwrap();

    // Rotate CEK
    let new_cek = ContentEncryptionKey::generate();
    contact.set_cek(new_cek);
    storage.save_contact(&contact).unwrap();

    // Should still load correctly with the new CEK
    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Dave Rotated");
    loaded.cek().expect("expected Some");
}
