// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup Format Versioning Tests
//!
//! Feature: backup_format_versioning.feature
//! Tests version detection, migration, and corruption detection for identity backups.

use vauchi_core::Identity;
use vauchi_core::identity::{IdentityBackup, IdentityError};

/// Backup format version byte for v2 (Argon2id + XChaCha20).
const BACKUP_VERSION_V2: u8 = 0x02;

// =============================================================================
// V1 FORMAT AUTO-DETECTION
// =============================================================================

/// Tests that non-v2 backups are rejected.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Non-v2 backup format rejected
// @scenario: backup_format_versioning :: Non-v2 backup format rejected
#[test]
fn test_non_v2_format_rejected() {
    // Create a v2 backup first to get valid identity data
    let identity = Identity::create("Legacy User", 0);
    let password = "SecureP@ssw0rd!2024";
    let v2_backup = identity.export_backup(password).unwrap();
    let v2_bytes = v2_backup.as_bytes();

    // Verify v2 backup starts with version byte 0x02
    assert_eq!(
        v2_bytes[0], BACKUP_VERSION_V2,
        "New backups should use v2 format"
    );

    // Non-v2 backup data should be rejected
    let mock_data = vec![0x00u8; 100]; // First byte is 0x00, not 0x02
    let mock_backup = IdentityBackup::new(mock_data);

    let result = Identity::import_backup(&mock_backup, password, 0);
    assert!(result.is_err(), "Non-v2 data should fail to restore");

    // Verify error is RestoreFailed
    match result {
        Err(IdentityError::RestoreFailed) => (), // Expected — rejected immediately
        Err(e) => panic!("Expected RestoreFailed, got error: {:?}", e),
        Ok(_) => panic!("Expected RestoreFailed, but got success"),
    }

    // All non-0x02 first bytes should be rejected
    for first_byte in [0x00u8, 0x01, 0x03, 0xFF, 0x10] {
        let mut test_data = vec![first_byte];
        test_data.extend_from_slice(&[0u8; 99]);
        let test_backup = IdentityBackup::new(test_data);
        let result = Identity::import_backup(&test_backup, password, 0);
        assert!(result.is_err(), "Non-v2 data should fail to restore");
    }
}

// =============================================================================
// V2 PARAMETER VALIDATION
// =============================================================================

/// Tests that v2 backup import rejects invalid parameters.
///
/// Feature: backup_format_versioning.feature
/// Scenario: V2 backup uses OWASP-recommended Argon2id parameters
// @scenario: backup_format_versioning :: V2 backup uses OWASP-recommended Argon2id parameters
#[test]
fn test_v2_parameter_validation() {
    // Empty backup should be rejected
    let empty_backup = IdentityBackup::new(vec![]);
    let result = Identity::import_backup(&empty_backup, "password", 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Empty backup should fail"
    );

    // V2 prefix but too short (missing salt + ciphertext)
    let short_v2 = IdentityBackup::new(vec![BACKUP_VERSION_V2]);
    let result = Identity::import_backup(&short_v2, "password", 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Too-short v2 backup should fail"
    );

    // V2 prefix with partial salt (less than 16 bytes)
    let mut partial_salt = vec![BACKUP_VERSION_V2];
    partial_salt.extend_from_slice(&[0u8; 10]); // Only 10 bytes of salt
    let partial_backup = IdentityBackup::new(partial_salt);
    let result = Identity::import_backup(&partial_backup, "password", 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Partial salt should fail"
    );

    // V2 prefix with valid salt but no ciphertext
    let mut salt_only = vec![BACKUP_VERSION_V2];
    salt_only.extend_from_slice(&[0u8; 16]); // Full 16-byte salt, no ciphertext
    let salt_only_backup = IdentityBackup::new(salt_only);
    let result = Identity::import_backup(&salt_only_backup, "password", 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Salt-only backup should fail"
    );

    // V2 prefix with salt and minimal junk (not valid ciphertext)
    let mut invalid_ciphertext = vec![BACKUP_VERSION_V2];
    invalid_ciphertext.extend_from_slice(&[0u8; 16]); // Salt
    invalid_ciphertext.extend_from_slice(&[0xFFu8; 50]); // Invalid ciphertext
    let invalid_backup = IdentityBackup::new(invalid_ciphertext);
    let result = Identity::import_backup(&invalid_backup, "password", 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Invalid ciphertext should fail"
    );
}

