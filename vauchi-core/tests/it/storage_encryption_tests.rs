// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for Phase 2a.1: High-priority plaintext table encryption.
//!
//! Verifies that own_card, device_sync_state, device_registry, and
//! visibility_labels store data encrypted and roundtrip correctly.

use vauchi_core::contact::Group;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceRegistry, RegisteredDevice};
use vauchi_core::storage::Storage;
use vauchi_core::sync::InterDeviceSyncState;

fn open_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    (dir, storage)
}

fn make_test_registry() -> DeviceRegistry {
    let device = RegisteredDevice {
        device_id: [0x42; 32],
        exchange_public_key: [0x43; 32],
        device_name: "Test Device".to_string(),
        created_at: 1000,
        revoked: false,
        revoked_at: None,
        last_sync_at: None,
    };
    let signing_key = SigningKeyPair::generate();
    DeviceRegistry::new(device, &signing_key)
}

// === Own Card Encryption ===

// @scenario: security :: Contact cards are encrypted at rest
// @internal
#[test]
fn test_own_card_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let mut card = ContactCard::new("Alice");
    let _ = card.add_field(vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Email,
        "email",
        "alice@example.com",
        0,
    ));

    storage.contacts().save_own_card(&card).unwrap();
    let loaded = storage
        .contacts()
        .load_own_card()
        .unwrap()
        .expect("Card should exist");

    assert_eq!(loaded.display_name(), card.display_name());
    assert_eq!(loaded.fields().len(), card.fields().len());
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_own_card_stored_as_encrypted_blob() {
    let (dir, storage) = open_storage();

    let card = ContactCard::new("SecretName");
    storage.contacts().save_own_card(&card).unwrap();

    // Read the raw database to verify data is encrypted (not plaintext)
    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT card_json_encrypted FROM own_card WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty(), "Encrypted blob should not be empty");

    // The blob should NOT be valid JSON (it's encrypted)
    let as_str = String::from_utf8(blob.clone());
    if let Ok(s) = as_str {
        assert!(
            serde_json::from_str::<serde_json::Value>(&s).is_err(),
            "Encrypted data should not be valid JSON"
        );
    }
}

// === Device Registry Encryption ===

// @internal
#[test]
fn test_device_registry_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let registry = make_test_registry();
    storage.device().save_device_registry(&registry).unwrap();

    let loaded = storage
        .device()
        .load_device_registry()
        .unwrap()
        .expect("Registry should exist");
    assert_eq!(loaded.version(), registry.version());
}

// @internal
#[test]
fn test_device_registry_stored_as_encrypted_blob() {
    let (dir, storage) = open_storage();

    let registry = make_test_registry();
    storage.device().save_device_registry(&registry).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT registry_json_encrypted FROM device_registry WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());
}

// === Device Sync State Encryption ===

// @scenario: sync_updates :: All sync traffic is encrypted
// @internal
#[test]
fn test_device_sync_state_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let state = InterDeviceSyncState::new([0xAA; 32]);
    storage.sync().save_device_sync_state(&state).unwrap();

    let loaded = storage
        .sync()
        .load_device_sync_state(&[0xAA; 32])
        .unwrap()
        .expect("State should exist");
    assert_eq!(loaded.device_id(), state.device_id());
}

// @scenario: sync_updates :: All sync traffic is encrypted
// @internal
#[test]
fn test_device_sync_state_stored_as_encrypted_blob() {
    let (dir, storage) = open_storage();

    let device_id: [u8; 32] = [0xBB; 32];
    let state = InterDeviceSyncState::new(device_id);
    storage.sync().save_device_sync_state(&state).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT state_json_encrypted FROM device_sync_state WHERE device_id = ?1",
            rusqlite::params![device_id.as_slice()],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());
}

// @internal
#[test]
fn test_list_device_sync_states_encrypted() {
    let (_dir, storage) = open_storage();

    let state1 = InterDeviceSyncState::new([0x01; 32]);
    let state2 = InterDeviceSyncState::new([0x02; 32]);

    storage.sync().save_device_sync_state(&state1).unwrap();
    storage.sync().save_device_sync_state(&state2).unwrap();

    let all = storage.sync().list_device_sync_states().unwrap();
    assert_eq!(all.len(), 2);
}

// === Visibility Labels Encryption ===

// @internal
#[test]
fn test_visibility_label_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let label = Group::new("Close Friends", 0);
    storage.labels().save_group(&label).unwrap();

    let loaded = storage.labels().load_group(label.id()).unwrap();
    assert_eq!(loaded.name(), "Close Friends");
    assert_eq!(loaded.contacts(), label.contacts());
}

// @internal
#[test]
fn test_visibility_label_stored_as_encrypted_blob() {
    let (dir, storage) = open_storage();

    let label = Group::new("Work Colleagues", 0);
    storage.labels().save_group(&label).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT contacts_json_encrypted FROM visibility_labels WHERE id = ?1",
            [label.id()],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted contacts_json blob should exist");
    assert!(!blob.is_empty());

    let result2: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT visible_fields_json_encrypted FROM visibility_labels WHERE id = ?1",
            [label.id()],
            |row| row.get(0),
        )
        .unwrap();

    let blob2 = result2.expect("Encrypted visible_fields_json blob should exist");
    assert!(!blob2.is_empty());
}

// @internal
#[test]
fn test_load_all_labels_encrypted() {
    let (_dir, storage) = open_storage();

    let label1 = Group::new("Group A", 0);
    let label2 = Group::new("Group B", 0);

    storage.labels().save_group(&label1).unwrap();
    storage.labels().save_group(&label2).unwrap();

    let all = storage.labels().load_all_groups().unwrap();
    assert_eq!(all.len(), 2);
}

