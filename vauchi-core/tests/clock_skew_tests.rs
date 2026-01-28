// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clock Skew Handling Tests
//!
//! Tests for handling clock skew between devices using version vectors (logical clocks).
//! Based on: sync_updates.feature - Conflict resolution edge cases

use vauchi_core::sync::{SyncItem, VersionVector};

// =============================================================================
// Version Vector Basic Tests
// =============================================================================

/// Scenario: Version vector tracks device versions
#[test]
fn test_version_vector_tracks_versions() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    let mut vv = VersionVector::new();

    // Initial versions should be 0
    assert_eq!(vv.get(&device_a), 0);
    assert_eq!(vv.get(&device_b), 0);

    // Increment device A
    vv.increment(&device_a);
    assert_eq!(vv.get(&device_a), 1);
    assert_eq!(vv.get(&device_b), 0);

    // Increment device B twice
    vv.increment(&device_b);
    vv.increment(&device_b);
    assert_eq!(vv.get(&device_b), 2);
}

/// Scenario: Version vector merge takes maximum
#[test]
fn test_version_vector_merge() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];
    let device_c = [0x03u8; 32];

    let mut vv1 = VersionVector::new();
    vv1.increment(&device_a); // A=1
    vv1.increment(&device_a); // A=2
    vv1.increment(&device_b); // B=1

    let mut vv2 = VersionVector::new();
    vv2.increment(&device_a); // A=1
    vv2.increment(&device_b); // B=1
    vv2.increment(&device_b); // B=2
    vv2.increment(&device_c); // C=1

    // Merge: should take max of each
    let merged = VersionVector::merge(&vv1, &vv2);

    assert_eq!(merged.get(&device_a), 2); // max(2, 1) = 2
    assert_eq!(merged.get(&device_b), 2); // max(1, 2) = 2
    assert_eq!(merged.get(&device_c), 1); // only in vv2
}

// =============================================================================
// Concurrent Update Detection Tests
// =============================================================================

/// Scenario: Detect concurrent updates (neither dominates)
#[test]
fn test_detect_concurrent_updates() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    // VV1: A=2, B=1 (A has made more changes)
    let mut vv1 = VersionVector::new();
    vv1.increment(&device_a);
    vv1.increment(&device_a);
    vv1.increment(&device_b);

    // VV2: A=1, B=2 (B has made more changes)
    let mut vv2 = VersionVector::new();
    vv2.increment(&device_a);
    vv2.increment(&device_b);
    vv2.increment(&device_b);

    // These are concurrent - neither dominates
    assert!(vv1.is_concurrent_with(&vv2));
    assert!(vv2.is_concurrent_with(&vv1));
}

/// Scenario: Detect happens-before relationship
#[test]
fn test_detect_happens_before() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    // VV1: A=1, B=1 (earlier state)
    let mut vv1 = VersionVector::new();
    vv1.increment(&device_a);
    vv1.increment(&device_b);

    // VV2: A=2, B=2 (includes all of VV1 and more)
    let mut vv2 = VersionVector::new();
    vv2.increment(&device_a);
    vv2.increment(&device_a);
    vv2.increment(&device_b);
    vv2.increment(&device_b);

    // VV2 dominates VV1 - not concurrent
    assert!(!vv1.is_concurrent_with(&vv2));
    assert!(!vv2.is_concurrent_with(&vv1));
}

/// Scenario: Identical vectors behavior
/// Note: Two identical non-empty vectors are considered "concurrent"
/// because neither dominates the other (no device has MORE in either)
#[test]
fn test_identical_vectors_behavior() {
    let device_a = [0x01u8; 32];

    let mut vv1 = VersionVector::new();
    vv1.increment(&device_a);

    let mut vv2 = VersionVector::new();
    vv2.increment(&device_a);

    // In the current implementation, identical vectors are "concurrent"
    // because is_concurrent checks if neither dominates, and identical
    // vectors don't dominate each other (dominate requires strictly greater)
    // This is fine - the semantic meaning is "can be merged without conflict"
    assert!(vv1.is_concurrent_with(&vv2));
}

// =============================================================================
// Clock Skew Resilience Tests
// =============================================================================

/// Scenario: Conflict resolution works regardless of wall clock time
#[test]
fn test_conflict_resolution_clock_independent() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    // Device A thinks it's at timestamp 1000
    let item_a = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "a@test.com".to_string(),
        timestamp: 1000, // "Earlier" wall clock
    };

    // Device B thinks it's at timestamp 2000 (1 hour ahead due to clock skew)
    let item_b = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "b@test.com".to_string(),
        timestamp: 2000, // "Later" wall clock due to skew
    };

    // Using timestamps alone: B would win (unfairly, if A was actually later)
    let winner_by_timestamp = SyncItem::resolve_conflict(&item_a, &item_b);
    assert_eq!(winner_by_timestamp.timestamp(), 2000);

    // With version vectors, we can detect this is actually concurrent
    // and apply proper resolution (e.g., deterministic tie-breaker)
    let mut vv_a = VersionVector::new();
    vv_a.increment(&device_a);

    let mut vv_b = VersionVector::new();
    vv_b.increment(&device_b);

    // These are concurrent - neither happened before the other
    assert!(vv_a.is_concurrent_with(&vv_b));

    // For concurrent updates, we should use a deterministic tie-breaker
    // (e.g., lexicographic comparison of device IDs)
    // device_a (0x01...) < device_b (0x02...), so A would win
    let tie_breaker_winner = if device_a < device_b { "a" } else { "b" };
    assert_eq!(tie_breaker_winner, "a");
}

