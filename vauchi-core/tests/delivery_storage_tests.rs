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
    assert!(retrieved.is_some());

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.message_id, "msg-001");
    assert_eq!(retrieved.recipient_id, "contact-abc");
    assert_eq!(retrieved.status, DeliveryStatus::Queued);
    assert_eq!(retrieved.expires_at, Some(timestamp + 604800));
}

#[test]
fn test_get_delivery_record_not_found() {
    let storage = test_storage();

    let retrieved = storage.get_delivery_record("nonexistent").unwrap();
    assert!(retrieved.is_none());
}

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

    // Update to Sent
    let updated = storage
        .update_delivery_status("msg-002", &DeliveryStatus::Sent, timestamp + 1)
        .unwrap();
    assert!(updated);

    let retrieved = storage.get_delivery_record("msg-002").unwrap().unwrap();
    assert_eq!(retrieved.status, DeliveryStatus::Sent);
    assert_eq!(retrieved.updated_at, timestamp + 1);

    // Update to Stored
    storage
        .update_delivery_status("msg-002", &DeliveryStatus::Stored, timestamp + 2)
        .unwrap();
    let retrieved = storage.get_delivery_record("msg-002").unwrap().unwrap();
    assert_eq!(retrieved.status, DeliveryStatus::Stored);

    // Update to Delivered
    storage
        .update_delivery_status("msg-002", &DeliveryStatus::Delivered, timestamp + 3)
        .unwrap();
    let retrieved = storage.get_delivery_record("msg-002").unwrap().unwrap();
    assert_eq!(retrieved.status, DeliveryStatus::Delivered);
}

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

#[test]
fn test_get_delivery_records_for_recipient() {
    let storage = test_storage();
    let timestamp = now();

    // Create records for two recipients
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

#[test]
fn test_get_pending_deliveries() {
    let storage = test_storage();
    let timestamp = now();

    // Create records with different statuses
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

#[test]
fn test_count_deliveries_by_status() {
    let storage = test_storage();
    let timestamp = now();

    // Create 3 Stored, 2 Delivered, 1 Failed
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

#[test]
fn test_expire_old_deliveries() {
    let storage = test_storage();
    let now_ts = now();
    let past = now_ts - 1000;
    let future = now_ts + 1000;

    // Create records: one expired, one not expired, one no expiry
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

    // Run expiration
    let expired_count = storage.expire_old_deliveries(now_ts).unwrap();
    assert_eq!(expired_count, 1);

    // Check statuses
    let expired_record = storage.get_delivery_record("expired").unwrap().unwrap();
    assert_eq!(expired_record.status, DeliveryStatus::Expired);

    let active_record = storage.get_delivery_record("active").unwrap().unwrap();
    assert_eq!(active_record.status, DeliveryStatus::Stored);

    let no_expiry_record = storage.get_delivery_record("no-expiry").unwrap().unwrap();
    assert_eq!(no_expiry_record.status, DeliveryStatus::Stored);
}

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

    // Verify it exists
    assert!(storage.get_delivery_record("to-delete").unwrap().is_some());

    // Delete it
    let deleted = storage.delete_delivery_record("to-delete").unwrap();
    assert!(deleted);

    // Verify it's gone
    assert!(storage.get_delivery_record("to-delete").unwrap().is_none());

    // Try to delete non-existent
    let deleted = storage.delete_delivery_record("nonexistent").unwrap();
    assert!(!deleted);
}

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

    // Progress through statuses
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
