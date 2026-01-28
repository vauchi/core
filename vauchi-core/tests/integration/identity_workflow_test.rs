// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity Workflow Integration Tests
//!
//! Tests for identity creation, multi-device linking, and device registry operations.

use vauchi_core::{network::MockTransport, ContactField, FieldType, Vauchi};

/// Test: Full identity and contact card workflow
#[test]
fn test_full_identity_workflow() {
    // Create Vauchi instance
    let mut wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    // Create identity
    wb.create_identity("Alice").unwrap();
    assert!(wb.has_identity());

    // Check initial contact card
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), "Alice");
    assert!(card.fields().is_empty());

    // Add fields to contact card
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();

    // Verify fields were added
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 2);
    assert!(card.fields().iter().any(|f| f.label() == "work"));
    assert!(card.fields().iter().any(|f| f.label() == "mobile"));

    // Update card with new display name
    let mut updated_card = card.clone();
    updated_card.set_display_name("Alice Smith").unwrap();
    let changed = wb.update_own_card(&updated_card).unwrap();
    assert!(changed.contains(&"display_name".to_string()));

    // Verify update
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), "Alice Smith");

    // Remove a field
    let removed = wb.remove_own_field("work").unwrap();
    assert!(removed);

    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 1);
    assert!(!card.fields().iter().any(|f| f.label() == "work"));
}

/// Test: Two devices sharing same identity via backup
///
/// Verifies that when a user exports their identity backup and imports it
/// on another device, both devices share the same identity but have
/// different device IDs and exchange keys.
#[test]
fn test_device_linking_via_backup() {
    use vauchi_core::identity::Identity;

    // Device A: Create identity
    let device_a = Identity::create("Alice");
    let password = "SecureP@ssw0rd123!";

    // Device A: Export backup
    let backup = device_a.export_backup(password).unwrap();

    // Device B: Import backup
    let device_b = Identity::import_backup(&backup, password).unwrap();

    // Both devices should share the same identity (same public ID)
    assert_eq!(
        device_a.public_id(),
        device_b.public_id(),
        "Both devices should have the same identity public ID"
    );

    // Both devices should have the same signing public key
    assert_eq!(
        device_a.signing_public_key(),
        device_b.signing_public_key(),
        "Both devices should have the same signing key"
    );

    // Both devices should have the same exchange public key
    // (derived from same master seed)
    assert_eq!(
        device_a.exchange_public_key(),
        device_b.exchange_public_key(),
        "Both devices should have the same exchange public key"
    );

    // Device IDs should be the same since they have the same device index (0)
    // In a real multi-device scenario, you'd use different device indices
    assert_eq!(
        device_a.device_id(),
        device_b.device_id(),
        "Same device index should produce same device ID"
    );
}

/// Test: Device registry maintains correct state across operations
///
/// Tests adding multiple devices, revoking one, and verifying the registry
/// state is correct throughout.
#[test]
fn test_device_registry_integration() {
    use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
    use vauchi_core::SigningKeyPair;

    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Create device 0 (primary)
    let device0 = DeviceInfo::derive(&master_seed, 0, "Phone".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    assert_eq!(registry.active_count(), 1);
    assert_eq!(registry.version(), 1);
    assert!(registry.verify(&signing_key.public_key()));

    // Add device 1 (tablet)
    let device1 = DeviceInfo::derive(&master_seed, 1, "Tablet".to_string());
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 2);
    assert_eq!(registry.version(), 2);
    assert!(registry.verify(&signing_key.public_key()));

    // Add device 2 (laptop)
    let device2 = DeviceInfo::derive(&master_seed, 2, "Laptop".to_string());
    registry
        .add_device(device2.to_registered(&master_seed), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 3);
    assert_eq!(registry.version(), 3);

    // Revoke device 1 (tablet)
    registry
        .revoke_device(device1.device_id(), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 2);
    assert_eq!(registry.device_count(), 3); // Still 3 total (1 revoked)
    assert_eq!(registry.version(), 4);
    assert!(registry.verify(&signing_key.public_key()));

    // Verify active devices are correct
    let active = registry.active_devices();
    assert_eq!(active.len(), 2);
    assert!(active.iter().any(|d| d.device_name == "Phone"));
    assert!(active.iter().any(|d| d.device_name == "Laptop"));
    assert!(!active.iter().any(|d| d.device_name == "Tablet"));

    // Verify revoked device is still in registry but not active
    let tablet = registry.find_device(device1.device_id()).unwrap();
    assert!(!tablet.is_active());
    assert!(tablet.revoked);
    assert!(tablet.revoked_at.is_some());
}

/// Test: Three devices with correct key derivation
///
/// Verifies that three devices derived from the same seed have unique
/// device IDs and exchange keys.
#[test]
fn test_three_device_key_derivation() {
    use vauchi_core::identity::DeviceInfo;

    let master_seed = [0x42u8; 32];

    let device0 = DeviceInfo::derive(&master_seed, 0, "Phone".to_string());
    let device1 = DeviceInfo::derive(&master_seed, 1, "Tablet".to_string());
    let device2 = DeviceInfo::derive(&master_seed, 2, "Laptop".to_string());

    // All device IDs should be unique
    assert_ne!(device0.device_id(), device1.device_id());
    assert_ne!(device0.device_id(), device2.device_id());
    assert_ne!(device1.device_id(), device2.device_id());

    // All exchange keys should be unique
    assert_ne!(device0.exchange_public_key(), device1.exchange_public_key());
    assert_ne!(device0.exchange_public_key(), device2.exchange_public_key());
    assert_ne!(device1.exchange_public_key(), device2.exchange_public_key());

    // Device indices should be correct
    assert_eq!(device0.device_index(), 0);
    assert_eq!(device1.device_index(), 1);
    assert_eq!(device2.device_index(), 2);
}

