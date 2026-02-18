// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Correctness Tests
//!
//! Verifies key correctness properties of the sync system:
//! - Version vector merge associativity and commutativity
//! - Equal-timestamp conflict resolution (device ID tie-breaker)
//! - Equal-timestamp rejection in process_incoming
//! - Checkpoint persistence through orchestrator API
//! - Concurrent updates from two devices resolve deterministically
//!
//! Cross-reference: Tracker #225, #34

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::*;

// -- Helpers -----------------------------------------------------------

fn create_test_storage() -> vauchi_core::Storage {
    let key = SymmetricKey::generate();
    vauchi_core::Storage::in_memory(key).unwrap()
}

fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
    DeviceInfo::derive(master_seed, index, name.to_string())
}

fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
    let signing_key = SigningKeyPair::from_seed(master_seed);
    DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
}

// -- Version Vector Merge Correctness ----------------------------------

/// Merge is commutative: merge(A, B) == merge(B, A)
#[test]
fn test_version_vector_merge_commutative() {
    let d1 = [0x11u8; 32];
    let d2 = [0x22u8; 32];
    let d3 = [0x33u8; 32];

    let mut a = VersionVector::new();
    a.increment(&d1);
    a.increment(&d1);
    a.increment(&d2);

    let mut b = VersionVector::new();
    b.increment(&d2);
    b.increment(&d2);
    b.increment(&d3);

    let ab = VersionVector::merge(&a, &b);
    let ba = VersionVector::merge(&b, &a);

    assert_eq!(ab.get(&d1), ba.get(&d1));
    assert_eq!(ab.get(&d2), ba.get(&d2));
    assert_eq!(ab.get(&d3), ba.get(&d3));
}

/// Merge is associative: merge(merge(A, B), C) == merge(A, merge(B, C))
#[test]
fn test_version_vector_merge_associative() {
    let d1 = [0x11u8; 32];
    let d2 = [0x22u8; 32];
    let d3 = [0x33u8; 32];

    let mut a = VersionVector::new();
    a.increment(&d1);
    a.increment(&d1);

    let mut b = VersionVector::new();
    b.increment(&d2);
    b.increment(&d2);
    b.increment(&d2);

    let mut c = VersionVector::new();
    c.increment(&d3);
    c.increment(&d1); // d1 also appears in C

    let ab_c = VersionVector::merge(&VersionVector::merge(&a, &b), &c);
    let a_bc = VersionVector::merge(&a, &VersionVector::merge(&b, &c));

    assert_eq!(ab_c.get(&d1), a_bc.get(&d1));
    assert_eq!(ab_c.get(&d2), a_bc.get(&d2));
    assert_eq!(ab_c.get(&d3), a_bc.get(&d3));
}

/// Merge is idempotent: merge(A, A) == A
#[test]
fn test_version_vector_merge_idempotent() {
    let d1 = [0x11u8; 32];
    let d2 = [0x22u8; 32];

    let mut a = VersionVector::new();
    a.increment(&d1);
    a.increment(&d1);
    a.increment(&d2);

    let merged = VersionVector::merge(&a, &a);

    assert_eq!(merged.get(&d1), a.get(&d1));
    assert_eq!(merged.get(&d2), a.get(&d2));
}

/// Merge always takes the max of each device's counter.
#[test]
fn test_version_vector_merge_takes_max() {
    let d1 = [0x11u8; 32];

    let mut a = VersionVector::new();
    for _ in 0..5 {
        a.increment(&d1);
    }

    let mut b = VersionVector::new();
    for _ in 0..3 {
        b.increment(&d1);
    }

    let merged = VersionVector::merge(&a, &b);
    assert_eq!(merged.get(&d1), 5, "Merge should take max(5, 3) = 5");

    let merged_rev = VersionVector::merge(&b, &a);
    assert_eq!(
        merged_rev.get(&d1),
        5,
        "Merge should take max(3, 5) = 5 regardless of order"
    );
}

// -- Concurrent Detection Correctness ----------------------------------

/// Two vectors with disjoint device sets are concurrent.
#[test]
fn test_concurrent_disjoint_devices() {
    let d1 = [0x11u8; 32];
    let d2 = [0x22u8; 32];

    let mut a = VersionVector::new();
    a.increment(&d1);

    let mut b = VersionVector::new();
    b.increment(&d2);

    assert!(
        a.is_concurrent_with(&b),
        "Disjoint device vectors should be concurrent"
    );
    assert!(
        b.is_concurrent_with(&a),
        "Concurrency should be symmetric"
    );
}

