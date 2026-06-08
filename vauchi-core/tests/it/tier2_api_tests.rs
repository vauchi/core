// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ADR-021 Tier 2 Core APIs.
//!
//! Traces to: features/adr021_api_surface.feature

use vauchi_core::api::*;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};
use vauchi_core::sync::device_sync::{ContactSyncData, SyncItem};

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ============================================================
// API 1: find_device_by_prefix (facade)
// ============================================================

// @internal
#[test]
fn test_vauchi_find_device_by_prefix_with_registry() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let identity = wb.identity().unwrap();
    let registry = identity.initial_device_registry();

    let primary = registry.primary_device().unwrap();
    let hex_id = primary.device_id_hex();
    let prefix = &hex_id[..8];

    wb.storage().save_device_registry(&registry).unwrap();

    // Find device by prefix through the facade
    let found = wb.find_device_by_prefix(prefix).unwrap();
    assert!(
        found.is_some(),
        "Should find device by prefix through facade"
    );
    assert_eq!(found.unwrap().device_id_hex(), hex_id);
}

// @internal
#[test]
fn test_vauchi_find_device_by_prefix_no_registry() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let found = wb.find_device_by_prefix("abcd").unwrap();
    assert!(
        found.is_none(),
        "Should return None when no device registry exists"
    );
}

// ============================================================
// API 2: get_delivery_status_for_contact / get_failed_deliveries
// ============================================================

// @internal
#[test]
fn test_get_delivery_status_for_contact_returns_matching_records() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let timestamp = now();

    let record1 = DeliveryRecord {
        message_id: "msg-001".to_string(),
        recipient_id: "contact-aaa".to_string(),
        status: DeliveryStatus::Sent,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: None,
    };
    let record2 = DeliveryRecord {
        message_id: "msg-002".to_string(),
        recipient_id: "contact-bbb".to_string(),
        status: DeliveryStatus::Queued,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: None,
    };
    let record3 = DeliveryRecord {
        message_id: "msg-003".to_string(),
        recipient_id: "contact-aaa".to_string(),
        status: DeliveryStatus::Delivered,
        created_at: timestamp + 1,
        updated_at: timestamp + 1,
        expires_at: None,
    };

    wb.storage().create_delivery_record(&record1).unwrap();
    wb.storage().create_delivery_record(&record2).unwrap();
    wb.storage().create_delivery_record(&record3).unwrap();

    let results = wb.get_delivery_status_for_contact("contact-aaa").unwrap();
    assert_eq!(
        results.len(),
        2,
        "Should return exactly 2 records for contact-aaa"
    );

    for record in &results {
        assert_eq!(record.recipient_id, "contact-aaa");
    }

    let message_ids: Vec<&str> = results.iter().map(|r| r.message_id.as_str()).collect();
    assert!(message_ids.contains(&"msg-001"), "Should contain msg-001");
    assert!(message_ids.contains(&"msg-003"), "Should contain msg-003");
}

// @internal
#[test]
fn test_get_delivery_status_for_contact_returns_empty_for_unknown() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let results = wb.get_delivery_status_for_contact("nonexistent").unwrap();
    assert!(
        results.is_empty(),
        "Should return empty vec for unknown contact"
    );
}

// @internal
#[test]
fn test_get_failed_deliveries_returns_only_failed() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let timestamp = now();

    let records = vec![
        DeliveryRecord {
            message_id: "msg-ok".to_string(),
            recipient_id: "contact-a".to_string(),
            status: DeliveryStatus::Delivered,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        },
        DeliveryRecord {
            message_id: "msg-fail-1".to_string(),
            recipient_id: "contact-b".to_string(),
            status: DeliveryStatus::Failed {
                reason: "timeout".to_string(),
            },
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        },
        DeliveryRecord {
            message_id: "msg-queued".to_string(),
            recipient_id: "contact-c".to_string(),
            status: DeliveryStatus::Queued,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        },
        DeliveryRecord {
            message_id: "msg-fail-2".to_string(),
            recipient_id: "contact-a".to_string(),
            status: DeliveryStatus::Failed {
                reason: "relay rejected".to_string(),
            },
            created_at: timestamp + 1,
            updated_at: timestamp + 1,
            expires_at: None,
        },
    ];

    for r in &records {
        wb.storage().create_delivery_record(r).unwrap();
    }

    let failed = wb.get_failed_deliveries().unwrap();
    assert_eq!(failed.len(), 2, "Should return exactly 2 failed records");

    for record in &failed {
        assert!(
            matches!(record.status, DeliveryStatus::Failed { .. }),
            "All returned records must have Failed status, got {:?}",
            record.status
        );
    }

    let message_ids: Vec<&str> = failed.iter().map(|r| r.message_id.as_str()).collect();
    assert!(message_ids.contains(&"msg-fail-1"));
    assert!(message_ids.contains(&"msg-fail-2"));
}

// ============================================================
// API 3: apply_sync_items
// ============================================================

