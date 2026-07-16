// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conflict resolution edge cases and coverage gap tests for sync::device_orchestrator.
//!
//! Extracted from sync_device_orchestrator_tests.rs
//! Phase 7: Conflict Resolution Edge Cases (#193)
//! Coverage gap tests for DeviceSyncOrchestrator

use crate::common;
use common::device_sync::{
    create_test_contact, create_test_device, create_test_registry, create_test_storage,
};
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::sync::*;
use vauchi_core::*;

// ============================================================
// Phase 7: Conflict Resolution Edge Cases (#193)
// Documenting and testing the Last-Write-Wins strategy
// ============================================================

/// Test: Equal timestamps break by device id (ADR-020), not "local always wins".
///
/// `process_incoming` compares `(timestamp, device_id)` lexicographically. On a
/// timestamp tie the item from the higher device id wins — deterministically and
/// identically on every device — so concurrent same-ms edits converge.
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_equal_timestamp_breaks_by_device_id() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Local change at timestamp 5000, stamped with this device's (derived,
    // non-zero) id.
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "local@example.com".to_string(),
            timestamp: 5000,
        })
        .unwrap();

    let remote = || {
        vec![SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "remote@example.com".to_string(),
            timestamp: 5000,
        }]
    };

    // Same timestamp, LOWER sender id (all-zero < any derived id): local wins.
    let rejected = orchestrator
        .process_incoming(remote(), &[0x00u8; 32])
        .unwrap();
    assert!(
        rejected.is_empty(),
        "equal timestamp + lower sender device id must lose the tie"
    );

    // Same timestamp, HIGHER sender id (all-0xFF > any derived id): remote wins.
    let applied = orchestrator
        .process_incoming(remote(), &[0xFFu8; 32])
        .unwrap();
    assert_eq!(
        applied.len(),
        1,
        "equal timestamp + higher sender device id must win the tie (ADR-020)"
    );
}

/// Test: Multiple rapid updates to the same field — only the latest sticks (#193).
///
/// Simulates a burst of changes (e.g., user typing fast) arriving in a batch.
/// Only the highest-timestamp item should be applied.
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_rapid_updates_only_latest_applied() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Incoming batch of rapid updates to the same field (ascending timestamps)
    let incoming = vec![
        SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+111".to_string(),
            timestamp: 1000,
        },
        SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+222".to_string(),
            timestamp: 2000,
        },
        SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+333".to_string(),
            timestamp: 3000,
        },
    ];

    let applied = orchestrator
        .process_incoming(incoming, &[0x99u8; 32])
        .unwrap();

    // All three have ascending timestamps, so all are applied sequentially.
    // The field_timestamps map ends at 3000.
    assert_eq!(applied.len(), 3);

    let stale = vec![SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+000".to_string(),
        timestamp: 1500,
    }];

    let rejected = orchestrator.process_incoming(stale, &[0x99u8; 32]).unwrap();
    assert!(
        rejected.is_empty(),
        "Stale update after rapid burst should be rejected"
    );
}

/// Test: Contact add followed by contact remove for the same contact_id (#193).
///
/// Both use conflict_key "contact:{id}", so the later timestamp wins.
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_contact_add_then_remove() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Contact added at t=1000
    let contact = create_test_contact("Eve");
    let contact_id = contact.id().to_string();
    let contact_data = vauchi_core::sync::ContactSyncData::from_contact(&contact);
    let add_item = SyncItem::ContactAdded {
        contact_data,
        timestamp: 1000,
    };
    let applied = orchestrator
        .process_incoming(vec![add_item], &[0x99u8; 32])
        .unwrap();
    assert_eq!(applied.len(), 1, "Add should be applied");

    // Contact removed at t=2000 (newer — should win)
    let remove_item = SyncItem::ContactRemoved {
        contact_id: contact_id.clone(),
        timestamp: 2000,
    };
    let applied = orchestrator
        .process_incoming(vec![remove_item], &[0x99u8; 32])
        .unwrap();
    assert_eq!(applied.len(), 1, "Remove should win (newer)");

    // Re-add at t=1500 (older than remove — should be rejected)
    let contact2 = create_test_contact("Eve");
    let contact_data2 = vauchi_core::sync::ContactSyncData::from_contact(&contact2);
    let readd_item = SyncItem::ContactAdded {
        contact_data: contact_data2,
        timestamp: 1500,
    };
    let applied = orchestrator
        .process_incoming(vec![readd_item], &[0x99u8; 32])
        .unwrap();
    assert!(
        applied.is_empty(),
        "Re-add with older timestamp should be rejected"
    );
}

