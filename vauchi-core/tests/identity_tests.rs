//! Tests for identity
//! Extracted from mod.rs

use vauchi_core::*;

#[test]
fn test_create_identity() {
    let identity = Identity::create("Test User");
    assert_eq!(identity.display_name(), "Test User");
}

#[test]
fn test_backup_restore_roundtrip() {
    let original = Identity::create("Alice");
    let password = "correct-horse-battery-staple";
    let backup = original.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password).unwrap();
    assert_eq!(original.public_id(), restored.public_id());
}

#[test]
fn test_identity_has_device_info() {
    let identity = Identity::create("Alice");
    assert_eq!(identity.device_index(), 0);
    assert_eq!(identity.device_info().device_name(), "Primary Device");
}

#[test]
fn test_backup_restore_preserves_device_info() {
    // Create identity with custom device info using public from_device_link
    let master_seed = [0x42u8; 32];
    let original =
        Identity::from_device_link(master_seed, "Alice".to_string(), 3, "My Phone".to_string());

    let password = "correct-horse-battery-staple";
    let backup = original.export_backup(password).unwrap();
    let restored = Identity::import_backup(&backup, password).unwrap();

    assert_eq!(restored.device_index(), 3);
    assert_eq!(restored.device_info().device_name(), "My Phone");
    assert_eq!(restored.device_id(), original.device_id());
}

#[test]
fn test_device_id_deterministic() {
    let identity1 = Identity::create("Alice");
    let identity2 = Identity::create("Bob");

    // Different identities have different device IDs
    assert_ne!(identity1.device_id(), identity2.device_id());
}
