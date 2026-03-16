// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for identity::device
//! Extracted from device.rs

use vauchi_core::identity::*;
use vauchi_core::*;

fn test_master_seed() -> [u8; 32] {
    [0x42u8; 32]
}

fn test_signing_keypair() -> SigningKeyPair {
    SigningKeyPair::from_seed(&test_master_seed())
}

// @scenario: device_management:Device-specific keys
#[test]
fn test_device_key_derivation_is_deterministic() {
    let seed = test_master_seed();

    let device1 = DeviceInfo::derive(&seed, 0, "Device 1".to_string());
    let device2 = DeviceInfo::derive(&seed, 0, "Device 1".to_string());

    assert_eq!(device1.device_id(), device2.device_id());
    assert_eq!(device1.exchange_public_key(), device2.exchange_public_key());
}

// @scenario: device_management:Device-specific keys
#[test]
fn test_different_index_different_keys() {
    let seed = test_master_seed();

    let device0 = DeviceInfo::derive(&seed, 0, "Device 0".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "Device 1".to_string());

    assert_ne!(device0.device_id(), device1.device_id());
    assert_ne!(device0.exchange_public_key(), device1.exchange_public_key());
}

// @scenario: device_management:Device-specific keys
#[test]
fn test_different_seed_different_keys() {
    let seed1 = [0x42u8; 32];
    let seed2 = [0x43u8; 32];

    let device1 = DeviceInfo::derive(&seed1, 0, "Device".to_string());
    let device2 = DeviceInfo::derive(&seed2, 0, "Device".to_string());

    assert_ne!(device1.device_id(), device2.device_id());
    assert_ne!(device1.exchange_public_key(), device2.exchange_public_key());
}

// @scenario: device_management:Device registry version tracking
#[test]
fn test_device_registry_creation() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

    assert_eq!(registry.version(), 1);
    assert_eq!(registry.active_count(), 1);
    assert!(registry.verify(&signing_key.public_key()));
}

// @scenario: device_management:Link new device via QR code
// @scenario: device_management:Device registry version tracking
#[test]
fn test_add_device_to_registry() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
    registry
        .add_device(device1.to_registered(&seed), &signing_key)
        .unwrap();

    assert_eq!(registry.version(), 2);
    assert_eq!(registry.active_count(), 2);
    assert!(registry.verify(&signing_key.public_key()));
}

// @scenario: device_management:Maximum devices reached
#[test]
fn test_max_devices_limit() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Device 0".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);

    // Add devices up to limit
    for i in 1..MAX_DEVICES {
        let device = DeviceInfo::derive(&seed, i as u32, format!("Device {}", i));
        registry
            .add_device(device.to_registered(&seed), &signing_key)
            .unwrap();
    }

    assert_eq!(registry.active_count(), MAX_DEVICES);

    // Adding one more should fail
    let extra = DeviceInfo::derive(&seed, MAX_DEVICES as u32, "Extra".to_string());
    let result = registry.add_device(extra.to_registered(&seed), &signing_key);
    assert!(matches!(result, Err(DeviceError::MaxDevicesReached)));
}

// @scenario: device_management:Unlink a device remotely
#[test]
fn test_revoke_device() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
    registry
        .add_device(device1.to_registered(&seed), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 2);

    registry
        .revoke_device(device1.device_id(), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 1);
    assert_eq!(registry.all_devices().len(), 2); // Still in registry, just revoked
    assert!(registry.verify(&signing_key.public_key()));
}

// @scenario: device_management:Cannot unlink last device
#[test]
fn test_cannot_revoke_last_device() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Only Device".to_string());

    let mut registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

    let result = registry.revoke_device(device.device_id(), &signing_key);
    assert!(matches!(result, Err(DeviceError::CannotRemoveLastDevice)));
}

// @scenario: device_management:View device details
#[test]
fn test_find_device() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device_id = *device.device_id();

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

    let found = registry.find_device(&device_id);
    found.expect("expected Some");
    assert_eq!(found.unwrap().device_name, "Primary");

    let not_found = registry.find_device(&[0u8; 32]);
    assert!(not_found.is_none());
}

// @scenario: device_management:Link same device twice
#[test]
fn test_duplicate_device_rejected() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let mut registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

    let result = registry.add_device(device.to_registered(&seed), &signing_key);
    assert!(matches!(result, Err(DeviceError::DeviceAlreadyExists)));
}

