// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for multi-device deletion sync via SyncItem.
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Account deletion propagates across all user devices"

use vauchi_core::sync::device_sync::SyncItem;

// @scenario: privacy_compliance.feature:Account deletion propagates across all user devices
#[test]
fn test_deletion_scheduled_sync_item() {
    let item = SyncItem::DeletionScheduled {
        scheduled_at: 1700000000,
        execute_at: 1700604800, // 7 days later
        timestamp: 1700000000,
    };

    assert_eq!(item.timestamp(), 1700000000);

    // Serialization roundtrip
    let json = serde_json::to_string(&item).unwrap();
    let decoded: SyncItem = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.timestamp(), 1700000000);

    match decoded {
        SyncItem::DeletionScheduled {
            scheduled_at,
            execute_at,
            ..
        } => {
            assert_eq!(scheduled_at, 1700000000);
            assert_eq!(execute_at, 1700604800);
        }
        _ => panic!("Expected DeletionScheduled"),
    }
}

// @scenario: privacy_compliance.feature:Cancel deletion during grace period
#[test]
fn test_deletion_cancelled_sync_item() {
    let item = SyncItem::DeletionCancelled {
        timestamp: 1700000500,
    };

    assert_eq!(item.timestamp(), 1700000500);

    // Serialization roundtrip
    let json = serde_json::to_string(&item).unwrap();
    let decoded: SyncItem = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.timestamp(), 1700000500);

    match decoded {
        SyncItem::DeletionCancelled { timestamp } => {
            assert_eq!(timestamp, 1700000500);
        }
        _ => panic!("Expected DeletionCancelled"),
    }
}

#[test]
fn test_existing_sync_items_still_deserialize() {
    // Ensure backward compat: existing SyncItem variants still work
    let contact_removed = SyncItem::ContactRemoved {
        contact_id: "test123".to_string(),
        timestamp: 1700000000,
    };

    let json = serde_json::to_string(&contact_removed).unwrap();
    let decoded: SyncItem = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.timestamp(), 1700000000);
}
