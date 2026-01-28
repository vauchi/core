// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync::device_orchestrator
//! Extracted from device_orchestrator.rs

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::*;
use vauchi_core::*;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
    DeviceInfo::derive(master_seed, index, name.to_string())
}

fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
    let signing_key = SigningKeyPair::from_seed(master_seed);
    DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
}

fn create_test_contact(name: &str) -> Contact {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

// ============================================================
// Phase 3: Device Sync Orchestrator Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Changes sync between devices
/// "When I update my phone number on Device A
///  Then Device B should receive the update"
#[test]
fn test_orchestrator_record_local_change() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Create two devices
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");

    // Create registry with both devices
    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Create orchestrator on Device A
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Record a local change
    let item = SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 1000,
    };
    orchestrator.record_local_change(item).unwrap();

    // Verify the change is queued for Device B
    let pending = orchestrator.pending_for_device(device_b.device_id());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].timestamp(), 1000);
}

/// Test that pending items returns correct results
#[test]
fn test_orchestrator_pending_for_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Initially no pending items
    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 0);

    // Add some changes
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+999".to_string(),
            timestamp: 2000,
        })
        .unwrap();

    // Now should have 2 pending items
    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 2);
}

/// Scenario: New device receives full state
/// "When Device B is newly linked
///  Then Device B should receive my complete contact card
///  And Device B should receive all my contacts"
#[test]
fn test_orchestrator_create_full_sync_payload() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    // Add some contacts and own card to storage
    let mut own_card = ContactCard::new("Alice");
    let _ = own_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));
    storage.save_own_card(&own_card).unwrap();

    let contact = create_test_contact("Bob");
    storage.save_contact(&contact).unwrap();

    // Create orchestrator
    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Create full sync payload
    let payload = orchestrator.create_full_sync_payload().unwrap();

    assert_eq!(payload.contact_count(), 1);
    assert!(!payload.own_card_json.is_empty());
}

/// Scenario: New device applies received state
#[test]
fn test_orchestrator_apply_full_sync() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    // Create orchestrator for new device (Device B)
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Create a sync payload (simulating what Device A would send)
    let own_card = ContactCard::new("Alice");
    let contact = create_test_contact("Bob");
    let payload = DeviceSyncPayload::new(&[contact], &own_card, 1);

    // Apply the sync payload
    orchestrator.apply_full_sync(payload).unwrap();

    // Verify own card was saved
    let loaded_card = storage.load_own_card().unwrap().unwrap();
    assert_eq!(loaded_card.display_name(), "Alice");

    // Verify contact was saved
    let contacts = storage.list_contacts().unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].display_name(), "Bob");
}

/// Test marking items as synced clears pending queue
#[test]
fn test_orchestrator_mark_synced() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Add a change
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 1);

    // Mark as synced
    orchestrator.mark_synced(&device_b_id, 1).unwrap();

    // Now should be empty
    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 0);
}

/// Test version vector is incremented on local changes
#[test]
fn test_orchestrator_version_vector_increment() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_a_id = *device_a.device_id();
    let registry = create_test_registry(&master_seed, &device_a);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Initially version is 0
    assert_eq!(orchestrator.version_vector().get(&device_a_id), 0);

    // Record a change
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    // Version should be incremented
    assert_eq!(orchestrator.version_vector().get(&device_a_id), 1);
}

/// Test loading state from storage
#[test]
fn test_orchestrator_load_persisted_state() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Get device_b_id first before consuming device_b
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    // Create orchestrator and add some changes
    {
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "test@example.com".to_string(),
                timestamp: 1000,
            })
            .unwrap();
    }

    // Create new instances for loading
    let device_a2 = create_test_device(&master_seed, 0, "Device A");
    let device_b2 = create_test_device(&master_seed, 1, "Device B");
    let mut registry2 = create_test_registry(&master_seed, &device_a2);
    registry2
        .add_device(device_b2.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Load state from storage
    let orchestrator = DeviceSyncOrchestrator::load(&storage, device_a2, registry2).unwrap();

    // Should still have the pending item
    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 1);
}