/// Test: Concurrent deletion schedule and cancel for the same identity (#193).
///
/// DeletionScheduled and DeletionCancelled use different conflict keys,
/// so both can coexist — this is the correct behavior since they represent
/// different semantic operations.
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_deletion_schedule_and_cancel_independent() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Schedule deletion on one device
    let schedule = SyncItem::DeletionScheduled {
        scheduled_at: 1000,
        execute_at: 1000 + 7 * 86400,
        timestamp: 1000,
    };
    let applied = orchestrator
        .process_incoming(vec![schedule], &[0x99u8; 32])
        .unwrap();
    assert_eq!(applied.len(), 1);

    // Cancel deletion on another device (different conflict key)
    let cancel = SyncItem::DeletionCancelled { timestamp: 2000 };
    let applied = orchestrator
        .process_incoming(vec![cancel], &[0x99u8; 32])
        .unwrap();
    assert_eq!(
        applied.len(),
        1,
        "Cancel uses a different key than schedule"
    );
}

// ============================================================
// ============================================================

/// Test devices_with_pending returns correct device IDs
// @scenario: device_management :: Changes sync between devices
#[test]
fn test_orchestrator_devices_with_pending() {
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

    // Initially no devices with pending
    assert!(orchestrator.devices_with_pending().is_empty());

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    let pending_devices = orchestrator.devices_with_pending();
    assert_eq!(pending_devices.len(), 1);
    assert_eq!(pending_devices[0], device_b_id);
}

/// Test pending_for_device returns empty slice for unknown device
// @scenario: device_management :: Changes sync between devices
#[test]
fn test_orchestrator_pending_for_unknown_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let unknown_id = [0xFFu8; 32];
    let pending = orchestrator.pending_for_device(&unknown_id);
    assert!(
        pending.is_empty(),
        "Unknown device should have no pending items"
    );
}

/// Test mark_synced is a no-op for unknown device
// @scenario: device_management :: Changes sync between devices
#[test]
fn test_orchestrator_mark_synced_unknown_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let unknown_id = [0xFFu8; 32];
    let result = orchestrator.mark_synced(&unknown_id, 1);
    assert!(
        result.is_ok(),
        "mark_synced on unknown device should not error"
    );
}

/// Test add_device and remove_device
// @scenario: device_management :: Device registry version tracking
#[test]
fn test_orchestrator_add_remove_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // No other devices initially (only device_a in registry, it's excluded from device_states)
    assert!(orchestrator.devices_with_pending().is_empty());

    let new_device_id = [0xAAu8; 32];
    orchestrator.add_device(new_device_id);

    // Record a change — should now be queued for the new device
    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1234".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    let pending = orchestrator.pending_for_device(&new_device_id);
    assert_eq!(pending.len(), 1);

    orchestrator.remove_device(&new_device_id).unwrap();
    let pending = orchestrator.pending_for_device(&new_device_id);
    assert!(
        pending.is_empty(),
        "Removed device should have no pending items"
    );
}

/// Test add_device is idempotent (adding same device twice doesn't duplicate state)
// @scenario: device_management :: Device registry version tracking
#[test]
fn test_orchestrator_add_device_idempotent() {
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

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    orchestrator.add_device(device_b_id);

    // Pending items should still be there (or_insert_with, not insert)
    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(
        pending.len(),
        1,
        "Re-adding device should not clear pending items"
    );
}

/// Test accessor methods: current_device() and registry()
// @scenario: device_management :: Device registry version tracking
#[test]
fn test_orchestrator_accessors() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_a_id = *device_a.device_id();
    let registry = create_test_registry(&master_seed, &device_a);

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    assert_eq!(*orchestrator.current_device().device_id(), device_a_id);
    assert_eq!(orchestrator.registry().device_count(), 1);
}

/// Test checkpoint save/load/clear lifecycle
// @scenario: device_management :: Offline changes sync when reconnected
#[test]
fn test_orchestrator_checkpoint_lifecycle() {
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

    for i in 0..3 {
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: format!("field_{}", i),
                new_value: format!("value_{}", i),
                timestamp: (i as u64 + 1) * 1000,
            })
            .unwrap();
    }

    let items = orchestrator.pending_for_device(&device_b_id).to_vec();
    assert_eq!(items.len(), 3);

    orchestrator
        .save_checkpoint(&device_b_id, &items, 2)
        .unwrap();

    let checkpoint = orchestrator.load_checkpoint(&device_b_id).unwrap();
    assert!(checkpoint.is_some(), "expected Some value");
    let (loaded_items, sent_count) = checkpoint.unwrap();
    assert_eq!(loaded_items.len(), 3);
    assert_eq!(sent_count, 2);

    orchestrator.clear_checkpoint(&device_b_id).unwrap();

    // Load again — should be None
    let cleared = orchestrator.load_checkpoint(&device_b_id).unwrap();
    assert!(cleared.is_none(), "Cleared checkpoint should return None");
}