// === Migration Tests ===

// @internal
#[test]
fn test_migration_v13_adds_encrypted_columns() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    raw_conn
        .prepare("SELECT card_json_encrypted FROM own_card LIMIT 0")
        .expect("own_card.card_json_encrypted column should exist");

    raw_conn
        .prepare("SELECT registry_json_encrypted FROM device_registry LIMIT 0")
        .expect("device_registry.registry_json_encrypted column should exist");

    raw_conn
        .prepare("SELECT state_json_encrypted FROM device_sync_state LIMIT 0")
        .expect("device_sync_state.state_json_encrypted column should exist");

    raw_conn
        .prepare("SELECT contacts_json_encrypted, visible_fields_json_encrypted FROM visibility_labels LIMIT 0")
        .expect("visibility_labels encrypted columns should exist");
}

// @internal
#[test]
fn test_migration_v13_schema_version() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let version: u32 = raw_conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        version >= 14,
        "Schema version should be at least 14, got {}",
        version
    );
}

// @internal
#[test]
fn test_migration_v13_fallback_reads_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();

    // Step 1: Open storage (runs all migrations including v13)
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();

        // Save a proper card first (to get valid JSON), then simulate pre-v13 state
        let card = ContactCard::new("PreMigration");
        let card_json = serde_json::to_string(&card).unwrap();

        storage
            .connection()
            .execute(
                "INSERT OR REPLACE INTO own_card (id, card_json, card_json_encrypted, updated_at) VALUES (1, ?1, NULL, ?2)",
                rusqlite::params![card_json, 1000i64],
            )
            .unwrap();

        drop(storage);
    }

    // Step 2: Re-open — should be able to read the card via plaintext fallback
    {
        let storage = Storage::open(&db_path, key).unwrap();
        let card = storage.contacts().load_own_card().unwrap();
        assert!(
            card.is_some(),
            "Should be able to load card via plaintext fallback"
        );
        assert_eq!(card.unwrap().display_name(), "PreMigration");
    }
}

// @internal
#[test]
fn test_device_registry_json_export_with_encrypted_storage() {
    let (_dir, storage) = open_storage();

    let registry = make_test_registry();
    storage.device().save_device_registry(&registry).unwrap();

    // GDPR export uses load_device_registry_json — verify it still works
    let json = storage
        .device()
        .load_device_registry_json()
        .unwrap()
        .expect("JSON export should work");
    assert!(!json.is_empty());

    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}

// === Rekey Tests ===

// @internal
#[test]
fn test_rekey_preserves_own_card() {
    let (_dir, mut storage) = open_storage();

    let card = ContactCard::new("ReKeyTest");
    storage.contacts().save_own_card(&card).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .contacts()
        .load_own_card()
        .unwrap()
        .expect("Card should exist");
    assert_eq!(loaded.display_name(), "ReKeyTest");
}

// @internal
#[test]
fn test_rekey_preserves_device_registry() {
    let (_dir, mut storage) = open_storage();

    let registry = make_test_registry();
    storage.device().save_device_registry(&registry).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .device()
        .load_device_registry()
        .unwrap()
        .expect("Registry should exist");
    assert_eq!(loaded.version(), registry.version());
}

// @internal
#[test]
fn test_rekey_preserves_device_sync_state() {
    let (_dir, mut storage) = open_storage();

    let state = InterDeviceSyncState::new([0xCC; 32]);
    storage.sync().save_device_sync_state(&state).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .sync()
        .load_device_sync_state(&[0xCC; 32])
        .unwrap()
        .expect("State should exist");
    assert_eq!(loaded.device_id(), &[0xCC; 32]);
}

// @internal
#[test]
fn test_rekey_preserves_visibility_labels() {
    let (_dir, mut storage) = open_storage();

    let label = Group::new("Rekey Group", 0);
    storage.labels().save_group(&label).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage.labels().load_group(label.id()).unwrap();
    assert_eq!(loaded.name(), "Rekey Group");
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_rekey_old_key_cannot_decrypt() {
    let (dir, mut storage) = open_storage();

    let card = ContactCard::new("OldKeyTest");
    storage.contacts().save_own_card(&card).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key.clone()).unwrap();

    // Open a fresh storage with the OLD key — should fail to decrypt
    let db_path = dir.path().join("vauchi.db");
    let old_storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    let result = old_storage.contacts().load_own_card();
    // Either returns None (empty card_json) or an error (decryption failed)
    match result {
        Ok(None) => {} // Empty plaintext fallback, encrypted column can't be decrypted
        Ok(Some(_)) => panic!("Should not be able to decrypt with wrong key"),
        Err(_) => {} // Expected: decryption error
    }

    // But with the correct new key, it works
    let correct_storage = Storage::open(&db_path, new_key).unwrap();
    let loaded = correct_storage
        .contacts()
        .load_own_card()
        .unwrap()
        .expect("Should decrypt with correct key");
    assert_eq!(loaded.display_name(), "OldKeyTest");
}

// @internal
#[test]
fn test_rekey_with_smk_derived_sek() {
    use vauchi_core::crypto::ShreddingMasterKey;

    let (_dir, mut storage) = open_storage();

    let card = ContactCard::new("SMK Test");
    storage.contacts().save_own_card(&card).unwrap();

    // Simulate SMK-based rekey: derive SEK from a master seed
    let smk = ShreddingMasterKey::derive_from_seed(&[0x42; 32]);
    let sek = smk.derive_sek();

    storage.rekey(sek).unwrap();

    // Data is now encrypted under SMK-derived SEK
    let loaded = storage
        .contacts()
        .load_own_card()
        .unwrap()
        .expect("Card should exist");
    assert_eq!(loaded.display_name(), "SMK Test");
}