// ============================================================
// Phase 4: Encryption Layer Tests (TDD)
// Device-to-device encryption using ECDH + AES-GCM
// ============================================================

/// Test encrypting data for another device
/// Uses ECDH: our_secret * their_public -> shared_secret
/// Then HKDF to derive encryption key
#[test]
fn test_encrypt_for_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_public_key = *device_b.exchange_public_key();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Encrypt some data for device B
    let plaintext = b"Hello from Device A!";
    let ciphertext = orchestrator
        .encrypt_for_device(&device_b_public_key, plaintext)
        .unwrap();

    // Ciphertext should be different from plaintext
    assert_ne!(ciphertext, plaintext);
    // Ciphertext should be longer (includes nonce + tag)
    assert!(ciphertext.len() > plaintext.len());
}

/// Test decrypting data from another device
#[test]
fn test_decrypt_from_device() {
    let storage_a = create_test_storage();
    let storage_b = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Create both devices
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_a_public_key = *device_a.exchange_public_key();
    let device_b_public_key = *device_b.exchange_public_key();

    // Registry for device A
    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Registry for device B
    let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
    let device_b_for_b = create_test_device(&master_seed, 1, "Device B");
    let mut registry_b = create_test_registry(&master_seed, &device_b_for_b);
    registry_b
        .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b_for_b, registry_b);

    // Device A encrypts for Device B
    let plaintext = b"Secret message from A to B";
    let ciphertext = orchestrator_a
        .encrypt_for_device(&device_b_public_key, plaintext)
        .unwrap();

    // Device B decrypts from Device A
    let decrypted = orchestrator_b
        .decrypt_from_device(&device_a_public_key, &ciphertext)
        .unwrap();

    assert_eq!(decrypted, plaintext);
}

/// Test that wrong device cannot decrypt
#[test]
fn test_wrong_device_cannot_decrypt() {
    let storage_a = create_test_storage();
    let storage_c = create_test_storage();
    let master_seed = [0x42u8; 32];
    let different_seed = [0x99u8; 32]; // Different identity
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let _signing_key_c = SigningKeyPair::from_seed(&different_seed);

    // Create devices A and B (same identity)
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_public_key = *device_b.exchange_public_key();

    // Create device C (different identity - attacker)
    let device_c = create_test_device(&different_seed, 0, "Device C");

    // Registry for device A
    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Registry for device C (pretending it has A in registry)
    let registry_c = create_test_registry(&different_seed, &device_c);

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let orchestrator_c = DeviceSyncOrchestrator::new(&storage_c, device_c, registry_c);

    // Device A encrypts for Device B
    let plaintext = b"Secret message for B only";
    let ciphertext = orchestrator_a
        .encrypt_for_device(&device_b_public_key, plaintext)
        .unwrap();

    // Device C (attacker) tries to decrypt - should fail
    // Even if C knows A's public key, C doesn't have B's secret key
    let device_a_public_key =
        *create_test_device(&master_seed, 0, "Device A").exchange_public_key();
    let result = orchestrator_c.decrypt_from_device(&device_a_public_key, &ciphertext);

    assert!(result.is_err());
}

// ============================================================
// Phase 5: Conflict Resolution Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Conflict resolution between devices
/// "Given I have made conflicting changes on Device A and Device B
///  Then the most recent change should win"
#[test]
fn test_conflict_resolution_last_write_wins() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Device B has a local change with timestamp 1000
    let local_item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "local@example.com".to_string(),
        timestamp: 1000,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Incoming change from Device A with timestamp 2000 (newer)
    let incoming_items = vec![SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "remote@example.com".to_string(),
        timestamp: 2000,
    }];

    // Process incoming items
    let applied = orchestrator.process_incoming(incoming_items).unwrap();

    // The newer remote change should be applied
    assert_eq!(applied.len(), 1);
    match &applied[0] {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(new_value, "remote@example.com");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

/// Test that older incoming changes are rejected
#[test]
fn test_conflict_resolution_rejects_older() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Device B has a local change with timestamp 2000
    let local_item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "newer_local@example.com".to_string(),
        timestamp: 2000,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Incoming change from Device A with timestamp 1000 (older)
    let incoming_items = vec![SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "older_remote@example.com".to_string(),
        timestamp: 1000,
    }];

    // Process incoming items
    let applied = orchestrator.process_incoming(incoming_items).unwrap();

    // The older remote change should be rejected (empty applied list)
    assert!(applied.is_empty());
}