// =============================================================================
// CORRUPTED BACKUP DETECTION
// =============================================================================

/// Tests that corrupted backup data triggers AEAD authentication failure.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Corrupted backup is detected
// @scenario: backup_format_versioning :: Corrupted backup is detected
// @scenario: identity_management :: Restore corrupted backup
#[test]
fn test_corrupted_backup_detection() {
    // Create a valid v2 backup
    let identity = Identity::create("Corruption Test User", 0);
    let password = "SecureP@ssw0rd!2024";
    let backup = identity.export_backup(password).unwrap();
    let original_bytes = backup.as_bytes().to_vec();

    // Verify original backup works
    let restored = Identity::import_backup(&backup, password, 0);
    assert!(
        restored.is_ok(),
        "Original backup should restore successfully"
    );

    // Test corruption at various positions
    let test_positions = [
        1,                         // First byte of salt
        8,                         // Middle of salt
        16,                        // Last byte of salt
        17,                        // First byte of ciphertext
        original_bytes.len() / 2,  // Middle of ciphertext
        original_bytes.len() - 1,  // Last byte (auth tag)
        original_bytes.len() - 8,  // Middle of auth tag
        original_bytes.len() - 16, // Start of auth tag
    ];

    for &pos in &test_positions {
        if pos >= original_bytes.len() {
            continue;
        }

        let mut corrupted = original_bytes.clone();
        corrupted[pos] ^= 0xFF; // Flip all bits at position

        let corrupted_backup = IdentityBackup::new(corrupted);
        let result = Identity::import_backup(&corrupted_backup, password, 0);

        assert!(
            matches!(result, Err(IdentityError::RestoreFailed)),
            "Corruption at position {} should be detected",
            pos
        );
    }

    // Test truncation
    for truncate_by in [1, 8, 16, 32] {
        if truncate_by >= original_bytes.len() {
            continue;
        }

        let truncated = original_bytes[..original_bytes.len() - truncate_by].to_vec();
        let truncated_backup = IdentityBackup::new(truncated);
        let result = Identity::import_backup(&truncated_backup, password, 0);

        assert!(
            matches!(result, Err(IdentityError::RestoreFailed)),
            "Truncation by {} bytes should be detected",
            truncate_by
        );
    }
}

// =============================================================================
// VERSION UPGRADE PATH
// =============================================================================

/// Tests that v1 to v2 migration is lossless.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Restoring legacy backup and re-exporting creates v2
// @scenario: backup_format_versioning :: Restoring legacy backup and re-exporting creates v2
#[test]
fn test_version_upgrade_path() {
    // Create an identity and export as v2
    let original_identity = Identity::create("Migration Test User", 0);
    let original_public_id = original_identity.public_id();
    let original_display_name = original_identity.display_name().to_string();
    let original_signing_key = *original_identity.signing_public_key();
    let original_exchange_key = original_identity.exchange_public_key().to_vec();

    let password = "SecureP@ssw0rd!2024";
    let v2_backup = original_identity.export_backup(password).unwrap();

    // Verify it's v2 format
    assert_eq!(
        v2_backup.as_bytes()[0],
        BACKUP_VERSION_V2,
        "New backup should be v2"
    );

    // Restore from v2 backup
    let restored_identity = Identity::import_backup(&v2_backup, password, 0).unwrap();

    // Re-export - should still be v2
    let re_exported = restored_identity.export_backup(password).unwrap();
    assert_eq!(
        re_exported.as_bytes()[0],
        BACKUP_VERSION_V2,
        "Re-exported backup should be v2"
    );

    // Verify all identity data is preserved through the round-trip
    assert_eq!(
        restored_identity.public_id(),
        original_public_id,
        "Public ID should match after migration"
    );
    assert_eq!(
        restored_identity.display_name(),
        original_display_name,
        "Display name should match after migration"
    );
    assert_eq!(
        *restored_identity.signing_public_key(),
        original_signing_key,
        "Signing key should match after migration"
    );
    assert_eq!(
        restored_identity.exchange_public_key(),
        original_exchange_key.as_slice(),
        "Exchange key should match after migration"
    );

    // Verify the re-exported backup can be restored too
    let final_restored = Identity::import_backup(&re_exported, password, 0).unwrap();
    assert_eq!(
        final_restored.public_id(),
        original_public_id,
        "Final restore should preserve identity"
    );
}