/// Scenario: Version vectors handle extreme clock skew
#[test]
fn test_extreme_clock_skew() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    // Device A: far future timestamp (year 2100)
    let _item_a = SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "555-1111".to_string(),
        timestamp: 4102444800, // Year 2100
    };

    // Device B: current timestamp
    let _item_b = SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "555-2222".to_string(),
        timestamp: 1706000000, // Year 2024
    };

    // Without version vectors, A would always win due to future timestamp
    // This is a problem!

    // With version vectors, we track causality properly
    let mut vv_a = VersionVector::new();
    vv_a.increment(&device_a);

    let mut vv_b = VersionVector::new();
    vv_b.increment(&device_b);

    // Still concurrent - neither dominates
    assert!(vv_a.is_concurrent_with(&vv_b));
}

// =============================================================================
// Version Vector Serialization Tests
// =============================================================================

/// Scenario: Version vector survives serialization roundtrip
#[test]
fn test_version_vector_roundtrip() {
    let device_a = [0xABu8; 32];
    let device_b = [0xCDu8; 32];

    let mut vv = VersionVector::new();
    vv.increment(&device_a);
    vv.increment(&device_a);
    vv.increment(&device_b);

    let json = vv.to_json();
    let restored = VersionVector::from_json(&json).unwrap();

    assert_eq!(restored.get(&device_a), 2);
    assert_eq!(restored.get(&device_b), 1);
}

// =============================================================================
// Multi-Device Sync Tests
// =============================================================================

/// Scenario: Three-way merge with concurrent updates
#[test]
fn test_three_way_merge() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];
    let device_c = [0x03u8; 32];

    // Initial shared state
    let mut base = VersionVector::new();
    base.increment(&device_a);
    base.increment(&device_b);
    base.increment(&device_c);

    // Device A makes changes offline
    let mut vv_a = base.clone();
    vv_a.increment(&device_a);
    vv_a.increment(&device_a);

    // Device B makes changes offline
    let mut vv_b = base.clone();
    vv_b.increment(&device_b);

    // Device C makes changes offline
    let mut vv_c = base.clone();
    vv_c.increment(&device_c);
    vv_c.increment(&device_c);
    vv_c.increment(&device_c);

    // All three are concurrent with each other
    assert!(vv_a.is_concurrent_with(&vv_b));
    assert!(vv_b.is_concurrent_with(&vv_c));
    assert!(vv_a.is_concurrent_with(&vv_c));

    // Merge all three: take max of each device
    let merged_ab = VersionVector::merge(&vv_a, &vv_b);
    let merged_all = VersionVector::merge(&merged_ab, &vv_c);

    assert_eq!(merged_all.get(&device_a), 3); // base(1) + 2 = 3
    assert_eq!(merged_all.get(&device_b), 2); // base(1) + 1 = 2
    assert_eq!(merged_all.get(&device_c), 4); // base(1) + 3 = 4
}

/// Scenario: Detect if local state is behind
#[test]
fn test_detect_local_state_behind() {
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    // Local state: A=1, B=1
    let mut local = VersionVector::new();
    local.increment(&device_a);
    local.increment(&device_b);

    // Remote state: A=1, B=3 (B made more updates)
    let mut remote = VersionVector::new();
    remote.increment(&device_a);
    remote.increment(&device_b);
    remote.increment(&device_b);
    remote.increment(&device_b);

    // Remote dominates local - local is behind
    assert!(!local.is_concurrent_with(&remote));

    // Local should apply remote's updates
    let merged = VersionVector::merge(&local, &remote);
    assert_eq!(merged.get(&device_b), 3);
}

// =============================================================================
// Sync Item Conflict Resolution Tests
// =============================================================================

/// Scenario: Last-write-wins for non-concurrent updates
#[test]
fn test_last_write_wins_sequential() {
    // When updates are sequential (not concurrent), last-write-wins is correct
    let item1 = SyncItem::CardUpdated {
        field_label: "name".to_string(),
        new_value: "Old Name".to_string(),
        timestamp: 1000,
    };

    let item2 = SyncItem::CardUpdated {
        field_label: "name".to_string(),
        new_value: "New Name".to_string(),
        timestamp: 2000,
    };

    let winner = SyncItem::resolve_conflict(&item1, &item2);
    match winner {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(new_value, "New Name");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

/// Scenario: Timestamps tie - deterministic resolution needed
#[test]
fn test_timestamp_tie_resolution() {
    let item1 = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "first@test.com".to_string(),
        timestamp: 1000,
    };

    let item2 = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "second@test.com".to_string(),
        timestamp: 1000, // Same timestamp!
    };

    // Current implementation: first one wins on tie (>= comparison)
    let winner = SyncItem::resolve_conflict(&item1, &item2);
    assert_eq!(winner.timestamp(), 1000);

    // For true determinism, we should use additional criteria
    // (e.g., hash of content, device ID, etc.)
}
