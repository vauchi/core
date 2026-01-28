// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync::device_sync
//! Extracted from device_sync.rs

use vauchi_core::contact_card::ContactCard;
use vauchi_core::sync::*;
use vauchi_core::*;

fn create_test_contact() -> Contact {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Alice");
    let shared_key = SymmetricKey::from_bytes([0x55u8; 32]);
    Contact::from_exchange(public_key, card, shared_key)
}

#[test]
fn test_contact_sync_data_roundtrip() {
    let contact = create_test_contact();
    let sync_data = ContactSyncData::from_contact(&contact);
    let restored = sync_data.to_contact().unwrap();

    assert_eq!(restored.id(), contact.id());
    assert_eq!(restored.public_key(), contact.public_key());
    assert_eq!(restored.display_name(), contact.display_name());
}

#[test]
fn test_contact_sync_data_serialization() {
    let contact = create_test_contact();
    let sync_data = ContactSyncData::from_contact(&contact);

    let json = serde_json::to_string(&sync_data).unwrap();
    let restored: ContactSyncData = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, sync_data.id);
    assert_eq!(restored.public_key, sync_data.public_key);
}

#[test]
fn test_device_sync_payload_roundtrip() {
    let contact1 = create_test_contact();
    let own_card = ContactCard::new("Bob");

    let payload = DeviceSyncPayload::new(&[contact1], &own_card, 1);

    let json = payload.to_json();
    let restored = DeviceSyncPayload::from_json(&json).unwrap();

    assert_eq!(restored.contact_count(), 1);
    assert_eq!(restored.version, 1);
}

#[test]
fn test_device_sync_payload_empty() {
    let payload = DeviceSyncPayload::empty();
    assert_eq!(payload.contact_count(), 0);
    assert_eq!(payload.version, 0);
}

// ============================================================
// Phase 4 Tests: Inter-Device Sync
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Changes sync between devices
/// "When I update my phone number on Device A
///  Then Device B should receive the update"
#[test]
fn test_sync_item_card_updated() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    let mut card = ContactCard::new("Alice");
    let _ = card.add_field(ContactField::new(FieldType::Phone, "mobile", "+1234567890"));

    // Create a SyncItem representing a card field update
    let item = SyncItem::CardUpdated {
        field_label: "mobile".to_string(),
        new_value: "+1987654321".to_string(),
        timestamp: 1000,
    };

    assert!(matches!(item, SyncItem::CardUpdated { .. }));

    // Verify timestamp is accessible for conflict resolution
    assert_eq!(item.timestamp(), 1000);
}

/// Scenario: Bidirectional sync
/// "When I add a field on Device A
///  And I add a different field on Device B
///  Then both fields should appear on both devices"
#[test]
fn test_sync_item_contact_added() {
    let contact = create_test_contact();
    let sync_data = ContactSyncData::from_contact(&contact);

    let item = SyncItem::ContactAdded {
        contact_data: sync_data,
        timestamp: 2000,
    };

    assert!(matches!(item, SyncItem::ContactAdded { .. }));
    assert_eq!(item.timestamp(), 2000);
}

/// Scenario: Conflict resolution between devices
/// "When I update my email to 'a@test.com' on Device A
///  And I update my email to 'b@test.com' on Device B
///  And both come online
///  Then the later change should win"
#[test]
fn test_conflict_resolution_last_write_wins() {
    // Device A update at timestamp 1000
    let item_a = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "a@test.com".to_string(),
        timestamp: 1000,
    };

    // Device B update at timestamp 2000 (later)
    let item_b = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "b@test.com".to_string(),
        timestamp: 2000,
    };

    // Resolve conflict - later timestamp wins
    let resolved = SyncItem::resolve_conflict(&item_a, &item_b);

    // Device B's change should win
    if let SyncItem::CardUpdated { new_value, .. } = resolved {
        assert_eq!(new_value, "b@test.com");
    } else {
        panic!("Expected CardUpdated variant");
    }
}

/// Test SyncItem visibility change
#[test]
fn test_sync_item_visibility_changed() {
    let item = SyncItem::VisibilityChanged {
        contact_id: "contact-123".to_string(),
        field_label: "phone".to_string(),
        is_visible: false,
        timestamp: 3000,
    };

    assert!(matches!(item, SyncItem::VisibilityChanged { .. }));
    assert_eq!(item.timestamp(), 3000);
}

/// Test SyncItem contact removed
#[test]
fn test_sync_item_contact_removed() {
    let item = SyncItem::ContactRemoved {
        contact_id: "contact-456".to_string(),
        timestamp: 4000,
    };

    assert!(matches!(item, SyncItem::ContactRemoved { .. }));
    assert_eq!(item.timestamp(), 4000);
}

/// Test InterDeviceSyncState for tracking sync with other own devices
#[test]
fn test_inter_device_sync_state_creation() {
    let device_id = [0x42u8; 32];

    let state = InterDeviceSyncState::new(device_id);

    assert_eq!(state.device_id(), &device_id);
    assert_eq!(state.pending_items().len(), 0);
    assert_eq!(state.last_sync_version(), 0);
}

/// Test adding items to sync queue
#[test]
fn test_inter_device_sync_state_queue_item() {
    let device_id = [0x42u8; 32];
    let mut state = InterDeviceSyncState::new(device_id);

    let item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "test@example.com".to_string(),
        timestamp: 1000,
    };

    state.queue_item(item);

    assert_eq!(state.pending_items().len(), 1);
}

/// Test serialization of SyncItem for transmission
#[test]
fn test_sync_item_serialization() {
    let item = SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 5000,
    };

    let json = item.to_json();
    let restored = SyncItem::from_json(&json).unwrap();

    assert_eq!(item.timestamp(), restored.timestamp());
}

/// Test version vector for causality tracking
#[test]
fn test_version_vector_increment() {
    let device_id = [0x42u8; 32];
    let mut version_vector = VersionVector::new();

    version_vector.increment(&device_id);
    assert_eq!(version_vector.get(&device_id), 1);

    version_vector.increment(&device_id);
    assert_eq!(version_vector.get(&device_id), 2);
}

/// Test version vector merge for conflict detection
#[test]
fn test_version_vector_merge() {
    let device_a = [0x41u8; 32];
    let device_b = [0x42u8; 32];

    let mut vv_a = VersionVector::new();
    vv_a.increment(&device_a);
    vv_a.increment(&device_a);

    let mut vv_b = VersionVector::new();
    vv_b.increment(&device_b);
    vv_b.increment(&device_b);
    vv_b.increment(&device_b);

    let merged = VersionVector::merge(&vv_a, &vv_b);

    assert_eq!(merged.get(&device_a), 2);
    assert_eq!(merged.get(&device_b), 3);
}

/// Test version vector comparison for conflict detection
#[test]
fn test_version_vector_concurrent_detection() {
    let device_a = [0x41u8; 32];
    let device_b = [0x42u8; 32];

    let mut vv_a = VersionVector::new();
    vv_a.increment(&device_a);

    let mut vv_b = VersionVector::new();
    vv_b.increment(&device_b);

    // Neither dominates the other - they are concurrent
    assert!(vv_a.is_concurrent_with(&vv_b));
}