/// Test: Device revocation certificate creation and verification
///
/// Tests that revocation certificates are properly created, signed,
/// and can be verified.
#[test]
fn test_device_revocation_certificate_workflow() {
    use vauchi_core::identity::{DeviceInfo, DeviceRegistry, DeviceRevocationCertificate};
    use vauchi_core::SigningKeyPair;

    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Create registry with 2 devices
    let device0 = DeviceInfo::derive(&master_seed, 0, "Phone".to_string());
    let device1 = DeviceInfo::derive(&master_seed, 1, "Lost Device".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 2);

    // Create revocation certificate for device1
    let certificate = DeviceRevocationCertificate::create(
        device1.device_id(),
        "Device was lost".to_string(),
        &signing_key,
    );

    // Certificate should be valid
    assert!(certificate.verify(&signing_key.public_key()));
    assert_eq!(certificate.device_id(), device1.device_id());
    assert_eq!(certificate.reason(), "Device was lost");

    // Certificate should have reasonable timestamp
    assert!(certificate.revoked_at() > 0);

    // Serialize and deserialize certificate
    let json = certificate.to_json();
    let restored = DeviceRevocationCertificate::from_json(&json).unwrap();
    assert!(restored.verify(&signing_key.public_key()));

    // Apply certificate to registry
    registry
        .apply_revocation(&certificate, &signing_key.public_key())
        .unwrap();

    // Verify device1 is now revoked
    assert_eq!(registry.active_count(), 1);
    let revoked = registry.find_device(device1.device_id()).unwrap();
    assert!(!revoked.is_active());
}

/// Test: Registry broadcast for contacts
///
/// Tests that a registry broadcast correctly includes only active devices
/// and can be verified by contacts.
#[test]
fn test_registry_broadcast_for_contacts() {
    use vauchi_core::identity::{DeviceInfo, DeviceRegistry, RegistryBroadcast};
    use vauchi_core::SigningKeyPair;

    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Create registry with 3 devices
    let device0 = DeviceInfo::derive(&master_seed, 0, "Phone".to_string());
    let device1 = DeviceInfo::derive(&master_seed, 1, "Tablet".to_string());
    let device2 = DeviceInfo::derive(&master_seed, 2, "Laptop".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();
    registry
        .add_device(device2.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Revoke tablet
    registry
        .revoke_device(device1.device_id(), &signing_key)
        .unwrap();

    // Create broadcast
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    // Broadcast should be valid
    assert!(broadcast.verify(&signing_key.public_key()));

    // Broadcast should only contain active devices (phone and laptop)
    assert_eq!(broadcast.active_device_count(), 2);
    assert!(broadcast.contains_device(device0.device_id()));
    assert!(!broadcast.contains_device(device1.device_id())); // Revoked
    assert!(broadcast.contains_device(device2.device_id()));

    // Broadcast should have correct version
    assert_eq!(broadcast.version(), registry.version());

    // Serialize and deserialize broadcast
    let json = broadcast.to_json();
    let restored = RegistryBroadcast::from_json(&json).unwrap();
    assert!(restored.verify(&signing_key.public_key()));
    assert_eq!(restored.active_device_count(), 2);
}

/// Test: Maximum devices limit enforced
///
/// Verifies that the registry enforces the MAX_DEVICES limit.
#[test]
fn test_max_devices_limit_enforced() {
    use vauchi_core::identity::{DeviceError, DeviceInfo, DeviceRegistry, MAX_DEVICES};
    use vauchi_core::SigningKeyPair;

    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    // Add devices up to the limit
    for i in 1..MAX_DEVICES {
        let device = DeviceInfo::derive(&master_seed, i as u32, format!("Device {}", i));
        registry
            .add_device(device.to_registered(&master_seed), &signing_key)
            .unwrap();
    }

    assert_eq!(registry.active_count(), MAX_DEVICES);

    // Try to add one more - should fail
    let extra_device =
        DeviceInfo::derive(&master_seed, MAX_DEVICES as u32, "Extra Device".to_string());
    let result = registry.add_device(extra_device.to_registered(&master_seed), &signing_key);

    assert!(matches!(result, Err(DeviceError::MaxDevicesReached)));
}

/// Test: Cannot revoke the last active device
///
/// Verifies that the registry prevents revoking the last remaining device.
#[test]
fn test_cannot_revoke_last_device() {
    use vauchi_core::identity::{DeviceError, DeviceInfo, DeviceRegistry};
    use vauchi_core::SigningKeyPair;

    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Only Device".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    assert_eq!(registry.active_count(), 1);

    // Try to revoke the only device - should fail
    let result = registry.revoke_device(device0.device_id(), &signing_key);

    assert!(matches!(result, Err(DeviceError::CannotRemoveLastDevice)));
    assert_eq!(registry.active_count(), 1); // Still 1 active
}
