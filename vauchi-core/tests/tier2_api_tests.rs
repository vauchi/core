// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ADR-021 Tier 2 Core APIs.
//!
//! Traces to: features/adr021_api_surface.feature

use vauchi_core::api::*;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};
use vauchi_core::*;

fn create_test_vauchi() -> Vauchi<MockTransport> {
    Vauchi::in_memory().unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ============================================================
// API 2: get_delivery_status_for_contact / get_failed_deliveries
// ============================================================

#[test]
fn test_get_delivery_status_for_contact_returns_matching_records() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let timestamp = now();

    // Insert delivery records directly into storage for two different contacts
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

    // Query for contact-aaa only
    let results = wb.get_delivery_status_for_contact("contact-aaa").unwrap();
    assert_eq!(
        results.len(),
        2,
        "Should return exactly 2 records for contact-aaa"
    );

    // Verify all returned records belong to the correct contact
    for record in &results {
        assert_eq!(record.recipient_id, "contact-aaa");
    }

    // Verify specific message IDs are present
    let message_ids: Vec<&str> = results.iter().map(|r| r.message_id.as_str()).collect();
    assert!(message_ids.contains(&"msg-001"), "Should contain msg-001");
    assert!(message_ids.contains(&"msg-003"), "Should contain msg-003");
}

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

#[test]
fn test_get_failed_deliveries_returns_only_failed() {
    let mut wb = create_test_vauchi();
    wb.create_identity("Alice").unwrap();

    let timestamp = now();

    // Insert records with various statuses
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
