// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for storage identity operations
//!
//! Coverage tests for storage/identity.rs

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

#[test]
fn test_save_and_load_identity() {
    let storage = create_test_storage();
    let backup_data = b"encrypted identity backup data here";
    let display_name = "Alice";

    // Initially no identity
    assert!(!storage.has_identity().unwrap());
    assert!(storage.load_identity().unwrap().is_none());

    // Save identity
    storage
        .save_identity(backup_data, display_name)
        .expect("Should save identity");

    // Now has identity
    assert!(storage.has_identity().unwrap());

    // Load identity
    let (loaded_data, loaded_name) = storage
        .load_identity()
        .unwrap()
        .expect("Should load identity");

    assert_eq!(loaded_data, backup_data);
    assert_eq!(loaded_name, display_name);
}

#[test]
fn test_identity_replace_on_save() {
    let storage = create_test_storage();

    // Save initial identity
    storage
        .save_identity(b"first backup", "First Name")
        .unwrap();

    // Save replacement identity
    storage
        .save_identity(b"second backup", "Second Name")
        .unwrap();

    // Should have the second one
    let (loaded_data, loaded_name) = storage
        .load_identity()
        .unwrap()
        .expect("Should load identity");

    assert_eq!(loaded_data, b"second backup");
    assert_eq!(loaded_name, "Second Name");
}

#[test]
fn test_has_identity() {
    let storage = create_test_storage();

    assert!(!storage.has_identity().unwrap());

    storage.save_identity(b"data", "name").unwrap();

    assert!(storage.has_identity().unwrap());
}

#[test]
fn test_identity_encryption() {
    let storage = create_test_storage();
    let sensitive_data = b"this is very secret identity data";

    storage.save_identity(sensitive_data, "User").unwrap();

    // Data should be encrypted in storage (the Storage implementation
    // encrypts before saving and decrypts on load)
    let (loaded, _) = storage.load_identity().unwrap().unwrap();
    assert_eq!(loaded, sensitive_data);
}

// ============================================================
// App Password & Duress PIN coverage tests
// ============================================================

/// Test load_password_config returns None when no identity exists
// @scenario: identity_management.feature:App password setup
#[test]
fn test_load_password_config_no_identity() {
    let storage = create_test_storage();

    // No identity exists — query_row returns NoRows
    let config = storage.load_password_config().unwrap();
    assert!(config.is_none(), "No identity means no password config");
}

/// Test load_password_config returns None when identity exists but no password set
// @scenario: identity_management.feature:App password setup
#[test]
fn test_load_password_config_no_password_set() {
    let storage = create_test_storage();
    storage.save_identity(b"backup data", "Alice").unwrap();

    // Identity exists but password columns are NULL
    let config = storage.load_password_config().unwrap();
    assert!(config.is_none(), "No password set means None");
}

/// Test save_app_password + load_password_config roundtrip
// @scenario: identity_management.feature:App password setup
#[test]
fn test_save_load_app_password() {
    let storage = create_test_storage();
    storage.save_identity(b"backup data", "Alice").unwrap();

    let hash = [0x42u8; 32];
    let salt = [0xABu8; 16];

    storage.save_app_password(&hash, &salt).unwrap();

    let config = storage
        .load_password_config()
        .unwrap()
        .expect("Should have password config");

    assert_eq!(*config.password_hash(), hash);
    assert_eq!(*config.password_salt(), salt);
    assert!(config.duress_hash().is_none());
    assert!(!config.duress_enabled());
}

/// Test save_duress_password enables duress mode
// @scenario: identity_management.feature:Duress PIN
#[test]
fn test_save_duress_password() {
    let storage = create_test_storage();
    storage.save_identity(b"backup data", "Alice").unwrap();

    // Set app password first
    let hash = [0x42u8; 32];
    let salt = [0xABu8; 16];
    storage.save_app_password(&hash, &salt).unwrap();

    // Set duress password
    let duress_hash = [0x99u8; 32];
    let duress_salt = [0xCDu8; 16];
    storage
        .save_duress_password(&duress_hash, &duress_salt)
        .unwrap();

    let config = storage
        .load_password_config()
        .unwrap()
        .expect("Should have password config");

    assert_eq!(*config.password_hash(), hash);
    assert!(config.duress_enabled());
    assert_eq!(*config.duress_hash().unwrap(), duress_hash);
    assert_eq!(*config.duress_salt().unwrap(), duress_salt);
}

/// Test disable_duress clears duress hash/salt and disables flag
// @scenario: identity_management.feature:Duress PIN
#[test]
fn test_disable_duress() {
    let storage = create_test_storage();
    storage.save_identity(b"backup data", "Alice").unwrap();

    // Set app password and duress
    storage
        .save_app_password(&[0x42u8; 32], &[0xABu8; 16])
        .unwrap();
    storage
        .save_duress_password(&[0x99u8; 32], &[0xCDu8; 16])
        .unwrap();

    // Verify duress is enabled
    let config = storage.load_password_config().unwrap().unwrap();
    assert!(config.duress_enabled());

    // Disable duress
    storage.disable_duress().unwrap();

    // Verify duress is disabled
    let config = storage.load_password_config().unwrap().unwrap();
    assert!(!config.duress_enabled());
    assert!(config.duress_hash().is_none());
    assert!(config.duress_salt().is_none());
}

/// Test app password can be updated (replaced)
// @scenario: identity_management.feature:App password setup
#[test]
fn test_update_app_password() {
    let storage = create_test_storage();
    storage.save_identity(b"backup data", "Alice").unwrap();

    // Set initial password
    let hash1 = [0x11u8; 32];
    let salt1 = [0xAAu8; 16];
    storage.save_app_password(&hash1, &salt1).unwrap();

    // Update password
    let hash2 = [0x22u8; 32];
    let salt2 = [0xBBu8; 16];
    storage.save_app_password(&hash2, &salt2).unwrap();

    let config = storage.load_password_config().unwrap().unwrap();
    assert_eq!(*config.password_hash(), hash2);
    assert_eq!(*config.password_salt(), salt2);
}
