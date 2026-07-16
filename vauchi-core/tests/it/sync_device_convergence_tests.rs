// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-device convergence tests for sync::device_orchestrator
//! (Phases 5-6): last-write-wins conflict resolution, bidirectional and
//! offline sync, and registry propagation. Orchestrator basics and the
//! device encryption layer live in `sync_device_orchestrator_tests`;
//! conflict edge cases live in `sync_device_conflict_tests`.

use crate::common;
use common::device_sync::{
    create_test_contact, create_test_device, create_test_registry, create_test_storage,
};
use vauchi_core::crypto::{DoubleRatchetState, SigningKeyPair, SymmetricKey, X3DHKeyPair};
use vauchi_core::identity::DeviceInfo;
use vauchi_core::sync::*;
use vauchi_core::*;

// ============================================================
// Phase 5: Conflict Resolution Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Conflict resolution between devices
/// "Given I have made conflicting changes on Device A and Device B
///  Then the most recent change should win"
// @scenario: device_management :: Conflict resolution between devices
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

    let applied = orchestrator
        .process_incoming(incoming_items, &[0x99u8; 32])
        .unwrap();

    assert_eq!(applied.len(), 1);
    match &applied[0] {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(new_value, "remote@example.com");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

/// Test that older incoming changes are rejected
// @scenario: device_management :: Conflict resolution between devices
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

    let applied = orchestrator
        .process_incoming(incoming_items, &[0x99u8; 32])
        .unwrap();

    // The older remote change should be rejected (empty applied list)
    assert!(applied.is_empty());
}

/// A field update and removal share one LWW conflict key (ADR-020).
// @scenario: sync_updates :: Own-card field removals converge across linked devices
#[test]
fn card_field_removal_conflicts_with_updates_for_the_same_field() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "local@example.com".to_string(),
            timestamp: 2000,
        })
        .unwrap();

    let stale = orchestrator
        .process_incoming(
            vec![SyncItem::CardFieldRemoved {
                field_label: "email".to_string(),
                timestamp: 1000,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(
        stale.is_empty(),
        "older removal must lose to the field update"
    );

    let newer = orchestrator
        .process_incoming(
            vec![SyncItem::CardFieldRemoved {
                field_label: "email".to_string(),
                timestamp: 3000,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(matches!(
        newer.as_slice(),
        [SyncItem::CardFieldRemoved { field_label, .. }] if field_label == "email"
    ));
}

/// A same-device remove+add pair (the CLI `card edit` shape) must apply
/// in order within one batch; the equal-stamp dedup must not drop the
/// re-add half and silently lose the edit (ADR-020).
// @scenario: sync_updates :: Own-card field removals converge across linked devices
#[test]
fn same_timestamp_remove_and_readd_both_apply_in_one_batch() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "old".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    // CLI `card edit` shape: remove + re-add in one batch with one
    // timestamp from one sender. The edit's re-add half must not be
    // rejected as a duplicate of its own removal half.
    let applied = orchestrator
        .process_incoming(
            vec![
                SyncItem::CardFieldRemoved {
                    field_label: "phone".to_string(),
                    timestamp: 2000,
                },
                SyncItem::CardUpdated {
                    field_label: "phone".to_string(),
                    new_value: "new".to_string(),
                    timestamp: 2000,
                },
            ],
            &[0x99u8; 32],
        )
        .unwrap();

    assert_eq!(
        applied.len(),
        2,
        "remove+add edit pair must apply in order, got {applied:?}"
    );

    // Redelivery of the same batch (retry/duplicate) must still dedup:
    // every item's stamp now equals the persisted stamp.
    let redelivered = orchestrator
        .process_incoming(
            vec![
                SyncItem::CardFieldRemoved {
                    field_label: "phone".to_string(),
                    timestamp: 2000,
                },
                SyncItem::CardUpdated {
                    field_label: "phone".to_string(),
                    new_value: "new".to_string(),
                    timestamp: 2000,
                },
            ],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(
        redelivered.is_empty(),
        "redelivered batch must dedup, got {redelivered:?}"
    );
}

/// Scenario: Bidirectional sync
/// "When I add a phone number on Device A
///  And I add an email on Device B
///  Then both devices should have both fields"
// @scenario: device_management :: Bidirectional sync
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
    let applied = orchestrator
        .process_incoming(incoming_items, &[0x99u8; 32])
        .unwrap();

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
// @scenario: device_management :: Bidirectional sync
#[test]
fn test_bidirectional_field_additions() {
    let storage_a = create_test_storage();
    let storage_b = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b_for_a = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b_for_a.device_id();
    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b_for_a.to_registered(&master_seed), &signing_key)
        .unwrap();

    let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_a_id = *device_a_for_b.device_id();
    let mut registry_b = create_test_registry(&master_seed, &device_b);
    registry_b
        .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);

    orchestrator_a
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1111111111".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    orchestrator_b
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "user@example.com".to_string(),
            timestamp: 1001,
        })
        .unwrap();

    let a_to_b = orchestrator_a.pending_for_device(&device_b_id).to_vec();
    let b_to_a = orchestrator_b.pending_for_device(&device_a_id).to_vec();

    let applied_on_b = orchestrator_b
        .process_incoming(a_to_b, &[0x99u8; 32])
        .unwrap();
    let applied_on_a = orchestrator_a
        .process_incoming(b_to_a, &[0x99u8; 32])
        .unwrap();

    // Both should have applied the other's changes (different fields, no conflict)
    assert_eq!(applied_on_b.len(), 1); // phone from A
    assert_eq!(applied_on_a.len(), 1); // email from B
}

/// Visibility overrides are per (contact, field): two overrides for the
/// same contact but different fields are independent changes. A
/// per-contact conflict key would drop the older-timestamped one as
/// stale under reordered delivery, silently diverging the devices.
// @internal
#[test]
fn visibility_changes_for_distinct_fields_apply_under_reorder() {
    let storage_a = create_test_storage();
    let storage_b = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");

    let device_b_for_a = create_test_device(&master_seed, 1, "Device B");
    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b_for_a.to_registered(&master_seed), &signing_key)
        .unwrap();

    let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
    let mut registry_b = create_test_registry(&master_seed, &device_b);
    registry_b
        .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);

    let email_change = SyncItem::VisibilityChanged {
        contact_id: "contact-bob".to_string(),
        field_id: "field-email".to_string(),
        is_visible: true,
        timestamp: 1000,
    };
    let phone_change = SyncItem::VisibilityChanged {
        contact_id: "contact-bob".to_string(),
        field_id: "field-phone".to_string(),
        is_visible: false,
        timestamp: 1001,
    };
    orchestrator_a
        .record_local_change(email_change.clone())
        .unwrap();
    orchestrator_a
        .record_local_change(phone_change.clone())
        .unwrap();

    // Reordered delivery: the newer phone change arrives first.
    let applied_first = orchestrator_b
        .process_incoming(vec![phone_change], &[0x99u8; 32])
        .unwrap();
    let applied_second = orchestrator_b
        .process_incoming(vec![email_change], &[0x99u8; 32])
        .unwrap();

    assert_eq!(applied_first.len(), 1, "newer field change applies");
    assert_eq!(
        applied_second.len(),
        1,
        "older timestamp for a DIFFERENT field must still apply; \
         a per-contact conflict key would drop it as stale"
    );
}