#[test]
fn test_registry_serialization() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

    let json = serde_json::to_string(&registry).unwrap();
    let restored: DeviceRegistry = serde_json::from_str(&json).unwrap();

    assert_eq!(registry.version(), restored.version());
    assert_eq!(registry.active_count(), restored.active_count());
    assert!(restored.verify(&signing_key.public_key()));
}

// @scenario: device_management:Rename a device
#[test]
fn test_empty_device_name_rejected() {
    let seed = test_master_seed();
    let mut device = DeviceInfo::derive(&seed, 0, "Valid".to_string());

    let result = device.set_device_name("".to_string());
    assert!(matches!(result, Err(DeviceError::EmptyDeviceName)));
}

// ============================================================
// Phase 5 Tests: Device Revocation
// Based on features/device_management.feature @unlink and @security
// ============================================================

/// Scenario: Unlink a device remotely
/// "Device B should no longer receive updates"
/// "Device B should be notified of removal"
// @scenario: device_management:Unlink a device remotely
#[test]
fn test_device_revocation_certificate_creation() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 1, "Lost Device".to_string());

    // Create a revocation certificate
    let certificate = DeviceRevocationCertificate::create(
        device.device_id(),
        "Lost device - reported stolen".to_string(),
        &signing_key,
    );

    assert_eq!(certificate.device_id(), device.device_id());
    assert!(certificate.verify(&signing_key.public_key()));
}

/// Scenario: Lost device revocation
/// "Device B's device key should be revoked"
// @scenario: device_management:Lost device revocation
#[test]
fn test_device_revocation_certificate_has_timestamp() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 1, "Lost Device".to_string());

    let certificate =
        DeviceRevocationCertificate::create(device.device_id(), "Lost".to_string(), &signing_key);

    // Certificate should have valid timestamp
    assert!(certificate.revoked_at() > 0);
}

/// Test certificate serialization for transmission
#[test]
fn test_device_revocation_certificate_serialization() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 1, "Lost Device".to_string());

    let certificate =
        DeviceRevocationCertificate::create(device.device_id(), "Lost".to_string(), &signing_key);

    let json = certificate.to_json();
    let restored = DeviceRevocationCertificate::from_json(&json).unwrap();

    assert_eq!(certificate.device_id(), restored.device_id());
    assert!(restored.verify(&signing_key.public_key()));
}

/// Scenario: contacts should be notified if necessary
// @scenario: device_management:Lost device revocation
#[test]
fn test_registry_broadcast_message_creation() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

    // Create broadcast message for contacts
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    assert_eq!(broadcast.version(), registry.version());
    assert!(broadcast.verify(&signing_key.public_key()));
}

/// Test registry broadcast includes active device keys
// @scenario: device_management:View list of linked devices
#[test]
fn test_registry_broadcast_contains_active_devices() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
    registry
        .add_device(device1.to_registered(&seed), &signing_key)
        .unwrap();

    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    assert_eq!(broadcast.active_device_count(), 2);
    assert!(broadcast.contains_device(device0.device_id()));
    assert!(broadcast.contains_device(device1.device_id()));
}

/// Test registry broadcast excludes revoked devices
// @scenario: device_management:Unlinked device data wiped
#[test]
fn test_registry_broadcast_excludes_revoked() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "Revoked".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
    registry
        .add_device(device1.to_registered(&seed), &signing_key)
        .unwrap();
    registry
        .revoke_device(device1.device_id(), &signing_key)
        .unwrap();

    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    assert_eq!(broadcast.active_device_count(), 1);
    assert!(broadcast.contains_device(device0.device_id()));
    assert!(!broadcast.contains_device(device1.device_id()));
}

/// Test registry broadcast serialization for transmission
#[test]
fn test_registry_broadcast_serialization() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    let json = broadcast.to_json();
    let restored = RegistryBroadcast::from_json(&json).unwrap();

    assert_eq!(broadcast.version(), restored.version());
    assert!(restored.verify(&signing_key.public_key()));
}

/// Test applying revocation certificate to local knowledge of contact
// @scenario: device_management:Lost device revocation
// @scenario: device_management:Unlink a device remotely
#[test]
fn test_apply_revocation_to_contact_registry() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "ToRevoke".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
    registry
        .add_device(device1.to_registered(&seed), &signing_key)
        .unwrap();

    // Create revocation certificate for device1
    let certificate = DeviceRevocationCertificate::create(
        device1.device_id(),
        "Revoked".to_string(),
        &signing_key,
    );

    // Apply certificate to registry (as if received from contact)
    registry
        .apply_revocation(&certificate, &signing_key.public_key())
        .unwrap();

    assert_eq!(registry.active_count(), 1);
    assert!(!registry
        .find_device(device1.device_id())
        .unwrap()
        .is_active());
}

