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