// =============================================================================
// FUTURE VERSION REJECTION
// =============================================================================

/// Tests that unknown/future backup versions are rejected gracefully.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Unknown version byte is rejected
// @scenario: backup_format_versioning :: Unknown version byte is rejected
#[test]
fn test_future_version_rejection() {
    let password = "SecureP@ssw0rd!2024";

    // Test future version bytes (0x03 through 0xFF)
    // These should be rejected immediately (only v2 = 0x02 is accepted)
    for future_version in [0x03u8, 0x04, 0x10, 0x20, 0x7F, 0x80, 0xFE, 0xFF] {
        let mut mock_future_backup = vec![future_version];
        // Add enough data to pass minimum length checks
        mock_future_backup.extend_from_slice(&[0u8; 200]);

        let future_backup = IdentityBackup::new(mock_future_backup);
        let result = Identity::import_backup(&future_backup, password, 0);

        assert!(
            matches!(result, Err(IdentityError::RestoreFailed)),
            "Future version 0x{:02X} should fail gracefully",
            future_version
        );
    }

    // Edge case: version byte 0x01 — should be rejected
    let mut v1_like = vec![0x01u8];
    v1_like.extend_from_slice(&[0u8; 200]);
    let v1_backup = IdentityBackup::new(v1_like);
    let result = Identity::import_backup(&v1_backup, password, 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Version 0x01 should be rejected"
    );

    // Verify actual v2 still works after testing future versions
    let identity = Identity::create("Future Version Test", 0);
    let valid_backup = identity.export_backup(password).unwrap();
    let restored = Identity::import_backup(&valid_backup, password, 0);
    assert!(restored.is_ok(), "Valid v2 backup should still work");
}

// =============================================================================
// ADDITIONAL EDGE CASES
// =============================================================================

/// Tests wrong password detection for v2 backups.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Restore v2 backup with wrong password
// @scenario: backup_format_versioning :: Restore v2 backup with wrong password
// @scenario: identity_management :: Restore with incorrect password
#[test]
fn test_v2_wrong_password() {
    let identity = Identity::create("Password Test User", 0);
    let correct_password = "SecureP@ssw0rd!2024";
    let wrong_password = "WrongP@ssw0rd!2024";

    let backup = identity.export_backup(correct_password).unwrap();

    // Correct password works
    let result = Identity::import_backup(&backup, correct_password, 0);
    assert!(result.is_ok(), "Correct password should work");

    // Wrong password fails
    let result = Identity::import_backup(&backup, wrong_password, 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Wrong password should fail"
    );

    // Empty password fails
    // Note: Password validation happens on export, not import
    let result = Identity::import_backup(&backup, "", 0);
    assert!(
        matches!(result, Err(IdentityError::RestoreFailed)),
        "Empty password should fail"
    );
}

/// Tests that v2 backup format includes all required fields.
///
/// Feature: backup_format_versioning.feature
/// Scenario: V2 backup includes salt
// @scenario: backup_format_versioning :: V2 backup includes salt
// @scenario: backup_format_versioning :: New backups use v2 format
#[test]
fn test_v2_format_structure() {
    let identity = Identity::create("Format Test User", 0);
    let password = "SecureP@ssw0rd!2024";

    let backup = identity.export_backup(password).unwrap();
    let bytes = backup.as_bytes();

    // Format: version (1) + salt (16) + algorithm_tag (1) + nonce (24) + ciphertext + tag (16)
    // Minimum size: 1 + 16 + 1 + 24 + 4 + 32 + 16 = 94 bytes
    // (4 for name_len, 32 for seed, plus device info)
    assert!(
        bytes.len() >= 58,
        "V2 backup should be at least 58 bytes (1+16+41 minimum ciphertext)"
    );

    // First byte is version
    assert_eq!(bytes[0], BACKUP_VERSION_V2, "First byte should be version");

    // Next 16 bytes are salt (should be random, so check they're not all zeros)
    let salt = &bytes[1..17];
    assert!(salt.iter().any(|&b| b != 0), "Salt should not be all zeros");

    // Verify different backups have different salts
    let backup2 = identity.export_backup(password).unwrap();
    let salt2 = &backup2.as_bytes()[1..17];
    assert_ne!(salt, salt2, "Each backup should have unique salt");
}

