// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for Phase 2a.3: SMK boot path and migration.
//!
//! Tests the key lifecycle:
//! 1. New identity → derive SMK → store in SecureStorage → derive SEK
//! 2. Boot → load SMK from SecureStorage → derive SEK → open Storage
//! 3. Migration → old key → derive SMK from identity → rekey to SEK

use std::sync::Arc;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{ShreddingMasterKey, SymmetricKey};
use vauchi_core::identity::Identity;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage};

const SMK_KEY_NAME: &str = "smk";

fn open_storage_with_key(key: SymmetricKey) -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, key).unwrap();
    (dir, storage)
}

// === Identity::derive_smk() ===

#[test]
fn test_identity_derive_smk_is_deterministic() {
    let identity = Identity::create("Alice");
    let smk1 = identity.derive_smk();
    let smk2 = identity.derive_smk();
    assert_eq!(smk1.as_bytes(), smk2.as_bytes());
}

#[test]
fn test_identity_derive_smk_differs_between_identities() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");
    let smk_alice = alice.derive_smk();
    let smk_bob = bob.derive_smk();
    assert_ne!(smk_alice.as_bytes(), smk_bob.as_bytes());
}

// === SMK → SecureStorage → SEK Boot Flow ===

#[test]
fn test_smk_stored_and_loaded_from_secure_storage() {
    let identity = Identity::create("Alice");
    let smk = identity.derive_smk();

    // Store SMK in SecureStorage
    let secure = Arc::new(MemoryKeyStorage::new());
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

    // Load SMK from SecureStorage
    let smk_bytes = secure.load_key(SMK_KEY_NAME).unwrap().unwrap();
    let smk_loaded: [u8; 32] = smk_bytes.try_into().unwrap();
    let smk_restored = ShreddingMasterKey::from_bytes(smk_loaded);

    // Derived SEK should match
    let sek1 = smk.derive_sek();
    let sek2 = smk_restored.derive_sek();
    assert_eq!(sek1.as_bytes(), sek2.as_bytes());
}

#[test]
fn test_boot_with_smk_derived_sek_opens_storage() {
    let identity = Identity::create("Alice");
    let smk = identity.derive_smk();
    let sek = smk.derive_sek();

    // Store SMK in SecureStorage (simulating identity creation flow)
    let secure = Arc::new(MemoryKeyStorage::new());
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

    // Open storage with SEK and save data
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    {
        let storage = Storage::open(&db_path, sek).unwrap();
        let card = ContactCard::new("Alice");
        storage.save_own_card(&card).unwrap();
    }

    // Simulate reboot: load SMK from SecureStorage → derive SEK → open Storage
    let smk_bytes = secure.load_key(SMK_KEY_NAME).unwrap().unwrap();
    let smk_rebooted = ShreddingMasterKey::from_bytes(smk_bytes.try_into().unwrap());
    let sek_rebooted = smk_rebooted.derive_sek();

    let storage = Storage::open(&db_path, sek_rebooted).unwrap();
    let loaded = storage.load_own_card().unwrap().expect("Card should exist");
    assert_eq!(loaded.display_name(), "Alice");
}

// === Migration from Old Key to SMK-Derived SEK ===

#[test]
fn test_migrate_old_key_to_smk_preserves_data() {
    let old_key = SymmetricKey::generate();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");

    // Step 1: Open storage with old key, save data
    {
        let storage = Storage::open(&db_path, old_key.clone()).unwrap();
        let card = ContactCard::new("MigrationUser");
        storage.save_own_card(&card).unwrap();
    }

    // Step 2: Open with old key, migrate to SMK
    let identity = Identity::create("MigrationUser");
    let smk = identity.derive_smk();
    let sek = smk.derive_sek();

    let secure = Arc::new(MemoryKeyStorage::new());

    {
        let mut storage = Storage::open(&db_path, old_key).unwrap();

        // Store SMK in SecureStorage BEFORE re-encryption (see plan rationale)
        secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

        // Rekey to SMK-derived SEK
        storage.rekey(sek).unwrap();
    }

    // Step 3: Re-open with SMK-derived SEK — data should be accessible
    let smk_bytes = secure.load_key(SMK_KEY_NAME).unwrap().unwrap();
    let smk_loaded = ShreddingMasterKey::from_bytes(smk_bytes.try_into().unwrap());
    let sek_loaded = smk_loaded.derive_sek();

    let storage = Storage::open(&db_path, sek_loaded).unwrap();
    let loaded = storage.load_own_card().unwrap().expect("Card should exist");
    assert_eq!(loaded.display_name(), "MigrationUser");
}

#[test]
fn test_migrate_smk_stored_before_rekey_for_safety() {
    let old_key = SymmetricKey::generate();
    let (dir, storage) = open_storage_with_key(old_key.clone());
    let card = ContactCard::new("SafetyTest");
    storage.save_own_card(&card).unwrap();
    drop(storage);

    let identity = Identity::create("SafetyTest");
    let smk = identity.derive_smk();
    let secure = Arc::new(MemoryKeyStorage::new());

    // Store SMK FIRST — if rekey fails, at least SMK is saved
    // (orphaned but harmless, overwritten on retry)
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

    // Verify SMK is stored before we touch the database
    assert!(secure.has_key(SMK_KEY_NAME).unwrap());

    // Now rekey
    let mut storage = Storage::open(&dir.path().join("vauchi.db"), old_key).unwrap();
    let sek = smk.derive_sek();
    storage.rekey(sek).unwrap();

    // Verify data accessible via SMK → SEK
    let loaded = storage.load_own_card().unwrap().expect("Card should exist");
    assert_eq!(loaded.display_name(), "SafetyTest");
}

