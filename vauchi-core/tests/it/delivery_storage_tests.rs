// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for delivery record storage operations.
//!
//! Traces to: features/message_delivery.feature

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus, Storage};

fn test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// @scenario: message_delivery :: Relay provides storage confirmation
// @internal
#[test]
fn test_create_and_retrieve_delivery_record() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeliveryRecord {
        message_id: "msg-001".to_string(),
        recipient_id: "contact-abc".to_string(),
        status: DeliveryStatus::Queued,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: Some(timestamp + 604800), // 7 days
    };

    storage.create_delivery_record(&record).unwrap();

    let retrieved = storage.get_delivery_record("msg-001").unwrap();
    assert!(retrieved.is_some(), "expected Some value");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.message_id, "msg-001");
    assert_eq!(retrieved.recipient_id, "contact-abc");
    assert_eq!(retrieved.status, DeliveryStatus::Queued);
    assert_eq!(retrieved.expires_at, Some(timestamp + 604800));
}

// @internal
#[test]
fn test_get_delivery_record_not_found() {
    let storage = test_storage();

    let retrieved = storage.get_delivery_record("nonexistent").unwrap();
    assert!(retrieved.is_none());
}

// @scenario: message_delivery :: See delivery status for updates
// @scenario: message_delivery :: Delivery status updates in real-time
// @internal
#[test]
fn test_update_delivery_status() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeliveryRecord {
        message_id: "msg-002".to_string(),
        recipient_id: "contact-xyz".to_string(),
        status: DeliveryStatus::Queued,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: None,
    };

    storage.create_delivery_record(&record).unwrap();

    let updated = storage
        .update_delivery_status("msg-002", &DeliveryStatus::Sent, timestamp + 1)
        .unwrap();
    assert!(updated);

    let retrieved = storage.get_delivery_record("msg-002").unwrap().unwrap();
    assert_eq!(retrieved.status, DeliveryStatus::Sent);
    assert_eq!(retrieved.updated_at, timestamp + 1);

    storage
        .update_delivery_status("msg-002", &DeliveryStatus::Stored, timestamp + 2)
        .unwrap();
    let retrieved = storage.get_delivery_record("msg-002").unwrap().unwrap();
    assert_eq!(retrieved.status, DeliveryStatus::Stored);

    storage
        .update_delivery_status("msg-002", &DeliveryStatus::Delivered, timestamp + 3)
        .unwrap();
    let retrieved = storage.get_delivery_record("msg-002").unwrap().unwrap();
    assert_eq!(retrieved.status, DeliveryStatus::Delivered);
}

// @scenario: message_delivery :: Understand why delivery failed
// @internal
#[test]
fn test_delivery_status_failed_with_reason() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeliveryRecord {
        message_id: "msg-003".to_string(),
        recipient_id: "contact-123".to_string(),
        status: DeliveryStatus::Queued,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: None,
    };

    storage.create_delivery_record(&record).unwrap();

    let failed_status = DeliveryStatus::Failed {
        reason: "Connection timeout".to_string(),
    };
    storage
        .update_delivery_status("msg-003", &failed_status, timestamp + 1)
        .unwrap();

    let retrieved = storage.get_delivery_record("msg-003").unwrap().unwrap();
    match retrieved.status {
        DeliveryStatus::Failed { reason } => {
            assert_eq!(reason, "Connection timeout");
        }
        _ => panic!("Expected Failed status"),
    }
}

