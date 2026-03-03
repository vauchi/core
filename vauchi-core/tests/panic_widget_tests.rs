// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the panic widget API.
//!
//! Traces to features/panic_widget.feature:
//!   - "Works without app unlock (intentional for emergency)"
//!   - "Full panic shred per emergency_shred.feature"
//!   - "Pre-signed messages sent FIRST"
//!   - "Keys destroyed"
//!   - "Completes within 5 seconds"
//!
//! The widget_panic_shred function must work WITHOUT full Vauchi initialization.
//! It only requires a data directory path and a SecureStorage implementation.
//! All tests use REAL crypto and REAL file operations — no mocking.

use vauchi_core::api::{widget_panic_shred, PreSignedShredMessages, WidgetConfirmationMode};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::secure::{MemoryKeyStorage, SecureStorage};
use vauchi_core::storage::Storage;

/// SMK key name used by the shred system.
const SMK_KEY_NAME: &str = "smk";

/// Helper: set up a realistic data directory with database, keys, identity, and pre-signed msgs.
fn setup_widget_test_env() -> (tempfile::TempDir, MemoryKeyStorage) {
    let dir = tempfile::tempdir().unwrap();

    // Create database
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    drop(storage); // Close the connection

    // Create identity file
    let identity = Identity::create("WidgetTestUser");
    std::fs::write(
        dir.path().join("identity.json"),
        b"test identity data for widget",
    )
    .unwrap();

    // Create key files
    let keys_dir = dir.path().join("keys");
    std::fs::create_dir_all(&keys_dir).unwrap();
    std::fs::write(keys_dir.join("key1"), b"secret key material 1").unwrap();
    std::fs::write(keys_dir.join("key2"), b"secret key material 2").unwrap();
    std::fs::write(keys_dir.join("key3"), b"secret key material 3").unwrap();

    // Create pre-signed messages
    let msgs = PreSignedShredMessages::generate(&identity);
    msgs.save(dir.path()).unwrap();

    // Store SMK in secure storage
    let secure_storage = MemoryKeyStorage::new();
    let smk = vauchi_core::crypto::ShreddingMasterKey::derive_from_seed(&[0x42; 32]);
    secure_storage
        .save_key(SMK_KEY_NAME, smk.as_bytes())
        .unwrap();

    (dir, secure_storage)
}

// =============================================================================
// Widget Panic Shred Tests — TDD Red Phase
// =============================================================================

/// The widget panic shred should securely delete the SQLite database.
///
/// Traces to: "Full panic shred per emergency_shred.feature"
// @scenario: panic_widget:Widget triggers full panic shred
// @scenario: emergency_shred.feature:Panic shred destroys everything immediately
// @scenario: emergency_shred.feature:Database WAL and SHM files are cleaned up
#[test]
fn test_widget_panic_shred_destroys_database() {
    let (dir, secure_storage) = setup_widget_test_env();
    let db_path = dir.path().join("vauchi.db");

    // Verify database exists before shred
    assert!(
        db_path.exists(),
        "Database should exist before widget shred"
    );

    // Execute widget panic shred
    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // Database should be destroyed
    assert!(
        report.sqlite_destroyed,
        "Report should indicate database was destroyed"
    );
    assert!(
        !db_path.exists(),
        "Database file should not exist after widget shred"
    );
}

/// The widget panic shred should delete all key files.
///
/// Traces to: "Keys destroyed"
// @scenario: panic_widget:Widget triggers full panic shred
// @scenario: emergency_shred.feature:Panic shred destroys everything immediately
// @scenario: emergency_shred.feature:Files are overwritten with zeros before deletion
#[test]
fn test_widget_panic_shred_destroys_keys() {
    let (dir, secure_storage) = setup_widget_test_env();
    let keys_dir = dir.path().join("keys");

    // Verify key files exist before shred
    assert!(keys_dir.exists(), "Keys directory should exist");
    let key_count = std::fs::read_dir(&keys_dir).unwrap().count();
    assert_eq!(key_count, 3, "Should have 3 key files");

    // Execute widget panic shred
    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // All key files should be destroyed
    assert_eq!(
        report.key_files_destroyed, 3,
        "All 3 key files should be destroyed"
    );
    assert!(
        !keys_dir.exists() || std::fs::read_dir(&keys_dir).unwrap().count() == 0,
        "Keys directory should be empty or removed"
    );
}