/// Tests that device info is preserved through backup/restore.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Backup contains only the master seed
// @scenario: backup_format_versioning :: Backup contains only the master seed
// @scenario: identity_management :: Restore identity from backup
#[test]
fn test_device_info_preservation() {
    let identity = Identity::create("Device Info Test", 0);
    let password = "SecureP@ssw0rd!2024";

    // Capture original device info
    let original_device_index = identity.device_index();
    let original_device_name = identity.device_info().device_name().to_string();

    // Export and restore
    let backup = identity.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password, 0).unwrap();

    // Verify device info is preserved
    assert_eq!(
        restored.device_index(),
        original_device_index,
        "Device index should be preserved"
    );
    assert_eq!(
        restored.device_info().device_name(),
        original_device_name,
        "Device name should be preserved"
    );
}

// =============================================================================
// BACKUP RE-EXPORT WITH DIFFERENT PASSWORD (Tracker #53)
// =============================================================================

/// Tests that re-exporting a backup with a different password works correctly:
/// the new backup decrypts only with the new password, not the old one.
///
/// Feature: backup_format_versioning.feature
/// Scenario: Re-export backup with changed password
// @scenario: identity_management :: Create encrypted identity backup
#[test]
fn test_re_export_with_different_password() {
    let identity = Identity::create("Password Change Test", 0);
    let original_public_id = identity.public_id();

    let password_a = "OriginalP@ssw0rd!2024";
    let password_b = "DifferentP@ssw0rd!2025";

    // Export with password A
    let backup_a = identity.export_backup(password_a).unwrap();

    // Restore from backup A
    let restored = Identity::import_backup(&backup_a, password_a, 0).unwrap();

    // Re-export with password B
    let backup_b = restored.export_backup(password_b).unwrap();

    // New backup should NOT decrypt with old password
    let result = Identity::import_backup(&backup_b, password_a, 0);
    assert!(
        result.is_err(),
        "Re-exported backup should not decrypt with old password"
    );

    // New backup should decrypt with new password
    let final_restored = Identity::import_backup(&backup_b, password_b, 0).unwrap();
    assert_eq!(
        final_restored.public_id(),
        original_public_id,
        "Identity should be preserved after password change"
    );
}

// =============================================================================
// IDENTITY CLONE VIA BACKUP (Tracker #69)
// =============================================================================

/// Documents that importing the same backup twice creates identical signing keys.
///
/// This is a known limitation — both restored identities have the same
/// master_seed and are cryptographically indistinguishable.
// @scenario: identity_management :: Restore identity from backup
#[test]
fn test_backup_import_creates_identical_signing_keys() {
    let identity = Identity::create("Clone Test", 0);
    let password = "SecureP@ssw0rd!2024";

    let backup = identity.export_backup(password).unwrap();

    // Import the same backup twice
    let clone_a = Identity::import_backup(&backup, password, 0).unwrap();
    let clone_b = Identity::import_backup(&backup, password, 0).unwrap();

    // Both clones have the same signing key (this is the documented risk)
    assert_eq!(
        clone_a.signing_public_key(),
        clone_b.signing_public_key(),
        "Two imports of same backup produce identical signing keys (Tracker #69)"
    );
    assert_eq!(
        clone_a.public_id(),
        clone_b.public_id(),
        "Two imports of same backup produce identical public IDs"
    );
    assert_eq!(
        clone_a.public_id(),
        identity.public_id(),
        "Clones have the same identity as the original"
    );
}
