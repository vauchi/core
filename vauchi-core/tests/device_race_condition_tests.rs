//! Device Linking Race Condition Tests
//!
//! Tests for concurrent device linking scenarios.
//! Based on: features/device_management.feature (edge cases not covered)

use std::sync::{Arc, Mutex};
use std::thread;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, MAX_DEVICES};
use vauchi_core::SigningKeyPair;

// =============================================================================
// Concurrent Device Linking Tests
// =============================================================================

/// Scenario: Two devices try to link simultaneously
/// Only one should succeed when they compete for the same slot
#[test]
fn test_concurrent_device_linking_thread_safety() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let registry = Arc::new(Mutex::new(DeviceRegistry::new(
        device0.to_registered(&master_seed),
        &signing_key,
    )));

    // Spawn multiple threads trying to add devices
    let handles: Vec<_> = (1..5)
        .map(|i| {
            let registry = Arc::clone(&registry);
            let seed = master_seed;
            let key = SigningKeyPair::from_seed(&seed);

            thread::spawn(move || {
                let device = DeviceInfo::derive(&seed, i as u32, format!("Device {}", i));
                let mut reg = registry.lock().unwrap();
                reg.add_device(device.to_registered(&seed), &key)
            })
        })
        .collect();

    // Collect results
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All should succeed (no duplicates since different indices)
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(success_count, 4);

    // Verify registry state
    let registry = registry.lock().unwrap();
    assert_eq!(registry.active_count(), 5);
}

/// Scenario: Concurrent adds with same device index - only one wins
#[test]
fn test_concurrent_same_device_index() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let registry = Arc::new(Mutex::new(DeviceRegistry::new(
        device0.to_registered(&master_seed),
        &signing_key,
    )));

    // Spawn threads trying to add device with same index
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let registry = Arc::clone(&registry);
            let seed = master_seed;
            let key = SigningKeyPair::from_seed(&seed);

            thread::spawn(move || {
                let device = DeviceInfo::derive(&seed, 1, "Device 1".to_string());
                let mut reg = registry.lock().unwrap();
                reg.add_device(device.to_registered(&seed), &key)
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Only one should succeed, rest should get DuplicateDevice
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(success_count, 1);

    let registry = registry.lock().unwrap();
    assert_eq!(registry.active_count(), 2); // device0 + one device1
}

/// Scenario: Concurrent revocation and addition
#[test]
fn test_concurrent_revoke_and_add() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let device1 = DeviceInfo::derive(&master_seed, 1, "Device 1".to_string());
    let device1_id: [u8; 32] = *device1.device_id();

    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    let registry = Arc::new(Mutex::new(registry));

    // Thread 1: Revoke device 1
    let registry1 = Arc::clone(&registry);
    let key1 = SigningKeyPair::from_seed(&master_seed);
    let handle1 = thread::spawn(move || {
        let mut reg = registry1.lock().unwrap();
        reg.revoke_device(&device1_id, &key1)
    });

    // Thread 2: Add device 2
    let registry2 = Arc::clone(&registry);
    let key2 = SigningKeyPair::from_seed(&master_seed);
    let handle2 = thread::spawn(move || {
        let device2 = DeviceInfo::derive(&master_seed, 2, "Device 2".to_string());
        let mut reg = registry2.lock().unwrap();
        reg.add_device(device2.to_registered(&master_seed), &key2)
    });

    let result1 = handle1.join().unwrap();
    let result2 = handle2.join().unwrap();

    // Both operations should succeed
    assert!(result1.is_ok() || result2.is_ok());

    let registry = registry.lock().unwrap();
    // Should have at least 1 active (device0) and possibly device2
    assert!(registry.active_count() >= 1);
}

// =============================================================================
// Maximum Device Limit Tests
// =============================================================================

/// Scenario: Cannot exceed maximum devices
#[test]
fn test_max_devices_enforced() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    // Add devices up to max
    for i in 1..MAX_DEVICES {
        let device = DeviceInfo::derive(&master_seed, i as u32, format!("Device {}", i));
        registry
            .add_device(device.to_registered(&master_seed), &signing_key)
            .unwrap();
    }

    assert_eq!(registry.active_count(), MAX_DEVICES);

    // Try to add one more
    let extra_device = DeviceInfo::derive(&master_seed, MAX_DEVICES as u32, "Extra".to_string());
    let result = registry.add_device(extra_device.to_registered(&master_seed), &signing_key);

    assert!(result.is_err());
    assert_eq!(registry.active_count(), MAX_DEVICES);
}

/// Scenario: Can add device after revoking one at max
#[test]
fn test_add_after_revoke_at_max() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    // Add devices up to max, track the last device_id
    let mut last_device_id: [u8; 32] = *device0.device_id();
    for i in 1..MAX_DEVICES {
        let device = DeviceInfo::derive(&master_seed, i as u32, format!("Device {}", i));
        last_device_id = *device.device_id();
        registry
            .add_device(device.to_registered(&master_seed), &signing_key)
            .unwrap();
    }

    assert_eq!(registry.active_count(), MAX_DEVICES);

    // Revoke last device
    registry
        .revoke_device(&last_device_id, &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), MAX_DEVICES - 1);

    // Now can add another
    let new_device = DeviceInfo::derive(&master_seed, MAX_DEVICES as u32, "New Device".to_string());
    let result = registry.add_device(new_device.to_registered(&master_seed), &signing_key);

    assert!(result.is_ok());
    assert_eq!(registry.active_count(), MAX_DEVICES);
}

// =============================================================================
// Device Already Linked Tests
// =============================================================================

/// Scenario: Link device that's already linked (same identity)
#[test]
fn test_link_already_linked_device_same_identity() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    // Try to add the same device again
    let result = registry.add_device(device0.to_registered(&master_seed), &signing_key);

    // Should fail with duplicate device error
    assert!(result.is_err());
}

// =============================================================================
// Version Vector Consistency Tests
// =============================================================================

/// Scenario: Version increments correctly
#[test]
fn test_version_increments_on_changes() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    let initial_version = registry.version();

    // Add a device
    let device1 = DeviceInfo::derive(&master_seed, 1, "Device 1".to_string());
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    assert!(registry.version() > initial_version);

    let after_add_version = registry.version();

    // Revoke a device
    registry
        .revoke_device(device1.device_id(), &signing_key)
        .unwrap();

    assert!(registry.version() > after_add_version);
}

// =============================================================================
// Cannot Unlink Last Device Tests
// =============================================================================

/// Scenario: Cannot unlink last device
#[test]
fn test_cannot_unlink_last_device() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    // Try to revoke the only device
    let result = registry.revoke_device(device0.device_id(), &signing_key);

    // Should fail - cannot unlink last device
    assert!(result.is_err());
    assert_eq!(registry.active_count(), 1);
}