/// Scenario: Bidirectional sync
/// "When I add a phone number on Device A
///  And I add an email on Device B
///  Then both devices should have both fields"
#[test]
fn test_concurrent_updates_different_fields_both_preserved() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Device B adds email locally
    let local_item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "b@example.com".to_string(),
        timestamp: 1000,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Device A added phone at roughly the same time
    let incoming_items = vec![SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 1001,
    }];

    // Process incoming - different fields, no conflict
    let applied = orchestrator.process_incoming(incoming_items).unwrap();

    // The phone update should be applied (different field)
    assert_eq!(applied.len(), 1);
    match &applied[0] {
        SyncItem::CardUpdated {
            field_label,
            new_value,
            ..
        } => {
            assert_eq!(field_label, "phone");
            assert_eq!(new_value, "+1234567890");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

// ============================================================
// Phase 6: Bidirectional Sync Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Bidirectional sync with merge
/// Both devices add different fields; both should end up with both
#[test]
fn test_bidirectional_field_additions() {
    let storage_a = create_test_storage();
    let storage_b = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    // Set up Device A
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b_for_a = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b_for_a.device_id();
    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b_for_a.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Set up Device B
    let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_a_id = *device_a_for_b.device_id();
    let mut registry_b = create_test_registry(&master_seed, &device_b);
    registry_b
        .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);

    // Device A adds phone
    orchestrator_a
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1111111111".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    // Device B adds email
    orchestrator_b
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "user@example.com".to_string(),
            timestamp: 1001,
        })
        .unwrap();

    // Exchange pending items
    let a_to_b = orchestrator_a.pending_for_device(&device_b_id).to_vec();
    let b_to_a = orchestrator_b.pending_for_device(&device_a_id).to_vec();

    // Apply on each side
    let applied_on_b = orchestrator_b.process_incoming(a_to_b).unwrap();
    let applied_on_a = orchestrator_a.process_incoming(b_to_a).unwrap();

    // Both should have applied the other's changes (different fields, no conflict)
    assert_eq!(applied_on_b.len(), 1); // phone from A
    assert_eq!(applied_on_a.len(), 1); // email from B
}

/// Scenario: Offline changes are queued
/// Changes made while offline should be stored for later sync
#[test]
fn test_offline_changes_queue() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Make multiple offline changes
    for i in 1..=5 {
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: format!("field_{}", i),
                new_value: format!("value_{}", i),
                timestamp: i * 1000,
            })
            .unwrap();
    }

    // All changes should be queued for Device B
    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(pending.len(), 5);
}

/// Scenario: Offline changes sync when reconnected
/// "Given Device B is offline
///  When Device B makes changes offline
///  And Device B reconnects
///  Then those changes should sync to Device A"
#[test]
fn test_offline_changes_sync_on_reconnect() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Create orchestrator
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Make offline changes
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "offline_field".to_string(),
            new_value: "offline_value".to_string(),
            timestamp: 5000,
        })
        .unwrap();

    // Verify the change is in pending queue
    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(pending.len(), 1);

    // Create sync message for reconnection
    let sync_message = orchestrator.create_sync_message(&device_b_id).unwrap();

    // Verify sync message contains the pending items
    assert!(!sync_message.items.is_empty());
    assert_eq!(sync_message.items.len(), 1);
}