/// The widget panic shred should securely delete the identity file.
///
/// Traces to: "Full panic shred per emergency_shred.feature"
// @scenario: panic_widget:Widget triggers full panic shred
// @scenario: emergency_shred.feature:Panic shred destroys everything immediately
// @scenario: emergency_shred.feature:Files are overwritten with zeros before deletion
#[test]
fn test_widget_panic_shred_destroys_identity() {
    let (dir, secure_storage) = setup_widget_test_env();
    let identity_path = dir.path().join("identity.json");

    // Verify identity file exists
    assert!(
        identity_path.exists(),
        "Identity file should exist before shred"
    );

    // Execute widget panic shred
    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // Identity file should be destroyed
    assert!(
        report.identity_file_destroyed,
        "Report should indicate identity was destroyed"
    );
    assert!(
        !identity_path.exists(),
        "Identity file should not exist after widget shred"
    );
}

/// The widget panic shred should return a complete ShredReport.
///
/// Traces to: "Full panic shred per emergency_shred.feature"
// @scenario: panic_widget:Widget triggers full panic shred
// @scenario: emergency_shred.feature:Shred report tracks what was destroyed
#[test]
fn test_widget_panic_shred_returns_report() {
    let (dir, secure_storage) = setup_widget_test_env();

    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // Report should reflect all operations
    assert!(report.smk_destroyed, "SMK should be destroyed");
    assert!(
        report.identity_file_destroyed,
        "Identity file should be destroyed"
    );
    assert_eq!(
        report.key_files_destroyed, 3,
        "3 key files should be destroyed"
    );
    assert!(report.sqlite_destroyed, "Database should be destroyed");
    assert!(
        report.pre_signed_deleted,
        "Pre-signed file should be deleted"
    );
    assert!(
        report.data_dir_deleted,
        "Data directory should be deleted after cleanup"
    );
}

/// The widget panic shred must work WITHOUT a fully initialized Vauchi instance.
/// This is the core requirement: widgets need to trigger shred without opening the app.
///
/// Traces to: "Works without app unlock (intentional for emergency)"
// @scenario: panic_widget:Widget works without app unlock
#[test]
fn test_widget_panic_shred_works_without_vauchi_init() {
    // Set up a data directory manually (no Vauchi instance created)
    let dir = tempfile::tempdir().unwrap();

    // Manually create the files that would exist in a real installation
    let db_path = dir.path().join("vauchi.db");
    std::fs::write(&db_path, b"fake database content for widget test").unwrap();

    let identity_path = dir.path().join("identity.json");
    std::fs::write(&identity_path, b"fake identity for widget test").unwrap();

    let keys_dir = dir.path().join("keys");
    std::fs::create_dir_all(&keys_dir).unwrap();
    std::fs::write(keys_dir.join("key1"), b"key material").unwrap();

    // Set up SMK in secure storage
    let secure_storage = MemoryKeyStorage::new();
    let smk = vauchi_core::crypto::ShreddingMasterKey::derive_from_seed(&[0x77; 32]);
    secure_storage
        .save_key(SMK_KEY_NAME, smk.as_bytes())
        .unwrap();

    // Execute widget panic shred — NO Vauchi instance, NO Storage, NO Identity
    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // Should succeed and destroy everything
    assert!(report.smk_destroyed, "SMK should be destroyed");
    assert!(
        report.identity_file_destroyed,
        "Identity file should be destroyed"
    );
    assert_eq!(report.key_files_destroyed, 1, "One key file destroyed");
    assert!(
        report.sqlite_destroyed,
        "Database should be destroyed (even fake)"
    );

    // Verify files are actually gone
    assert!(!db_path.exists(), "Database should not exist");
    assert!(!identity_path.exists(), "Identity should not exist");
}