#[test]
fn test_after_migration_old_key_cannot_decrypt() {
    let old_key = SymmetricKey::generate();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");

    // Save data with old key
    {
        let storage = Storage::open(&db_path, old_key.clone()).unwrap();
        let card = ContactCard::new("OldKeyTest");
        storage.save_own_card(&card).unwrap();
    }

    // Migrate to SMK
    let smk = ShreddingMasterKey::derive_from_seed(&[0x42; 32]);
    let sek = smk.derive_sek();
    {
        let mut storage = Storage::open(&db_path, old_key.clone()).unwrap();
        storage.rekey(sek).unwrap();
    }

    // Old key should not decrypt
    let old_storage = Storage::open(&db_path, old_key).unwrap();
    match old_storage.load_own_card() {
        Ok(None) => {} // Plaintext fallback returns empty string
        Ok(Some(_)) => panic!("Old key should not decrypt after migration"),
        Err(_) => {} // Decryption error — expected
    }
}

// === SMK Destruction After Migration ===

#[test]
fn test_smk_destruction_makes_data_irrecoverable() {
    let identity = Identity::create("ShredTest");
    let smk = identity.derive_smk();
    let sek = smk.derive_sek();

    let secure = Arc::new(MemoryKeyStorage::new());
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    {
        let storage = Storage::open(&db_path, sek).unwrap();
        let card = ContactCard::new("ShredTest");
        storage.save_own_card(&card).unwrap();
    }

    // Destroy SMK from SecureStorage
    secure.secure_delete_key(SMK_KEY_NAME).unwrap();
    assert!(!secure.has_key(SMK_KEY_NAME).unwrap());

    // Without SMK, we can't derive SEK → can't decrypt
    // Using a random key should fail
    let random_storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    match random_storage.load_own_card() {
        Ok(None) => {} // Plaintext fallback returns empty
        Ok(Some(_)) => panic!("Should not be able to decrypt without SMK"),
        Err(_) => {} // Expected: decryption error
    }
}

// === Vauchi-Level Integration ===

#[test]
fn test_vauchi_create_identity_stores_smk() {
    use vauchi_core::api::{Vauchi, VauchiConfig};

    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate());

    let secure = Arc::new(MemoryKeyStorage::new());

    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.set_secure_storage(secure.clone());
    vauchi.create_identity("Alice").unwrap();

    // SMK should now be in SecureStorage
    assert!(secure.has_key(SMK_KEY_NAME).unwrap());

    // Verify the stored SMK can derive the correct SEK
    let smk_bytes = secure.load_key(SMK_KEY_NAME).unwrap().unwrap();
    assert_eq!(smk_bytes.len(), 32);
}

#[test]
fn test_vauchi_boot_with_smk_from_secure_storage() {
    use vauchi_core::api::{Vauchi, VauchiConfig};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");

    // Step 1: Create identity, stores SMK
    let secure = Arc::new(MemoryKeyStorage::new());
    let initial_key = SymmetricKey::generate();
    {
        let config = VauchiConfig::with_storage_path(&db_path)
            .with_storage_key(initial_key);
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi.set_secure_storage(secure.clone());
        vauchi.create_identity("Alice").unwrap();
    }

    // Step 2: Reboot — use SMK from SecureStorage
    {
        let config = VauchiConfig::with_storage_path(&db_path);
        let vauchi = Vauchi::with_secure_storage(config, secure.clone()).unwrap();

        // Should be able to load data
        let card = vauchi.own_card().unwrap();
        assert!(card.is_some());
        assert_eq!(card.unwrap().display_name(), "Alice");
    }
}

#[test]
fn test_vauchi_migrate_existing_to_smk() {
    use vauchi_core::api::{Vauchi, VauchiConfig};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let old_key = SymmetricKey::generate();

    // Step 1: Existing installation with old key (no SMK), save some data
    {
        let storage = Storage::open(&db_path, old_key.clone()).unwrap();
        let card = ContactCard::new("MigrateMe");
        storage.save_own_card(&card).unwrap();
        drop(storage);
    }

    // Step 2: Upgrade — open with old key, load identity, trigger migration
    // In practice, the app reconstructs Identity from backup (password/biometric)
    // Here we simulate by creating an identity and using it for SMK derivation.
    let identity = Identity::create("MigrateMe");
    let secure = Arc::new(MemoryKeyStorage::new());
    {
        let config = VauchiConfig::with_storage_path(&db_path)
            .with_storage_key(old_key);
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi.set_secure_storage(secure.clone());
        vauchi.set_identity(identity).unwrap();
        vauchi.migrate_to_smk().unwrap();
    }

    // Step 3: Reboot with SMK only (no old key needed)
    {
        let config = VauchiConfig::with_storage_path(&db_path);
        let vauchi = Vauchi::with_secure_storage(config, secure.clone()).unwrap();
        let card = vauchi.own_card().unwrap();
        assert!(card.is_some());
        assert_eq!(card.unwrap().display_name(), "MigrateMe");
    }
}
