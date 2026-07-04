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
    Contact::from_exchange(public_key, card, shared_key, 0)
}

// @scenario: device_management :: New device receives full state
// @scenario: sync_updates :: New device receives full state
// @internal
#[test]
fn test_contact_sync_data_roundtrip() {
    let contact = create_test_contact();
    let sync_data = ContactSyncData::from_contact(&contact);
    let restored = sync_data.to_contact().unwrap();

    assert_eq!(restored.id(), contact.id());
    assert_eq!(restored.public_key(), contact.public_key());
    assert_eq!(restored.display_name(), contact.display_name());
}

// @scenario: device_management :: New device receives full state
// @internal
#[test]
fn test_contact_sync_data_serialization() {
    let contact = create_test_contact();
    let sync_data = ContactSyncData::from_contact(&contact);

    let json = serde_json::to_string(&sync_data).unwrap();
    let restored: ContactSyncData = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, sync_data.id);
    assert_eq!(restored.public_key, sync_data.public_key);
}

// @scenario: device_management :: New device receives full state
// @scenario: sync_updates :: New device receives full state
// @internal
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

// @internal
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
// @scenario: device_management :: Changes sync between devices
// @internal
#[test]
fn test_sync_item_card_updated() {
    use vauchi_core::contact_card::{ContactField, FieldType};

    let mut card = ContactCard::new("Alice");
    let _ = card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+1234567890",
        0,
    ));

    let item = SyncItem::CardUpdated {
        field_label: "mobile".to_string(),
        new_value: "+1987654321".to_string(),
        timestamp: 1000,
    };

    assert!(matches!(item, SyncItem::CardUpdated { .. }));

    assert_eq!(item.timestamp(), 1000);
}

/// Scenario: Bidirectional sync
/// "When I add a field on Device A
///  And I add a different field on Device B
///  Then both fields should appear on both devices"
// @scenario: device_management :: Bidirectional sync
// @internal
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
// @scenario: device_management :: Conflict resolution between devices
// @internal
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
    let device_a_id = [0xAA; 32];
    let device_b_id = [0xBB; 32];
    let resolved = SyncItem::resolve_conflict(&item_a, &item_b, &device_a_id, &device_b_id);

    if let SyncItem::CardUpdated { new_value, .. } = resolved {
        assert_eq!(new_value, "b@test.com");
    } else {
        panic!("Expected CardUpdated variant");
    }
}

/// Test SyncItem visibility change
// @scenario: device_management :: Some settings sync across devices
// @internal
#[test]
fn test_sync_item_visibility_changed() {
    let item = SyncItem::VisibilityChanged {
        contact_id: "contact-123".to_string(),
        field_id: "phone".to_string(),
        is_visible: false,
        timestamp: 3000,
    };

    assert!(matches!(item, SyncItem::VisibilityChanged { .. }));
    assert_eq!(item.timestamp(), 3000);
}

/// Test SyncItem contact removed
// @internal
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
// @scenario: device_management :: Changes sync between devices
// @internal
#[test]
fn test_inter_device_sync_state_creation() {
    let device_id = [0x42u8; 32];

    let state = InterDeviceSyncState::new(device_id);

    assert_eq!(state.device_id(), &device_id);
    assert_eq!(state.pending_items().len(), 0);
    assert_eq!(state.last_sync_version(), 0);
}

/// Test adding items to sync queue
// @scenario: device_management :: Offline changes sync when reconnected
// @internal
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
// @internal
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
// @scenario: device_management :: Device registry version tracking
// @internal
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
// @scenario: device_management :: Conflict resolution between devices
// @internal
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
// @scenario: device_management :: Conflict resolution between devices
// @internal
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

// ============================================================
// Task 18: New SyncItem variants — notes and proposal_trusted
// ============================================================

/// Verify PersonalNoteChanged serialises and deserialises correctly.
// @scenario: device_management :: Personal note syncs to linked devices
// @internal
#[test]
fn test_personal_note_sync_item_roundtrip() {
    let item = SyncItem::PersonalNoteChanged {
        contact_id: "c1".into(),
        note: "Met at FOSDEM".into(),
        timestamp: 1000,
    };
    let json = serde_json::to_string(&item).unwrap();
    let restored: SyncItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, restored);
}

/// Verify ContactFieldNoteChanged serialises and deserialises correctly.
// @scenario: device_management :: Contact field note syncs to linked devices
// @internal
#[test]
fn test_contact_field_note_sync_item_roundtrip() {
    let item = SyncItem::ContactFieldNoteChanged {
        contact_id: "c1".into(),
        field_id: "f1".into(),
        note: "His work phone".into(),
        timestamp: 2000,
    };
    let json = serde_json::to_string(&item).unwrap();
    let restored: SyncItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, restored);
}