// =============================================================================
// BROADCAST REPLAY PROTECTION (Tracker #78)
// =============================================================================

/// Tests that verify_with_freshness rejects broadcasts with old versions.
///
/// Feature: device_management.feature
/// Scenario: Replayed broadcast with stale version is rejected
#[test]
fn test_broadcast_rejects_old_version() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Broadcast version is 1 (initial), last accepted is 1 → should reject
    let result = broadcast.verify_with_freshness(
        &signing_key.public_key(),
        1, // last_accepted_version == broadcast.version()
        now,
        3600, // 1 hour max age
    );
    assert!(
        result.is_err(),
        "Should reject broadcast with version <= last accepted"
    );
    assert!(result.unwrap_err().to_string().contains("version"));
}

/// Tests that verify_with_freshness accepts fresh broadcasts.
#[test]
fn test_broadcast_accepts_fresh() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Broadcast version is 1, last accepted is 0 → should accept
    let result = broadcast.verify_with_freshness(
        &signing_key.public_key(),
        0, // last_accepted_version < broadcast.version()
        now,
        3600, // 1 hour max age
    );
    assert!(
        result.is_ok(),
        "Should accept fresh broadcast: {:?}",
        result
    );
}

/// Tests that verify_with_freshness rejects stale timestamps.
#[test]
fn test_broadcast_rejects_stale_timestamp() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    let far_future = broadcast.timestamp() + 7200; // 2 hours later

    // Broadcast timestamp is now, but we pretend now is 2 hours later → stale
    let result = broadcast.verify_with_freshness(
        &signing_key.public_key(),
        0,
        far_future,
        3600, // 1 hour max age → broadcast is 2 hours old
    );
    assert!(result.is_err(), "Should reject stale broadcast");
    assert!(result.unwrap_err().to_string().contains("old"));
}

/// Tests that verify_with_freshness rejects future timestamps.
#[test]
fn test_broadcast_rejects_future_timestamp() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);
    let broadcast = RegistryBroadcast::new(&registry, &signing_key);

    let past = broadcast.timestamp().saturating_sub(120); // 2 minutes before broadcast

    // We pretend now is 2 minutes before the broadcast → future timestamp
    let result = broadcast.verify_with_freshness(&signing_key.public_key(), 0, past, 3600);
    assert!(result.is_err(), "Should reject future broadcast");
    assert!(result.unwrap_err().to_string().contains("future"));
}

// ============================================================
// find_device_by_prefix tests
// ============================================================

// @scenario: device_management:View device details
#[test]
fn test_find_device_by_prefix_matches_hex_start() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let registered0 = device0.to_registered(&seed);
    let expected_hex = registered0.device_id_hex();

    let registry = DeviceRegistry::new(registered0.clone(), &signing_key);

    // Use the first 8 hex characters as prefix
    let prefix = &expected_hex[..8];
    let found = registry.find_device_by_prefix(prefix);
    assert!(found.is_some(), "Should find device by hex prefix");
    assert_eq!(
        found.unwrap().device_id_hex(),
        expected_hex,
        "Found device should match the expected device"
    );
}

#[test]
fn test_find_device_by_prefix_returns_none_for_no_match() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);

    // Use a prefix that won't match any device
    let found = registry.find_device_by_prefix("0000000000000000");
    assert!(
        found.is_none(),
        "Should return None when no device matches prefix"
    );
}

// @scenario: device_management:Unlinked device data wiped
#[test]
fn test_find_device_by_prefix_only_matches_active_devices() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());
    let device1_hex = device1.to_registered(&seed).device_id_hex();

    let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
    registry
        .add_device(device1.to_registered(&seed), &signing_key)
        .unwrap();

    // Revoke device1
    registry
        .revoke_device(
            device1
                .to_registered(&seed)
                .device_id
                .as_ref()
                .try_into()
                .unwrap(),
            &signing_key,
        )
        .unwrap();

    // Searching by device1's prefix should return None (it's revoked)
    let prefix = &device1_hex[..8];
    let found = registry.find_device_by_prefix(prefix);
    assert!(found.is_none(), "Should not find revoked device by prefix");
}

#[test]
fn test_find_device_by_prefix_empty_prefix_returns_first_active() {
    let seed = test_master_seed();
    let signing_key = test_signing_keypair();
    let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());

    let registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);

    // Empty prefix matches everything (starts_with("") is always true)
    let found = registry.find_device_by_prefix("");
    assert!(
        found.is_some(),
        "Empty prefix should match the first active device"
    );
}
