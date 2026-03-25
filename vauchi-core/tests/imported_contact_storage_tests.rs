// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for imported contact storage (Tasks 3-4: migration v34 + CRUD).
//!
//! Validates that imported contacts can be saved, loaded, listed, searched,
//! and deleted, and that existing exchanged contacts are unaffected.

use vauchi_core::ImportSource;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn make_exchanged(name: &str) -> Contact {
    // Use a hash of the name to get a unique 32-byte key
    let mut public_key = [0u8; 32];
    let bytes = name.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        public_key[i % 32] ^= b;
    }
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

fn make_imported(name: &str, source: ImportSource) -> Contact {
    let card = ContactCard::new(name);
    Contact::from_import(card, source, Some(format!("uid-{}", name)))
}

// ── Save and load roundtrip ──────────────────────────────────────

#[test]
fn save_and_load_imported_contact() {
    let storage = open_storage();
    let contact = make_imported("Alice Import", ImportSource::VcardFile);
    let id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();
    let loaded = storage.load_contact(&id).unwrap().unwrap();

    assert_eq!(loaded.id(), id);
    assert_eq!(loaded.display_name(), "Alice Import");
    assert!(loaded.is_imported(), "Loaded contact must be imported kind");
    assert!(!loaded.is_exchanged());
}

// ── List contacts includes both kinds ────────────────────────────

#[test]
fn list_contacts_includes_both_kinds() {
    let storage = open_storage();

    let exchanged = make_exchanged("Bob");
    let imported = make_imported("Carol Imported", ImportSource::CsvFile);
    let exchanged_id = exchanged.id().to_string();
    let imported_id = imported.id().to_string();

    storage.save_contact(&exchanged).unwrap();
    storage.save_contact(&imported).unwrap();

    let contacts = storage.list_contacts().unwrap();
    assert_eq!(contacts.len(), 2, "Both exchanged and imported must appear");

    let ids: Vec<&str> = contacts.iter().map(|c| c.id()).collect();
    assert!(ids.contains(&exchanged_id.as_str()));
    assert!(ids.contains(&imported_id.as_str()));

    // Verify kinds
    let bob = contacts.iter().find(|c| c.id() == exchanged_id).unwrap();
    assert!(bob.is_exchanged());
    let carol = contacts.iter().find(|c| c.id() == imported_id).unwrap();
    assert!(carol.is_imported());
}

// ── Search contacts finds imported ───────────────────────────────

#[test]
fn search_contacts_finds_imported() {
    let storage = open_storage();

    let imported = make_imported("Diana Searchable", ImportSource::IosPlatform);
    storage.save_contact(&imported).unwrap();

    let results = storage.search_contacts("Diana").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].display_name(), "Diana Searchable");
    assert!(results[0].is_imported());
}

// ── Delete imported contact ──────────────────────────────────────

#[test]
fn delete_imported_contact() {
    let storage = open_storage();

    let imported = make_imported("Eve Deletable", ImportSource::Manual);
    let id = imported.id().to_string();

    storage.save_contact(&imported).unwrap();
    assert!(storage.load_contact(&id).unwrap().is_some());

    let deleted = storage.delete_contact(&id).unwrap();
    assert!(
        deleted,
        "delete_contact must return true for existing contact"
    );

    assert!(
        storage.load_contact(&id).unwrap().is_none(),
        "Deleted contact must not be loadable"
    );
}

// ── Imported contact roundtrip preserves metadata ────────────────

#[test]
fn imported_contact_roundtrip_preserves_metadata() {
    let storage = open_storage();
    let contact = make_imported("Frank Meta", ImportSource::AndroidPlatform);
    let id = contact.id().to_string();

    // Capture original metadata
    let original_data = contact.kind().imported_data().unwrap();
    let original_source = original_data.source.clone();
    let original_imported_at = original_data.imported_at;
    let original_uid = original_data.original_uid.clone();

    storage.save_contact(&contact).unwrap();
    let loaded = storage.load_contact(&id).unwrap().unwrap();

    let loaded_data = loaded
        .kind()
        .imported_data()
        .expect("Loaded contact must be imported");
    assert_eq!(
        loaded_data.source, original_source,
        "ImportSource must survive roundtrip"
    );
    assert_eq!(
        loaded_data.imported_at, original_imported_at,
        "imported_at must survive roundtrip"
    );
    assert_eq!(
        loaded_data.original_uid, original_uid,
        "original_uid must survive roundtrip"
    );
}

// ── Existing exchanged contacts unaffected ───────────────────────

#[test]
fn existing_exchanged_contacts_unaffected() {
    let storage = open_storage();

    let exchanged = make_exchanged("Grace");
    let id = exchanged.id().to_string();
    let original_pk = exchanged.public_key().unwrap().to_owned();
    let original_display = exchanged.display_name().to_string();

    storage.save_contact(&exchanged).unwrap();
    let loaded = storage.load_contact(&id).unwrap().unwrap();

    assert!(loaded.is_exchanged(), "Must still be exchanged after load");
    assert_eq!(
        loaded.public_key().unwrap(),
        &original_pk,
        "Public key must be intact"
    );
    assert_eq!(loaded.display_name(), original_display);

    // Verify contact_kind is 'exchanged' implicitly by checking kind
    assert!(loaded.kind().exchanged_data().is_some());
    assert_eq!(*loaded.public_key().unwrap(), original_pk);
}

// ── Imported contacts have no crypto fields ──────────────────────

#[test]
fn imported_contact_has_no_crypto_fields() {
    let storage = open_storage();
    let imported = make_imported("Hank NoCrypto", ImportSource::VcardFile);
    let id = imported.id().to_string();

    storage.save_contact(&imported).unwrap();
    let loaded = storage.load_contact(&id).unwrap().unwrap();

    assert!(
        loaded.public_key().is_none(),
        "Imported must have no public key"
    );
    assert!(
        loaded.shared_key().is_none(),
        "Imported must have no shared key"
    );
    assert!(!loaded.is_recovery_trusted());
    assert!(!loaded.is_fingerprint_verified());
}

// ── Pagination includes imported contacts ────────────────────────

#[test]
fn list_contacts_paginated_includes_imported() {
    let storage = open_storage();

    for i in 0..5 {
        let name = format!("Exchanged-{:02}", i);
        storage.save_contact(&make_exchanged(&name)).unwrap();
    }
    for i in 0..3 {
        let name = format!("Imported-{:02}", i);
        storage
            .save_contact(&make_imported(&name, ImportSource::Manual))
            .unwrap();
    }

    let all = storage.list_contacts().unwrap();
    assert_eq!(all.len(), 8, "Should have 5 exchanged + 3 imported");

    let page = storage.list_contacts_paginated(0, 100).unwrap();
    assert_eq!(page.len(), 8);
}

// ── Imported contact local flags survive roundtrip ───────────────

#[test]
fn imported_contact_flags_survive_roundtrip() {
    let storage = open_storage();

    let mut imported = make_imported("Ivy Flagged", ImportSource::CsvFile);
    imported.set_favorite(true);
    imported.set_blocked(true);
    let id = imported.id().to_string();

    storage.save_contact(&imported).unwrap();
    let loaded = storage.load_contact(&id).unwrap().unwrap();

    assert!(loaded.is_favorite(), "Favorite flag must survive roundtrip");
    assert!(loaded.is_blocked(), "Blocked flag must survive roundtrip");
    assert!(loaded.is_imported());
}