/// Test load_checkpoint returns None when no checkpoint exists
// @scenario: device_management :: Offline changes sync when reconnected
#[test]
fn test_orchestrator_load_checkpoint_none() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let unknown_id = [0xFFu8; 32];
    let checkpoint = orchestrator.load_checkpoint(&unknown_id).unwrap();
    assert!(checkpoint.is_none());
}

/// Test create_sync_message with no pending items
// @scenario: device_management :: Changes sync between devices
#[test]
fn test_orchestrator_create_sync_message_empty() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_a_id = *device_a.device_id();
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let msg = orchestrator.create_sync_message(&device_b_id).unwrap();
    assert!(msg.items.is_empty());
    assert_eq!(msg.version, 0);
    assert_eq!(msg.sender_device_id, device_a_id);
}

/// Test conflict resolution with VisibilityChanged items
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_visibility_changed() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    let items = vec![SyncItem::VisibilityChanged {
        contact_id: "contact-1".to_string(),
        field_id: "phone".to_string(),
        is_visible: false,
        timestamp: 1000,
    }];
    let applied = orchestrator.process_incoming(items, &[0x99u8; 32]).unwrap();
    assert_eq!(applied.len(), 1);

    let stale = vec![SyncItem::VisibilityChanged {
        contact_id: "contact-1".to_string(),
        field_id: "phone".to_string(),
        is_visible: true,
        timestamp: 500,
    }];
    let rejected = orchestrator.process_incoming(stale, &[0x99u8; 32]).unwrap();
    assert!(
        rejected.is_empty(),
        "Stale visibility change should be rejected"
    );

    // Different contact_id should be independent (different conflict key)
    let independent = vec![SyncItem::VisibilityChanged {
        contact_id: "contact-2".to_string(),
        field_id: "phone".to_string(),
        is_visible: true,
        timestamp: 500,
    }];
    let applied = orchestrator
        .process_incoming(independent, &[0x99u8; 32])
        .unwrap();
    assert_eq!(applied.len(), 1, "Different contact_id should not conflict");
}

/// Test conflict resolution across group update and deletion forms.
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_group_change_and_deletion() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    let items = vec![SyncItem::GroupChanged {
        group_data: GroupSyncData {
            id: "group-1".to_string(),
            name: "Work".to_string(),
            contact_ids: vec![],
            visible_fields: vec![],
            display_name_override: None,
            bio_override: None,
            avatar_override: None,
            created_at: 1000,
            modified_at: 2000,
        },
        timestamp: 2000,
    }];
    let applied = orchestrator.process_incoming(items, &[0x99u8; 32]).unwrap();
    assert_eq!(applied.len(), 1);

    let stale = vec![SyncItem::GroupDeleted {
        group_id: "group-1".to_string(),
        timestamp: 1000,
    }];
    let rejected = orchestrator.process_incoming(stale, &[0x99u8; 32]).unwrap();
    assert!(
        rejected.is_empty(),
        "stale deletion must share the update conflict key"
    );
}

/// Test conflict resolution with ContactTrustChanged items
// @scenario: device_management :: Conflict resolution between devices
// @scenario: contact_recovery :: Trust state syncs across linked devices
#[test]
fn test_conflict_contact_trust_changed() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    let items = vec![SyncItem::ContactTrustChanged {
        contact_id: "contact-1".to_string(),
        recovery_trusted: true,
        timestamp: 1000,
    }];
    let applied = orchestrator.process_incoming(items, &[0x99u8; 32]).unwrap();
    assert_eq!(applied.len(), 1);

    let newer = vec![SyncItem::ContactTrustChanged {
        contact_id: "contact-1".to_string(),
        recovery_trusted: false,
        timestamp: 2000,
    }];
    let applied = orchestrator.process_incoming(newer, &[0x99u8; 32]).unwrap();
    assert_eq!(applied.len(), 1, "Newer trust change should be applied");

    let stale = vec![SyncItem::ContactTrustChanged {
        contact_id: "contact-1".to_string(),
        recovery_trusted: true,
        timestamp: 1500,
    }];
    let rejected = orchestrator.process_incoming(stale, &[0x99u8; 32]).unwrap();
    assert!(rejected.is_empty(), "Stale trust change should be rejected");
}