/// Scenario: Offline changes are queued
/// Changes made while offline should be stored for later sync
// @scenario: device_management :: Offline changes sync when reconnected
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

    for i in 1..=5 {
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: format!("field_{}", i),
                new_value: format!("value_{}", i),
                timestamp: i * 1000,
            })
            .unwrap();
    }

    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(pending.len(), 5);
}

/// Scenario: Offline changes sync when reconnected
/// "Given Device B is offline
///  When Device B makes changes offline
///  And Device B reconnects
///  Then those changes should sync to Device A"
// @scenario: device_management :: Offline changes sync when reconnected
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

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "offline_field".to_string(),
            new_value: "offline_value".to_string(),
            timestamp: 5000,
        })
        .unwrap();

    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(pending.len(), 1);

    let sync_message = orchestrator.create_sync_message(&device_b_id).unwrap();

    assert!(!sync_message.items.is_empty());
    assert_eq!(sync_message.items.len(), 1);
}

// G3: the LWW gate must survive an orchestrator reload. A local edit's
// timestamp is persisted (field_timestamps) so a later-loaded orchestrator
// rejects an older incoming edit and accepts a newer one — without this,
// load() starts empty and an older remote change would overwrite a newer
// local one (2026-06-06-multi-device-sync-live-wiring).
// @scenario: device_management :: Concurrent edits converge by last-write-wins
#[test]
fn field_timestamps_persist_across_reload_for_lww() {
    let storage = create_test_storage();
    let master_seed = [0x55u8; 32];
    // DeviceInfo::derive is deterministic from (seed, index), so re-deriving
    // yields an equivalent device (DeviceInfo isn't Clone).
    let registry = create_test_registry(
        &master_seed,
        &create_test_device(&master_seed, 0, "Device 0"),
    );

    {
        let mut orch = DeviceSyncOrchestrator::new(
            &storage,
            create_test_device(&master_seed, 0, "Device 0"),
            registry.clone(),
        );
        orch.record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "new@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();
    }

    // Reload a fresh orchestrator — field_timestamps must be restored.
    let mut reloaded = DeviceSyncOrchestrator::load(
        &storage,
        create_test_device(&master_seed, 0, "Device 0"),
        registry.clone(),
    )
    .unwrap();

    // An OLDER incoming edit to the same field is rejected (LWW).
    let stale = reloaded
        .process_incoming(
            vec![SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "stale@example.com".to_string(),
                timestamp: 500,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(
        stale.is_empty(),
        "older incoming edit must lose to the persisted newer local timestamp"
    );

    // A NEWER incoming edit to the same field is applied.
    let fresh = reloaded
        .process_incoming(
            vec![SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "fresh@example.com".to_string(),
                timestamp: 1500,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert_eq!(fresh.len(), 1, "newer incoming edit must win");
}

fn save_ratchet_for(storage: &Storage, contact_id: &str) {
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::generate(), *their_dh.public_key())
            .unwrap();
    storage
        .ratchets()
        .save_ratchet_state(contact_id, &ratchet, true)
        .unwrap();
}

/// An add-device link carries contact state without any live ratchet chain.
// @scenario: device_management :: New device receives full state
// @internal
#[test]
fn add_device_sync_payload_carries_contacts_without_ratchets() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let contact = create_test_contact("Bob");
    storage.contacts().save_contact(&contact).unwrap();
    save_ratchet_for(&storage, contact.id());

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
    let payload = orchestrator
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();

    assert_eq!(payload.contact_count(), 1);
}

/// Replacement also establishes fresh pair sessions; it must not inherit a
/// chain that could still advance on the source device.
// @scenario: device_management :: New device receives full state
// @internal
#[test]
fn replacement_sync_payload_carries_contacts_without_ratchets() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let contact = create_test_contact("Bob");
    storage.contacts().save_contact(&contact).unwrap();
    save_ratchet_for(&storage, contact.id());

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
    let payload = orchestrator
        .create_full_sync_payload(DeviceLinkIntent::ReplaceDevice)
        .unwrap();

    assert_eq!(payload.contact_count(), 1);
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn expanded_registry_is_queued_for_devices_linked_earlier() {
    let storage = create_test_storage();
    let master_seed = [0x63u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let identity =
        Identity::from_device_link(master_seed, "Alice".into(), 0, "Alice phone".into(), 1);
    let device_a = identity.create_device_info(1);
    let device_b = create_test_device(&master_seed, 1, "Alice tablet");
    let device_c = create_test_device(&master_seed, 2, "Alice laptop");

    let mut previous = create_test_registry(&master_seed, &device_a);
    previous
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();
    storage.device().save_device_registry(&previous).unwrap();

    let mut expanded = previous;
    expanded
        .add_device(device_c.to_registered(&master_seed), &signing_key)
        .unwrap();
    DeviceSyncOrchestrator::persist_device_registry_change(&storage, &identity, &expanded, 2)
        .unwrap();

    let stored = storage.device().load_device_registry().unwrap().unwrap();
    assert_eq!(stored.active_count(), 3);
    let orchestrator = DeviceSyncOrchestrator::load(&storage, device_a, stored).unwrap();
    assert!(
        orchestrator
            .pending_for_device(device_b.device_id())
            .iter()
            .any(|item| matches!(item, SyncItem::DeviceRegistryChanged { .. })),
        "the pre-existing tablet must receive the expanded registry"
    );
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn device_registry_sync_accepts_newer_signed_state_and_rejects_forgery() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let identity = wb.identity().unwrap();
    let seed = *identity.master_seed();
    let second = DeviceInfo::derive(&seed, 1, "Alice tablet".into(), 2);
    let mut expanded = identity.initial_device_registry();
    expanded
        .add_device(second.to_registered(&seed), identity.signing_keypair())
        .unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::DeviceRegistryChanged {
            registry_json: expanded.to_json(),
            version: expanded.version(),
        }])
        .unwrap();
    assert_eq!(applied, 1);
    assert_eq!(wb.list_devices().unwrap().len(), 2);

    let attacker = SigningKeyPair::from_seed(&[0x99u8; 32]);
    let third = DeviceInfo::derive(&seed, 2, "Forged laptop".into(), 3);
    let mut forged = expanded;
    forged
        .add_device(third.to_registered(&seed), &attacker)
        .unwrap();
    let applied = wb
        .apply_sync_items(vec![SyncItem::DeviceRegistryChanged {
            registry_json: forged.to_json(),
            version: forged.version(),
        }])
        .unwrap();

    assert_eq!(applied, 0);
    assert_eq!(wb.list_devices().unwrap().len(), 2);
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn same_second_registry_expansions_order_by_signed_version() {
    let storage = create_test_storage();
    let seed = [0x73u8; 32];
    let current = create_test_device(&seed, 1, "Alice tablet");
    let registry = create_test_registry(&seed, &current);
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, current, registry);
    let sender = [0x11u8; 32];

    let applied = orchestrator
        .process_incoming(
            vec![
                SyncItem::DeviceRegistryChanged {
                    registry_json: "registry-v2".to_string(),
                    version: 2,
                },
                SyncItem::DeviceRegistryChanged {
                    registry_json: "registry-v3".to_string(),
                    version: 3,
                },
            ],
            &sender,
        )
        .unwrap();

    assert_eq!(applied.len(), 2);
    assert_eq!(applied[0].timestamp(), 2);
    assert_eq!(applied[1].timestamp(), 3);
}