/// Widget panic shred should succeed even on an empty/missing directory.
/// This handles the case where the widget is triggered but data was already deleted.
///
/// Traces to: "Works without app unlock (intentional for emergency)"
// @scenario: panic_widget:Widget works without app unlock
#[test]
fn test_widget_panic_shred_on_empty_directory_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let secure_storage = MemoryKeyStorage::new();

    // No files, no SMK — should still succeed gracefully
    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // Nothing to destroy, but should not error
    assert_eq!(report.key_files_destroyed, 0);
    // SMK wasn't present, but deletion attempt shouldn't fail
    // (MemoryKeyStorage.delete_key succeeds even for non-existent keys)
    assert!(report.smk_destroyed, "SMK deletion should report success");
}

/// Widget panic shred should destroy the SMK from SecureStorage.
///
/// Traces to: "Keys destroyed"
// @scenario: panic_widget:Widget triggers full panic shred
// @scenario: emergency_shred.feature:Panic shred destroys everything immediately
#[test]
fn test_widget_panic_shred_destroys_smk() {
    let (dir, secure_storage) = setup_widget_test_env();

    // Verify SMK exists before shred
    assert!(
        secure_storage.load_key(SMK_KEY_NAME).unwrap().is_some(),
        "SMK should exist before widget shred"
    );

    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    assert!(report.smk_destroyed, "SMK should be destroyed");
    assert!(
        secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none(),
        "SMK should be absent from SecureStorage after widget shred"
    );
}

/// Widget panic shred should attempt to load pre-signed messages before destroying data.
///
/// Traces to: "Pre-signed messages sent FIRST"
// @scenario: panic_widget:Widget shred sends pre-signed notifications
// @scenario: emergency_shred.feature:Panic shred follows sign-before-destroy pattern
#[test]
fn test_widget_panic_shred_loads_pre_signed_before_destruction() {
    let (dir, secure_storage) = setup_widget_test_env();

    // Verify pre-signed messages file exists
    let pre_signed_path = PreSignedShredMessages::file_path(dir.path());
    assert!(
        pre_signed_path.exists(),
        "Pre-signed file should exist before shred"
    );

    let report = widget_panic_shred(dir.path(), &secure_storage).unwrap();

    // Pre-signed file should have been loaded and then deleted
    assert!(
        report.pre_signed_deleted,
        "Pre-signed messages should be deleted after use"
    );
    assert!(
        !pre_signed_path.exists(),
        "Pre-signed file should not exist after shred"
    );
}

// =============================================================================
// WidgetConfirmationMode Tests
// =============================================================================

/// Test that WidgetConfirmationMode variants are properly constructible.
// @scenario: panic_widget:Configure widget confirmation mode
#[test]
fn test_widget_confirmation_mode_variants() {
    let mode = WidgetConfirmationMode::TapConfirm;
    assert!(matches!(mode, WidgetConfirmationMode::TapConfirm));

    let mode = WidgetConfirmationMode::LongPress;
    assert!(matches!(mode, WidgetConfirmationMode::LongPress));

    let mode = WidgetConfirmationMode::DoubleTap;
    assert!(matches!(mode, WidgetConfirmationMode::DoubleTap));
}

/// Test that WidgetConfirmationMode can be cloned and compared.
// @scenario: panic_widget:Configure widget confirmation mode
#[test]
fn test_widget_confirmation_mode_clone_eq() {
    let mode1 = WidgetConfirmationMode::TapConfirm;
    let mode2 = mode1.clone();
    assert_eq!(mode1, mode2);

    let mode3 = WidgetConfirmationMode::LongPress;
    assert_ne!(mode1, mode3);
}