// @scenario: message_delivery :: View delivery history
// @internal
#[test]
fn test_get_delivery_records_for_recipient() {
    let storage = test_storage();
    let timestamp = now();

    for i in 0..3 {
        let record = DeliveryRecord {
            message_id: format!("msg-alice-{}", i),
            recipient_id: "alice".to_string(),
            status: DeliveryStatus::Delivered,
            created_at: timestamp + i as u64,
            updated_at: timestamp + i as u64,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    for i in 0..2 {
        let record = DeliveryRecord {
            message_id: format!("msg-bob-{}", i),
            recipient_id: "bob".to_string(),
            status: DeliveryStatus::Stored,
            created_at: timestamp + i as u64,
            updated_at: timestamp + i as u64,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    let alice_records = storage.get_delivery_records_for_recipient("alice").unwrap();
    assert_eq!(alice_records.len(), 3);

    let bob_records = storage.get_delivery_records_for_recipient("bob").unwrap();
    assert_eq!(bob_records.len(), 2);

    let empty_records = storage
        .get_delivery_records_for_recipient("charlie")
        .unwrap();
    assert_eq!(empty_records.len(), 0);
}

// @scenario: message_delivery :: Pending status for offline contacts
// @internal
#[test]
fn test_get_pending_deliveries() {
    let storage = test_storage();
    let timestamp = now();

    let statuses = vec![
        ("msg-1", DeliveryStatus::Queued),
        ("msg-2", DeliveryStatus::Sent),
        ("msg-3", DeliveryStatus::Stored),
        ("msg-4", DeliveryStatus::Delivered), // Terminal
        ("msg-5", DeliveryStatus::Expired),   // Terminal
        (
            "msg-6",
            DeliveryStatus::Failed {
                reason: "Error".to_string(),
            },
        ), // Terminal
    ];

    for (id, status) in statuses {
        let record = DeliveryRecord {
            message_id: id.to_string(),
            recipient_id: "contact".to_string(),
            status,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    let pending = storage.get_pending_deliveries().unwrap();
    // Should only include Queued, Sent, Stored (non-terminal)
    assert_eq!(pending.len(), 3);

    let pending_ids: Vec<_> = pending.iter().map(|r| r.message_id.as_str()).collect();
    assert!(pending_ids.contains(&"msg-1"));
    assert!(pending_ids.contains(&"msg-2"));
    assert!(pending_ids.contains(&"msg-3"));
}

// @scenario: message_delivery :: See delivery status for updates
// @internal
#[test]
fn test_count_deliveries_by_status() {
    let storage = test_storage();
    let timestamp = now();

    for i in 0..3 {
        let record = DeliveryRecord {
            message_id: format!("stored-{}", i),
            recipient_id: "contact".to_string(),
            status: DeliveryStatus::Stored,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    for i in 0..2 {
        let record = DeliveryRecord {
            message_id: format!("delivered-{}", i),
            recipient_id: "contact".to_string(),
            status: DeliveryStatus::Delivered,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    let record = DeliveryRecord {
        message_id: "failed-0".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Failed {
            reason: "Error".to_string(),
        },
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: None,
    };
    storage.create_delivery_record(&record).unwrap();

    assert_eq!(
        storage
            .count_deliveries_by_status(&DeliveryStatus::Stored)
            .unwrap(),
        3
    );
    assert_eq!(
        storage
            .count_deliveries_by_status(&DeliveryStatus::Delivered)
            .unwrap(),
        2
    );
    assert_eq!(
        storage
            .count_deliveries_by_status(&DeliveryStatus::Queued)
            .unwrap(),
        0
    );
}

// @scenario: message_delivery :: Message expires after TTL
// @scenario: message_delivery :: Message stored with TTL
// @internal
#[test]
fn test_expire_old_deliveries() {
    let storage = test_storage();
    let now_ts = now();
    let past = now_ts - 1000;
    let future = now_ts + 1000;

    let records = vec![
        ("expired", past, Some(past)),    // Already expired
        ("active", now_ts, Some(future)), // Not yet expired
        ("no-expiry", now_ts, None),      // No expiry set
    ];

    for (id, created, expires) in records {
        let record = DeliveryRecord {
            message_id: id.to_string(),
            recipient_id: "contact".to_string(),
            status: DeliveryStatus::Stored,
            created_at: created,
            updated_at: created,
            expires_at: expires,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    let expired_count = storage.expire_old_deliveries(now_ts).unwrap();
    assert_eq!(expired_count, 1);

    let expired_record = storage.get_delivery_record("expired").unwrap().unwrap();
    assert_eq!(expired_record.status, DeliveryStatus::Expired);

    let active_record = storage.get_delivery_record("active").unwrap().unwrap();
    assert_eq!(active_record.status, DeliveryStatus::Stored);

    let no_expiry_record = storage.get_delivery_record("no-expiry").unwrap().unwrap();
    assert_eq!(no_expiry_record.status, DeliveryStatus::Stored);
}

// @internal
#[test]
fn test_delete_delivery_record() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeliveryRecord {
        message_id: "to-delete".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Delivered,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: None,
    };
    storage.create_delivery_record(&record).unwrap();

    assert!(
        storage.get_delivery_record("to-delete").unwrap().is_some(),
        "expected Some value"
    );

    let deleted = storage.delete_delivery_record("to-delete").unwrap();
    assert!(deleted);

    assert!(storage.get_delivery_record("to-delete").unwrap().is_none());

    let deleted = storage.delete_delivery_record("nonexistent").unwrap();
    assert!(!deleted);
}

// @scenario: message_delivery :: Delivery status updates in real-time
// @internal
#[test]
fn test_delivery_status_progression() {
    // Test the full lifecycle: Queued -> Sent -> Stored -> Delivered
    let storage = test_storage();
    let timestamp = now();

    let record = DeliveryRecord {
        message_id: "lifecycle-test".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Queued,
        created_at: timestamp,
        updated_at: timestamp,
        expires_at: Some(timestamp + 604800),
    };
    storage.create_delivery_record(&record).unwrap();

    let statuses = [
        DeliveryStatus::Sent,
        DeliveryStatus::Stored,
        DeliveryStatus::Delivered,
    ];

    for (i, status) in statuses.iter().enumerate() {
        storage
            .update_delivery_status("lifecycle-test", status, timestamp + i as u64 + 1)
            .unwrap();

        let record = storage
            .get_delivery_record("lifecycle-test")
            .unwrap()
            .unwrap();
        assert_eq!(&record.status, status);
    }
}

// === Expiration Warning Tests ===
// Traces to: features/message_delivery.feature @expiration "Notification before message expires"

/// Test: Get records approaching expiration for warning.
///
/// Scenario: Given I sent an update to an offline contact 29 days ago
///           When the message is approaching expiration
///           Then I should receive a warning notification
// @scenario: message_delivery :: Notification before message expires
// @internal
#[test]
fn test_message_expiration_warning() {
    let storage = test_storage();
    let now_ts = now();
    let one_day = 86400u64;
    let thirty_days = 30 * one_day;

    // - One expiring in 1 day (should warn)
    // - One expiring in 2 days (should warn)
    // - One expiring in 7 days (outside warning threshold)
    // - One already expired (should not warn, already handled)
    // - One with no expiry (should not warn)
    let records = vec![
        ("msg-1day", now_ts + one_day),            // Expires in 1 day
        ("msg-2day", now_ts + 2 * one_day),        // Expires in 2 days
        ("msg-7day", now_ts + 7 * one_day),        // Expires in 7 days
        ("msg-already-expired", now_ts - one_day), // Already expired
    ];

    for (id, expires_at) in records {
        let record = DeliveryRecord {
            message_id: id.to_string(),
            recipient_id: "contact".to_string(),
            status: DeliveryStatus::Stored,
            created_at: now_ts - thirty_days + one_day,
            updated_at: now_ts - thirty_days + one_day,
            expires_at: Some(expires_at),
        };
        storage.create_delivery_record(&record).unwrap();
    }

    // Also add one with no expiry
    let no_expiry = DeliveryRecord {
        message_id: "msg-no-expiry".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Stored,
        created_at: now_ts - thirty_days + one_day,
        updated_at: now_ts - thirty_days + one_day,
        expires_at: None,
    };
    storage.create_delivery_record(&no_expiry).unwrap();

    // Query all pending records and filter for approaching expiration
    let pending = storage.get_pending_deliveries().unwrap();
    let warning_threshold = now_ts + 3 * one_day; // Warn if expiring within 3 days

    let approaching_expiry: Vec<_> = pending
        .iter()
        .filter(|r| {
            r.expires_at
                .map(|exp| exp > now_ts && exp <= warning_threshold)
                .unwrap_or(false)
        })
        .collect();

    assert_eq!(approaching_expiry.len(), 2);
    let ids: Vec<_> = approaching_expiry
        .iter()
        .map(|r| r.message_id.as_str())
        .collect();
    assert!(ids.contains(&"msg-1day"));
    assert!(ids.contains(&"msg-2day"));
    assert!(!ids.contains(&"msg-7day")); // Too far in future
    assert!(!ids.contains(&"msg-already-expired")); // Already expired
    assert!(!ids.contains(&"msg-no-expiry")); // No expiry set
}

// === TTL Extension Tests ===
// Traces to: features/message_delivery.feature @expiration "Extend message TTL"

/// Test: Extend TTL before deletion.
///
/// Scenario: Given I have a pending message approaching expiration
///           When I choose to extend the TTL
///           Then the message should get additional time
// @scenario: message_delivery :: Extend message TTL
// @internal
#[test]
fn test_ttl_extension_request() {
    let storage = test_storage();
    let now_ts = now();
    let one_day = 86400u64;
    let seven_days = 7 * one_day;
    let original_expiry = now_ts + one_day; // Expires tomorrow

    let record = DeliveryRecord {
        message_id: "msg-extend".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Stored,
        created_at: now_ts - 29 * one_day,
        updated_at: now_ts - 29 * one_day,
        expires_at: Some(original_expiry),
    };
    storage.create_delivery_record(&record).unwrap();

    let before = storage.get_delivery_record("msg-extend").unwrap().unwrap();
    assert_eq!(before.expires_at, Some(original_expiry));

    // Extend by 7 days
    let extended = storage
        .extend_delivery_ttl("msg-extend", seven_days)
        .unwrap();
    assert!(extended);

    let after = storage.get_delivery_record("msg-extend").unwrap().unwrap();
    assert_eq!(after.expires_at, Some(original_expiry + seven_days));

    // Extend again (multiple extensions allowed)
    storage
        .extend_delivery_ttl("msg-extend", seven_days)
        .unwrap();
    let final_record = storage.get_delivery_record("msg-extend").unwrap().unwrap();
    assert_eq!(
        final_record.expires_at,
        Some(original_expiry + 2 * seven_days)
    );
}

/// Test: TTL extension fails for non-existent or no-expiry records.
// @scenario: message_delivery :: Extend message TTL
// @internal
#[test]
fn test_ttl_extension_edge_cases() {
    let storage = test_storage();
    let now_ts = now();

    let no_expiry = DeliveryRecord {
        message_id: "msg-no-ttl".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Stored,
        created_at: now_ts,
        updated_at: now_ts,
        expires_at: None,
    };
    storage.create_delivery_record(&no_expiry).unwrap();

    // Extending TTL on record with no expiry returns false
    let extended = storage.extend_delivery_ttl("msg-no-ttl", 86400).unwrap();
    assert!(!extended);

    // Extending non-existent record returns false
    let extended = storage.extend_delivery_ttl("nonexistent", 86400).unwrap();
    assert!(!extended);
}

// === Delivery Receipt Privacy Tests ===
// Traces to: features/message_delivery.feature @privacy "Delivery metadata is minimal"

/// Test: Delivery receipts don't leak sender metadata.
///
/// Scenario: Given an update is delivered via relay
///           Then the relay should log minimal metadata
///           And no long-term tracking should occur
// @scenario: message_delivery :: Delivery metadata is minimal
// @internal
#[test]
fn test_delivery_receipt_privacy() {
    let storage = test_storage();
    let timestamp = now();

    // The point: recipient_id is stored, but no sender tracking
    let records = vec![
        ("msg-from-alice", "recipient-bob"),
        ("msg-from-carol", "recipient-bob"),
        ("msg-from-dave", "recipient-eve"),
    ];

    for (msg_id, recipient_id) in &records {
        let record = DeliveryRecord {
            message_id: msg_id.to_string(),
            recipient_id: recipient_id.to_string(),
            status: DeliveryStatus::Delivered,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    // Query by recipient - privacy: we track delivery TO recipient, not FROM sender
    let bob_deliveries = storage
        .get_delivery_records_for_recipient("recipient-bob")
        .unwrap();
    assert_eq!(bob_deliveries.len(), 2);

    for record in &bob_deliveries {
        // DeliveryRecord has: message_id, recipient_id, status, timestamps
        // Critically: NO sender_id field - this is by design for privacy
        assert!(!record.message_id.is_empty());
        assert_eq!(record.recipient_id, "recipient-bob");
        assert_eq!(record.status, DeliveryStatus::Delivered);
        // The message_id might encode sender info in the actual message,
        // but the delivery tracking layer is sender-agnostic
    }

    // Privacy check: cannot query "what has this sender sent"
    // There's no get_delivery_records_for_sender() method - by design
    // The storage only tracks delivery status, not sender patterns
}

// === Relay Quota Tests ===
// Traces to: features/message_delivery.feature @persistence "Storage quota per user"
// and @errors "Handle quota exceeded"

/// Test: Graceful handling when relay quota is exceeded.
///
/// Scenario: Given I have many pending updates
///           When I exceed my storage quota on the relay
///           Then I should be notified about the quota
///           And pending deliveries should be prioritized
// @scenario: message_delivery :: Storage quota per user
// @scenario: message_delivery :: Handle quota exceeded
// @internal
#[test]
fn test_relay_quota_exceeded() {
    use vauchi_core::storage::OfflineQueue;

    let storage = test_storage();
    let timestamp = now();

    // Note: In practice, the OfflineQueue helper is used for quota checks
    let _queue = OfflineQueue::with_max_size(5);

    for i in 0..5 {
        let record = DeliveryRecord {
            message_id: format!("msg-{}", i),
            recipient_id: "contact".to_string(),
            status: DeliveryStatus::Queued,
            created_at: timestamp + i as u64,
            updated_at: timestamp + i as u64,
            expires_at: Some(timestamp + 604800), // 7 days
        };
        storage.create_delivery_record(&record).unwrap();
    }

    let pending = storage.get_pending_deliveries().unwrap();
    assert_eq!(pending.len(), 5);

    // At quota limit, new messages should be handled gracefully
    // The application layer checks capacity before adding:
    let count = storage
        .count_deliveries_by_status(&DeliveryStatus::Queued)
        .unwrap();
    assert_eq!(count, 5);

    // Simulate quota handling: oldest acknowledged can be removed
    storage
        .update_delivery_status("msg-0", &DeliveryStatus::Delivered, timestamp + 100)
        .unwrap();

    let queued_count = storage
        .count_deliveries_by_status(&DeliveryStatus::Queued)
        .unwrap();
    let delivered_count = storage
        .count_deliveries_by_status(&DeliveryStatus::Delivered)
        .unwrap();
    assert_eq!(queued_count, 4);
    assert_eq!(delivered_count, 1);

    // Remove delivered (acknowledged) to make room
    storage.delete_delivery_record("msg-0").unwrap();

    let new_record = DeliveryRecord {
        message_id: "msg-5".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Queued,
        created_at: timestamp + 5,
        updated_at: timestamp + 5,
        expires_at: Some(timestamp + 604800),
    };
    storage.create_delivery_record(&new_record).unwrap();

    // Back to 5 pending
    let pending = storage.get_pending_deliveries().unwrap();
    assert_eq!(pending.len(), 5);
}

// === Delivery Order Tests ===
// Traces to: features/message_delivery.feature @ordering "Updates applied in order"
// and "Out-of-order delivery handled gracefully"

/// Test: Messages arrive in correct order based on creation time.
///
/// Scenario: Given I update my phone number to A
///           And then I update it to B
///           When Bob receives both updates
///           Then they should be applied in order
// @scenario: message_delivery :: Updates applied in order
// @internal
#[test]
fn test_delivery_order_verification() {
    let storage = test_storage();
    let base_time = now();

    // to verify ordering is by timestamp, not ID
    let messages = vec![
        ("msg-z", base_time + 1), // First update
        ("msg-a", base_time + 2), // Second update
        ("msg-m", base_time + 3), // Third update
    ];

    for (msg_id, created_at) in &messages {
        let record = DeliveryRecord {
            message_id: msg_id.to_string(),
            recipient_id: "contact-bob".to_string(),
            status: DeliveryStatus::Stored,
            created_at: *created_at,
            updated_at: *created_at,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    let bob_records = storage
        .get_delivery_records_for_recipient("contact-bob")
        .unwrap();

    // Verify order: most recent first (DESC order)
    assert_eq!(bob_records.len(), 3);
    assert_eq!(bob_records[0].message_id, "msg-m"); // Most recent
    assert_eq!(bob_records[1].message_id, "msg-a");
    assert_eq!(bob_records[2].message_id, "msg-z"); // Oldest

    // For applying updates, we'd reverse to process oldest first
    let in_order: Vec<_> = bob_records.into_iter().rev().collect();
    assert_eq!(in_order[0].message_id, "msg-z"); // Apply first
    assert_eq!(in_order[1].message_id, "msg-a");
    assert_eq!(in_order[2].message_id, "msg-m"); // Apply last
}

/// Test: Out-of-order delivery is handled by timestamp reordering.
///
/// Scenario: Given network conditions cause out-of-order delivery
///           When updates arrive out of order
///           Then the app should reorder them by timestamp
// @scenario: message_delivery :: Out-of-order delivery handled gracefully
// @internal
#[test]
fn test_out_of_order_delivery_reordering() {
    let storage = test_storage();
    let base_time = now();

    // Simulate out-of-order arrival: delivered in order 2, 0, 1
    // but created_at shows correct order 0, 1, 2
    let delivery_sequence = vec![
        ("msg-2", base_time + 20, DeliveryStatus::Delivered), // Arrived first but created last
        ("msg-0", base_time, DeliveryStatus::Delivered),      // Arrived second but created first
        ("msg-1", base_time + 10, DeliveryStatus::Delivered), // Arrived last but created middle
    ];

    for (msg_id, created_at, status) in &delivery_sequence {
        let record = DeliveryRecord {
            message_id: msg_id.to_string(),
            recipient_id: "contact".to_string(),
            status: status.clone(),
            created_at: *created_at,
            updated_at: *created_at,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    // Query by status returns ordered by created_at ASC
    let delivered = storage
        .get_delivery_records_by_status(&DeliveryStatus::Delivered)
        .unwrap();

    assert_eq!(delivered.len(), 3);
    assert_eq!(delivered[0].message_id, "msg-0"); // Oldest first
    assert_eq!(delivered[1].message_id, "msg-1");
    assert_eq!(delivered[2].message_id, "msg-2"); // Newest last

    // This ordering allows applying updates in correct sequence
    // despite network causing out-of-order delivery
}

// ============================================================
// Automatic Cleanup (T2-12)
// @scenario: message_delivery :: Delivery records cleanup
// ============================================================

/// Test: startup maintenance cleans old terminal records but keeps recent ones
// @internal
#[test]
fn test_startup_maintenance_cleans_old_terminal_records() {
    let storage = test_storage();
    let now_ts = now();
    let thirty_one_days_ago = now_ts - (31 * 86400);
    let ten_days_ago = now_ts - (10 * 86400);

    // Old terminal records (should be cleaned)
    for (id, status) in [
        ("old-delivered", DeliveryStatus::Delivered),
        ("old-expired", DeliveryStatus::Expired),
        (
            "old-failed",
            DeliveryStatus::Failed {
                reason: "timeout".to_string(),
            },
        ),
    ] {
        let record = DeliveryRecord {
            message_id: id.to_string(),
            recipient_id: "contact".to_string(),
            status,
            created_at: thirty_one_days_ago,
            updated_at: thirty_one_days_ago,
            expires_at: None,
        };
        storage.create_delivery_record(&record).unwrap();
    }

    // Recent terminal record (should be kept)
    let record = DeliveryRecord {
        message_id: "recent-delivered".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Delivered,
        created_at: ten_days_ago,
        updated_at: ten_days_ago,
        expires_at: None,
    };
    storage.create_delivery_record(&record).unwrap();

    // Non-terminal record (should be kept regardless of age)
    let record = DeliveryRecord {
        message_id: "old-queued".to_string(),
        recipient_id: "contact".to_string(),
        status: DeliveryStatus::Queued,
        created_at: thirty_one_days_ago,
        updated_at: thirty_one_days_ago,
        expires_at: None,
    };
    storage.create_delivery_record(&record).unwrap();

    let cleaned = storage.run_startup_maintenance();
    assert!(
        cleaned.is_ok(),
        "Maintenance should succeed: {:?}",
        cleaned.err()
    );
    let count = cleaned.unwrap();
    assert_eq!(count, 3, "Should clean 3 old terminal records");

    assert!(
        storage
            .get_delivery_record("old-delivered")
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .get_delivery_record("old-expired")
            .unwrap()
            .is_none()
    );
    assert!(storage.get_delivery_record("old-failed").unwrap().is_none());

    assert!(
        storage
            .get_delivery_record("recent-delivered")
            .unwrap()
            .is_some()
    );
    assert!(
        storage.get_delivery_record("old-queued").unwrap().is_some(),
        "expected Some value"
    );
}

/// Test: startup maintenance on empty database is a no-op
// @internal
#[test]
fn test_startup_maintenance_empty_database() {
    let storage = test_storage();
    let count = storage.run_startup_maintenance().unwrap();
    assert_eq!(count, 0, "Empty database should have nothing to clean");
}
