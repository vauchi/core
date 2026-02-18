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
    assert!(
        storage.schema_version().unwrap() >= 13,
        "schema version should be at least 13"
    );
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

// === Crypto-Shredding Integration ===

#[test]
fn test_deleted_cek_renders_encrypted_data_unreadable() {
    let storage = test_storage();
    let contact = test_contact("integration-alice");
    storage.save_contact(&contact).unwrap();

    // Encrypt card data with CEK
    let cek = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek).unwrap();

    let card_data = b"sensitive card payload with email and phone";
    let ciphertext = cek.encrypt(card_data).unwrap();

    // Verify data is readable before shredding
    assert_eq!(cek.decrypt(&ciphertext).unwrap(), card_data);

    // Crypto-shred: delete the CEK
    storage.delete_contact_cek(contact.id()).unwrap();

    // The stored CEK is gone — no way to decrypt
    let loaded_cek = storage.load_contact_cek(contact.id()).unwrap();
    assert!(
        loaded_cek.is_none(),
        "CEK must be gone after crypto-shredding"
    );

    // Attacker has the ciphertext but no CEK — try decryption with a random key
    let wrong_cek = ContentEncryptionKey::generate();
    let result = wrong_cek.decrypt(&ciphertext);
    assert!(
        result.is_err(),
        "Ciphertext must be unreadable without the original CEK"
    );
}

#[test]
fn test_cek_rotation_old_cek_cannot_decrypt_new_data() {
    let storage = test_storage();
    let contact = test_contact("integration-bob");
    storage.save_contact(&contact).unwrap();

    // Initial CEK
    let cek_v1 = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek_v1).unwrap();
    let data_v1 = b"card version 1";
    let ct_v1 = cek_v1.encrypt(data_v1).unwrap();

    // Rotate CEK
    let cek_v2 = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek_v2).unwrap();
    let data_v2 = b"card version 2";
    let ct_v2 = cek_v2.encrypt(data_v2).unwrap();

    // New CEK can decrypt new data
    let loaded = storage.load_contact_cek(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.decrypt(&ct_v2).unwrap(), data_v2);

    // New CEK cannot decrypt old data (different key)
    assert!(
        loaded.decrypt(&ct_v1).is_err(),
        "Rotated CEK must not decrypt data from previous CEK"
    );
}

#[test]
fn test_multi_contact_cek_isolation() {
    let storage = test_storage();

    // Need different public keys to get different contact IDs
    let alice = {
        let mut card = ContactCard::new("Alice");
        card.add_field(ContactField::new(FieldType::Email, "email", "a@test.com"))
            .unwrap();
        Contact::from_exchange([0x01; 32], card, SymmetricKey::generate())
    };
    let bob = {
        let mut card = ContactCard::new("Bob");
        card.add_field(ContactField::new(FieldType::Email, "email", "b@test.com"))
            .unwrap();
        Contact::from_exchange([0x02; 32], card, SymmetricKey::generate())
    };
    storage.save_contact(&alice).unwrap();
    storage.save_contact(&bob).unwrap();

    let cek_alice = ContentEncryptionKey::generate();
    let cek_bob = ContentEncryptionKey::generate();
    storage.save_contact_cek(alice.id(), &cek_alice).unwrap();
    storage.save_contact_cek(bob.id(), &cek_bob).unwrap();

    let alice_data = b"alice card data";
    let bob_data = b"bob card data";
    let ct_alice = cek_alice.encrypt(alice_data).unwrap();
    let ct_bob = cek_bob.encrypt(bob_data).unwrap();

    // Crypto-shred Alice's CEK
    storage.delete_contact_cek(alice.id()).unwrap();

    // Alice's CEK is gone
    assert!(storage.load_contact_cek(alice.id()).unwrap().is_none());

    // Bob's CEK is unaffected
    let loaded_bob = storage.load_contact_cek(bob.id()).unwrap().unwrap();
    assert_eq!(
        loaded_bob.decrypt(&ct_bob).unwrap(),
        bob_data,
        "Bob's data must remain readable after Alice's CEK is shredded"
    );

    // Cross-contact: Bob's CEK cannot decrypt Alice's data
    assert!(loaded_bob.decrypt(&ct_alice).is_err());
}

#[test]
fn test_cek_deletion_is_idempotent() {
    let storage = test_storage();
    let contact = test_contact("idempotent-carol");
    storage.save_contact(&contact).unwrap();

    let cek = ContentEncryptionKey::generate();
    storage.save_contact_cek(contact.id(), &cek).unwrap();

    // Delete once
    storage.delete_contact_cek(contact.id()).unwrap();
    assert!(storage.load_contact_cek(contact.id()).unwrap().is_none());

    // Delete again — should not error
    storage.delete_contact_cek(contact.id()).unwrap();
    assert!(storage.load_contact_cek(contact.id()).unwrap().is_none());
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