/// Creates a ContactSyncData with a deterministic public key derived from a seed byte.
/// The contact ID will be hex::encode([seed_byte; 32]).
fn make_contact_sync_data(seed_byte: u8, name: &str) -> ContactSyncData {
    let public_key = [seed_byte; 32];
    let card = ContactCard::new(name);
    let card_json = serde_json::to_string(&card).unwrap();
    let visibility_rules =
        serde_json::to_string(&vauchi_core::contact::VisibilityRules::default()).unwrap();
    let id = hex::encode(public_key);

    ContactSyncData {
        id: id.clone(),
        public_key: vauchi_core::identifiers::IdentityKey::from_bytes(public_key),
        display_name: name.to_string(),
        card_json,
        shared_key: [0xBB; 32],
        exchange_timestamp: now(),
        fingerprint_verified: false,
        visibility_rules_json: visibility_rules,
        recovery_trusted: false,
    }
}

/// Returns the contact ID that will result from a given seed byte.
fn contact_id_for_seed(seed_byte: u8) -> String {
    hex::encode([seed_byte; 32])
}

// @internal
#[test]
fn test_apply_sync_items_processes_contact_added() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    assert_eq!(wb.contact_count().unwrap(), 0);

    let bob_id = contact_id_for_seed(0xAA);
    let items = vec![SyncItem::ContactAdded {
        contact_data: make_contact_sync_data(0xAA, "Bob"),
        timestamp: now(),
    }];

    let applied = wb.apply_sync_items(items).unwrap();
    assert_eq!(applied, 1, "Should have applied 1 sync item");

    assert_eq!(wb.contact_count().unwrap(), 1);
    let contact = wb.get_contact(&bob_id).unwrap();
    assert!(contact.is_some(), "Contact should exist after sync");
    assert_eq!(contact.unwrap().display_name(), "Bob");
}

// @internal
#[test]
fn test_apply_sync_items_processes_contact_removed() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let charlie_id = contact_id_for_seed(0xCC);

    let add_items = vec![SyncItem::ContactAdded {
        contact_data: make_contact_sync_data(0xCC, "Charlie"),
        timestamp: now(),
    }];
    wb.apply_sync_items(add_items).unwrap();
    assert_eq!(wb.contact_count().unwrap(), 1);

    let remove_items = vec![SyncItem::ContactRemoved {
        contact_id: charlie_id.clone(),
        timestamp: now() + 1,
    }];
    let applied = wb.apply_sync_items(remove_items).unwrap();
    assert_eq!(applied, 1, "Should have applied the removal");

    let contact = wb.get_contact(&charlie_id).unwrap();
    assert!(contact.is_none(), "Contact should be removed after sync");
}

// @internal
#[test]
fn test_apply_sync_items_processes_visibility_changed() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let diana_id = contact_id_for_seed(0xDD);

    let add_items = vec![SyncItem::ContactAdded {
        contact_data: make_contact_sync_data(0xDD, "Diana"),
        timestamp: now(),
    }];
    wb.apply_sync_items(add_items).unwrap();

    let vis_items = vec![SyncItem::VisibilityChanged {
        contact_id: diana_id.clone(),
        field_label: "email".to_string(),
        is_visible: false,
        timestamp: now() + 1,
    }];

    let applied = wb.apply_sync_items(vis_items).unwrap();
    assert_eq!(applied, 1, "Should have applied visibility change");

    let overrides = wb.get_contact_visibility_overrides(&diana_id).unwrap();
    assert_eq!(
        overrides.get("email"),
        Some(&false),
        "email field should be hidden after visibility sync"
    );
}

// @internal
#[test]
fn test_apply_sync_items_empty_list_returns_zero() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let applied = wb.apply_sync_items(vec![]).unwrap();
    assert_eq!(applied, 0, "Empty item list should apply 0 items");
}

// ============================================================
// SyncResult convenience methods (V13)
// ============================================================

// @internal
#[test]
fn test_sync_result_total_sums_all_operations() {
    let result = SyncResult {
        sent: 3,
        acknowledged: 2,
        failed: 1,
        timed_out: 0,
        errors: vec![],
    };
    assert_eq!(
        result.total(),
        6,
        "total() should sum sent + acknowledged + failed + timed_out"
    );
}

// @internal
#[test]
fn test_sync_result_total_zero_for_default() {
    let result = SyncResult::default();
    assert_eq!(
        result.total(),
        0,
        "Default SyncResult should have total() == 0"
    );
}

// @internal
#[test]
fn test_sync_result_has_changes_true_when_sent() {
    let result = SyncResult {
        sent: 1,
        ..SyncResult::default()
    };
    assert!(
        result.has_changes(),
        "has_changes() should be true when sent > 0"
    );
}

// @internal
#[test]
fn test_sync_result_has_changes_true_when_acknowledged() {
    let result = SyncResult {
        acknowledged: 1,
        ..SyncResult::default()
    };
    assert!(
        result.has_changes(),
        "has_changes() should be true when acknowledged > 0"
    );
}

// @internal
#[test]
fn test_sync_result_has_changes_false_for_default() {
    let result = SyncResult::default();
    assert!(
        !result.has_changes(),
        "Default SyncResult should have has_changes() == false"
    );
}

// API 4: check_content_updates — moved to vauchi-app