/// When one vector has strictly higher version for the only shared device,
/// they should NOT be concurrent (one causally follows the other).
/// This relies on the `is_concurrent_with` public API.
#[test]
fn test_not_concurrent_when_one_dominates() {
    let d1 = [0x11u8; 32];

    let mut a = VersionVector::new();
    a.increment(&d1);
    a.increment(&d1);

    let mut b = VersionVector::new();
    b.increment(&d1);

    // a has d1=2, b has d1=1. a causally follows b.
    assert!(
        !a.is_concurrent_with(&b),
        "When one vector strictly dominates, they should not be concurrent"
    );
    assert!(
        !b.is_concurrent_with(&a),
        "Concurrency check should be symmetric for dominated case"
    );
}

// -- Equal-Timestamp Conflict Resolution (Device ID Tie-Breaker) -------

/// When two SyncItems have equal timestamps, device ID tie-breaks
/// deterministically — higher device ID wins.
#[test]
fn test_conflict_resolution_equal_timestamp_device_id_tiebreaker() {
    let item_a = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "from_device_a@example.com".to_string(),
        timestamp: 1000,
    };

    let item_b = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "from_device_b@example.com".to_string(),
        timestamp: 1000, // same timestamp
    };

    let device_a_id = [0x11u8; 32]; // lower ID
    let device_b_id = [0x99u8; 32]; // higher ID

    // Regardless of argument order, the higher device_id (0x99) should win
    let resolved_ab =
        SyncItem::resolve_conflict(&item_a, &item_b, &device_a_id, &device_b_id);
    let resolved_ba =
        SyncItem::resolve_conflict(&item_b, &item_a, &device_b_id, &device_a_id);

    match &resolved_ab {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(
                new_value, "from_device_b@example.com",
                "Higher device ID (0x99) should win on equal timestamp"
            );
        }
        _ => panic!("Expected CardUpdated"),
    }

    match &resolved_ba {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(
                new_value, "from_device_b@example.com",
                "Result should be deterministic regardless of argument order"
            );
        }
        _ => panic!("Expected CardUpdated"),
    }
}

// -- Equal-Timestamp Rejection in process_incoming ---------------------

/// When an incoming SyncItem has the same timestamp as a local item
/// for the same field, the incoming item is rejected (local wins).
#[test]
fn test_process_incoming_rejects_equal_timestamp() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Local change at timestamp 1500
    let local_item = SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "local_phone".to_string(),
        timestamp: 1500,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Incoming at same timestamp 1500
    let incoming = vec![SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "remote_phone".to_string(),
        timestamp: 1500,
    }];

    let applied = orchestrator.process_incoming(incoming).unwrap();

    // Equal timestamp should reject incoming (local wins)
    assert!(
        applied.is_empty(),
        "Incoming item at equal timestamp should be rejected"
    );
}

// -- Concurrent Updates Resolve Deterministically ----------------------

