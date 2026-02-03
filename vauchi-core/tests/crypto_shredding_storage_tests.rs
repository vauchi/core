// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto-shredding storage operations (CEK + revoked_senders).
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Card updates use per-contact content encryption key"
//!   - "Account deletion destroys all content encryption keys"
//!   - "Card update arriving after revocation is discarded"

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).expect("in-memory storage")
}

fn test_contact(id: &str) -> Contact {
    let mut card = ContactCard::new("Test User");
    card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "test@example.com",
    ))
    .unwrap();
    let pk = [0x42u8; 32];
    let shared_key = SymmetricKey::generate();
    // Use a deterministic public key that matches the expected id
    let _ = id; // id is derived from public key in real code
    Contact::from_exchange(pk, card, shared_key)
}

// === Migration V13 ===

#[test]
fn test_schema_version_is_13() {
    let storage = test_storage();
    assert_eq!(storage.schema_version().unwrap(), 13);
}

// === CEK Storage ===

#[test]
fn test_save_and_load_cek() {
    let storage = test_storage();
    let contact = test_contact("alice");
    storage.save_contact(&contact).unwrap();

    let cek = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek).unwrap();

    let loaded = storage.load_contact_cek(contact.id()).unwrap();
    assert!(loaded.is_some(), "CEK should be loaded");

    // Verify the loaded CEK can decrypt what the original encrypted
    let plaintext = b"test card data";
    let ciphertext = cek.encrypt(plaintext).unwrap();
    let decrypted = loaded.unwrap().decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_load_cek_returns_none_for_legacy_contact() {
    let storage = test_storage();
    let contact = test_contact("bob");
    storage.save_contact(&contact).unwrap();

    // No CEK saved — legacy contact
    let loaded = storage.load_contact_cek(contact.id()).unwrap();
    assert!(loaded.is_none(), "Legacy contact should have no CEK");
}

#[test]
fn test_delete_cek_crypto_shreds() {
    let storage = test_storage();
    let contact = test_contact("carol");
    storage.save_contact(&contact).unwrap();

    let cek = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek).unwrap();

    // Verify CEK exists
    assert!(storage.load_contact_cek(contact.id()).unwrap().is_some());

    // Delete CEK (crypto-shred)
    storage.delete_contact_cek(contact.id()).unwrap();

    // Verify CEK is gone
    assert!(storage.load_contact_cek(contact.id()).unwrap().is_none());
}

#[test]
fn test_cek_stored_encrypted_with_storage_key() {
    let storage = test_storage();
    let contact = test_contact("dave");
    storage.save_contact(&contact).unwrap();

    let cek = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek).unwrap();

    // Read raw cek_encrypted column — it should not be the raw CEK bytes
    let raw: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT cek_encrypted FROM contacts WHERE id = ?1",
            rusqlite::params![contact.id()],
            |row| row.get(0),
        )
        .unwrap();

    // Raw column value should not equal the 32-byte CEK
    assert_ne!(raw, cek.to_bytes().to_vec());
    // It should be longer (1 byte tag + 24 byte nonce + 32 bytes + 16 byte tag)
    assert!(raw.len() > 32);
}

#[test]
fn test_save_cek_nonexistent_contact_fails() {
    let storage = test_storage();
    let cek = ContentEncryptionKey::generate();

    let result = storage.save_contact_cek("nonexistent", &cek);
    assert!(result.is_err());
}

#[test]
fn test_cek_rotation_replaces_old() {
    let storage = test_storage();
    let contact = test_contact("eve");
    storage.save_contact(&contact).unwrap();

    let cek_v1 = ContentEncryptionKey::generate();
    let cek_v2 = ContentEncryptionKey::generate();

    storage.save_contact_cek(contact.id(), &cek_v1).unwrap();
    storage.save_contact_cek(contact.id(), &cek_v2).unwrap();

    // Only the latest CEK should be stored
    let loaded = storage.load_contact_cek(contact.id()).unwrap().unwrap();

    let plaintext = b"new card version";
    let ct = cek_v2.encrypt(plaintext).unwrap();
    assert_eq!(loaded.decrypt(&ct).unwrap(), plaintext);

    // Old CEK should not work
    let ct_old = cek_v1.encrypt(b"old data").unwrap();
    assert!(loaded.decrypt(&ct_old).is_err());
}

// === Revoked Senders ===

#[test]
fn test_record_and_check_revoked_sender() {
    let storage = test_storage();

    assert!(!storage.is_sender_revoked("alice_pk").unwrap());

    storage
        .record_revoked_sender("alice_pk", 1700000000)
        .unwrap();

    assert!(storage.is_sender_revoked("alice_pk").unwrap());
    assert!(!storage.is_sender_revoked("bob_pk").unwrap());
}

#[test]
fn test_revoked_sender_persists_after_contact_deletion() {
    let storage = test_storage();
    let contact = test_contact("frank");
    storage.save_contact(&contact).unwrap();

    // Revoke and delete
    storage
        .record_revoked_sender(contact.id(), 1700000000)
        .unwrap();
    storage.delete_contact(&contact.id()).unwrap();

    // Tombstone should persist
    assert!(storage.is_sender_revoked(contact.id()).unwrap());
}

#[test]
fn test_revoked_sender_idempotent() {
    let storage = test_storage();

    storage
        .record_revoked_sender("alice_pk", 1700000000)
        .unwrap();
    storage
        .record_revoked_sender("alice_pk", 1700000001)
        .unwrap();

    assert!(storage.is_sender_revoked("alice_pk").unwrap());
}
