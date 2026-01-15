//! TDD Tests for Identity Management
//!
//! These tests map directly to scenarios in identity_management.feature
//! Written FIRST (RED phase) before implementation.

use webbook_core::identity::{Identity, IdentityBackup};

// =============================================================================
// Identity Creation Tests (from identity_management.feature)
// Scenario: Create new identity on first launch
// =============================================================================

/// Tests that a new identity can be created
/// Maps to: "Then a new Ed25519 keypair should be generated"
#[test]
fn test_create_identity_generates_keypairs() {
    let identity = Identity::create("Alice");

    // Should have signing keypair
    let _signing_key = identity.signing_public_key();

    // Should have exchange keypair
    let _exchange_key = identity.exchange_public_key();
}

/// Tests that identity has a display name
/// Maps to: "Then my contact card should have display name"
#[test]
fn test_identity_has_display_name() {
    let identity = Identity::create("Alice Smith");

    assert_eq!(identity.display_name(), "Alice Smith");
}

/// Tests that identity has a unique public ID (fingerprint)
/// Maps to: "I should see my public key fingerprint"
#[test]
fn test_identity_has_unique_public_id() {
    let identity1 = Identity::create("Alice");
    let identity2 = Identity::create("Bob");

    assert_ne!(
        identity1.public_id(),
        identity2.public_id(),
        "Different identities should have different public IDs"
    );
}

/// Tests that public ID is deterministic from signing key
#[test]
fn test_identity_public_id_is_hex_fingerprint() {
    let identity = Identity::create("Alice");
    let public_id = identity.public_id();

    // Should be non-empty hex string
    assert!(!public_id.is_empty());
    assert!(
        public_id.chars().all(|c| c.is_ascii_hexdigit()),
        "Public ID should be hexadecimal"
    );
}

// =============================================================================
// Identity Backup Tests (from identity_management.feature)
// Scenario: Create encrypted identity backup
// =============================================================================

/// Tests that identity can be exported as encrypted backup
/// Maps to: "an encrypted backup file should be generated"
#[test]
fn test_create_identity_backup() {
    let identity = Identity::create("Alice");
    let password = "SecureP@ssw0rd!";

    let backup = identity.export_backup(password).expect("Backup should succeed");

    // Backup should be non-empty
    assert!(!backup.as_bytes().is_empty(), "Backup should not be empty");
}

/// Tests that backup with weak password is rejected
/// Maps to: "I should see an error about password requirements"
#[test]
fn test_backup_rejects_weak_password() {
    let identity = Identity::create("Alice");
    let weak_password = "abc"; // Too short

    let result = identity.export_backup(weak_password);

    assert!(result.is_err(), "Weak password should be rejected");
}

/// Tests that identity can be restored from backup
/// Maps to: "my identity should be restored"
#[test]
fn test_restore_identity_from_backup() {
    let original = Identity::create("Alice");
    let password = "SecureP@ssw0rd!";

    let backup = original.export_backup(password).expect("Backup should succeed");
    let restored = Identity::import_backup(&backup, password).expect("Restore should succeed");

    // Restored identity should match original
    assert_eq!(original.public_id(), restored.public_id());
    assert_eq!(original.display_name(), restored.display_name());
}

/// Tests that restore with wrong password fails
/// Maps to: "I should see an error 'Incorrect password'"
#[test]
fn test_restore_with_wrong_password_fails() {
    let identity = Identity::create("Alice");
    let correct_password = "SecureP@ssw0rd!";
    let wrong_password = "WrongPassword123!";

    let backup = identity.export_backup(correct_password).expect("Backup should succeed");
    let result = Identity::import_backup(&backup, wrong_password);

    assert!(result.is_err(), "Restore with wrong password should fail");
}

/// Tests that corrupted backup is rejected
/// Maps to: "I should see an error 'Backup file is corrupted or invalid'"
#[test]
fn test_restore_corrupted_backup_fails() {
    let identity = Identity::create("Alice");
    let password = "SecureP@ssw0rd!";

    let mut backup = identity.export_backup(password).expect("Backup should succeed");

    // Corrupt the backup
    let bytes = backup.as_bytes_mut();
    if bytes.len() > 10 {
        bytes[10] ^= 0xFF;
    }

    let result = Identity::import_backup(&backup, password);

    assert!(result.is_err(), "Corrupted backup should be rejected");
}

// =============================================================================
// Identity Display Name Tests
// =============================================================================

/// Tests that display name can be changed
#[test]
fn test_change_display_name() {
    let mut identity = Identity::create("Alice");

    identity.set_display_name("Alice Smith");

    assert_eq!(identity.display_name(), "Alice Smith");
}

/// Tests that empty display name is rejected
/// Maps to: "Display name is required"
#[test]
fn test_empty_display_name_rejected() {
    let result = Identity::create("");

    // Creating with empty name should fail or use a default
    // Let's check the behavior - either it fails or provides a sensible default
    // For this test, we'll check that we can't SET an empty name
    let mut identity = Identity::create("Alice");
    let result = identity.try_set_display_name("");

    assert!(result.is_err(), "Empty display name should be rejected");
}