/// Two devices update the same field concurrently. The newer update wins.
/// This verifies the full flow: record_local → process_incoming → verify winner.
#[test]
fn test_concurrent_same_field_newer_wins() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orch_a = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Device A updates email at t=1000
    orch_a
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "a@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    // Device B updates email at t=2000 (newer) — arrives as incoming
    let applied = orch_a
        .process_incoming(vec![SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "b@example.com".to_string(),
            timestamp: 2000,
        }])
        .unwrap();

    // Newer wins
    assert_eq!(applied.len(), 1);
    match &applied[0] {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(new_value, "b@example.com");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

/// Two devices update different fields concurrently. Both are preserved.
#[test]
fn test_concurrent_different_fields_both_accepted() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orch_a = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Device A updates phone locally
    orch_a
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1234567890".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    // Device B updates email — arrives as incoming (different field, no conflict)
    let applied = orch_a
        .process_incoming(vec![SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "b@example.com".to_string(),
            timestamp: 1001,
        }])
        .unwrap();

    assert_eq!(
        applied.len(),
        1,
        "Non-conflicting field update should be applied"
    );
    match &applied[0] {
        SyncItem::CardUpdated {
            field_label,
            new_value,
            ..
        } => {
            assert_eq!(field_label, "email");
            assert_eq!(new_value, "b@example.com");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

// -- Checkpoint Persistence Through Orchestrator -----------------------

/// Verify checkpoint round-trip: save → load → resume from sent_count.
#[test]
fn test_checkpoint_save_load_roundtrip() {
    let storage = create_test_storage();
    let target_device_id = [0xAAu8; 32];

    let items = vec![
        SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1".to_string(),
            timestamp: 100,
        },
        SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "a@b.com".to_string(),
            timestamp: 200,
        },
        SyncItem::ContactAdded {
            contact_data: ContactSyncData::from_contact(&Contact::from_exchange(
                [0x42u8; 32],
                ContactCard::new("Bob"),
                SymmetricKey::generate(),
            )),
            timestamp: 300,
        },
    ];

    // Save checkpoint at sent_count = 1 (first item sent)
    storage
        .save_sync_checkpoint(&target_device_id, &items, 1)
        .unwrap();

    // Load and verify
    let loaded = storage.load_sync_checkpoint(&target_device_id).unwrap();
    assert!(loaded.is_some(), "Checkpoint should exist");

    let (loaded_items, sent_count) = loaded.unwrap();
    assert_eq!(sent_count, 1, "Sent count should be 1");
    assert_eq!(loaded_items.len(), 3, "All items should be preserved");

    // Remaining unsent items start from index 1
    assert_eq!(loaded_items[1].timestamp(), 200);
    assert_eq!(loaded_items[2].timestamp(), 300);
}

/// Clear checkpoint removes it completely.
#[test]
fn test_checkpoint_clear() {
    let storage = create_test_storage();
    let target_device_id = [0xBBu8; 32];

    let items = vec![SyncItem::CardUpdated {
        field_label: "test".to_string(),
        new_value: "value".to_string(),
        timestamp: 100,
    }];

    storage
        .save_sync_checkpoint(&target_device_id, &items, 0)
        .unwrap();

    // Verify exists
    assert!(storage
        .load_sync_checkpoint(&target_device_id)
        .unwrap()
        .is_some());

    // Clear
    storage.clear_sync_checkpoint(&target_device_id).unwrap();

    // Verify gone
    assert!(
        storage
            .load_sync_checkpoint(&target_device_id)
            .unwrap()
            .is_none(),
        "Checkpoint should be cleared"
    );
}

/// Checkpoint overwrites previous value for same device.
#[test]
fn test_checkpoint_overwrite() {
    let storage = create_test_storage();
    let target = [0xCCu8; 32];

    let items_v1 = vec![SyncItem::CardUpdated {
        field_label: "v1".to_string(),
        new_value: "old".to_string(),
        timestamp: 100,
    }];

    storage.save_sync_checkpoint(&target, &items_v1, 0).unwrap();

    let items_v2 = vec![
        SyncItem::CardUpdated {
            field_label: "v2a".to_string(),
            new_value: "new_a".to_string(),
            timestamp: 200,
        },
        SyncItem::CardUpdated {
            field_label: "v2b".to_string(),
            new_value: "new_b".to_string(),
            timestamp: 300,
        },
    ];

    storage.save_sync_checkpoint(&target, &items_v2, 1).unwrap();

    let (loaded, sent) = storage.load_sync_checkpoint(&target).unwrap().unwrap();
    assert_eq!(loaded.len(), 2, "Should have v2 items, not v1");
    assert_eq!(sent, 1, "Should have v2 sent_count");
}

// -- Validate Timestamp ------------------------------------------------

/// Timestamps must be non-zero and not too far in the future.
#[test]
fn test_validate_timestamp_rejects_zero() {
    assert!(
        !validate_timestamp(0),
        "Zero timestamp should be rejected"
    );
}

#[test]
fn test_validate_timestamp_accepts_recent() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert!(
        validate_timestamp(now),
        "Current timestamp should be valid"
    );
    assert!(
        validate_timestamp(now - 3600),
        "Timestamp 1 hour ago should be valid"
    );
}

#[test]
fn test_validate_timestamp_rejects_far_future() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert!(
        !validate_timestamp(now + 301),
        "Timestamp >300s in future should be rejected"
    );
}