/// Verify ProposalTrustChanged serialises and deserialises correctly.
// @scenario: device_management :: Proposal trust syncs to linked devices
// @internal
#[test]
fn test_proposal_trust_sync_item_roundtrip() {
    let item = SyncItem::ProposalTrustChanged {
        contact_id: "c1".into(),
        proposal_trusted: true,
        timestamp: 3000,
    };
    let json = serde_json::to_string(&item).unwrap();
    let restored: SyncItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, restored);
}

/// timestamp() accessor works for all three new variants.
// @internal
#[test]
fn test_new_sync_item_timestamps() {
    let personal = SyncItem::PersonalNoteChanged {
        contact_id: "c1".into(),
        note: "note".into(),
        timestamp: 111,
    };
    let field = SyncItem::ContactFieldNoteChanged {
        contact_id: "c1".into(),
        field_id: "f1".into(),
        note: "note".into(),
        timestamp: 222,
    };
    let trust = SyncItem::ProposalTrustChanged {
        contact_id: "c1".into(),
        proposal_trusted: false,
        timestamp: 333,
    };
    assert_eq!(personal.timestamp(), 111);
    assert_eq!(field.timestamp(), 222);
    assert_eq!(trust.timestamp(), 333);
}

// ============================================================
// A4: DeviceSyncPayload::new() must not panic on mixed contacts
// ============================================================

fn create_imported_test_contact() -> Contact {
    let card = ContactCard::new("Imported José");
    Contact::from_import(card, vauchi_core::contact::ImportSource::VcardFile, None, 0)
}

/// DeviceSyncPayload::new() with a mix of exchanged and imported contacts
/// must not panic and must separate them into the correct fields.
// @internal
#[test]
fn test_device_sync_payload_new_mixed_contacts_no_panic() {
    let exchanged = create_test_contact();
    let imported = create_imported_test_contact();

    let own_card = ContactCard::new("Owner");
    let payload = DeviceSyncPayload::new(&[exchanged, imported], &own_card, 5);

    assert_eq!(payload.contacts.len(), 1, "only exchanged contacts");
    assert_eq!(payload.imported_contacts.len(), 1, "only imported contacts");
    assert_eq!(payload.contact_count(), 2, "total count");
    assert_eq!(payload.version, 5);
}

/// DeviceSyncPayload with imported_contacts serializes and deserializes
/// correctly, including backward-compat (missing field defaults to empty vec).
// @internal
#[test]
fn test_device_sync_payload_imported_contacts_roundtrip() {
    let exchanged = create_test_contact();
    let imported = create_imported_test_contact();
    let own_card = ContactCard::new("Owner");

    let payload = DeviceSyncPayload::new(&[exchanged, imported], &own_card, 3);
    let json = payload.to_json();
    let restored = DeviceSyncPayload::from_json(&json).unwrap();

    assert_eq!(restored.contacts.len(), 1);
    assert_eq!(restored.imported_contacts.len(), 1);
    assert_eq!(restored.imported_contacts[0].display_name, "Imported José");
}

/// Old payloads without imported_contacts field deserialize with empty vec.
// @internal
#[test]
fn test_device_sync_payload_backward_compat_no_imported_field() {
    // Simulate a payload from before the imported_contacts field was added
    let json = r#"{"contacts":[],"own_card_json":"{}","version":1}"#;
    let payload = DeviceSyncPayload::from_json(json).unwrap();

    assert_eq!(payload.imported_contacts.len(), 0);
    assert_eq!(payload.contact_count(), 0);
}

// @scenario: device_management :: New device receives full state
// @internal
// CC-14 adversarial: peer-supplied sync data is a trust boundary; an all-zeros
// shared_key (the only DegenerateKey case rejected by SymmetricKey::try_from_bytes)
// must be rejected at ingestion. encryption.rs:95-100 explicitly directs trust-
// boundary callers to use try_from_bytes, not from_bytes_unchecked.
#[test]
fn test_contact_sync_data_to_contact_rejects_degenerate_shared_key() {
    let contact = create_test_contact();
    let mut sync_data = ContactSyncData::from_contact(&contact);
    sync_data.shared_key = [0u8; 32];

    let result = sync_data.to_contact();
    assert!(
        matches!(result, Err(DeviceSyncError::Deserialization(_))),
        "expected Deserialization error for all-zeros shared_key, got {:?}",
        result
    );
}
